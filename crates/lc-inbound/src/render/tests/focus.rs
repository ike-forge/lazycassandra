use super::*;

#[test]
fn only_the_focused_panel_is_bold() {
    let buffer = draw(40, 12);
    let bold = |x, y| {
        buffer
            .cell((x, y))
            .expect("corner is in bounds")
            .modifier
            .contains(Modifier::BOLD)
    };

    // Top-left corner of each panel; Tables is focused by default.
    assert!(bold(0, 0), "Tables is focused");
    assert!(!bold(16, 0), "Contents is not");
    assert!(!bold(0, 5), "Ddl is not");
    assert!(!bold(16, 9), "Queries is not");
}

#[test]
fn only_the_focused_panel_wears_the_focus_colour() {
    let buffer = draw(40, 12);
    let focus_fg = |x, y| buffer.cell((x, y)).expect("corner is in bounds").fg;
    let focus = Theme::default().palette().focus;

    // Top-left corner of each panel; Tables is focused by default.
    assert_eq!(focus_fg(0, 0), focus, "Tables' border is focus-coloured");
    assert_ne!(focus_fg(16, 0), focus, "Contents' is not");
    assert_ne!(focus_fg(0, 7), focus, "Ddl's is not");
    assert_ne!(focus_fg(16, 9), focus, "Queries' is not");
}

#[test]
fn the_focused_panels_plain_title_text_wears_the_focus_colour() {
    // Row 0 is "╭── [1]-Tables ╮╭── [3]-Table contents ╮": x=2 is inside
    // Tables' inset and x=18 inside Contents', which is not focused.
    let buffer = draw(40, 12);
    let fg = |x, y| buffer.cell((x, y)).expect("cell is in bounds").fg;
    let focus = Theme::default().palette().focus;

    assert_eq!(
        fg(2, 0),
        focus,
        "Tables' plain title text is focus-coloured"
    );
    assert_ne!(fg(18, 0), focus, "Contents' plain title text is not");
}

#[test]
fn focus_follows_the_state_rather_than_the_draw_order() {
    // Contents is drawn third, so bolding it cannot be positional luck.
    let buffer = draw_focused(40, 12, Panel::Contents);
    let bold = |x, y| {
        buffer
            .cell((x, y))
            .expect("corner is in bounds")
            .modifier
            .contains(Modifier::BOLD)
    };

    assert!(bold(16, 0), "Contents is focused");
    assert!(!bold(0, 0), "Tables is not");
    assert!(!bold(0, 5), "Ddl is not");
    assert!(!bold(16, 9), "Queries is not");
}

#[test]
fn each_panel_opens_its_title_with_the_key_that_focuses_it() {
    let buffer = draw(40, 12);

    for (panel, opening) in [
        (Panel::Tables, "[1]-Tables"),
        (Panel::Ddl, "[2]-DDL"),
        (Panel::Contents, "[3]-Table contents"),
        (Panel::Queries, "[4]-Queries"),
    ] {
        assert_eq!(
            top_border(&buffer, opening).1,
            match panel {
                Panel::Tables | Panel::Contents => 0,
                Panel::Ddl => 5,
                Panel::Queries => 9,
            },
            "{panel:?} is not titled {opening:?} where the draft puts it"
        );
    }
}

/// The hint bar's keys are the only thing left wearing it: a title that
/// coloured one of its own characters is what `[1]-` replaced.
#[test]
fn no_title_wears_the_shortcut_colour() {
    let buffer = draw(40, 12);
    let shortcut = Theme::default().palette().shortcut;

    for y in [0, 5, 7, 8] {
        for x in 0..40 {
            assert!(
                buffer.cell((x, y)).expect("cell is in bounds").fg != shortcut,
                "column {x} of border row {y} wears the hint bar's colour"
            );
        }
    }
}
