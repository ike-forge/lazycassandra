//! Writing the chosen theme back into the file it is read from.
//!
//! A splice over one value rather than a re-serialisation: the user-level file
//! is entirely comments (example config design §3), and a writer that produced
//! valid TOML would take every one of them with it (theme persistence design
//! §4).

use std::fmt::Write as _;
use std::path::Path;

use lc_domain::ports::ConfigWriteError;
use serde::Deserialize;
use toml::Spanned;

/// The root table's `theme`, and nothing else.
///
/// `serde` ignores unknown keys, so every other setting in the file is
/// irrelevant here — and a `theme` inside `[[themes]]` or any other table is a
/// different key this never sees.
#[derive(Debug, Deserialize)]
struct Root {
    theme: Option<Spanned<String>>,
}

fn root_theme(source: &str) -> Result<Option<Spanned<String>>, String> {
    match toml::from_str::<Root>(source) {
        Ok(root) => Ok(root.theme),
        Err(error) => Err(error.message().to_owned()),
    }
}

/// Whether this file is the one whose `theme` is in force.
pub(crate) fn states_theme(source: &str) -> bool {
    matches!(root_theme(source), Ok(Some(_)))
}

/// `source` with its root `theme` set to `slug`, and everything else — every
/// comment, every other key, the alignment — as it was.
///
/// The three rules, in order: replace a stated value where it sits; else
/// uncomment the example's own commented line; else insert the key before the
/// first table header. Never at the end of a file that has one: the example
/// says why — "a key without its header lands in the wrong table and is
/// reported as unknown" (design §4).
pub(crate) fn set_theme(source: &str, slug: &str) -> Result<String, ConfigWriteError> {
    let value = quote(slug);
    let stated = root_theme(source)
        .map_err(|message| ConfigWriteError::Unparsable(String::new(), message))?;

    if let Some(spanned) = stated {
        let span = spanned.span();

        return Ok(format!(
            "{}{value}{}",
            &source[..span.start],
            &source[span.end..]
        ));
    }

    Ok(uncommented(source, &value).unwrap_or_else(|| inserted(source, &value)))
}

/// A TOML basic string.
///
/// `Cargo.toml` keeps `toml`'s `display` feature off — one scalar is not a
/// reason to carry a serialiser — and `ThemeName::try_new` rejects whitespace
/// but not a quote or a backslash, so a custom theme's name can need escaping.
pub(crate) fn quote(value: &str) -> String {
    let mut quoted = String::with_capacity(value.len() + 2);
    quoted.push('"');

    for character in value.chars() {
        match character {
            '"' => quoted.push_str("\\\""),
            '\\' => quoted.push_str("\\\\"),
            control if control.is_control() => {
                // Writing into a String cannot fail.
                let _written = write!(quoted, "\\u{:04X}", control as u32);
            }
            other => quoted.push(other),
        }
    }

    quoted.push('"');
    quoted
}

/// Each line, and where it starts, with its terminator trimmed off.
fn lines(source: &str) -> impl Iterator<Item = (usize, &str)> {
    let mut offset = 0;

    source.split_inclusive('\n').map(move |line| {
        let at = offset;
        offset += line.len();

        (at, line.trim_end_matches('\n').trim_end_matches('\r'))
    })
}

/// Uncomments the first commented line that is a root `theme` assignment.
///
/// The candidate is uncommented first and then parsed as a document of its
/// own, so what counts as "a theme line" is the same TOML parser rule the
/// stated case uses — a prose comment does not parse, and
/// `# background = "theme"` parses to a different key.
fn uncommented(source: &str, value: &str) -> Option<String> {
    for (at, line) in lines(source) {
        let Some(bare) = bare(line) else { continue };
        let Ok(Some(spanned)) = root_theme(&bare) else {
            continue;
        };
        let span = spanned.span();

        return Some(format!(
            "{}{}{value}{}{}",
            &source[..at],
            &bare[..span.start],
            &bare[span.end..],
            &source[at + line.len()..]
        ));
    }

    None
}

