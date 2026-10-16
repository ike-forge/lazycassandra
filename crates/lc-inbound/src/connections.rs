//! The connection picker. Rows are name, hosts and posture, since a name alone
//! does not say which cluster. An empty list draws guidance naming the config
//! file.

use ratatui::Frame;
use ratatui::layout::{Margin, Rect};
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Clear, List, ListItem, ListState, Paragraph};

use lc_domain::search::Search;
use lc_domain::style::{Severity, Token};
use lc_domain::view::ConnectionRow;

use crate::theme::Theme;

/// Below this the picker is refused.
const MIN_WIDTH: u16 = 34;
const MAX_WIDTH: u16 = 72;
const PAD: u16 = 1;
const MARKER: &str = "● ";
const NO_MARKER: &str = "  ";
const GAP: &str = "  ";

/// A glyph, not "(default)", which would push the hosts off a narrow block.
fn marker(row: &ConnectionRow) -> &'static str {
    if row.is_default { MARKER } else { NO_MARKER }
}

/// The block does not scroll, so a longer list is refused rather than opened
/// dead.
pub(crate) fn visible_rows(area: Rect) -> usize {
    if area.width < MIN_WIDTH || area.height < 2 {
        return 0;
    }

    usize::from(area.height - 2)
}

/// Measured over every connection, so columns hold still as a filter narrows.
fn columns(rows: &[ConnectionRow]) -> [usize; 3] {
    let widest = |of: fn(&ConnectionRow) -> usize| rows.iter().map(of).max().unwrap_or(0);

    [
        widest(|row| row.name.chars().count()),
        widest(|row| row.hosts.chars().count()),
        widest(|row| row.posture.label().chars().count()),
    ]
}

fn width(area: Rect, rows: &[ConnectionRow]) -> u16 {
    let [name, hosts, posture] = columns(rows);
    let widest = MARKER.chars().count() + name + GAP.len() + hosts + GAP.len() + posture;
    let wanted = u16::try_from(widest.saturating_add(usize::from(2 + 2 * PAD)))
        .unwrap_or(u16::MAX)
        .clamp(MIN_WIDTH, MAX_WIDTH);

    wanted.min(area.width)
}

fn rect(area: Rect, rows: &[ConnectionRow]) -> Option<Rect> {
    let height = u16::try_from(rows.len().saturating_add(2)).unwrap_or(u16::MAX);

    if rows.len() > visible_rows(area) {
        return None;
    }

    let width = width(area, rows);

    Some(Rect::new(
        area.x + (area.width - width) / 2,
        area.y + (area.height - height) / 2,
        width,
        height,
    ))
}

/// Sized from every row; `matched` indexes the survivors and `selected` counts
/// in it. `config_home` is drawn only for an empty list.
pub(crate) fn render(
    frame: &mut Frame,
    rows: &[ConnectionRow],
    matched: &[usize],
    selected: usize,
    search: Option<&Search>,
    config_home: Option<&str>,
    theme: Theme,
) {
    if rows.is_empty() {
        empty(frame, config_home, theme);
        return;
    }

    let Some(area) = rect(frame.area(), rows) else {
        return;
    };

    let block = Block::bordered()
        .border_type(BorderType::Rounded)
        .border_style(theme.focus_border())
        .title("Connection");
    let inner = block.inner(area).inner(Margin {
        horizontal: PAD,
        vertical: 0,
    });

    frame.render_widget(Clear, area);
    frame.buffer_mut().set_style(area, theme.ground());
    frame.render_widget(block, area);

    // Says so rather than drawing an empty box.
    if matched.is_empty() {
        frame.render_widget(
            List::new([ListItem::new(Line::from(Span::styled(
                "no connection matches",
                theme.style(Token::Faint),
            )))]),
            inner,
        );
        return;
    }

    let [name_width, hosts_width, _] = columns(rows);
    let items: Vec<ListItem> = matched
        .iter()
        .filter_map(|at| rows.get(*at))
        .map(|row| {
            let mut spans = vec![Span::styled(
                marker(row),
                theme.style(Token::Posture(row.posture)),
            )];

            // Drawn as spans to mark matches, so padding follows them.
            spans.extend(crate::highlight::spans(
                &row.name,
                search,
                Style::default(),
                theme,
            ));
            spans.push(Span::raw(
                " ".repeat(name_width.saturating_sub(row.name.chars().count())),
            ));
            spans.push(Span::raw(GAP));
            spans.push(Span::styled(
                format!("{:hosts_width$}", row.hosts),
                theme.style(Token::Faint),
            ));
            spans.push(Span::raw(GAP));
            spans.push(Span::styled(
                row.posture.label(),
                theme.style(Token::Posture(row.posture)),
            ));

            ListItem::new(Line::from(spans))
        })
        .collect();
    let mut state = ListState::default().with_selected(Some(selected));

    frame.render_stateful_widget(
        List::new(items).highlight_style(theme.style(Token::Selected)),
        inner,
        &mut state,
    );
}

