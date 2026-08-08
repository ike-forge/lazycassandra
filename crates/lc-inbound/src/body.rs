//! Drawing what a panel contains.
//!
//! Scrolling lives here rather than in the domain: the domain has no idea the
//! panel is eight rows tall. A `ListState` built per frame from the selected
//! index computes the offset, so no scroll state is stored anywhere.

use lc_domain::style::Token;
use lc_domain::view::{KeyspaceRow, PanelBody};
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::widgets::{List, ListItem, ListState, Paragraph, Wrap};

use crate::theme::Theme;

/// A keyspace is a tree node whose children arrive when it is unfolded.
const COLLAPSED: &str = "▸ ";

pub(crate) fn render(frame: &mut Frame, area: Rect, body: &PanelBody, theme: Theme) {
    if area.is_empty() {
        return;
    }

    match body {
        PanelBody::Blank => {}
        PanelBody::Notice(text) => notice(frame, area, text, theme),
        PanelBody::Keyspaces { rows, selected } => keyspaces(frame, area, rows, *selected, theme),
    }
}

fn notice(frame: &mut Frame, area: Rect, text: &str, theme: Theme) {
    frame.render_widget(
        Paragraph::new(text)
            .style(theme.style(Token::Muted))
            .wrap(Wrap { trim: true }),
        area,
    );
}

fn keyspaces(frame: &mut Frame, area: Rect, rows: &[KeyspaceRow], selected: usize, theme: Theme) {
    let items: Vec<ListItem> = rows
        .iter()
        .map(|row| {
            let item = ListItem::new(format!("{COLLAPSED}{}", row.name));
            match row.token {
                Some(token) => item.style(theme.style(token)),
                None => item,
            }
        })
        .collect();

    let list = List::new(items).highlight_style(theme.style(Token::Selected));
    let mut state = ListState::default().with_selected(Some(selected));

    frame.render_stateful_widget(list, area, &mut state);
}
