//! What is drawn when the frame cannot hold the shell.

use ratatui::Frame;
use ratatui::layout::{Alignment, Rect};
use ratatui::text::Line;
use ratatui::widgets::Paragraph;

use crate::layout::{MIN_H, MIN_W};

/// The most lines the notice draws, and so the most rows it needs.
const LINES: u16 = 3;

pub(crate) fn render(frame: &mut Frame, area: Rect) {
    let lines = lines(area);
    if lines.is_empty() {
        return;
    }

    let height = area.height.min(LINES);
    let y = area.y + (area.height - height) / 2;
    frame.render_widget(
        Paragraph::new(lines).alignment(Alignment::Center),
        Rect::new(area.x, y, area.width, height),
    );
}

/// Short lines rather than a sentence: a sentence explaining that the terminal
/// is too narrow does not fit in a terminal that is too narrow. They drop least
/// actionable first, so what the user must do survives longest.
fn lines(area: Rect) -> Vec<Line<'static>> {
    let need = Line::from(format!("need {MIN_W}×{MIN_H}"));
    let have = Line::from(format!("have {}×{}", area.width, area.height));

    match area.height {
        0 => Vec::new(),
        1 => vec![need],
        2 => vec![need, have],
        _ => vec![Line::from("Too small"), need, have],
    }
}
