//! The command line. Hand-rolled for two flags, and pure, so fast-tier tested.

use std::ffi::OsString;
use std::path::PathBuf;

pub(crate) const USAGE: &str = "\
lazycassandra — a terminal UI for Apache Cassandra

Usage: lazycassandra [OPTIONS]

Options:
      --config <PATH>      Read this config file, above the user-level and
                           project files rather than instead of them
      --connection <NAME>  Open this connection at startup, instead of asking
  -h, --help               Print this help
  -V, --version            Print the version";

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Invocation {
    Run(Options),
    /// Printed before any terminal state is touched.
    Print(String),
}

/// Unresolved: whether the file or connection exists is the config source's
/// question.
#[derive(Debug, Default, PartialEq, Eq)]
pub(crate) struct Options {
    pub(crate) config: Option<PathBuf>,
    pub(crate) connection: Option<String>,
}

pub(crate) fn parse<I>(args: I) -> Result<Invocation, String>
where
    I: IntoIterator<Item = OsString>,
{
    let mut args = args.into_iter();
    let mut config: Option<PathBuf> = None;
    let mut connection: Option<String> = None;

    while let Some(arg) = args.next() {
        // Flags are UTF-8 and paths need not be, so non-UTF-8 is not a flag; a
        // separated value passes through untouched.
        let Some(text) = arg.to_str() else {
            return Err(unknown(&arg.to_string_lossy()));
        };

        match text {
            "-h" | "--help" => return Ok(Invocation::Print(USAGE.to_owned())),
            "-V" | "--version" => {
                return Ok(Invocation::Print(format!(
                    "lazycassandra {}",
                    env!("CARGO_PKG_VERSION")
                )));
            }
            _ => {
                if let Some(value) = value_of("--config", text, &mut args) {
                    set(&mut config, PathBuf::from(value?), "--config")?;
                } else if let Some(value) = value_of("--connection", text, &mut args) {
                    let name = value?.to_string_lossy().into_owned();
                    set(&mut connection, name, "--connection")?;
                } else {
                    return Err(unknown(text));
                }
            }
        }
    }

    Ok(Invocation::Run(Options { config, connection }))
}

/// `None` when `text` names another flag.
fn value_of(
    flag: &str,
    text: &str,
    args: &mut impl Iterator<Item = OsString>,
) -> Option<Result<OsString, String>> {
    if text == flag {
        return Some(
            args.next()
                .filter(|value| !value.is_empty())
                .ok_or_else(|| missing(flag)),
        );
    }

    let value = text.strip_prefix(flag)?.strip_prefix('=')?;

    Some(if value.is_empty() {
        Err(missing(flag))
    } else {
        Ok(OsString::from(value))
    })
}

/// A repeated flag is an error: taking the last would hide one the user can
/// see.
fn set<T>(slot: &mut Option<T>, value: T, flag: &str) -> Result<(), String> {
    if slot.is_some() {
        return Err(format!("`{flag}` given more than once"));
    }

    *slot = Some(value);

    Ok(())
}

fn missing(flag: &str) -> String {
    format!("`{flag}` needs a value\n\n{USAGE}")
}

fn unknown(arg: &str) -> String {
    format!("unknown argument `{arg}`\n\n{USAGE}")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_args(args: &[&str]) -> Result<Invocation, String> {
        parse(args.iter().map(OsString::from))
    }

    fn run(config: Option<&str>) -> Invocation {
        Invocation::Run(Options {
            config: config.map(PathBuf::from),
            connection: None,
        })
    }

    #[test]
    fn no_arguments_run_the_tool_with_no_named_config() {
        assert_eq!(parse_args(&[]), Ok(run(None)));
    }

    #[test]
    fn a_config_path_is_taken_separated_or_joined() {
        assert_eq!(
            parse_args(&["--config", "/tmp/c.toml"]),
            Ok(run(Some("/tmp/c.toml")))
        );
        assert_eq!(
            parse_args(&["--config=/tmp/c.toml"]),
            Ok(run(Some("/tmp/c.toml")))
        );
    }

    #[test]
    fn a_config_path_with_an_equals_sign_in_it_survives() {
        assert_eq!(
            parse_args(&["--config=/tmp/a=b.toml"]),
            Ok(run(Some("/tmp/a=b.toml")))
        );
    }

    #[test]
    fn a_config_without_a_path_is_an_error_that_shows_the_usage() {
        let error = parse_args(&["--config"]).expect_err("a flag with no value");
        assert!(error.contains("needs a value"), "{error}");
        assert!(error.contains("Usage:"), "{error}");

        assert!(parse_args(&["--config="]).is_err(), "nor an empty one");
    }

    #[test]
    fn a_repeated_config_is_an_error_rather_than_a_silent_last_wins() {
        assert_eq!(
            parse_args(&["--config", "a.toml", "--config", "b.toml"]),
            Err("`--config` given more than once".to_owned())
        );
    }

    #[test]
    fn a_connection_name_is_taken_separated_or_joined() {
        let named = |name: &str| {
            Ok(Invocation::Run(Options {
                config: None,
                connection: Some(name.to_owned()),
            }))
        };

        assert_eq!(parse_args(&["--connection", "prod"]), named("prod"));
        assert_eq!(parse_args(&["--connection=prod"]), named("prod"));
        assert!(parse_args(&["--connection"]).is_err(), "and it needs one");
    }

    #[test]
    fn the_two_flags_are_taken_together() {
        assert_eq!(
            parse_args(&["--config=/tmp/c.toml", "--connection", "prod"]),
            Ok(Invocation::Run(Options {
                config: Some(PathBuf::from("/tmp/c.toml")),
                connection: Some("prod".to_owned()),
            }))
        );
    }

    #[test]
    fn a_flag_the_known_ones_merely_prefix_is_unknown() {
        assert!(
            parse_args(&["--configuration=x"]).is_err(),
            "`--config` prefixes it, and a prefix is not a match"
        );
    }

    #[test]
    fn help_and_version_print_instead_of_running() {
        assert_eq!(parse_args(&["--help"]), Ok(Invocation::Print(USAGE.into())));
        assert_eq!(parse_args(&["-h"]), Ok(Invocation::Print(USAGE.into())));

        let Ok(Invocation::Print(version)) = parse_args(&["-V"]) else {
            panic!("-V prints");
        };
        assert!(version.starts_with("lazycassandra "), "{version}");
    }

    #[test]
    fn an_unknown_argument_is_an_error_rather_than_a_file_name() {
        let error = parse_args(&["keyspace"]).expect_err("no positional arguments exist");
        assert!(error.contains("unknown argument `keyspace`"), "{error}");
    }
}
