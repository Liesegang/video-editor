//! Same-directory atomic file publication.
//!
//! Producers write a newly created staging file while the previous
//! destination remains untouched. A successful commit synchronizes the
//! staging file and atomically replaces the destination. Aborting, returning
//! an error, or dropping an unfinished transaction removes the staging file.

use std::ffi::OsString;
use std::fs::{self, File, Metadata, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
#[cfg(test)]
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

use crate::util::local_file::{DirectRegularFile, validate_explicit_output_path};

/// Instance-local observation and one-shot fault control for the shared
/// staging-file synchronization boundary.
#[cfg(test)]
#[derive(Default)]
pub(crate) struct AtomicSyncTestControl {
    sync_failures_remaining: AtomicUsize,
    sync_attempts: AtomicUsize,
    injected_sync_failures: AtomicUsize,
}

#[cfg(test)]
impl AtomicSyncTestControl {
    pub(crate) fn fail_next_sync(&self) -> io::Result<()> {
        self.sync_failures_remaining
            .compare_exchange(0, 1, Ordering::SeqCst, Ordering::SeqCst)
            .map(|_| ())
            .map_err(|_| {
                io::Error::new(
                    io::ErrorKind::AlreadyExists,
                    "an atomic staging sync failure is already armed",
                )
            })
    }

    fn before_sync(&self, staging: &Path) -> io::Result<()> {
        self.sync_attempts.fetch_add(1, Ordering::SeqCst);
        if self.sync_failures_remaining.swap(0, Ordering::SeqCst) == 0 {
            return Ok(());
        }
        self.injected_sync_failures.fetch_add(1, Ordering::SeqCst);
        Err(io::Error::other(format!(
            "injected atomic staging sync_all failure for '{}'",
            staging.display()
        )))
    }

    pub(crate) fn observation(&self) -> (usize, usize) {
        (
            self.sync_attempts.load(Ordering::SeqCst),
            self.injected_sync_failures.load(Ordering::SeqCst),
        )
    }
}

/// One staged write to a single file destination.
///
/// The staging file is created in the destination directory with
/// `create_new`, which both reserves its unpredictable name and guarantees
/// that commit remains a same-filesystem rename. Callers that delegate writing
/// to another process may pass [`Self::staging_path`] to it; that process must
/// truncate/write the existing file rather than replace its directory entry.
pub(crate) struct AtomicFileTransaction {
    destination: PathBuf,
    staging: PathBuf,
    staging_file: Option<File>,
    active: bool,
    #[cfg(test)]
    sync_test_control: Option<Arc<AtomicSyncTestControl>>,
}

impl AtomicFileTransaction {
    /// Create and exclusively reserve a staging file beside `destination`.
    pub(crate) fn begin(destination: &Path) -> io::Result<Self> {
        destination.file_name().ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                "atomic file destination must name a file",
            )
        })?;
        // File transactions may outlive the call stack that created them.
        // Freeze a relative destination at begin time so a process-wide CWD
        // change cannot redirect commit or cleanup to another directory.
        let destination = if destination.is_absolute() {
            destination.to_path_buf()
        } else {
            std::env::current_dir()?.join(destination)
        };
        validate_explicit_output_path(&destination)?;
        let file_name = destination.file_name().ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                "atomic file destination must name a file",
            )
        })?;
        let directory = destination
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
            .unwrap_or_else(|| Path::new("."));
        let staging = unique_staging_path(directory, file_name);
        let staging_file = OpenOptions::new()
            .read(true)
            .write(true)
            .create_new(true)
            .open(&staging)?;

        Ok(Self {
            destination,
            staging,
            staging_file: Some(staging_file),
            active: true,
            #[cfg(test)]
            sync_test_control: None,
        })
    }

    #[cfg(test)]
    pub(crate) fn set_sync_test_control(&mut self, control: Arc<AtomicSyncTestControl>) {
        self.sync_test_control = Some(control);
    }

    pub(crate) fn staging_path(&self) -> &Path {
        &self.staging
    }

    /// Verify that the staging path still names the regular file reserved by
    /// `begin`, then return its current metadata.
    pub(crate) fn staging_metadata(&self) -> io::Result<Metadata> {
        let reserved = self.staging_file.as_ref().ok_or_else(closed_error)?;
        let opened = DirectRegularFile::open_explicit_output(&self.staging)?;
        if !crate::util::local_file::file_handles_share_identity(reserved, opened.file())? {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!(
                    "atomic staging path '{}' no longer names its reserved file",
                    self.staging.display()
                ),
            ));
        }
        opened.file().metadata()
    }

    /// Return the reserved file for in-process writers.
    pub(crate) fn staging_file_mut(&mut self) -> io::Result<&mut File> {
        self.staging_file.as_mut().ok_or_else(closed_error)
    }

    /// Synchronize and atomically publish the complete staging file.
    pub(crate) fn commit(self) -> io::Result<()> {
        self.commit_with_validation(|_| Ok(()))
    }

    /// Synchronize the stage, run the caller's final domain validation, then
    /// atomically publish. The callback runs after the potentially long
    /// `sync_all` and while the reserved staging handle is still open. The
    /// stage identity is checked again after it returns and immediately before
    /// the handle is closed for rename.
    pub(crate) fn commit_with_validation<F>(mut self, validate: F) -> io::Result<()>
    where
        F: FnOnce(&Self) -> io::Result<()>,
    {
        let result = (|| {
            self.staging_metadata()?;
            self.sync_staging()?;
            self.staging_metadata()?;
            validate(&self)?;
            self.staging_metadata()?;
            drop(self.staging_file.take().ok_or_else(closed_error)?);
            replace_file(&self.staging, &self.destination)
        })();

        match result {
            Ok(()) => {
                self.active = false;
                Ok(())
            }
            Err(commit_error) => match self.cleanup() {
                Ok(()) => Err(commit_error),
                Err(cleanup_error) => Err(io::Error::new(
                    commit_error.kind(),
                    format!(
                        "atomic file commit failed: {commit_error}; staging cleanup also failed: {cleanup_error}"
                    ),
                )),
            },
        }
    }

    fn sync_staging(&self) -> io::Result<()> {
        #[cfg(test)]
        if let Some(control) = &self.sync_test_control {
            control.before_sync(&self.staging)?;
        }
        self.staging_file
            .as_ref()
            .ok_or_else(closed_error)?
            .sync_all()
    }

    /// Discard the staged file without changing the destination.
    pub(crate) fn abort(mut self) -> io::Result<()> {
        self.cleanup()
    }

    fn cleanup(&mut self) -> io::Result<()> {
        self.staging_file.take();
        if !self.active {
            return Ok(());
        }
        match fs::remove_file(&self.staging) {
            Ok(()) => {
                self.active = false;
                Ok(())
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                self.active = false;
                Ok(())
            }
            Err(error) => Err(error),
        }
    }
}

