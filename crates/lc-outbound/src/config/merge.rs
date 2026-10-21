//! Layering, lowest to highest. Produces a `Layer`: cross-file references
//! resolve only after both are one.

use lc_domain::config::{Diagnostic, Origin};
use lc_domain::style::Severity;

use super::wire::{DriverLayer, ExportLayer, GridLayer, HistoryLayer, Layer, LayoutLayer, Located};

/// Only a name stated twice within one file; `parse` reported the rest.
pub(crate) fn merge(layers: Vec<Layer>) -> (Layer, Vec<Diagnostic>) {
    let mut diagnostics = Vec::new();
    let mut merged = Layer::default();

    for layer in layers {
        merged.theme = layer.theme.or(merged.theme);
        merged.background = layer.background.or(merged.background);
        merged.icons = layer.icons.or(merged.icons);
        merged.mouse = layer.mouse.or(merged.mouse);
        merged.default_connection = layer.default_connection.or(merged.default_connection);
        merged.layout = merge_layout(merged.layout, layer.layout);
        merged.grid = merge_grid(merged.grid, layer.grid);
        merged.driver = merge_driver(merged.driver, layer.driver);
        merged.history = merge_history(merged.history, layer.history);
        merged.export = merge_export(merged.export, layer.export);
        merged.connections = union(
            merged.connections,
            dedup(
                layer.connections,
                "connection",
                |c| &c.name,
                &mut diagnostics,
            ),
            |connection| connection.name.value.clone(),
        );
        merged.themes = union(
            merged.themes,
            dedup(layer.themes, "theme", |t| &t.name, &mut diagnostics),
            |theme| theme.name.value.clone(),
        );
    }

    (merged, diagnostics)
}

/// A name in both keeps its position and takes the higher value whole: field
/// merging would inherit `posture` from a forgotten file.
fn union<T>(lower: Vec<T>, upper: Vec<T>, key: impl Fn(&T) -> String) -> Vec<T> {
    let mut upper: Vec<Option<T>> = upper.into_iter().map(Some).collect();
    let mut merged: Vec<T> = Vec::with_capacity(lower.len() + upper.len());

    for existing in lower {
        let name = key(&existing);
        let replacement = upper
            .iter_mut()
            .find(|slot| slot.as_ref().is_some_and(|entry| key(entry) == name))
            .and_then(Option::take);

        merged.push(replacement.unwrap_or(existing));
    }

    merged.extend(upper.into_iter().flatten());
    merged
}

/// The first wins within a file: the order a user reads.
fn dedup<T>(
    entries: Vec<T>,
    what: &str,
    name: impl Fn(&T) -> &Located<String>,
    diagnostics: &mut Vec<Diagnostic>,
) -> Vec<T> {
    let mut seen: Vec<String> = Vec::new();
    let mut kept = Vec::with_capacity(entries.len());

    for entry in entries {
        let located = name(&entry);

        if seen.contains(&located.value) {
            diagnostics.push(Diagnostic {
                origin: located
                    .origin()
                    .cloned()
                    .unwrap_or_else(|| Origin::new("<config>")),
                span: located.span(),
                severity: Severity::Warning,
                message: format!(
                    "{what} `{}` is stated twice — the later one is dropped",
                    located.value
                ),
            });
            continue;
        }

        seen.push(located.value.clone());
        kept.push(entry);
    }

    kept
}

fn merge_layout(lower: Option<LayoutLayer>, upper: Option<LayoutLayer>) -> Option<LayoutLayer> {
    match (lower, upper) {
        (lower, None) => lower,
        (None, upper) => upper,
        (Some(lower), Some(upper)) => Some(LayoutLayer {
            sidebar: upper.sidebar.or(lower.sidebar),
            ddl: upper.ddl.or(lower.ddl),
            queries: upper.queries.or(lower.queries),
            unknown: lower.unknown,
        }),
    }
}

fn merge_grid(lower: Option<GridLayer>, upper: Option<GridLayer>) -> Option<GridLayer> {
    match (lower, upper) {
        (lower, None) => lower,
        (None, upper) => upper,
        (Some(lower), Some(upper)) => Some(GridLayer {
            max_column_width: upper.max_column_width.or(lower.max_column_width),
            page_rows: upper.page_rows.or(lower.page_rows),
            unknown: lower.unknown,
        }),
    }
}

fn merge_history(lower: Option<HistoryLayer>, upper: Option<HistoryLayer>) -> Option<HistoryLayer> {
    match (lower, upper) {
        (lower, None) => lower,
        (None, upper) => upper,
        (Some(lower), Some(upper)) => Some(HistoryLayer {
            limit: upper.limit.or(lower.limit),
            unknown: lower.unknown,
        }),
    }
}

