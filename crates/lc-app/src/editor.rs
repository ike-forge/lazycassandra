//! Handing the terminal to the user's editor and taking it back. Not a task:
//! the child needs the terminal.

use std::fs::File;
use std::io::{Write, stdout};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};

use anyhow::Result;
use crossterm::execute;
use crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};
use lc_domain::Msg;
use ratatui::DefaultTerminal;

use crate::input::Suspension;
use crate::mouse::MouseCapture;

/// With the pid, a unique temp name without a clock.
static OPENINGS: AtomicUsize = AtomicUsize::new(0);

/// Not `ratatui::restore`/`init`, which chain a new panic hook per call.
///
/// # Errors
///
/// Only if the terminal cannot be released or retaken. An editor that will not
/// run is a `Msg::EditorFailed`.
pub(crate) fn open(
    terminal: &mut DefaultTerminal,
    capture: &MouseCapture,
    suspension: &Suspension,
    buffer: &str,
) -> Result<Msg> {
    suspension.suspend();
    // Capture off before leaving the alternate screen, back on after
    // re-entering: otherwise pointer motion echoes into the user's shell.
    let released = capture.suspended(|| release(terminal, || session(buffer)));
    suspension.resume();

    Ok(match released?? {
        Ok(text) => Msg::EditorClosed { buffer: text },
        Err(reason) => Msg::EditorFailed { reason },
    })
}

/// Retaken even if `body` failed, or the user is dropped in their shell with
/// the program running.
fn release<T>(terminal: &mut DefaultTerminal, body: impl FnOnce() -> T) -> Result<T> {
    disable_raw_mode()?;
    execute!(stdout(), LeaveAlternateScreen)?;

    let value = body();

    enable_raw_mode()?;
    execute!(stdout(), EnterAlternateScreen)?;
    terminal.clear()?;

    Ok(value)
}

/// `Err` is in the user's words.
fn session(buffer: &str) -> Result<String, String> {
    let path = temp_path();
    write_fresh(&path, buffer)
        .map_err(|error| format!("could not write {}: {error}", path.display()))?;

    let edited = run(&path);
    // Removed either way.
    let _removed = std::fs::remove_file(&path);

    edited
}

/// `create_new` refuses a pre-planted symlink: the name is guessable, so this
/// keeps it from being hijackable.
fn write_fresh(path: &Path, buffer: &str) -> std::io::Result<()> {
    File::options()
        .write(true)
        .create_new(true)
        .open(path)?
        .write_all(buffer.as_bytes())
}

fn run(path: &Path) -> Result<String, String> {
    let visual = std::env::var("VISUAL").ok();
    let editor = std::env::var("EDITOR").ok();
    let argv = program(visual.as_deref(), editor.as_deref());
    let Some((program, arguments)) = argv.split_first() else {
        return Err("no editor to run".to_owned());
    };

    let status = Command::new(program)
        .args(arguments)
        .arg(path)
        .status()
        .map_err(|error| format!("could not run {program}: {error}"))?;

    if !status.success() {
        return Err(format!("{program} exited with {status}"));
    }

    std::fs::read_to_string(path)
        .map_err(|error| format!("could not read the edited buffer back: {error}"))
}

/// `$VISUAL`, `$EDITOR`, then `vi`. Split on whitespace so `EDITOR="code -w"`
/// works; this is not a shell.
fn program(visual: Option<&str>, editor: Option<&str>) -> Vec<String> {
    let chosen = [visual, editor]
        .into_iter()
        .flatten()
        .find(|value| !value.trim().is_empty())
        .unwrap_or("vi");

    chosen.split_whitespace().map(str::to_owned).collect()
}

/// `.sql`, not `.cql`: Neovim maps `.cql` to Clinical Quality Language.
fn temp_path() -> PathBuf {
    let opening = OPENINGS.fetch_add(1, Ordering::Relaxed);

    std::env::temp_dir().join(format!(
        "lazycassandra-{}-{opening}.sql",
        std::process::id()
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn visual_wins_over_editor() {
        assert_eq!(program(Some("hx"), Some("vim")), ["hx"]);
    }

    #[test]
    fn editor_is_the_fallback_and_vi_the_last_one() {
        assert_eq!(program(None, Some("vim")), ["vim"]);
        assert_eq!(program(None, None), ["vi"]);
    }

    #[test]
    fn an_empty_variable_is_not_a_choice() {
        assert_eq!(program(Some(""), Some("vim")), ["vim"]);
        assert_eq!(program(Some("   "), None), ["vi"]);
    }

    #[test]
    fn arguments_come_along_but_a_shell_does_not() {
        assert_eq!(program(Some("code -w"), None), ["code", "-w"]);
        assert_eq!(
            program(Some("emacsclient -c -a ''"), None),
            ["emacsclient", "-c", "-a", "''"],
            "no quoting is interpreted: an invocation needing a shell needs a wrapper script"
        );
    }
}
