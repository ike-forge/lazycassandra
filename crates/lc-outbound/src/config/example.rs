//! The annotated example written on first run.
//!
//! Authored rather than generated: the values in it could be derived from the
//! types, but the comments could not, and the comments are the reason the file
//! exists (example config design §3). What generation would have guaranteed —
//! that every stated default is the real default — is bought back by
//! `the_example_states_the_defaults_it_claims_to`.

// The writer that reads this lands separately (design §4); until then only
// the tests below use it.
#[cfg(test)]
pub(crate) const EXAMPLE: &str = include_str!("example.toml");

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

    /// A line stating a setting, as opposed to prose or a trailing comment
    /// continued onto its own line. Both forms have to be told apart for
    /// `uncommented` to produce a file rather than a mess.
    fn is_setting(text: &str) -> bool {
        text.starts_with('[')
            || text.split_once('=').is_some_and(|(key, _)| {
                let key = key.trim();
                !key.is_empty() && key.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
            })
    }

    /// The example with every setting turned on and every prose comment left
    /// alone — what a user gets by deleting the `#` in front of everything.
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
        assert_eq!(config.connections.len(), 1);
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
        assert_eq!(config.driver, default.driver, "{mismatch}");
    }

    /// Every key `wire.rs` declares. Listed by hand rather than derived:
    /// `serde` gives no way to enumerate a struct's fields, and a list that
    /// has to be edited alongside the struct is the point — adding a field
    /// without documenting it fails here.
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
        "row_limit",
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

    #[test]
    fn every_theme_and_role_is_named_in_the_example() {
        for built_in in lc_domain::theme::BuiltIn::ALL {
            assert!(
                EXAMPLE.contains(built_in.slug()),
                "the built-in theme `{}` is missing from the `theme` comment",
                built_in.slug()
            );
        }

        for role in lc_domain::theme::Role::ALL {
            assert!(
                EXAMPLE.contains(role.key()),
                "the role `{}` is missing from the roles list",
                role.key()
            );
        }
    }
}