/// The line with its comment marker removed: the `#`, and one space after it
/// if there is one, so the `=` keeps its column relative to the key.
fn bare(line: &str) -> Option<String> {
    let indent = &line[..line.len() - line.trim_start().len()];
    let rest = line.trim_start().strip_prefix('#')?;
    let rest = rest.strip_prefix(' ').unwrap_or(rest);

    Some(format!("{indent}{rest}"))
}

fn inserted(source: &str, value: &str) -> String {
    let line = format!("theme = {value}\n");
    // A commented `# [grid]` is a comment, not a table, so it does not push
    // the key above itself.
    let header = lines(source).find(|(_, line)| line.trim_start().starts_with('['));

    match header {
        Some((at, _)) => format!("{}{line}{}", &source[..at], &source[at..]),
        None if source.is_empty() || source.ends_with('\n') => format!("{source}{line}"),
        None => format!("{source}\n{line}"),
    }
}

/// Replaces `path`'s contents with `contents`.
///
/// A temporary file beside the target and a rename, so a crash or a full disk
/// cannot leave half a config where a whole one was. Two details carry their
/// own reason: the target is canonicalised first, so a `config.toml` symlinked
/// into a dotfiles repository is followed rather than replaced by a regular
/// file; and the original's permissions are copied onto the temporary file,
/// because a config may hold a `password` literal (configuration design §2.2)
/// and must not widen from 0600 (theme persistence design §4).
pub(crate) fn write(path: &Path, contents: &str) -> Result<(), ConfigWriteError> {
    let target = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    let failed =
        |error: std::io::Error| ConfigWriteError::Io(format!("{}: {error}", target.display()));

    if let Some(parent) = target.parent() {
        std::fs::create_dir_all(parent).map_err(failed)?;
    }

    let temporary = temporary(&target);
    std::fs::write(&temporary, contents).map_err(failed)?;

    if let Ok(existing) = std::fs::metadata(&target) {
        let _ignored = std::fs::set_permissions(&temporary, existing.permissions());
    }

    std::fs::rename(&temporary, &target).map_err(|error| {
        let _ignored = std::fs::remove_file(&temporary);
        failed(error)
    })
}

/// Beside the target, so the rename stays on one filesystem, and named after
/// this process, so two of them cannot collide.
fn temporary(target: &Path) -> std::path::PathBuf {
    let name = target.file_name().map_or_else(
        || String::from("config.toml"),
        |name| name.to_string_lossy().into_owned(),
    );

    target.with_file_name(format!(".{name}.{}.tmp", std::process::id()))
}

#[cfg(test)]
mod tests {
    use lc_domain::theme::BuiltIn;

    use super::*;

    fn saved(source: &str) -> String {
        set_theme(source, "quill-dark").expect("a file that parses is a file we can edit")
    }

    #[test]
    fn a_stated_theme_is_replaced_where_it_sits() {
        assert_eq!(
            saved("icons = true\ntheme = \"ash-dark\"\nmouse = false\n"),
            "icons = true\ntheme = \"quill-dark\"\nmouse = false\n"
        );
    }

    #[test]
    fn a_trailing_annotation_survives_the_replacement() {
        assert_eq!(
            saved("theme      = \"ash-dark\"   # ash-dark | quill-dark\n"),
            "theme      = \"quill-dark\"   # ash-dark | quill-dark\n",
            "the value moves, the line's own documentation does not"
        );
    }

    #[test]
    fn the_commented_line_the_example_ships_is_uncommented_in_place() {
        let source = concat!(
            "# ── appearance ──\n",
            "# theme      = \"default-dark\"   # default-dark | ash-dark\n",
            "# icons      = true\n",
        );
        let expected = concat!(
            "# ── appearance ──\n",
            "theme      = \"quill-dark\"   # default-dark | ash-dark\n",
            "# icons      = true\n",
        );

        assert_eq!(
            saved(source),
            expected,
            "uncommenting is one '#' and the space after it — the rest of the line, its \
             annotation included, is the user's"
        );
    }

