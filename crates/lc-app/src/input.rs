//! Terminal input, forwarded as messages.
//!
//! crossterm reads on a dedicated blocking thread so the main loop never
//! blocks on a keystroke (architecture.md §3).

use crossterm::event::{self, Event, MouseEvent};
use lc_domain::Msg;
use std::sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError};
use std::time::Duration;
use tokio::sync::mpsc::UnboundedSender;

/// What the main loop receives.
///
/// A pointer event only means something against the frame it happened in, and
/// this thread has no frame — so a bound one is forwarded whole, and the main
/// loop, which holds the map, resolves it
/// (docs/superpowers/specs/2026-08-09-mouse-input-design.md §4). Gestures `is_bound` refuses
/// — chiefly `Moved`, which any-event tracking reports for every cell the
/// pointer crosses — never reach the channel at all (design §2).
#[derive(Debug)]
pub(crate) enum Incoming {
    Msg(Msg),
    Mouse(MouseEvent),
}

impl From<Msg> for Incoming {
    fn from(msg: Msg) -> Self {
        Self::Msg(msg)
    }
}

/// How long the reader waits for an event before looking at the suspension
/// flag again. Long enough that an idle terminal costs nothing measurable,
/// short enough that handing the terminal over is not a visible pause
/// (docs/superpowers/specs/2026-08-12-external-editor-design.md §4).
const POLL: Duration = Duration::from_millis(50);

/// The handshake that gets the reader thread off stdin.
///
/// A child process spawned while the reader is inside `event::read()` competes
/// with it for keystrokes, and the ones the reader wins are gone. So the main
/// loop suspends the reader and waits for it to say it has parked before it
/// spawns anything (design §4).
#[derive(Debug, Clone, Default)]
pub(crate) struct Suspension(Arc<(Mutex<Flags>, Condvar)>);

#[derive(Debug, Default)]
struct Flags {
    suspended: bool,
    parked: bool,
}

impl Suspension {
    /// Blocks until the reader is parked. Called before the child is spawned.
    ///
    /// Task 5 wires the call site around the editor's child process; until
    /// then nothing outside this module's tests calls it.
    #[allow(dead_code)]
    pub(crate) fn suspend(&self) {
        let (flags, changed) = &*self.0;
        let mut flags = lock(flags);
        flags.suspended = true;
        while !flags.parked {
            flags = wait(changed, flags);
        }
    }

    /// Puts the reader back on stdin.
    ///
    /// Task 5 wires the call site around the editor's child process; until
    /// then nothing outside this module's tests calls it.
    #[allow(dead_code)]
    pub(crate) fn resume(&self) {
        let (flags, changed) = &*self.0;
        lock(flags).suspended = false;
        changed.notify_all();
    }

    /// The reader's half: parks while suspended, announcing that it has.
    fn park(&self) {
        let (flags, changed) = &*self.0;
        let mut flags = lock(flags);
        while flags.suspended {
            flags.parked = true;
            changed.notify_all();
            flags = wait(changed, flags);
        }
        flags.parked = false;
    }
}

/// A poisoned lock means the other half panicked while holding two bools.
/// There is no invariant left to restore and nobody to report to, so the
/// flags are taken as they stand — the alternative is `unwrap`, which
/// architecture.md §12 bans outside tests.
fn lock(flags: &Mutex<Flags>) -> MutexGuard<'_, Flags> {
    flags.lock().unwrap_or_else(PoisonError::into_inner)
}

fn wait<'a>(changed: &Condvar, flags: MutexGuard<'a, Flags>) -> MutexGuard<'a, Flags> {
    changed.wait(flags).unwrap_or_else(PoisonError::into_inner)
}

pub(crate) fn spawn_reader(tx: UnboundedSender<Incoming>, suspension: Suspension) {
    std::thread::spawn(move || {
        loop {
            suspension.park();

            // Polled rather than blocked, so the park above is reached between
            // reads and the thread is never inside `read()` when the main loop
            // wants stdin for a child process (design §4).
            match event::poll(POLL) {
                Ok(true) => {}
                Ok(false) => continue,
                // A poll error means the terminal is gone; there is nobody
                // left to report it to.
                Err(_) => return,
            }

            let Ok(event) = event::read() else { return };

            let incoming = match event {
                Event::Key(key) => lc_inbound::input::to_key_press(key)
                    .map(Msg::Input)
                    .map(Incoming::Msg),
                Event::Mouse(mouse) if lc_inbound::input::is_bound(mouse) => {
                    Some(Incoming::Mouse(mouse))
                }
                Event::Resize(..) => Some(Msg::Resized.into()),
                _ => None,
            };

            if let Some(incoming) = incoming
                && tx.send(incoming).is_err()
            {
                return;
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The handshake's whole contract: `suspend` does not return until the
    /// other half has parked. A thread standing in for the reader.
    #[test]
    fn suspend_waits_for_the_reader_to_park() {
        let suspension = Suspension::default();
        let reader = suspension.clone();
        let parked = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let flag = Arc::clone(&parked);

        let handle = std::thread::spawn(move || {
            reader.park();
            flag.store(true, std::sync::atomic::Ordering::SeqCst);
        });

        suspension.suspend();
        assert!(
            !handle.is_finished(),
            "suspend returned while the reader could still be inside read()"
        );

        suspension.resume();
        handle.join().expect("the reader thread parked and woke");
        assert!(parked.load(std::sync::atomic::Ordering::SeqCst));
    }

    #[test]
    fn park_returns_immediately_when_nothing_is_suspended() {
        Suspension::default().park();
    }
}
