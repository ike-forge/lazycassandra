//! The command line.
//!
//! Hand-rolled rather than `clap`: one flag, and the parse is pure, so it is
//! fast-tier tested against `OsString` arguments the way `discover` is tested
//! against paths (configuration design §5).

use std::ffi::OsString;
use std::path::PathBuf;

pub(crate) const USAGE: &str = "\
lazycassandra — a terminal UI for Apache Cassandra

Usage: lazycassandra [OPTIONS]

Options:
      --config <PATH>  Read this config file, above the user-level and project
                       files rather than instead of them
  -h, --help           Print this help
  -V, --version        Print the version";

/// What the arguments asked for.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Invocation {
    Run {
        config: Option<PathBuf>,
    },
    /// Help or version: print it and exit, having touched no terminal state.
    Print(String),
}

pub(crate) fn parse<I>(args: I) -> Result<Invocation, String>
where
    I: IntoIterator<Item = OsString>,
{
    let mut args = args.into_iter();
    let mut config: Option<PathBuf> = None;

    while let Some(arg) = args.next() {
        // A path is not required to be UTF-8, and a flag is; anything that is
        // not UTF-8 is therefore not a flag, and the separated `--config`
        // form carries its value untouched.
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
            "--config" => {
                let value = args.next().ok_or_else(|| missing("--config"))?;
                set(&mut config, value)?;
            }
            _ if text.starts_with("--config=") => {
                set(&mut config, OsString::from(&text["--config=".len()..]))?;
            }
            _ => return Err(unknown(text)),
        }
    }

    Ok(Invocation::Run { config })
}

/// Repeating a flag is a mistake worth naming: silently taking the last would
/// leave a user looking at settings from a file they can see on their own
/// command line.
fn set(config: &mut Option<PathBuf>, value: OsString) -> Result<(), String> {
    if value.is_empty() {
        return Err(missing("--config"));
    }
    if config.is_some() {
        return Err("`--config` given more than once".to_owned());
    }

    *config = Some(PathBuf::from(value));

    Ok(())
}

fn missing(flag: &str) -> String {
    format!("`{flag}` needs a path\n\n{USAGE}")
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
        Invocation::Run {
            config: config.map(PathBuf::from),
        }
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
        assert!(error.contains("needs a path"), "{error}");
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
