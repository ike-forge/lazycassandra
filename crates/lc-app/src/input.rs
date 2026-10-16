//! Terminal input, read on a dedicated blocking thread so the main loop never
//! blocks on a keystroke.

use crossterm::event::{self, Event, MouseEvent};
use lc_domain::Msg;
use std::sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError};
use std::time::Duration;
use tokio::sync::mpsc::UnboundedSender;

/// A pointer event is forwarded whole: only the main loop has the frame map.
/// Unbound events such as `Moved` never reach the channel.
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

/// Idle-cheap, yet short enough that handing over the terminal shows no pause.
const POLL: Duration = Duration::from_millis(50);

/// A child spawned while the reader is in `event::read()` steals keystrokes, so
/// the reader parks first.
#[derive(Debug, Clone, Default)]
pub(crate) struct Suspension(Arc<(Mutex<Flags>, Condvar)>);

#[derive(Debug, Default)]
struct Flags {
    suspended: bool,
    parked: bool,
    /// Set when the reader dies, so a `suspend` never waits for a park that
    /// cannot come.
    gone: bool,
}

impl Suspension {
    /// Blocks until the reader is parked.
    pub(crate) fn suspend(&self) {
        let (flags, changed) = &*self.0;
        let mut flags = lock(flags);
        flags.suspended = true;
        while !flags.parked && !flags.gone {
            flags = wait(changed, flags);
        }
    }

    pub(crate) fn resume(&self) {
        let (flags, changed) = &*self.0;
        lock(flags).suspended = false;
        changed.notify_all();
    }

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

    /// Any `suspend`, now or later, must stop waiting.
    fn leave(&self) {
        let (flags, changed) = &*self.0;
        lock(flags).gone = true;
        changed.notify_all();
    }
}

/// Poisoning leaves nothing to restore in two bools; `unwrap` is banned outside
/// tests.
fn lock(flags: &Mutex<Flags>) -> MutexGuard<'_, Flags> {
    flags.lock().unwrap_or_else(PoisonError::into_inner)
}

fn wait<'a>(changed: &Condvar, flags: MutexGuard<'a, Flags>) -> MutexGuard<'a, Flags> {
    changed.wait(flags).unwrap_or_else(PoisonError::into_inner)
}

/// On drop, so a panicking reader still sets `gone`; a `leave()` after the loop
/// is skipped on unwind.
struct LeaveOnDrop<'a>(&'a Suspension);

impl Drop for LeaveOnDrop<'_> {
    fn drop(&mut self) {
        self.0.leave();
    }
}

pub(crate) fn spawn_reader(tx: UnboundedSender<Incoming>, suspension: Suspension) {
    std::thread::spawn(move || {
        let _leave_on_drop = LeaveOnDrop(&suspension);
        loop {
            suspension.park();

            // Polled, so the thread is never inside `read()` when a child needs
            // stdin.
            match event::poll(POLL) {
                Ok(true) => {}
                Ok(false) => continue,
                // The terminal is gone; nobody is left to tell.
                Err(_) => break,
            }

            let Ok(event) = event::read() else { break };

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
                break;
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicBool, Ordering};

    /// The stand-in parks in a loop, as the reader does: a single early
    /// `park()` would leave `suspend` waiting.
    #[test]
    fn suspend_waits_for_the_reader_to_park() {
        let suspension = Suspension::default();
        let reader = suspension.clone();
        let stop = Arc::new(AtomicBool::new(false));
        let stopped = Arc::clone(&stop);

        let handle = std::thread::spawn(move || {
            while !stopped.load(Ordering::Acquire) {
                reader.park();
                std::thread::yield_now();
            }
        });

        suspension.suspend();
        let (flags, _changed) = &*suspension.0;
        assert!(
            lock(flags).parked,
            "suspend returned before the reader announced it had parked"
        );

        stop.store(true, Ordering::Release);
        suspension.resume();
        handle.join().expect("the reader thread parked and woke");
    }

    #[test]
    fn park_returns_immediately_when_nothing_is_suspended() {
        Suspension::default().park();
    }

    /// Without `gone` this hangs, which nextest reports as a timeout.
    #[test]
    fn suspend_stops_waiting_once_the_reader_is_gone() {
        let suspension = Suspension::default();
        suspension.leave();
        suspension.suspend();
    }

    /// `LeaveOnDrop` must fire on unwind.
    #[test]
    fn leave_on_drop_fires_even_when_the_reader_panics() {
        let suspension = Suspension::default();
        let reader = suspension.clone();

        let handle = std::thread::spawn(move || {
            let _leave_on_drop = LeaveOnDrop(&reader);
            panic!("standing in for a reader that crashed mid-loop");
        });

        assert!(handle.join().is_err(), "the reader thread panicked");
        suspension.suspend();
        let (flags, _changed) = &*suspension.0;
        assert!(
            lock(flags).gone,
            "the drop guard must set `gone` on unwind, not only on a clean exit"
        );
    }
}
