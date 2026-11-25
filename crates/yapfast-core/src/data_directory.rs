//! Data-directory compatibility is a lookup, never a migration. Credential
//! service/account IDs are intentionally independent of the desktop app ID.

#[cfg(any(windows, test))]
use std::path::Path;
use std::path::PathBuf;

#[cfg(any(windows, test))]
pub(crate) fn windows(local: &Path) -> PathBuf {
    // Matches the installed Harmony launcher's explicit --data-dir.
    local.join("Harmony").join("data")
}

#[cfg(any(target_os = "linux", test))]
pub(crate) fn preferred_or_legacy(
    preferred: PathBuf,
    legacy: Option<PathBuf>,
) -> std::io::Result<PathBuf> {
    // One exact predecessor path, no scanning or copying. An inaccessible new
    // directory is an error, not permission to reopen a different account.
    if !preferred.try_exists()?
        && let Some(legacy) = legacy
        && legacy.is_dir()
    {
        return Ok(legacy);
    }
    Ok(preferred)
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Fixture(PathBuf);
    impl Fixture {
        fn new() -> Self {
            let root = std::env::temp_dir().join(format!(
                "harmony-data-directory-{}-{}",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_nanos()
            ));
            std::fs::create_dir(&root).unwrap();
            Self(root)
        }
        fn path(&self, name: &str) -> PathBuf {
            self.0.join(name)
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn windows_matches_installed_data_directory() {
        assert_eq!(
            windows(Path::new("local-app-data")),
            Path::new("local-app-data").join("Harmony").join("data")
        );
    }

    #[test]
    fn uses_harmony_for_a_new_install_without_creating_it() {
        let fixture = Fixture::new();
        let preferred = fixture.path("harmony");
        assert_eq!(
            preferred_or_legacy(preferred.clone(), Some(fixture.path("yapfast"))).unwrap(),
            preferred
        );
        assert!(!preferred.exists());
    }

    #[test]
    fn existing_harmony_wins_even_when_legacy_exists() {
        let fixture = Fixture::new();
        let preferred = fixture.path("harmony");
        let legacy = fixture.path("yapfast");
        std::fs::create_dir(&preferred).unwrap();
        std::fs::create_dir(&legacy).unwrap();
        assert_eq!(
            preferred_or_legacy(preferred.clone(), Some(legacy)).unwrap(),
            preferred
        );
    }

    #[test]
    fn reuses_only_the_existing_legacy_directory_without_moving_it() {
        let fixture = Fixture::new();
        let preferred = fixture.path("harmony");
        let legacy = fixture.path("yapfast");
        std::fs::create_dir(&legacy).unwrap();
        std::fs::write(legacy.join("sentinel"), b"existing data").unwrap();
        assert_eq!(
            preferred_or_legacy(preferred.clone(), Some(legacy.clone())).unwrap(),
            legacy
        );
        assert!(!preferred.exists());
        assert_eq!(
            std::fs::read(legacy.join("sentinel")).unwrap(),
            b"existing data"
        );
    }

    #[test]
    fn a_legacy_file_is_not_a_data_directory() {
        let fixture = Fixture::new();
        let preferred = fixture.path("harmony");
        let legacy = fixture.path("yapfast");
        std::fs::write(&legacy, b"not a directory").unwrap();
        assert_eq!(
            preferred_or_legacy(preferred.clone(), Some(legacy)).unwrap(),
            preferred
        );
    }

    #[test]
    fn a_harmony_file_does_not_allow_fallback_to_another_account() {
        let fixture = Fixture::new();
        let preferred = fixture.path("harmony");
        let legacy = fixture.path("yapfast");
        std::fs::write(&preferred, b"not a directory").unwrap();
        std::fs::create_dir(&legacy).unwrap();
        assert_eq!(
            preferred_or_legacy(preferred.clone(), Some(legacy)).unwrap(),
            preferred
        );
    }

    #[test]
    fn no_legacy_identity_still_selects_harmony() {
        let fixture = Fixture::new();
        let preferred = fixture.path("harmony");
        assert_eq!(
            preferred_or_legacy(preferred.clone(), None).unwrap(),
            preferred
        );
    }
}
