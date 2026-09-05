use crate::error::LibraryError;
use crate::plugin::ExportDestination;
use crate::util::atomic_file::AtomicFileTransaction;
#[cfg(test)]
use crate::util::atomic_file::AtomicSyncTestControl;
use crate::util::output_path_identity::{OutputPathIdentity, output_path_identity};
#[cfg(test)]
use std::sync::Arc;

/// Owns one host-controlled video staging artifact until it is either
/// published or explicitly discarded.
pub(super) struct AuthoringVideoOutput {
    destination: ExportDestination,
    initial_destination: OutputPathIdentity,
    transaction: Option<AtomicFileTransaction>,
}

impl AuthoringVideoOutput {
    pub(super) fn begin(output_path: &str) -> Result<Self, LibraryError> {
        let initial_destination = output_path_identity(output_path)?;
        // Resolve a final symlink now and publish to its target. Replacing the
        // logical symlink itself would unexpectedly sever the user's link.
        let transaction = AtomicFileTransaction::begin(initial_destination.resolved_path())
            .map_err(|error| {
                LibraryError::Render(format!(
                    "cannot create a staging file for video export '{output_path}': {error}"
                ))
            })?;
        let writable_path = transaction
            .staging_path()
            .to_str()
            .map(str::to_owned)
            .ok_or_else(|| {
                LibraryError::Render(format!(
                    "video export staging path is not valid UTF-8: {}",
                    transaction.staging_path().display()
                ))
            })?;
        Ok(Self {
            destination: ExportDestination::staged(output_path, writable_path),
            initial_destination,
            transaction: Some(transaction),
        })
    }

    pub(super) fn destination(&self) -> &ExportDestination {
        &self.destination
    }

    #[cfg(test)]
    pub(super) fn with_sync_test_control(
        mut self,
        control: Arc<AtomicSyncTestControl>,
    ) -> Result<Self, LibraryError> {
        self.transaction
            .as_mut()
            .ok_or_else(Self::missing_transaction)?
            .set_sync_test_control(control);
        Ok(self)
    }

    pub(super) fn publish<F>(mut self, validate_domain: F) -> Result<(), LibraryError>
    where
        F: FnOnce() -> Result<(), LibraryError>,
    {
        let logical_path = self.destination.logical_path().to_owned();
        let initial_destination = self.initial_destination.clone();
        self.transaction
            .take()
            .ok_or_else(Self::missing_transaction)?
            .commit_with_validation(|transaction| {
                Self::require_nonempty_regular_file(transaction, &logical_path)
                    .and_then(|()| validate_domain())
                    // Destination identity is deliberately last: the domain
                    // callback can inspect every input alias, and the atomic
                    // transaction has already completed its potentially slow
                    // data synchronization.
                    .and_then(|()| {
                        Self::require_unchanged_destination(&logical_path, &initial_destination)
                    })
                    .map_err(|error| std::io::Error::other(error.to_string()))
            })
            .map_err(|error| {
                LibraryError::Render(format!(
                    "cannot atomically publish video export '{logical_path}' from its staging file: {error}"
                ))
            })
    }

    pub(super) fn abort(mut self) -> Result<(), LibraryError> {
        self.abort_inner()
    }

    fn require_nonempty_regular_file(
        transaction: &AtomicFileTransaction,
        logical_path: &str,
    ) -> Result<(), LibraryError> {
        let metadata = transaction.staging_metadata().map_err(|error| {
            LibraryError::Render(format!(
                "cannot inspect completed video staging file for '{logical_path}': {error}"
            ))
        })?;
        if !metadata.is_file() || metadata.len() == 0 {
            return Err(LibraryError::Render(format!(
                "video exporter produced no file data at '{}'",
                transaction.staging_path().display()
            )));
        }
        Ok(())
    }

    fn require_unchanged_destination(
        logical_path: &str,
        initial_destination: &OutputPathIdentity,
    ) -> Result<(), LibraryError> {
        let current = output_path_identity(logical_path)?;
        if &current != initial_destination {
            return Err(LibraryError::Render(format!(
                "video export destination '{}' changed while the export was running; the completed staging file was not published",
                logical_path
            )));
        }
        Ok(())
    }

    fn abort_inner(&mut self) -> Result<(), LibraryError> {
        let Some(transaction) = self.transaction.take() else {
            return Ok(());
        };
        transaction.abort().map_err(|error| {
            LibraryError::Render(format!(
                "cannot remove failed video export staging file: {error}"
            ))
        })
    }

    fn missing_transaction() -> LibraryError {
        LibraryError::Render("video export staging transaction is unavailable".to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::AuthoringVideoOutput;
    use std::fs;

    #[test]
    fn publish_rejects_an_external_file_created_at_a_missing_destination() {
        let directory = tempfile::tempdir().unwrap();
        let output_path = directory.path().join("movie.mp4");
        let output = AuthoringVideoOutput::begin(output_path.to_str().unwrap()).unwrap();
        let staging = output.destination().writable_path().to_owned();
        fs::write(&staging, b"encoded video").unwrap();
        fs::write(&output_path, b"external owner").unwrap();

        let error = output.publish(|| Ok(())).unwrap_err();

        assert!(error.to_string().contains("changed while the export"));
        assert_eq!(fs::read(&output_path).unwrap(), b"external owner");
        assert!(!std::path::Path::new(&staging).exists());
    }

    #[test]
    fn publish_rejects_replacement_of_an_existing_destination_entry() {
        let directory = tempfile::tempdir().unwrap();
        let output_path = directory.path().join("movie.mp4");
        fs::write(&output_path, b"original owner").unwrap();
        let output = AuthoringVideoOutput::begin(output_path.to_str().unwrap()).unwrap();
        let staging = output.destination().writable_path().to_owned();
        fs::write(&staging, b"encoded video").unwrap();
        fs::remove_file(&output_path).unwrap();
        fs::write(&output_path, b"replacement owner").unwrap();

        let error = output.publish(|| Ok(())).unwrap_err();

        assert!(error.to_string().contains("changed while the export"));
        assert_eq!(fs::read(&output_path).unwrap(), b"replacement owner");
        assert!(!std::path::Path::new(&staging).exists());
    }

    #[test]
    fn post_sync_domain_validation_cannot_hide_a_destination_replacement() {
        let directory = tempfile::tempdir().unwrap();
        let output_path = directory.path().join("movie.mp4");
        fs::write(&output_path, b"original owner").unwrap();
        let output = AuthoringVideoOutput::begin(output_path.to_str().unwrap()).unwrap();
        let staging = output.destination().writable_path().to_owned();
        fs::write(&staging, b"encoded video").unwrap();

        let error = output
            .publish(|| {
                fs::remove_file(&output_path)?;
                fs::write(&output_path, b"late external owner")?;
                Ok(())
            })
            .unwrap_err();

        assert!(error.to_string().contains("changed while the export"));
        assert_eq!(fs::read(&output_path).unwrap(), b"late external owner");
        assert!(!std::path::Path::new(&staging).exists());
    }
}
