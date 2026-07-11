//! Pure filesystem path-completion helpers used by the path-suggestion overlay.

use std::path::{Path, PathBuf};

use ferrowl_ui::traits::{Suggestion, SuggestionProvider};

/// Maximum number of suggestions returned by [`complete_path`].
pub const MAX_SUGGESTIONS: usize = 12;

/// Expand a leading `~/` (or bare `~`) in `input` to `home`, for use as a directory to read.
///
/// `home` is injected so callers can supply a fake value in tests; production callers should
/// pass the real home directory (e.g. from `std::env::home_dir` or the `HOME` env var).
fn expand_tilde(input: &str, home: Option<&Path>) -> PathBuf {
    if let Some(rest) = input.strip_prefix("~/") {
        if let Some(home) = home {
            return home.join(rest);
        }
    } else if input == "~"
        && let Some(home) = home
    {
        return home.to_path_buf();
    }
    PathBuf::from(input)
}

/// Complete a partially typed path.
///
/// Returns `(completed path in the user's prefix form, is_dir)` pairs, directories first, then
/// files, each group sorted alphabetically, capped at `max` entries.
///
/// Filesystem errors (e.g., missing directory, permission denied) are silently treated as
/// "no suggestions" rather than surfaced as errors. This is intentional: callers implement
/// the [`SuggestionProvider`] trait, which has no error channel (only returns `Vec<Suggestion>`),
/// and autocomplete widgets must never interrupt user typing with error dialogs.
pub fn complete_path(input: &str, extensions: Option<&[&str]>, max: usize) -> Vec<(String, bool)> {
    let (dir_part, partial) = match input.rfind('/') {
        Some(idx) => (&input[..=idx], &input[idx + 1..]),
        None => ("", input),
    };

    let read_dir_path = if dir_part.is_empty() {
        PathBuf::from(".")
    } else {
        let home = std::env::home_dir().or_else(|| std::env::var_os("HOME").map(PathBuf::from));
        expand_tilde(dir_part, home.as_deref())
    };

    let entries = match std::fs::read_dir(&read_dir_path) {
        Ok(e) => e,
        // Directory doesn't exist or isn't readable: intentional silent fallback to no suggestions.
        // See function doc comment for rationale.
        Err(_) => return Vec::new(),
    };

    let show_hidden = partial.starts_with('.');

    let mut dirs: Vec<String> = Vec::new();
    let mut files: Vec<String> = Vec::new();

    for entry in entries.flatten() {
        let name = entry.file_name();
        let name = match name.to_str() {
            Some(n) => n,
            None => continue,
        };

        if name.starts_with('.') && !show_hidden {
            continue;
        }
        if !name.starts_with(partial) {
            continue;
        }

        let file_type = match entry.file_type() {
            Ok(ft) => ft,
            Err(_) => continue,
        };

        if file_type.is_dir() {
            dirs.push(format!("{}{}/", dir_part, name));
        } else if file_type.is_file() {
            if let Some(exts) = extensions {
                let matches_ext = Path::new(name)
                    .extension()
                    .and_then(|e| e.to_str())
                    .map(|e| exts.iter().any(|allowed| allowed.eq_ignore_ascii_case(e)))
                    .unwrap_or(false);
                if !matches_ext {
                    continue;
                }
            }
            files.push(format!("{}{}", dir_part, name));
        }
    }

    dirs.sort();
    files.sort();

    dirs.into_iter()
        .map(|p| (p, true))
        .chain(files.into_iter().map(|p| (p, false)))
        .take(max)
        .collect()
}

/// Filesystem-backed [`SuggestionProvider`] wrapping [`complete_path`].
#[derive(Debug, Clone, Default)]
pub struct FsPathProvider {
    extensions: Option<Vec<&'static str>>,
}

impl FsPathProvider {
    /// Restrict file suggestions (directories are always shown) to the given extensions.
    pub fn with_extensions(extensions: &'static [&'static str]) -> Self {
        Self {
            extensions: Some(extensions.to_vec()),
        }
    }
}