fn closed_error() -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidInput,
        "atomic file transaction is already closed",
    )
}

impl Drop for AtomicFileTransaction {
    fn drop(&mut self) {
        if let Err(error) = self.cleanup() {
            log::warn!(
                "failed to remove unfinished atomic staging file '{}': {error}",
                self.staging.display()
            );
        }
    }
}

/// Atomically replace `destination` with `bytes`.
pub(crate) fn atomic_write(destination: &Path, bytes: &[u8]) -> io::Result<()> {
    let mut transaction = AtomicFileTransaction::begin(destination)?;
    transaction.staging_file_mut()?.write_all(bytes)?;
    transaction.commit()
}

fn unique_staging_path(directory: &Path, file_name: &std::ffi::OsStr) -> PathBuf {
    let file_name_path = Path::new(file_name);
    let stem = file_name_path.file_stem().unwrap_or(file_name);
    let extension = file_name_path
        .extension()
        .filter(|extension| !extension.is_empty());
    let mut staging_name = OsString::from(".");
    staging_name.push(stem);
    staging_name.push(".");
    staging_name.push(uuid::Uuid::new_v4().as_simple().to_string());
    staging_name.push(".tmp");
    if let Some(extension) = extension {
        staging_name.push(".");
        staging_name.push(extension);
    }
    directory.join(staging_name)
}

#[cfg(not(windows))]
fn replace_file(staging: &Path, destination: &Path) -> io::Result<()> {
    fs::rename(staging, destination)
}