    #[test]
    fn a_file_stating_nothing_gets_the_key_before_the_first_table() {
        assert_eq!(
            saved("icons = true\n\n[grid]\npage_rows = 500\n"),
            "icons = true\n\ntheme = \"quill-dark\"\n[grid]\npage_rows = 500\n",
            "a root key after a [section] header lands in that table and is reported as unknown"
        );
    }

    #[test]
    fn a_file_with_no_table_at_all_gets_the_key_appended() {
        assert_eq!(
            saved("icons = true\n"),
            "icons = true\ntheme = \"quill-dark\"\n"
        );
        assert_eq!(saved(""), "theme = \"quill-dark\"\n");
    }

    #[test]
    fn a_file_whose_last_line_has_no_newline_still_gets_a_line_of_its_own() {
        assert_eq!(
            saved("icons = true"),
            "icons = true\ntheme = \"quill-dark\"\n"
        );
    }

    #[test]
    fn a_theme_key_inside_a_table_is_a_different_key_and_is_untouched() {
        assert_eq!(
            saved("[grid]\ntheme = \"not-this-one\"\n"),
            "theme = \"quill-dark\"\n[grid]\ntheme = \"not-this-one\"\n",
            "only the root table is the target"
        );
    }

    #[test]
    fn a_file_that_does_not_parse_is_refused_rather_than_clobbered() {
        let error = set_theme("theme = \n", "quill-dark").expect_err("it does not parse");

        assert!(
            matches!(error, ConfigWriteError::Unparsable(..)),
            "we do not write over a document we cannot read: {error:?}"
        );
    }

    #[test]
    fn a_name_that_needs_escaping_is_escaped() {
        assert_eq!(
            set_theme("", "he\"llo").expect("an empty file parses"),
            "theme = \"he\\\"llo\"\n",
            "ThemeName forbids whitespace, not a quote (theme.rs try_new)"
        );
    }

    #[test]
    fn a_file_that_states_a_theme_is_told_apart_from_one_that_does_not() {
        assert!(states_theme("theme = \"ash-dark\"\n"));
        assert!(!states_theme("# theme = \"ash-dark\"\n"));
        assert!(!states_theme("[grid]\ntheme = \"ash-dark\"\n"));
        assert!(
            !states_theme("theme = \n"),
            "a file that does not parse states nothing"
        );
    }

    /// Rule 2 is a promise about a file this repository ships. A reformat of
    /// the example that moved its `theme` line out of reach would silently
    /// downgrade every fresh install to rule 3 — the key appended somewhere
    /// else, the annotated line still commented above it.
    #[test]
    fn the_examples_own_theme_line_is_the_one_rule_two_uncomments() {
        let saved = saved(crate::config::example::EXAMPLE);

        assert!(
            saved.contains("\ntheme      = \"quill-dark\""),
            "the example's commented theme line is what gets uncommented"
        );
        assert_eq!(
            saved.matches("theme      =").count(),
            1,
            "and it is edited in place, not duplicated"
        );
    }

    /// The round trip the feature is: what `slug()` writes, `validate` reads
    /// back as the same choice.
    #[test]
    fn every_built_in_survives_a_save_and_a_reload() {
        for built_in in BuiltIn::ALL {
            let slug = built_in.slug();
            let source = set_theme("", slug).expect("an empty file parses");
            let origin = lc_domain::config::Origin::new("config.toml");
            let (layer, diagnostics) = crate::config::parse(&source, &origin);
            let layer = layer.expect("what we wrote is a valid file");
            let (merged, _) = crate::config::merge::merge(vec![layer]);
            let (config, more) = crate::config::validate::validate(merged);

            assert!(diagnostics.is_empty(), "{slug}: {diagnostics:?}");
            assert!(more.is_empty(), "{slug}: {more:?}");
            assert_eq!(config.theme.slug(), slug);
        }
    }
}
