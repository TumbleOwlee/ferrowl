//! NF-R-042 — shared `~` expansion for user-supplied filesystem paths, plus NF-R-069/NF-R-072 —
//! base-directory resolution and relativization for file-relative path fields.

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
pub(crate) fn expand_with_home(path: &str, home: Option<&Path>) -> PathBuf {
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

/// NF-R-069 — the absolute directory that `file`'s file-relative path fields resolve against.
pub fn base_dir_of(file: &str) -> PathBuf {
    base_dir_of_with_cwd(file, std::env::current_dir().ok().as_deref())
}

/// [`base_dir_of`] with an injectable working directory, so tests can supply a fake one instead
/// of the process's real CWD.
pub(crate) fn base_dir_of_with_cwd(file: &str, cwd: Option<&Path>) -> PathBuf {
    let parent = expand(file).parent().unwrap_or(Path::new("")).to_path_buf();
    if parent.is_absolute() {
        return parent;
    }
    match cwd {
        Some(cwd) => cwd.join(parent),
        None => parent,
    }
}

/// NF-R-069 — `path` resolved against `base`. A blank `path` stays blank (CS-R-067).
pub fn resolve_against(base: &Path, path: &str) -> String {
    if path.trim().is_empty() {
        return path.to_string();
    }
    let expanded = expand(path);
    if expanded.is_absolute() {
        return expanded.to_string_lossy().into_owned();
    }
    base.join(expanded).to_string_lossy().into_owned()
}

/// NF-R-072 — `path` re-encoded relative to `base` when it lies under it, else unchanged.
/// Blank stays blank (CS-E-029).
pub fn relativize_under(base: &Path, path: &str) -> String {
    if path.trim().is_empty() {
        return path.to_string();
    }
    match Path::new(path).strip_prefix(base) {
        Ok(rest) => rest.to_string_lossy().into_owned(),
        Err(_) => path.to_string(),
    }
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

    #[test]
    /// NF-R-069 — a relative path is joined onto the base directory.
    fn ut_resolve_against_joins_relative_onto_base() {
        let base = Path::new("/base/dir");
        assert_eq!(
            resolve_against(base, "sub/dev.toml"),
            base.join("sub/dev.toml").to_string_lossy().into_owned()
        );
    }

    #[test]
    /// NF-R-069 — an already-absolute path is left unchanged, base ignored.
    fn ut_resolve_against_leaves_absolute_unchanged() {
        let base = Path::new("/base/dir");
        assert_eq!(resolve_against(base, "/abs/dev.toml"), "/abs/dev.toml");
    }

    #[test]
    /// NF-R-069, NF-R-052 — `~` expansion runs before base-directory resolution, and the expanded
    /// (now absolute) path ignores the base.
    fn ut_resolve_against_tilde_expands_and_ignores_base() {
        let base = Path::new("/base/dir");
        let home = std::env::home_dir()
            .or_else(|| std::env::var_os("HOME").map(PathBuf::from))
            .expect("HOME must resolve in test environment");
        assert_eq!(
            resolve_against(base, "~/dev.toml"),
            home.join("dev.toml").to_string_lossy().into_owned()
        );
    }

    #[test]
    /// NF-R-069 — the base directory of an absolute file is its parent.
    fn ut_base_dir_of_absolute_file_is_its_parent() {
        assert_eq!(base_dir_of("/a/b/session.toml"), PathBuf::from("/a/b"));
    }

    #[test]
    /// NF-R-069 — the base directory of a relative file is its parent prepended with the process
    /// working directory.
    fn ut_base_dir_of_relative_file_prepends_cwd() {
        assert_eq!(
            base_dir_of_with_cwd("sub/session.toml", Some(Path::new("/cwd"))),
            PathBuf::from("/cwd/sub")
        );
    }

    #[test]
    /// NF-R-069 — a bare filename with no directory component resolves to the working directory.
    fn ut_base_dir_of_bare_filename_is_cwd() {
        assert_eq!(
            base_dir_of_with_cwd("session.toml", Some(Path::new("/cwd"))),
            PathBuf::from("/cwd")
        );
    }

    #[test]
    /// NF-R-069 — `resolve_against` on a blank path (CS-R-067) leaves it blank.
    fn ut_resolve_against_blank_stays_blank() {
        assert_eq!(resolve_against(Path::new("/base"), ""), "");
        assert_eq!(resolve_against(Path::new("/base"), "  "), "  ");
    }

    #[test]
    /// NF-R-072 — a path under the base is re-encoded relative to it.
    fn ut_relativize_under_strips_base_prefix() {
        assert_eq!(
            relativize_under(Path::new("/base/dir"), "/base/dir/sub/dev.toml"),
            "sub/dev.toml"
        );
    }

    #[test]
    /// NF-R-072 — a path outside the base is encoded unchanged.
    fn ut_relativize_under_outside_base_stays_unchanged() {
        assert_eq!(
            relativize_under(Path::new("/base/dir"), "/other/dev.toml"),
            "/other/dev.toml"
        );
    }

    #[test]
    /// NF-R-072 — a blank path (CS-E-029) stays blank.
    fn ut_relativize_under_blank_stays_blank() {
        assert_eq!(relativize_under(Path::new("/base"), ""), "");
    }

    #[test]
    /// NF-R-074 — `..` components survive resolution unresolved; no canonicalization.
    fn ut_resolve_against_preserves_dot_dot_components() {
        let base = Path::new("/base/dir");
        assert_eq!(
            resolve_against(base, "../up/dev.toml"),
            base.join("../up/dev.toml").to_string_lossy().into_owned()
        );
    }

    #[test]
    /// NF-R-074 — "lies under the directory" is decided by `Path`'s own component-wise
    /// comparison, never by touching the filesystem: a base spelled with a `.` component still
    /// strips as a prefix of the matching path even though neither exists on disk.
    fn ut_relativize_under_is_lexical_not_filesystem() {
        assert_eq!(
            relativize_under(Path::new("/base/./dir"), "/base/dir/dev.toml"),
            "dev.toml"
        );
    }
}
