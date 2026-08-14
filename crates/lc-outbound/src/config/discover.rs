//! Where the two files are.
//!
//! Split so the logic is fast-tier testable: `global_path` and
//! `local_candidates` are pure, and `first_existing` is the only thing that
//! touches a disk and has no logic worth testing (design §12).

use std::path::{Path, PathBuf};

const GLOBAL: &str = "lazycassandra/config.toml";
const LOCAL: &str = ".lazycassandra.toml";

/// `$XDG_CONFIG_HOME` if set, else `$HOME/.config`.
///
/// Resolved with `std::env` rather than a `dirs`-style crate: ten lines
/// against a dependency whose value is Windows support this project does not
/// target (design §3).
pub(crate) fn global_path(xdg: Option<&Path>, home: Option<&Path>) -> Option<PathBuf> {
    xdg.map(|dir| dir.join(GLOBAL))
        .or_else(|| home.map(|dir| dir.join(".config").join(GLOBAL)))
}

/// Every `.lazycassandra.toml` that could apply, nearest first.
pub(crate) fn local_candidates(cwd: &Path) -> Vec<PathBuf> {
    cwd.ancestors().map(|dir| dir.join(LOCAL)).collect()
}

/// The first candidate that is there. Taking only the first is what keeps the
/// model at two layers, which is what keeps the merge rule short enough to
/// hold in the head (design §3).
pub(crate) fn first_existing(candidates: &[PathBuf]) -> Option<&PathBuf> {
    candidates.iter().find(|path| path.is_file())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_global_path_prefers_xdg_over_home() {
        assert_eq!(
            global_path(Some(Path::new("/x")), Some(Path::new("/h"))),
            Some(PathBuf::from("/x/lazycassandra/config.toml"))
        );
        assert_eq!(
            global_path(None, Some(Path::new("/h"))),
            Some(PathBuf::from("/h/.config/lazycassandra/config.toml"))
        );
        assert_eq!(global_path(None, None), None);
    }

    #[test]
    fn the_local_candidates_walk_up_from_the_working_directory_to_the_root() {
        assert_eq!(
            local_candidates(Path::new("/a/b/c")),
            vec![
                PathBuf::from("/a/b/c/.lazycassandra.toml"),
                PathBuf::from("/a/b/.lazycassandra.toml"),
                PathBuf::from("/a/.lazycassandra.toml"),
                PathBuf::from("/.lazycassandra.toml"),
            ],
            "running from a subdirectory of a repository must behave as running from its root"
        );
    }

    #[test]
    fn a_relative_working_directory_still_yields_its_own_candidate() {
        assert_eq!(
            local_candidates(Path::new("")),
            vec![PathBuf::from(".lazycassandra.toml")]
        );
    }
}