impl SuggestionProvider for FsPathProvider {
    fn suggest(&self, input: &str) -> Vec<Suggestion> {
        let extensions = self.extensions.as_deref();
        complete_path(input, extensions, MAX_SUGGESTIONS)
            .into_iter()
            .map(|(value, is_dir)| {
                let trimmed = value.strip_suffix('/').unwrap_or(&value);
                let last = trimmed.rsplit('/').next().unwrap_or(trimmed);
                let label = if is_dir {
                    format!("{}/", last)
                } else {
                    last.to_string()
                };
                Suggestion {
                    value,
                    label,
                    partial: is_dir,
                }
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn tmp(name: &str) -> PathBuf {
        std::env::temp_dir().join(name)
    }

    fn cleanup(dir: &Path) {
        let _ = fs::remove_dir_all(dir);
    }

    /// Build a temp dir tree:
    ///   <root>/configs/          (dir)
    ///   <root>/config.toml       (file)
    ///   <root>/config.json       (file)
    ///   <root>/notes.txt         (file)
    ///   <root>/.hidden.toml      (file)
    ///   <root>/.hiddendir/       (dir)
    ///   <root>/sub/configs/      (dir)
    fn setup(root_name: &str) -> PathBuf {
        let root = tmp(root_name);
        cleanup(&root);
        fs::create_dir_all(root.join("configs")).unwrap();
        fs::create_dir_all(root.join("sub/configs")).unwrap();
        fs::create_dir_all(root.join(".hiddendir")).unwrap();
        fs::write(root.join("config.toml"), "").unwrap();
        fs::write(root.join("config.json"), "").unwrap();
        fs::write(root.join("notes.txt"), "").unwrap();
        fs::write(root.join(".hidden.toml"), "").unwrap();
        root
    }

    #[test]
    fn ut_prefix_filtering() {
        let root = setup("ferrowl_pathsuggest_prefix");
        let dir_prefix = format!("{}/", root.to_string_lossy());
        let input = format!("{}conf", dir_prefix);
        let results = complete_path(&input, None, MAX_SUGGESTIONS);
        let names: Vec<&str> = results.iter().map(|(p, _)| p.as_str()).collect();
        assert!(names.contains(&format!("{}configs/", dir_prefix).as_str()));
        assert!(names.contains(&format!("{}config.toml", dir_prefix).as_str()));
        assert!(names.contains(&format!("{}config.json", dir_prefix).as_str()));
        assert!(!names.iter().any(|n| n.ends_with("notes.txt")));
        cleanup(&root);
    }

    #[test]
    fn ut_trailing_slash_on_dirs() {
        let root = setup("ferrowl_pathsuggest_slash");
        let dir_prefix = format!("{}/", root.to_string_lossy());
        let input = format!("{}configs", dir_prefix);
        let results = complete_path(&input, None, MAX_SUGGESTIONS);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].0, format!("{}configs/", dir_prefix));
        assert!(results[0].1);
        cleanup(&root);
    }

    #[test]
    fn ut_hidden_excluded_by_default() {
        let root = setup("ferrowl_pathsuggest_hidden_excl");
        let dir_prefix = format!("{}/", root.to_string_lossy());
        let input = dir_prefix.clone();
        let results = complete_path(&input, None, MAX_SUGGESTIONS);
        let names: Vec<&str> = results.iter().map(|(p, _)| p.as_str()).collect();
        assert!(!names.iter().any(|n| n.contains(".hidden")));
        cleanup(&root);
    }

    #[test]
    fn ut_hidden_included_when_typed() {
        let root = setup("ferrowl_pathsuggest_hidden_incl");
        let dir_prefix = format!("{}/", root.to_string_lossy());
        let input = format!("{}.", dir_prefix);
        let results = complete_path(&input, None, MAX_SUGGESTIONS);
        let names: Vec<&str> = results.iter().map(|(p, _)| p.as_str()).collect();
        assert!(names.contains(&format!("{}.hiddendir/", dir_prefix).as_str()));
        assert!(names.contains(&format!("{}.hidden.toml", dir_prefix).as_str()));
        cleanup(&root);
    }

    #[test]
    fn ut_extension_filter_keeps_dirs() {
        let root = setup("ferrowl_pathsuggest_ext");
        let dir_prefix = format!("{}/", root.to_string_lossy());
        let input = dir_prefix.clone();
        let results = complete_path(&input, Some(&["toml"]), MAX_SUGGESTIONS);
        let names: Vec<&str> = results.iter().map(|(p, _)| p.as_str()).collect();
        // dirs always pass the filter
        assert!(names.contains(&format!("{}configs/", dir_prefix).as_str()));
        assert!(names.contains(&format!("{}sub/", dir_prefix).as_str()));
        // files: only .toml
        assert!(names.contains(&format!("{}config.toml", dir_prefix).as_str()));
        assert!(!names.iter().any(|n| n.ends_with("config.json")));
        assert!(!names.iter().any(|n| n.ends_with("notes.txt")));
        cleanup(&root);
    }

    #[test]
    fn ut_missing_dir_returns_empty() {
        let results = complete_path("/no/such/ferrowl/dir/foo", None, MAX_SUGGESTIONS);
        assert!(results.is_empty());
    }

    #[test]
    fn ut_tilde_expansion_with_fake_home() {
        let root = setup("ferrowl_pathsuggest_tilde");
        let expanded = expand_tilde("~/conf", Some(&root));
        assert_eq!(expanded, root.join("conf"));

        let expanded_bare = expand_tilde("~", Some(&root));
        assert_eq!(expanded_bare, root);
        cleanup(&root);
    }

    #[test]
    fn ut_result_cap() {
        let root = tmp("ferrowl_pathsuggest_cap");
        cleanup(&root);
        fs::create_dir_all(&root).unwrap();
        for i in 0..20 {
            fs::write(root.join(format!("file{:02}.txt", i)), "").unwrap();
        }
        let input = format!("{}/", root.to_string_lossy());
        let results = complete_path(&input, None, MAX_SUGGESTIONS);
        assert_eq!(results.len(), MAX_SUGGESTIONS);
        cleanup(&root);
    }

    // Guards tests that temporarily change the process cwd, so they don't race each other
    // within this test binary.
    static CWD_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    #[test]
    fn ut_empty_input_lists_current_dir_without_prefix() {
        let _guard = CWD_LOCK.lock().unwrap();
        let original_cwd = std::env::current_dir().unwrap();

        let root = tmp("ferrowl_pathsuggest_empty_input");
        cleanup(&root);
        fs::create_dir_all(root.join("marker_dir")).unwrap();

        std::env::set_current_dir(&root).unwrap();
        let result = std::panic::catch_unwind(|| {
            // With empty dir part, results should not be prefixed with "./".
            let results = complete_path("marker", None, MAX_SUGGESTIONS);
            let names: Vec<&str> = results.iter().map(|(p, _)| p.as_str()).collect();
            assert!(names.contains(&"marker_dir/"));
            assert!(!names.iter().any(|n| n.starts_with("./")));
        });
        std::env::set_current_dir(&original_cwd).unwrap();
        cleanup(&root);

        result.unwrap();
    }

    #[test]
    fn ut_provider_dir_partial_and_trailing_slash_label() {
        let root = setup("ferrowl_pathsuggest_provider_dir");
        let dir_prefix = format!("{}/", root.to_string_lossy());
        let provider = FsPathProvider::default();
        let input = format!("{}sub/conf", dir_prefix);
        let results = provider.suggest(&input);
        let hit = results
            .iter()
            .find(|s| s.value == format!("{}sub/configs/", dir_prefix))
            .expect("expected sub/configs/ suggestion");
        assert_eq!(hit.label, "configs/");
        assert!(hit.partial);
        cleanup(&root);
    }

    #[test]
    fn ut_provider_file_label_is_last_component() {
        let root = setup("ferrowl_pathsuggest_provider_file");
        let dir_prefix = format!("{}/", root.to_string_lossy());
        let provider = FsPathProvider::default();
        let input = format!("{}config.toml", dir_prefix);
        let results = provider.suggest(&input);
        let hit = results
            .iter()
            .find(|s| s.value == format!("{}config.toml", dir_prefix))
            .expect("expected config.toml suggestion");
        assert_eq!(hit.label, "config.toml");
        assert!(!hit.partial);
        cleanup(&root);
    }

    #[test]
    fn ut_provider_extension_filter_honored() {
        let root = setup("ferrowl_pathsuggest_provider_ext");
        let dir_prefix = format!("{}/", root.to_string_lossy());
        let provider = FsPathProvider::with_extensions(&["toml"]);
        let results = provider.suggest(&dir_prefix);
        let values: Vec<&str> = results.iter().map(|s| s.value.as_str()).collect();
        assert!(values.contains(&format!("{}config.toml", dir_prefix).as_str()));
        assert!(!values.iter().any(|v| v.ends_with("config.json")));
        assert!(!values.iter().any(|v| v.ends_with("notes.txt")));
        cleanup(&root);
    }

    #[test]
    fn ut_provider_missing_dir_returns_empty() {
        let provider = FsPathProvider::default();
        let results = provider.suggest("/no/such/ferrowl/dir/foo");
        assert!(results.is_empty());
    }
}
