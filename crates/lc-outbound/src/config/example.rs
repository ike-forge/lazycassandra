//! The annotated example written on first run. Authored, not generated, for the
//! comments; `the_example_states_the_defaults_it_claims_to` keeps the values
//! honest.

use std::fs::File;
use std::io::Write;
use std::path::Path;

pub(crate) const EXAMPLE: &str = include_str!("example.toml");

/// `create_new` checks and creates in one syscall and refuses a pre-planted
/// symlink.
pub(crate) fn write_if_absent(path: &Path) {
    if let Some(parent) = path.parent() {
        let _ignored = std::fs::create_dir_all(parent);
    }

    let _ignored = File::options()
        .write(true)
        .create_new(true)
        .open(path)
        .and_then(|mut file| file.write_all(EXAMPLE.as_bytes()));
}

#[cfg(test)]
mod tests {
    use lc_domain::config::{Config, Diagnostic, Origin};

    use super::*;

    fn load(source: &str) -> (Config, Vec<Diagnostic>) {
        let origin = Origin::new("example.toml");
        let (layer, mut diagnostics) = crate::config::parse(source, &origin);
        let layer = layer.expect("the example is a valid TOML file");
        let (merged, more) = crate::config::merge::merge(vec![layer]);
        diagnostics.extend(more);
        let (config, more) = crate::config::validate::validate(merged);
        diagnostics.extend(more);

        (config, diagnostics)
    }

    /// Tells settings from prose and continued comments.
    fn is_setting(text: &str) -> bool {
        text.starts_with('[')
            || text.split_once('=').is_some_and(|(key, _)| {
                let key = key.trim();
                !key.is_empty() && key.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
            })
    }

    /// Every setting uncommented, prose left alone.
    fn uncommented(source: &str) -> String {
        source
            .lines()
            .map(|line| match line.trim_start().strip_prefix("# ") {
                Some(rest) if is_setting(rest) => rest.to_owned(),
                _ => line.to_owned(),
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    #[test]
    fn the_example_states_the_defaults_it_claims_to() {
        let (config, diagnostics) = load(EXAMPLE);

        assert_eq!(
            diagnostics,
            Vec::new(),
            "the file written on first run must not report anything on the run that reads it"
        );
        assert_eq!(
            config,
            Config::default(),
            "every line is commented, so the example must contribute nothing"
        );
    }

    #[test]
    fn uncommenting_the_whole_example_yields_a_valid_config() {
        let (config, diagnostics) = load(&uncommented(EXAMPLE));

        assert_eq!(
            diagnostics,
            Vec::new(),
            "a key spelt wrong, a value out of range or a section referring to one that is not \
             there would surface here"
        );
        assert_ne!(
            config,
            Config::default(),
            "if this matches the default the uncommenter stopped working and the test above it \
             is the only one still doing anything"
        );
        assert_eq!(
            config.connections.len(),
            2,
            "both credential shapes are shown, and each is a whole entry: `cqlshrc` and \
             `username`/`password` cannot share one"
        );
        assert_eq!(config.themes.len(), 1);
        assert!(
            config.default_connection.is_some(),
            "`default_connection` names the connection the example declares, which is what lets \
             this test assert on zero diagnostics"
        );

        let default = Config::default();
        let mismatch = "example.toml claims this is the default but the type disagrees — \
                         update the comment in example.toml to match";
        assert_eq!(config.theme, default.theme, "{mismatch}");
        assert_eq!(config.background, default.background, "{mismatch}");
        assert_eq!(config.icons, default.icons, "{mismatch}");
        assert_eq!(config.mouse, default.mouse, "{mismatch}");
        assert_eq!(config.layout, default.layout, "{mismatch}");
        assert_eq!(config.grid, default.grid, "{mismatch}");
        assert_eq!(config.history, default.history, "{mismatch}");
        assert_eq!(config.driver, default.driver, "{mismatch}");
    }

    /// By hand: `serde` cannot enumerate fields. An undocumented new key fails
    /// here.
    const KEYS: &[&str] = &[
        "theme",
        "background",
        "icons",
        "mouse",
        "default_connection",
        "[layout]",
        "sidebar",
        "ddl",
        "queries",
        "[grid]",
        "max_column_width",
        "page_rows",
        "[history]",
        "limit",
        "[driver]",
        "page_size",
        "request_timeout_ms",
        "connect_timeout_ms",
        "consistency",
        "compression",
        "[export]",
        "path",
        "prefix",
        "[[connections]]",
        "name",
        "hosts",
        "posture",
        "datacenter",
        "username",
        "password",
        "{ env = ",
        "{ file = ",
        "tls",
        "ca",
        "cert",
        "verify",
        "server_name",
        "[[themes]]",
        "extends",
        "[themes.colors]",
    ];

    #[test]
    fn every_key_the_schema_accepts_is_in_the_example() {
        for key in KEYS {
            assert!(
                EXAMPLE.contains(key),
                "`{key}` is accepted by wire.rs and undocumented in the example"
            );
        }
    }

    /// From the example's own list: `table` and `comment` are ordinary words.
    fn documented_roles(source: &str) -> Vec<&str> {
        let list = source.split_once("The roles:").map_or("", |(_, rest)| rest);
        let list = list.split_once('.').map_or(list, |(before, _)| before);

        list.split(',')
            .map(|role| role.trim_matches(|c: char| c.is_whitespace() || c == '#'))
            .collect()
    }

    #[test]
    fn every_theme_and_role_is_named_in_the_example() {
        for built_in in lc_domain::theme::BuiltIn::ALL {
            assert!(
                EXAMPLE.contains(built_in.slug()),
                "the built-in theme `{}` is missing from the `theme` comment",
                built_in.slug()
            );
        }

        let expected: Vec<&str> = lc_domain::theme::Role::ALL
            .iter()
            .map(|role| role.key())
            .collect();

        assert_eq!(
            documented_roles(EXAMPLE),
            expected,
            "the roles list must name every role, in the order `Role::ALL` declares them"
        );
    }
}