#[cfg(windows)]
fn replace_file(staging: &Path, destination: &Path) -> io::Result<()> {
    use std::os::windows::ffi::OsStrExt;

    use windows_sys::Win32::Storage::FileSystem::{
        MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH, MoveFileExW,
    };

    let staging: Vec<u16> = staging
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();
    let destination: Vec<u16> = destination
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();
    // SAFETY: both pointers reference live, NUL-terminated UTF-16 buffers for
    // the duration of the call, and these flags require no additional data.
    let replaced = unsafe {
        MoveFileExW(
            staging.as_ptr(),
            destination.as_ptr(),
            MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
        )
    };
    if replaced == 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::{AtomicFileTransaction, AtomicSyncTestControl, atomic_write};
    use std::fs;
    use std::io::Write;
    use std::sync::Arc;

    #[test]
    fn commit_replaces_an_existing_destination_and_removes_staging() {
        let directory = tempfile::tempdir().unwrap();
        let destination = directory.path().join("project.ruvie");
        fs::write(&destination, b"previous").unwrap();

        let mut transaction = AtomicFileTransaction::begin(&destination).unwrap();
        let staging = transaction.staging_path().to_path_buf();
        assert_eq!(staging.parent(), destination.parent());
        transaction
            .staging_file_mut()
            .unwrap()
            .write_all(b"complete")
            .unwrap();
        transaction.commit().unwrap();

        assert_eq!(fs::read(destination).unwrap(), b"complete");
        assert!(!staging.exists());
    }

    #[test]
    fn commit_publishes_a_previously_missing_destination() {
        let directory = tempfile::tempdir().unwrap();
        let destination = directory.path().join("new-project.ruvie");

        atomic_write(&destination, b"new document").unwrap();

        assert_eq!(fs::read(destination).unwrap(), b"new document");
    }

    #[test]
    fn injected_sync_failure_preserves_destination_cleans_stage_and_is_one_shot() {
        let directory = tempfile::tempdir().unwrap();
        let destination = directory.path().join("project.ruvie");
        let sentinel = b"previous project";
        fs::write(&destination, sentinel).unwrap();
        let control = Arc::new(AtomicSyncTestControl::default());
        control.fail_next_sync().unwrap();

        let mut failed = AtomicFileTransaction::begin(&destination).unwrap();
        let failed_staging = failed.staging_path().to_path_buf();
        failed.set_sync_test_control(Arc::clone(&control));
        failed
            .staging_file_mut()
            .unwrap()
            .write_all(b"unpublished project")
            .unwrap();
        let error = failed.commit().unwrap_err();

        assert!(
            error
                .to_string()
                .contains("injected atomic staging sync_all")
        );
        assert_eq!(fs::read(&destination).unwrap(), sentinel);
        assert!(!failed_staging.exists());
        assert_eq!(control.observation(), (1, 1));

        let mut retry = AtomicFileTransaction::begin(&destination).unwrap();
        let retry_staging = retry.staging_path().to_path_buf();
        retry.set_sync_test_control(Arc::clone(&control));
        retry
            .staging_file_mut()
            .unwrap()
            .write_all(b"published project")
            .unwrap();
        retry.commit().unwrap();

        assert_eq!(fs::read(&destination).unwrap(), b"published project");
        assert!(!retry_staging.exists());
        assert_eq!(control.observation(), (2, 1));
    }

    #[cfg(windows)]
    #[test]
    fn windows_delete_share_denial_blocks_real_replace_and_retry_recovers() {
        use std::os::windows::fs::OpenOptionsExt;

        use windows_sys::Win32::Foundation::{ERROR_ACCESS_DENIED, ERROR_SHARING_VIOLATION};
        use windows_sys::Win32::Storage::FileSystem::{FILE_SHARE_READ, FILE_SHARE_WRITE};

        let directory = tempfile::tempdir().unwrap();
        let destination = directory.path().join("project.ruvie");
        let sentinel = b"previous project";
        fs::write(&destination, sentinel).unwrap();
        let mut transaction = AtomicFileTransaction::begin(&destination).unwrap();
        let staging = transaction.staging_path().to_path_buf();
        transaction
            .staging_file_mut()
            .unwrap()
            .write_all(b"blocked project")
            .unwrap();
        let destination_lock = fs::OpenOptions::new()
            .read(true)
            .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE)
            .open(&destination)
            .unwrap();

        let error = transaction.commit().unwrap_err();

        let raw_os_error = error.raw_os_error();
        assert_eq!(error.kind(), std::io::ErrorKind::PermissionDenied);
        assert!(
            matches!(
                raw_os_error,
                Some(code)
                    if code == ERROR_ACCESS_DENIED as i32
                        || code == ERROR_SHARING_VIOLATION as i32
            ),
            "unexpected MoveFileExW error: {error}"
        );
        assert_eq!(fs::read(&destination).unwrap(), sentinel);
        assert!(!staging.exists());

        drop(destination_lock);
        atomic_write(&destination, b"published project").unwrap();
        assert_eq!(fs::read(&destination).unwrap(), b"published project");
    }

    #[test]
    fn an_external_writer_can_fill_the_reserved_staging_file() {
        let directory = tempfile::tempdir().unwrap();
        let destination = directory.path().join("render.mp4");
        fs::write(&destination, b"previous video").unwrap();
        let transaction = AtomicFileTransaction::begin(&destination).unwrap();
        let staging = transaction.staging_path().to_path_buf();

        fs::write(&staging, b"encoded video").unwrap();
        transaction.commit().unwrap();

        assert_eq!(fs::read(destination).unwrap(), b"encoded video");
        assert!(!staging.exists());
    }

    #[test]
    fn abort_preserves_the_existing_destination_and_removes_staging() {
        let directory = tempfile::tempdir().unwrap();
        let destination = directory.path().join("project.ruvie");
        fs::write(&destination, b"previous").unwrap();

        let mut transaction = AtomicFileTransaction::begin(&destination).unwrap();
        let staging = transaction.staging_path().to_path_buf();
        transaction
            .staging_file_mut()
            .unwrap()
            .write_all(b"partial")
            .unwrap();
        transaction.abort().unwrap();

        assert_eq!(fs::read(destination).unwrap(), b"previous");
        assert!(!staging.exists());
    }

    #[test]
    fn drop_preserves_the_existing_destination_and_removes_staging() {
        let directory = tempfile::tempdir().unwrap();
        let destination = directory.path().join("project.ruvie");
        fs::write(&destination, b"previous").unwrap();

        let staging = {
            let mut transaction = AtomicFileTransaction::begin(&destination).unwrap();
            let staging = transaction.staging_path().to_path_buf();
            transaction
                .staging_file_mut()
                .unwrap()
                .write_all(b"partial")
                .unwrap();
            staging
        };

        assert_eq!(fs::read(destination).unwrap(), b"previous");
        assert!(!staging.exists());
    }

    #[test]
    fn drop_does_not_create_a_missing_destination() {
        let directory = tempfile::tempdir().unwrap();
        let destination = directory.path().join("missing.ruvie");

        let staging = {
            let transaction = AtomicFileTransaction::begin(&destination).unwrap();
            transaction.staging_path().to_path_buf()
        };

        assert!(!destination.exists());
        assert!(!staging.exists());
    }

    #[test]
    fn failed_commit_preserves_the_destination_and_removes_staging() {
        let directory = tempfile::tempdir().unwrap();
        let destination = directory.path().join("occupied");
        fs::create_dir(&destination).unwrap();
        let mut transaction = AtomicFileTransaction::begin(&destination).unwrap();
        let staging = transaction.staging_path().to_path_buf();
        transaction
            .staging_file_mut()
            .unwrap()
            .write_all(b"complete")
            .unwrap();

        transaction.commit().unwrap_err();

        assert!(destination.is_dir());
        assert!(!staging.exists());
    }

    #[test]
    fn post_sync_validation_rejects_a_destination_replacement_without_overwriting_it() {
        let directory = tempfile::tempdir().unwrap();
        let destination = directory.path().join("render.mp4");
        fs::write(&destination, b"original video").unwrap();
        let mut transaction = AtomicFileTransaction::begin(&destination).unwrap();
        let staging = transaction.staging_path().to_path_buf();
        transaction
            .staging_file_mut()
            .unwrap()
            .write_all(b"encoded video")
            .unwrap();

        let error = transaction
            .commit_with_validation(|_| {
                fs::remove_file(&destination)?;
                fs::write(&destination, b"external replacement")?;
                Err(std::io::Error::other(
                    "destination changed after stage synchronization",
                ))
            })
            .unwrap_err();

        assert!(error.to_string().contains("changed after stage"));
        assert_eq!(fs::read(&destination).unwrap(), b"external replacement");
        assert!(!staging.exists());
    }

    #[test]
    fn commit_rejects_a_replaced_staging_directory_entry() {
        let directory = tempfile::tempdir().unwrap();
        let destination = directory.path().join("render.mp4");
        fs::write(&destination, b"previous video").unwrap();
        let transaction = AtomicFileTransaction::begin(&destination).unwrap();
        let staging = transaction.staging_path().to_path_buf();

        fs::remove_file(&staging).unwrap();
        fs::write(&staging, b"unreserved replacement").unwrap();
        let error = transaction.commit().unwrap_err();

        assert!(
            error.to_string().contains("reserved file"),
            "unexpected identity error: {error}"
        );
        assert_eq!(fs::read(destination).unwrap(), b"previous video");
        assert!(!staging.exists());
    }

    #[test]
    fn staging_name_is_reserved_with_create_new() {
        let directory = tempfile::tempdir().unwrap();
        let destination = directory.path().join("project.ruvie");
        let transaction = AtomicFileTransaction::begin(&destination).unwrap();

        assert_eq!(
            transaction.staging_path().extension(),
            destination.extension()
        );
        let error = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(transaction.staging_path())
            .unwrap_err();

        assert_eq!(error.kind(), std::io::ErrorKind::AlreadyExists);
    }

    #[test]
    fn relative_destination_is_frozen_as_an_absolute_staging_path() {
        let name = format!("atomic-relative-{}.ruvie", uuid::Uuid::new_v4());
        let transaction = AtomicFileTransaction::begin(std::path::Path::new(&name)).unwrap();
        let staging = transaction.staging_path().to_path_buf();

        assert!(staging.is_absolute());
        let current_directory = std::env::current_dir().unwrap();
        assert_eq!(staging.parent(), Some(current_directory.as_path()));

        transaction.abort().unwrap();
    }
}
