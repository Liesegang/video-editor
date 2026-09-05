//! Host-level exclusion for logical export destinations.
//!
//! Exporters write host-owned staging paths, so their session maps cannot
//! protect the final user-selected destination. This registry reserves that
//! logical destination independently and releases it through an RAII lease.

use std::sync::{Arc, Mutex, MutexGuard};

use uuid::Uuid;

use crate::error::LibraryError;
use crate::util::output_path_identity::{OutputPathIdentity, output_path_identity};

#[derive(Clone, Default)]
pub(super) struct ExportDestinationLeaseRegistry {
    entries: Arc<Mutex<Vec<LeaseEntry>>>,
}

#[derive(Debug)]
struct LeaseEntry {
    id: Uuid,
    logical_path: String,
    identity: OutputPathIdentity,
}

/// Exclusive ownership of one logical export destination.
///
/// Dropping the value releases the reservation. It is intentionally not
/// cloneable: one export coordinator owns one lease lifetime.
#[must_use = "dropping an export destination lease immediately releases its reservation"]
#[derive(Debug)]
pub(crate) struct ExportDestinationLease {
    id: Uuid,
    entries: Arc<Mutex<Vec<LeaseEntry>>>,
}

impl ExportDestinationLeaseRegistry {
    pub(super) fn reserve(
        &self,
        logical_path: &str,
    ) -> Result<ExportDestinationLease, LibraryError> {
        let requested = output_path_identity(logical_path)?;
        let mut entries = lock_entries(&self.entries);
        for entry in entries.iter_mut() {
            entry.identity.refresh_existing_file();
            if same_logical_locator(&entry.logical_path, logical_path)
                || entry.identity.aliases(&requested)
            {
                return Err(LibraryError::Render(format!(
                    "export destination '{logical_path}' is already reserved by an active export targeting '{}'",
                    entry.logical_path
                )));
            }
        }

        let id = Uuid::new_v4();
        entries.push(LeaseEntry {
            id,
            logical_path: logical_path.to_string(),
            identity: requested,
        });
        Ok(ExportDestinationLease {
            id,
            entries: Arc::clone(&self.entries),
        })
    }
}

#[cfg(windows)]
fn same_logical_locator(left: &str, right: &str) -> bool {
    left.to_lowercase() == right.to_lowercase()
}

#[cfg(not(windows))]
fn same_logical_locator(left: &str, right: &str) -> bool {
    left == right
}

impl Drop for ExportDestinationLease {
    fn drop(&mut self) {
        let mut entries = lock_entries(&self.entries);
        if let Some(index) = entries.iter().position(|entry| entry.id == self.id) {
            entries.swap_remove(index);
        }
    }
}

