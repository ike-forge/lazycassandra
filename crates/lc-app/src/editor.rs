//! Handing the terminal to the user's editor, and taking it back.
//!
//! The one effect that is not a tokio task: the child needs the terminal, so
//! the main loop stops drawing, gets off stdin, and waits
//! (docs/superpowers/specs/2026-08-12-external-editor-design.md §4).

use std::io::stdout;
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

/// Openings within this process. With the pid it makes the temp file's name
/// unique without reading a clock (design §4.1).
static OPENINGS: AtomicUsize = AtomicUsize::new(0);

/// Runs the user's editor on `buffer` and reports what came back.
///
/// The terminal is released and retaken here rather than through
/// `ratatui::restore`/`init`: those install a panic hook per call, and one
/// more hook chained onto the last for every edit is a leak the sequence does
/// not need (design §4).
///
/// # Errors
///
/// Only if the terminal itself cannot be released or retaken. An editor that
/// will not run is a `Msg::EditorFailed`, not an error: the program keeps
/// running and the user is told.
pub(crate) fn open(
    terminal: &mut DefaultTerminal,
    capture: &MouseCapture,
    suspension: &Suspension,
    buffer: &str,
) -> Result<Msg> {
    suspension.suspend();
    let released = release(terminal, || capture.suspended(|| session(buffer)));
    suspension.resume();

    Ok(match released?? {
        Ok(text) => Msg::EditorClosed { buffer: text },
        Err(reason) => Msg::EditorFailed { reason },
    })
}

/// Leaves the alternate screen for `body` and comes back to a cleared one.
///
/// The retake runs whether or not `body` succeeded: a failed spawn that left
/// the terminal released would leave the user in their shell with the program
/// still running.
fn release<T>(terminal: &mut DefaultTerminal, body: impl FnOnce() -> T) -> Result<T> {
    disable_raw_mode()?;
    execute!(stdout(), LeaveAlternateScreen)?;

    let value = body();

    enable_raw_mode()?;
    execute!(stdout(), EnterAlternateScreen)?;
    terminal.clear()?;

    Ok(value)
}

/// The temp file's whole life. `Err` is what the user is told, in their words
/// rather than a type's.
fn session(buffer: &str) -> Result<String, String> {
    let path = temp_path();
    std::fs::write(&path, buffer)
        .map_err(|error| format!("could not write {}: {error}", path.display()))?;

    let edited = run(&path);
    // The editor is done with it either way, and a failure has nothing left
    // to read out of it.
    let _removed = std::fs::remove_file(&path);

    edited
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

/// `$VISUAL`, then `$EDITOR`, then `vi` — the chain every tool that opens an
/// editor walks. The value is split on whitespace so `EDITOR="code -w"` works;
/// this is not a shell, and an invocation that needs one needs a wrapper
/// script (design §4.1).
fn program(visual: Option<&str>, editor: Option<&str>) -> Vec<String> {
    let chosen = [visual, editor]
        .into_iter()
        .flatten()
        .find(|value| !value.trim().is_empty())
        .unwrap_or("vi");

    chosen.split_whitespace().map(str::to_owned).collect()
}

/// `.cql` because the extension is what makes the user's editor highlight the
/// buffer, which is half the reason to leave the TUI (design §4.1).
fn temp_path() -> PathBuf {
    let opening = OPENINGS.fetch_add(1, Ordering::Relaxed);

    std::env::temp_dir().join(format!(
        "lazycassandra-{}-{opening}.cql",
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
