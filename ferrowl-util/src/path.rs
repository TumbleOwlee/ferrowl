//! NF-R-042 — shared `~` expansion for user-supplied filesystem paths.

use std::path::{Path, PathBuf};

/// Expand a leading `~` in `path` to the current user's home directory (NF-R-042): a bare `~`
/// expands to the home directory itself, `~/rest` expands to `<home>/rest`. Any other path
/// (including `~otheruser/...`, which no portable std API can resolve) passes through unchanged.
/// If the home directory can't be determined, the path also passes through unchanged rather than
/// erroring.
///
/// Home is resolved via `std::env::home_dir()`, falling back to the `HOME` env var.
pub fn expand(path: &str) -> PathBuf {
    let home = std::env::home_dir().or_else(|| std::env::var_os("HOME").map(PathBuf::from));
    expand_with_home(path, home.as_deref())
}

/// [`expand`] with an injectable home directory, so tests can supply a fake one instead of the
/// process's real `$HOME`.
pub fn expand_with_home(path: &str, home: Option<&Path>) -> PathBuf {
    if let Some(rest) = path.strip_prefix("~/") {
        if let Some(home) = home {
            return home.join(rest);
        }
    } else if path == "~"
        && let Some(home) = home
    {
        return home.to_path_buf();
    }
    PathBuf::from(path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    /// NF-R-042 — a bare `~` expands to the home directory itself.
    fn ut_expand_bare_tilde_with_fake_home() {
        let home = PathBuf::from("/home/x");
        assert_eq!(expand_with_home("~", Some(&home)), home);
    }

    #[test]
    /// NF-R-042 — `~/rest` expands to `<home>/rest`.
    fn ut_expand_tilde_slash_rest_with_fake_home() {
        let home = PathBuf::from("/home/x");
        assert_eq!(
            expand_with_home("~/conf/dev.toml", Some(&home)),
            home.join("conf/dev.toml")
        );
    }

    #[test]
    /// NF-R-052 — `~otheruser/...` is not supported (no portable std API resolves another
    /// user's home directory) and passes through unchanged.
    fn ut_expand_otheruser_tilde_passes_through() {
        let home = PathBuf::from("/home/x");
        assert_eq!(
            expand_with_home("~otheruser/x", Some(&home)),
            PathBuf::from("~otheruser/x")
        );
    }

    #[test]
    /// NF-R-052 — an absolute path not starting with `~` passes through unchanged.
    fn ut_expand_non_tilde_path_passes_through_unchanged() {
        let home = PathBuf::from("/home/x");
        assert_eq!(
            expand_with_home("/abs/path", Some(&home)),
            PathBuf::from("/abs/path")
        );
    }

    #[test]
    /// NF-R-052 — a relative path not starting with `~` passes through unchanged.
    fn ut_expand_relative_non_tilde_path_passes_through_unchanged() {
        let home = PathBuf::from("/home/x");
        assert_eq!(
            expand_with_home("relative/path", Some(&home)),
            PathBuf::from("relative/path")
        );
    }

    #[test]
    /// NF-R-052 — if the home directory can't be determined, a `~`-prefixed path passes through
    /// unchanged rather than erroring.
    fn ut_expand_tilde_with_no_home_passes_through_unchanged() {
        assert_eq!(expand_with_home("~/conf", None), PathBuf::from("~/conf"));
        assert_eq!(expand_with_home("~", None), PathBuf::from("~"));
    }

    #[test]
    /// NF-R-042 — `expand` wires to the real home-resolution fallback chain
    /// (`std::env::home_dir()`, falling back to the `HOME` env var), not just `expand_with_home`.
    fn ut_expand_uses_real_home_dir() {
        let home = std::env::home_dir()
            .or_else(|| std::env::var_os("HOME").map(PathBuf::from))
            .expect("HOME must resolve in test environment");
        assert_eq!(expand("~"), home);
    }
}
