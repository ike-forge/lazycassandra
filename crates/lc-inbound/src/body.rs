//! Drawing what a panel contains.
//!
//! Scrolling lives here rather than in the domain: the domain has no idea the
//! panel is eight rows tall. A `ListState` built per frame from the selected
//! index computes the offset, so no scroll state is stored anywhere.

use lc_domain::style::Token;
use lc_domain::view::{Marker, PanelBody, TreeRow};
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::widgets::{List, ListItem, ListState, Paragraph, Wrap};

use crate::theme::Theme;

const FOLDED: &str = "▸ ";
const UNFOLDED: &str = "▾ ";
/// A row with nothing to unfold still spends the marker's width, so names stay
/// aligned down the panel.
const LEAF: &str = "  ";

const INDENT: &str = "  ";

pub(crate) fn render(frame: &mut Frame, area: Rect, body: &PanelBody, theme: Theme) {
    if area.is_empty() {
        return;
    }

    match body {
        PanelBody::Blank => {}
        PanelBody::Notice { text, token } => notice(frame, area, text, *token, theme),
        PanelBody::Tree { rows, selected } => tree(frame, area, rows, *selected, theme),
    }
}

fn notice(frame: &mut Frame, area: Rect, text: &str, token: Option<Token>, theme: Theme) {
    let mut paragraph = Paragraph::new(text).wrap(Wrap { trim: true });
    if let Some(token) = token {
        paragraph = paragraph.style(theme.style(token));
    }
    frame.render_widget(paragraph, area);
}

fn tree(frame: &mut Frame, area: Rect, rows: &[TreeRow], selected: usize, theme: Theme) {
    let items: Vec<ListItem> = rows.iter().map(|row| item(row, theme)).collect();

    let list = List::new(items).highlight_style(theme.style(Token::Selected));
    let mut state = ListState::default().with_selected(Some(selected));

    frame.render_stateful_widget(list, area, &mut state);
}

fn item(row: &TreeRow, theme: Theme) -> ListItem<'static> {
    let indent = INDENT.repeat(usize::from(row.depth));
    let marker = match row.marker {
        Marker::Folded => FOLDED,
        Marker::Unfolded => UNFOLDED,
        Marker::Leaf => LEAF,
    };

    let item = ListItem::new(format!("{indent}{marker}{}", row.label));
    match row.token {
        Some(token) => item.style(theme.style(token)),
        None => item,
    }
}