fn merge_driver(lower: Option<DriverLayer>, upper: Option<DriverLayer>) -> Option<DriverLayer> {
    match (lower, upper) {
        (lower, None) => lower,
        (None, upper) => upper,
        (Some(lower), Some(upper)) => Some(DriverLayer {
            page_size: upper.page_size.or(lower.page_size),
            request_timeout_ms: upper.request_timeout_ms.or(lower.request_timeout_ms),
            connect_timeout_ms: upper.connect_timeout_ms.or(lower.connect_timeout_ms),
            consistency: upper.consistency.or(lower.consistency),
            compression: upper.compression.or(lower.compression),
            unknown: lower.unknown,
        }),
    }
}

fn merge_export(lower: Option<ExportLayer>, upper: Option<ExportLayer>) -> Option<ExportLayer> {
    match (lower, upper) {
        (lower, None) => lower,
        (None, upper) => upper,
        (Some(lower), Some(upper)) => Some(ExportLayer {
            path: upper.path.or(lower.path),
            prefix: upper.prefix.or(lower.prefix),
            clipboard_mb: upper.clipboard_mb.or(lower.clipboard_mb),
            unknown: lower.unknown,
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::parse;

    fn layer(source: &str) -> Layer {
        let origin = Origin::new("test.toml");
        parse(source, &origin).0.expect("the fixture parses")
    }

    #[test]
    fn a_scalar_the_local_layer_states_wins() {
        let (merged, _) = merge(vec![
            layer("theme = \"ash-dark\"\nicons = true\n"),
            layer("theme = \"quill-light\"\n"),
        ]);

        assert_eq!(
            merged.theme.map(|theme| theme.value),
            Some("quill-light".to_owned())
        );
        assert_eq!(
            merged.icons,
            Some(true),
            "a key local did not state is not cleared"
        );
    }

    #[test]
    fn a_table_merges_key_by_key_rather_than_wholesale() {
        let (merged, _) = merge(vec![
            layer("[layout]\nsidebar = 30\nddl = 70\nqueries = 15\n"),
            layer("[layout]\nqueries = 20\n"),
        ]);
        let layout = merged.layout.expect("the table survives");

        assert_eq!(layout.sidebar.map(|share| share.value), Some(30));
        assert_eq!(layout.ddl.map(|share| share.value), Some(70));
        assert_eq!(layout.queries.map(|share| share.value), Some(20));
    }

    #[test]
    fn connections_union_by_name_and_a_name_in_both_is_taken_from_local_whole() {
        let (merged, _) = merge(vec![
            layer(
                "[[connections]]\nname = \"prod\"\nhosts = [\"a:9042\"]\nposture = \"writable\"\n\
                 [[connections]]\nname = \"staging\"\nhosts = [\"b:9042\"]\nposture = \"writable\"\n",
            ),
            layer(
                "[[connections]]\nname = \"staging\"\nhosts = [\"c:9042\"]\n\
                 [[connections]]\nname = \"dev\"\nhosts = [\"d:9042\"]\n",
            ),
        ]);
        let names: Vec<String> = merged
            .connections
            .iter()
            .map(|c| c.name.value.clone())
            .collect();

        assert_eq!(
            names,
            vec!["prod", "staging", "dev"],
            "global order first, then local-only entries; a shared name keeps its global position"
        );

        let staging = &merged.connections[1];
        assert_eq!(
            staging.hosts.as_deref(),
            Some(["c:9042".to_owned()].as_slice())
        );
        assert_eq!(
            staging.posture, None,
            "a local entry replaces the global one whole; posture is not inherited"
        );
    }

    #[test]
    fn an_array_of_plain_values_is_replaced_rather_than_concatenated() {
        let (merged, _) = merge(vec![
            layer("[[connections]]\nname = \"prod\"\nhosts = [\"a:9042\", \"b:9042\"]\n"),
            layer("[[connections]]\nname = \"prod\"\nhosts = [\"c:9042\"]\n"),
        ]);

        assert_eq!(
            merged.connections[0].hosts.as_deref(),
            Some(["c:9042".to_owned()].as_slice())
        );
    }

    #[test]
    fn a_name_stated_twice_in_one_file_keeps_the_first_and_reports_the_second() {
        let (merged, diagnostics) = merge(vec![layer(
            "[[connections]]\nname = \"prod\"\nhosts = [\"a:9042\"]\n\
             [[connections]]\nname = \"prod\"\nhosts = [\"b:9042\"]\n",
        )]);

        assert_eq!(merged.connections.len(), 1);
        assert_eq!(
            merged.connections[0].hosts.as_deref(),
            Some(["a:9042".to_owned()].as_slice())
        );
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].severity, Severity::Warning);
        assert!(diagnostics[0].message.contains("prod"));
    }

    #[test]
    fn themes_union_by_name_the_same_way_connections_do() {
        let (merged, _) = merge(vec![
            layer("[[themes]]\nname = \"midnight\"\nextends = \"ash-dark\"\n"),
            layer("[[themes]]\nname = \"midnight\"\nextends = \"quill-dark\"\n"),
        ]);

        assert_eq!(merged.themes.len(), 1);
        assert_eq!(
            merged.themes[0].extends.as_ref().map(|e| e.value.as_str()),
            Some("quill-dark")
        );
    }
}
