//! crossterm events into domain key presses.

use crossterm::event::{
    KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
};
use lc_domain::model::{Key, KeyPress, Modifiers};
use lc_domain::pointer::{Pointer, Wheel};

use crate::hit::FrameMap;

/// Returns `None` for events the domain has no vocabulary for.
///
/// Key *release* events are among them: Windows and the kitty protocol report
/// them, and treating one as a press double-fires every binding.
#[must_use]
pub fn to_key_press(event: KeyEvent) -> Option<KeyPress> {
    if event.kind == KeyEventKind::Release {
        return None;
    }

    let key = match event.code {
        KeyCode::Char(c) => Key::Char(c),
        KeyCode::Enter => Key::Enter,
        KeyCode::Esc => Key::Escape,
        KeyCode::Backspace => Key::Backspace,
        KeyCode::Tab => Key::Tab,
        KeyCode::BackTab => Key::BackTab,
        KeyCode::Delete => Key::Delete,
        KeyCode::Insert => Key::Insert,
        KeyCode::Home => Key::Home,
        KeyCode::End => Key::End,
        KeyCode::PageUp => Key::PageUp,
        KeyCode::PageDown => Key::PageDown,
        KeyCode::Up => Key::Up,
        KeyCode::Down => Key::Down,
        KeyCode::Left => Key::Left,
        KeyCode::Right => Key::Right,
        KeyCode::F(n) => Key::F(n),
        _ => return None,
    };

    Some(KeyPress {
        key,
        modifiers: Modifiers {
            ctrl: event.modifiers.contains(KeyModifiers::CONTROL),
            alt: event.modifiers.contains(KeyModifiers::ALT),
            shift: event.modifiers.contains(KeyModifiers::SHIFT),
        },
    })
}

/// Whether a pointer event is one of the gestures anything binds.
///
/// The reader thread refuses the rest before they cost a channel send:
/// `EnableMouseCapture` turns on any-event tracking, so a pointer crossing
/// the window reports one event per cell
/// (docs/superpowers/specs/2026-08-09-mouse-input-design.md §2).
#[must_use]
pub fn is_bound(event: MouseEvent) -> bool {
    event.modifiers.is_empty()
        && matches!(
            event.kind,
            MouseEventKind::Down(MouseButton::Left)
                | MouseEventKind::ScrollUp
                | MouseEventKind::ScrollDown
        )
}

/// Resolves a pointer event against the frame it happened in.
///
/// Returns `None` for a gesture the design does not bind, and for one that
/// landed outside every panel
/// (docs/superpowers/specs/2026-08-09-mouse-input-design.md §2).
#[must_use]
pub fn to_pointer(event: MouseEvent, map: &FrameMap) -> Option<Pointer> {
    // ctrl and alt make a gesture of their own, refused here rather than
    // folded into the plain one, exactly as `handle_key` refuses them. shift
    // is refused too, for a different reason: under mouse capture, shift-drag
    // is the terminal's own text-selection escape hatch, so a shifted press is
    // a user selecting text rather than clicking a row (design §6).
    if !is_bound(event) {
        return None;
    }

    match event.kind {
        MouseEventKind::Down(MouseButton::Left) => {
            map.hit(event.column, event.row).map(Pointer::Click)
        }
        MouseEventKind::ScrollDown => notch(map, event, Wheel::Down),
        MouseEventKind::ScrollUp => notch(map, event, Wheel::Up),
        _ => None,
    }
}