/// Sized here: `overlay::render` under-counts embedded newlines.
fn empty(frame: &mut Frame, config_home: Option<&str>, theme: Theme) {
    let area = frame.area();
    let where_to = config_home.unwrap_or("your LazyCassandra config file");
    let lines = guidance(where_to);
    let widest = lines
        .iter()
        .map(|line| line.chars().count())
        .max()
        .unwrap_or(0);
    let width = u16::try_from(widest.saturating_add(usize::from(2 + 2 * PAD)))
        .unwrap_or(u16::MAX)
        .min(area.width);
    let height = u16::try_from(lines.len().saturating_add(2)).unwrap_or(u16::MAX);

    if width < MIN_WIDTH || height > area.height {
        return;
    }

    let block = Block::bordered()
        .border_type(BorderType::Rounded)
        .border_style(theme.style(Token::Severity(Severity::Warning)))
        .title("No connections");
    let rect = Rect::new(
        area.x + (area.width - width) / 2,
        area.y + (area.height - height) / 2,
        width,
        height,
    );
    let inner = block.inner(rect).inner(Margin {
        horizontal: PAD,
        vertical: 0,
    });

    frame.render_widget(Clear, rect);
    frame.buffer_mut().set_style(rect, theme.ground());
    frame.render_widget(block, rect);
    frame.render_widget(
        Paragraph::new(
            lines
                .into_iter()
                .map(|line| Line::from(Span::raw(line)))
                .collect::<Vec<_>>(),
        ),
        inner,
    );
}

/// Pre-broken: wrapping would break the copyable TOML.
fn guidance(where_to: &str) -> Vec<String> {
    vec![
        "No connection is configured.".to_owned(),
        String::new(),
        format!("Add one to {where_to}:"),
        String::new(),
        "  [[connections]]".to_owned(),
        "  name  = \"local\"".to_owned(),
        "  hosts = [\"127.0.0.1:9042\"]".to_owned(),
        String::new(),
        "Then name it under default_connection".to_owned(),
        "to open it on startup.".to_owned(),
    ]
}

#[cfg(test)]
mod tests {
    use lc_domain::posture::Posture;
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;
    use ratatui::buffer::Cell;

    use super::*;

    fn row(name: &str, is_default: bool) -> ConnectionRow {
        ConnectionRow {
            name: name.to_owned(),
            hosts: format!("{name}.example:9042"),
            posture: Posture::ReadOnly,
            is_default,
        }
    }

    fn drawn(width: u16, height: u16, rows: &[ConnectionRow], matched: &[usize]) -> String {
        let mut terminal = Terminal::new(TestBackend::new(width, height)).expect("a backend");

        terminal
            .draw(|frame| {
                render(
                    frame,
                    rows,
                    matched,
                    0,
                    None,
                    Some("/x/config.toml"),
                    Theme::default(),
                );
            })
            .expect("a frame");

        terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .map(Cell::symbol)
            .collect()
    }

    #[test]
    fn a_row_names_the_connection_its_hosts_and_its_posture() {
        let rows = [row("prod", false), row("staging", false)];
        let frame = drawn(80, 24, &rows, &[0, 1]);

        assert!(frame.contains("prod"));
        assert!(frame.contains("prod.example:9042"));
        assert!(
            frame.contains("read-only"),
            "a name alone does not say what opening it permits"
        );
        assert!(frame.contains("staging"));
    }

    #[test]
    fn the_default_connection_is_marked_and_no_other_is() {
        let rows = [row("prod", false), row("local", true)];
        let frame = drawn(80, 24, &rows, &[0, 1]);

        assert_eq!(
            frame.matches(MARKER.trim()).count(),
            1,
            "exactly one entry wears the mark"
        );
    }

    #[test]
    fn no_connections_draws_guidance_naming_the_config_file() {
        let frame = drawn(80, 24, &[], &[]);

        assert!(frame.contains("No connection is configured"));
        assert!(
            frame.contains("/x/config.toml"),
            "the guidance points somewhere"
        );
        assert!(
            frame.contains("[[connections]]"),
            "and shows the shape, so the user has something to copy"
        );
    }

    #[test]
    fn a_filter_moves_no_border() {
        let rows = [
            row("prod", false),
            row("a-very-long-connection-name", false),
        ];
        let area = Rect::new(0, 0, 80, 24);

        assert_eq!(
            rect(area, &rows).expect("fits").width,
            width(area, &rows),
            "the block is sized from every row"
        );

        let whole = drawn(80, 24, &rows, &[0, 1]);
        let filtered = drawn(80, 24, &rows, &[0]);
        let corner = |frame: &str| frame.find('╭');

        assert_eq!(
            corner(&whole),
            corner(&filtered),
            "the block's corner does not move as the term narrows the list"
        );
    }

    #[test]
    fn a_frame_too_narrow_for_a_row_draws_none_of_it() {
        let frame = drawn(MIN_WIDTH - 1, 24, &[row("prod", false)], &[0]);

        assert!(
            !frame.contains("prod"),
            "a truncated connection name is a lie about which cluster it reaches"
        );
    }

    #[test]
    fn a_term_that_matches_nothing_draws_a_line_saying_so() {
        let frame = drawn(80, 24, &[row("prod", false)], &[]);

        assert!(frame.contains("no connection matches"));
        assert!(!frame.contains("prod.example"), "and lists nothing");
    }
}