fn lock_entries(entries: &Mutex<Vec<LeaseEntry>>) -> MutexGuard<'_, Vec<LeaseEntry>> {
    entries.lock().unwrap_or_else(|poisoned| {
        log::error!(
            "export destination lease registry lock was poisoned; recovering committed reservations"
        );
        poisoned.into_inner()
    })
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::Path;

    use super::ExportDestinationLeaseRegistry;

    fn path_string(path: &Path) -> String {
        path.to_string_lossy().into_owned()
    }

    #[test]
    fn exact_destination_is_exclusive_until_lease_drop() {
        let directory = tempfile::tempdir().unwrap();
        let destination = path_string(&directory.path().join("render.mp4"));
        let registry = ExportDestinationLeaseRegistry::default();
        let lease = registry.reserve(&destination).unwrap();

        let error = registry.reserve(&destination).unwrap_err();
        assert!(error.to_string().contains("already reserved"));
        assert!(error.to_string().contains(&destination));

        drop(lease);
        let _replacement = registry.reserve(&destination).unwrap();
    }

    #[test]
    fn relative_and_absolute_spellings_share_one_reservation() {
        let current = std::env::current_dir().unwrap();
        let directory = tempfile::tempdir_in(&current).unwrap();
        let absolute = directory.path().join("render.mp4");
        let relative = absolute.strip_prefix(&current).unwrap();
        let registry = ExportDestinationLeaseRegistry::default();
        let _lease = registry.reserve(&path_string(relative)).unwrap();

        assert!(
            registry
                .reserve(&path_string(&absolute))
                .unwrap_err()
                .to_string()
                .contains("already reserved")
        );
    }

    #[cfg(any(unix, windows))]
    #[test]
    fn hard_link_aliases_share_one_reservation() {
        let directory = tempfile::tempdir().unwrap();
        let destination = directory.path().join("render.mp4");
        let alias = directory.path().join("render-alias.mp4");
        fs::write(&destination, b"existing output").unwrap();
        fs::hard_link(&destination, &alias).unwrap();
        let registry = ExportDestinationLeaseRegistry::default();
        let _lease = registry.reserve(&path_string(&destination)).unwrap();

        assert!(
            registry
                .reserve(&path_string(&alias))
                .unwrap_err()
                .to_string()
                .contains("already reserved")
        );
    }

    #[cfg(any(unix, windows))]
    #[test]
    fn reservation_refreshes_identity_when_destination_appears() {
        let directory = tempfile::tempdir().unwrap();
        let destination = directory.path().join("future-render.mp4");
        let alias = directory.path().join("future-render-alias.mp4");
        let registry = ExportDestinationLeaseRegistry::default();
        let _lease = registry.reserve(&path_string(&destination)).unwrap();
        fs::write(&destination, b"new output owned by active job").unwrap();
        fs::hard_link(&destination, &alias).unwrap();

        assert!(
            registry
                .reserve(&path_string(&alias))
                .unwrap_err()
                .to_string()
                .contains("already reserved")
        );
    }

    #[cfg(unix)]
    #[test]
    fn symbolic_link_aliases_share_one_reservation() {
        use std::os::unix::fs::symlink;

        let directory = tempfile::tempdir().unwrap();
        let destination = directory.path().join("render.mp4");
        let alias = directory.path().join("render-alias.mp4");
        fs::write(&destination, b"existing output").unwrap();
        symlink(&destination, &alias).unwrap();
        let registry = ExportDestinationLeaseRegistry::default();
        let _lease = registry.reserve(&path_string(&destination)).unwrap();

        assert!(
            registry
                .reserve(&path_string(&alias))
                .unwrap_err()
                .to_string()
                .contains("already reserved")
        );
    }

    #[cfg(unix)]
    #[test]
    fn retargeted_symbolic_link_keeps_its_logical_reservation() {
        use std::os::unix::fs::symlink;

        let directory = tempfile::tempdir().unwrap();
        let first = directory.path().join("first.mp4");
        let second = directory.path().join("second.mp4");
        let alias = directory.path().join("render.mp4");
        fs::write(&first, b"first").unwrap();
        fs::write(&second, b"second").unwrap();
        symlink(&first, &alias).unwrap();
        let registry = ExportDestinationLeaseRegistry::default();
        let logical_path = path_string(&alias);
        let _lease = registry.reserve(&logical_path).unwrap();

        fs::remove_file(&alias).unwrap();
        symlink(&second, &alias).unwrap();

        assert!(
            registry
                .reserve(&logical_path)
                .unwrap_err()
                .to_string()
                .contains("already reserved")
        );
    }

    #[cfg(windows)]
    #[test]
    fn windows_case_variants_keep_one_logical_reservation() {
        let directory = tempfile::tempdir().unwrap();
        let mixed_case = path_string(&directory.path().join("Render.MP4"));
        let lower_case = mixed_case.to_lowercase();
        let registry = ExportDestinationLeaseRegistry::default();
        let _lease = registry.reserve(&mixed_case).unwrap();

        assert!(
            registry
                .reserve(&lower_case)
                .unwrap_err()
                .to_string()
                .contains("already reserved")
        );
    }

    #[test]
    fn distinct_destinations_can_be_reserved_together() {
        let directory = tempfile::tempdir().unwrap();
        let first = path_string(&directory.path().join("first.mp4"));
        let second = path_string(&directory.path().join("second.mp4"));
        let registry = ExportDestinationLeaseRegistry::default();

        let _first = registry.reserve(&first).unwrap();
        let _second = registry.reserve(&second).unwrap();
    }
}