fn notch(map: &FrameMap, event: MouseEvent, direction: Wheel) -> Option<Pointer> {
    let panel = map.hit(event.column, event.row)?.panel();

    Some(Pointer::Wheel { panel, direction })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hit::{PanelHit, RowStrip};
    use lc_domain::pointer::Click;
    use lc_domain::state::Panel;
    use ratatui::layout::Rect;

    /// Tables at the top-left of a 40×12 frame, drawing three rows from the
    /// first row inside its border.
    fn map() -> FrameMap {
        FrameMap::new([
            PanelHit {
                panel: Panel::Tables,
                area: Rect::new(0, 0, 16, 7),
                inner: Rect::new(1, 1, 14, 5),
                tabs: Vec::new(),
                columns: Vec::new(),
                rows: Some(RowStrip::Rows {
                    top: 1,
                    first: 0,
                    len: 3,
                }),
            },
            PanelHit::default(),
            PanelHit::default(),
            PanelHit::default(),
        ])
    }

    fn at(kind: MouseEventKind, column: u16, row: u16) -> MouseEvent {
        MouseEvent {
            kind,
            column,
            row,
            modifiers: KeyModifiers::NONE,
        }
    }

    fn left(column: u16, row: u16) -> MouseEvent {
        at(MouseEventKind::Down(MouseButton::Left), column, row)
    }

    #[test]
    fn a_left_press_on_a_row_becomes_a_click_on_it() {
        assert_eq!(
            to_pointer(left(3, 2), &map()),
            Some(Pointer::Click(Click::Row {
                panel: Panel::Tables,
                row: 1,
                column: None
            }))
        );
    }

    #[test]
    fn a_left_press_on_chrome_becomes_a_click_on_the_panel() {
        assert_eq!(
            to_pointer(left(0, 0), &map()),
            Some(Pointer::Click(Click::Panel(Panel::Tables)))
        );
    }

    #[test]
    fn a_press_outside_every_panel_is_dropped() {
        assert_eq!(to_pointer(left(30, 11), &map()), None);
    }

    #[test]
    fn only_the_left_button_is_bound() {
        for button in [MouseButton::Right, MouseButton::Middle] {
            assert_eq!(
                to_pointer(at(MouseEventKind::Down(button), 3, 2), &map()),
                None,
                "{button:?} has no binding to be a second way to reach"
            );
        }
    }

    #[test]
    fn a_release_a_drag_and_a_move_are_dropped() {
        let kinds = [
            MouseEventKind::Up(MouseButton::Left),
            MouseEventKind::Drag(MouseButton::Left),
            MouseEventKind::Moved,
        ];

        for kind in kinds {
            assert_eq!(
                to_pointer(at(kind, 3, 2), &map()),
                None,
                "{kind:?} must not fire the press again"
            );
        }
    }

    #[test]
    fn a_wheel_notch_names_the_panel_under_the_pointer() {
        assert_eq!(
            to_pointer(at(MouseEventKind::ScrollDown, 3, 2), &map()),
            Some(Pointer::Wheel {
                panel: Panel::Tables,
                direction: Wheel::Down
            })
        );
        assert_eq!(
            to_pointer(at(MouseEventKind::ScrollUp, 0, 0), &map()),
            Some(Pointer::Wheel {
                panel: Panel::Tables,
                direction: Wheel::Up
            }),
            "a notch over the border scrolls the panel it borders"
        );
    }

    #[test]
    fn a_horizontal_wheel_is_dropped() {
        for kind in [MouseEventKind::ScrollLeft, MouseEventKind::ScrollRight] {
            assert_eq!(
                to_pointer(at(kind, 3, 2), &map()),
                None,
                "there is no horizontal navigation to reach"
            );
        }
    }

    /// The reader thread's filter and `to_pointer`'s refusal must never
    /// disagree in the direction that drops something the domain would have
    /// accepted — a filter that is merely stricter is safe, one that is looser
    /// is a silent behaviour change.
    #[test]
    fn is_bound_accepts_everything_to_pointer_would() {
        let kinds = [
            MouseEventKind::Down(MouseButton::Left),
            MouseEventKind::Down(MouseButton::Right),
            MouseEventKind::Down(MouseButton::Middle),
            MouseEventKind::Up(MouseButton::Left),
            MouseEventKind::Drag(MouseButton::Left),
            MouseEventKind::Moved,
            MouseEventKind::ScrollUp,
            MouseEventKind::ScrollDown,
            MouseEventKind::ScrollLeft,
            MouseEventKind::ScrollRight,
        ];
        let modifiers = [
            KeyModifiers::NONE,
            KeyModifiers::CONTROL,
            KeyModifiers::ALT,
            KeyModifiers::SHIFT,
        ];

        for kind in kinds {
            for modifiers in modifiers {
                let event = MouseEvent {
                    kind,
                    column: 3,
                    row: 2,
                    modifiers,
                };

                if to_pointer(event, &map()).is_some() {
                    assert!(
                        is_bound(event),
                        "to_pointer resolved {event:?} but is_bound would have \
                         dropped it before it reached to_pointer"
                    );
                }
            }
        }
    }

    #[test]
    fn a_modified_press_is_not_a_gesture() {
        for modifier in [
            KeyModifiers::CONTROL,
            KeyModifiers::ALT,
            KeyModifiers::SHIFT,
        ] {
            let event = MouseEvent {
                modifiers: modifier,
                ..left(3, 2)
            };

            assert_eq!(
                to_pointer(event, &map()),
                None,
                "{modifier:?} makes a gesture of its own, and shift is the \
                 terminal's own selection escape hatch"
            );
        }
    }
}
