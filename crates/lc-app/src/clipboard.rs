//! Putting a yanked line on the system clipboard.
//!
//! Two routes, both taken. OSC 52 asks the terminal, which is the only route
//! that survives SSH onto a host with no display; a helper process asks the
//! platform's clipboard, which is the only route that survives a multiplexer
//! that declines to forward OSC 52 (yank design §3, amended).

use std::process::Stdio;

use tokio::io::AsyncWriteExt;
use tokio::process::Command;

/// What the environment says about which clipboard is reachable.
///
/// Read at the edge so the choice below stays a pure function of its inputs,
/// the way `editor::program` is.
pub(crate) struct Reach<'a> {
    /// `$WSL_INTEROP`. Absent means `clip.exe` cannot be launched at all, not
    /// merely that it is missing.
    pub(crate) interop: Option<&'a str>,
    pub(crate) wsl: Option<&'a str>,
    pub(crate) wayland: Option<&'a str>,
    pub(crate) display: Option<&'a str>,
    pub(crate) mac: bool,
}

/// The helpers worth trying, in the order they are worth trying, given what
/// the environment says is reachable.
///
/// A list rather than a choice: an environment variable says a display exists,
/// not that the tool for it is installed, so a candidate that will not spawn
/// hands over to the next one.
///
/// WSL comes before X11 and Wayland deliberately. A WSL with a GUI layer
/// offers all three sockets, and only `clip.exe` reaches the clipboard the
/// user pastes from in Windows — the one they mean. It is also the only one of
/// the three that exits when it is done: `wl-copy` and `xclip` must stay alive
/// to serve the selection they own.
fn helpers(reach: &Reach<'_>) -> Vec<&'static [&'static str]> {
    let set = |value: Option<&str>| value.is_some_and(|value| !value.trim().is_empty());
    let mut helpers: Vec<&'static [&'static str]> = Vec::new();

    if reach.mac {
        helpers.push(&["pbcopy"]);
    }
    if set(reach.interop) || set(reach.wsl) {
        helpers.push(&["clip.exe"]);
    }
    if set(reach.wayland) {
        helpers.push(&["wl-copy"]);
    }
    if set(reach.display) {
        helpers.push(&["xclip", "-selection", "clipboard"]);
    }

    helpers
}

/// Hands `line` to the first helper that will take it.
///
/// Nothing is reported and nothing is waited for on the caller's side. A
/// helper that owns a selection (`xclip`, `wl-copy`) does not exit until the
/// next yank takes the selection from it, so waiting here would park the main
/// loop for as long as the user goes without yanking again — and even
/// `clip.exe`, which does exit, costs a tenth of a second per keystroke on the
/// key most likely to be held down. The child is reaped by the task, not left
/// a zombie.
pub(crate) fn hand_off(line: &str) {
    let read = |name| std::env::var(name).ok();
    let (interop, wsl, wayland, display) = (
        read("WSL_INTEROP"),
        read("WSL_DISTRO_NAME"),
        read("WAYLAND_DISPLAY"),
        read("DISPLAY"),
    );
    let candidates = helpers(&Reach {
        interop: interop.as_deref(),
        wsl: wsl.as_deref(),
        wayland: wayland.as_deref(),
        display: display.as_deref(),
        mac: cfg!(target_os = "macos"),
    });
    if candidates.is_empty() {
        return;
    }

    let line = line.to_owned();
    tokio::spawn(async move {
        for candidate in candidates {
            if write_to(candidate, &line).await.is_some() {
                return;
            }
        }
    });
}

/// `None` is a helper that would not start or would not take the line — the
/// next candidate's turn. There is no route back to the user either way
/// (yank design §4).
async fn write_to(argv: &'static [&'static str], line: &str) -> Option<()> {
    let (program, arguments) = argv.split_first()?;
    let mut child = Command::new(program)
        .args(arguments)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;

    // Dropping the handle closes the pipe, which is what tells the helper the
    // line is complete.
    child.stdin.take()?.write_all(line.as_bytes()).await.ok()?;
    let _reaped = child.wait().await;

    Some(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn nowhere() -> Reach<'static> {
        Reach {
            interop: None,
            wsl: None,
            wayland: None,
            display: None,
            mac: false,
        }
    }

    #[test]
    fn a_host_with_no_display_and_no_windows_has_no_helper() {
        assert!(
            helpers(&nowhere()).is_empty(),
            "the SSH case: OSC 52 is the only route, and asking for a helper would spawn nothing"
        );
    }

    #[test]
    fn wsl_reaches_the_clipboard_the_user_pastes_from() {
        let reach = Reach {
            interop: Some("/run/WSL/1_interop"),
            ..nowhere()
        };

        assert_eq!(helpers(&reach), [["clip.exe"]]);
    }

    #[test]
    fn a_gui_capable_wsl_prefers_windows_over_the_sockets_it_also_offers() {
        let reach = Reach {
            interop: Some("/run/WSL/1_interop"),
            wsl: Some("Ubuntu"),
            wayland: Some("wayland-0"),
            display: Some(":0"),
            mac: false,
        };

        assert_eq!(
            helpers(&reach),
            [
                &["clip.exe"][..],
                &["wl-copy"][..],
                &["xclip", "-selection", "clipboard"][..]
            ],
            "clip.exe first, but the sockets stay as fallbacks for a WSL without interop tools"
        );
    }

    #[test]
    fn a_display_names_a_helper_per_protocol() {
        let wayland = Reach {
            wayland: Some("wayland-0"),
            ..nowhere()
        };
        let x11 = Reach {
            display: Some(":0"),
            ..nowhere()
        };

        assert_eq!(helpers(&wayland), [["wl-copy"]]);
        assert_eq!(helpers(&x11), [&["xclip", "-selection", "clipboard"][..]]);
    }

    #[test]
    fn macos_comes_first_because_it_needs_no_variable_to_be_true() {
        let reach = Reach {
            display: Some(":0"),
            mac: true,
            ..nowhere()
        };

        assert_eq!(
            helpers(&reach),
            [&["pbcopy"][..], &["xclip", "-selection", "clipboard"][..]]
        );
    }

    #[test]
    fn an_empty_variable_is_not_a_display() {
        let reach = Reach {
            wayland: Some(""),
            display: Some("   "),
            ..nowhere()
        };

        assert!(helpers(&reach).is_empty());
    }
}
