static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

#[derive(Debug)]
pub struct TempDirGuard {
    path: std::path::PathBuf,
}

/// NF-R-044 — creates `<temp>/<prefix>_<pid>_<counter>`, unique per run.
/// Creation is exclusive: a colliding directory (e.g. left behind by an
/// earlier process whose pid has since been reused) is skipped, never
/// adopted and never emptied, and the counter advances to the next value.
/// Any other failure to create the directory panics rather than handing
/// back a directory this call did not create.
pub fn reserve_temp_dir(prefix: &str) -> TempDirGuard {
    loop {
        let n = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!("{prefix}_{}_{n}", std::process::id()));
        match std::fs::create_dir(&path) {
            Ok(()) => return TempDirGuard { path },
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(e) => panic!("creating temp dir {path:?} failed: {e}"),
        }
    }
}

impl TempDirGuard {
    pub fn path(&self) -> &std::path::Path {
        &self.path
    }

    pub fn join(&self, name: impl AsRef<std::path::Path>) -> std::path::PathBuf {
        self.path.join(name)
    }
}

impl Drop for TempDirGuard {
    /// A cleanup race (directory already gone) must not fail an otherwise
    /// passing test, so the removal result is discarded.
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// NF-R-044 — `reserve_temp_dir` creates a directory whose name carries
    /// the current process id, so concurrent runs cannot collide.
    #[test]
    fn ut_reserve_temp_dir_creates_unique_dir() {
        let guard = reserve_temp_dir("ferrowl_test_support_ut");
        assert!(guard.path().is_dir());
        let pid = std::process::id().to_string();
        assert!(guard.path().to_string_lossy().contains(&pid));
    }

    /// NF-R-044 — two guards created in the same process get different
    /// paths, via the monotonic per-process counter.
    #[test]
    fn ut_reserve_temp_dir_two_guards_differ() {
        let a = reserve_temp_dir("ferrowl_test_support_ut");
        let b = reserve_temp_dir("ferrowl_test_support_ut");
        assert_ne!(a.path(), b.path());
    }

    /// NF-R-044 — `join` builds a path under the guard's own directory, not
    /// the shared temp root.
    #[test]
    fn ut_temp_dir_join_is_under_path() {
        let guard = reserve_temp_dir("ferrowl_test_support_ut");
        assert!(guard.join("a.toml").starts_with(guard.path()));
    }

    /// NF-R-044 — dropping the guard removes the directory and its
    /// contents.
    #[test]
    fn ut_temp_dir_drop_removes_contents() {
        let guard = reserve_temp_dir("ferrowl_test_support_ut");
        std::fs::write(guard.join("a.toml"), b"x").unwrap();
        let path = guard.path().to_path_buf();
        drop(guard);
        assert!(!path.exists());
    }

    /// NF-R-044 — a cleanup race (directory already gone) must not panic on
    /// drop.
    #[test]
    fn ut_temp_dir_drop_ignores_missing_dir() {
        let guard = reserve_temp_dir("ferrowl_test_support_ut");
        std::fs::remove_dir_all(guard.path()).unwrap();
        drop(guard);
    }

    /// Removes a set of hand-created pre-collision directories on drop, so a
    /// panic partway through creating or asserting on them in
    /// `ut_reserve_temp_dir_skips_existing_dir` still cleans up on unwind
    /// instead of leaking directories and marker files under the shared
    /// temp root.
    struct PreCreatedDirs(Vec<std::path::PathBuf>);

    impl PreCreatedDirs {
        fn push(&mut self, path: std::path::PathBuf) {
            self.0.push(path);
        }
    }

    impl Drop for PreCreatedDirs {
        fn drop(&mut self) {
            for path in &self.0 {
                let _ = std::fs::remove_file(path.join("marker"));
                let _ = std::fs::remove_dir(path);
            }
        }
    }

    /// NF-R-044 — a directory colliding with a derived path is skipped, never
    /// adopted or emptied: the guard retries with the next counter value and
    /// every pre-created directory survives untouched.
    #[test]
    fn ut_reserve_temp_dir_skips_existing_dir() {
        const K: u64 = 64;
        let prefix = "ferrowl_test_support_ut_skip";
        let probe = reserve_temp_dir(prefix);
        let file_name = probe
            .path()
            .file_name()
            .unwrap()
            .to_string_lossy()
            .into_owned();
        let n: u64 = file_name.rsplit('_').next().unwrap().parse().unwrap();
        drop(probe);

        let pid = std::process::id();
        let mut pre_created = PreCreatedDirs(Vec::with_capacity(K as usize));
        for i in (n + 1)..=(n + K) {
            let path = std::env::temp_dir().join(format!("{prefix}_{pid}_{i}"));
            match std::fs::create_dir(&path) {
                Ok(()) => {}
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
                Err(e) => panic!("pre-creating {path:?} failed: {e}"),
            }
            pre_created.push(path.clone());
            std::fs::write(path.join("marker"), b"x").unwrap();
        }

        let guard = reserve_temp_dir(prefix);
        assert!(!pre_created.0.contains(&guard.path().to_path_buf()));
        assert!(guard.path().is_dir());
        for path in &pre_created.0 {
            assert!(path.is_dir(), "{path:?} should still exist");
            assert!(
                path.join("marker").is_file(),
                "{path:?} marker should survive"
            );
        }
    }
}
