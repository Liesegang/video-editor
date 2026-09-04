//! Bounded, exact-byte loading for local OpenColorIO configuration files.
//!
//! A Project path is only a locator. The returned immutable snapshot owns the
//! exact bytes and their SHA-256 identity, so validation and OCIO stream
//! construction cannot observe different file revisions. Files larger than
//! 16 MiB are rejected: `.ocio` files are text configuration, and accepting an
//! unbounded document on the synchronous Preview path would make Project data
//! an allocation and I/O denial-of-service primitive.

use std::collections::VecDeque;
use std::fmt;
use std::fs::{self, File, OpenOptions};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};

use sha2::{Digest, Sha256};

/// Maximum accepted size of one self-contained `.ocio` text configuration.
pub const MAX_EXACT_COLOR_CONFIG_BYTES: u64 = 16 * 1024 * 1024;

const READ_BUFFER_BYTES: usize = 64 * 1024;
#[cfg(any(unix, windows))]
const SNAPSHOT_CACHE_ENTRIES: usize = 8;

/// Immutable bytes and content identity read from one direct regular file.
///
/// Clones share the same byte allocation. `read` may reuse a previously
/// hashed snapshot only when a strong opened-handle identity matches. Unix
/// uses device, inode, length, mtime, and ctime; Windows uses volume serial,
/// file index, length, last-write time, and change time. Change time prevents
/// an in-place rewrite followed by an authored mtime rollback from
/// masquerading as the cached revision. Other targets conservatively read and
/// hash again because `std` does not expose an equally strong identity.
#[derive(Clone, Debug)]
pub struct ExactColorConfigFile {
    path: PathBuf,
    bytes: Arc<[u8]>,
    sha256: Arc<str>,
}

impl ExactColorConfigFile {
    /// Open and snapshot a direct local regular file without following a final
    /// symlink. FIFO/device/directory inputs are rejected before content I/O;
    /// the opened handle is checked again to close the preflight replacement
    /// race as far as the platform's Rust file APIs permit.
    pub fn read(path: impl AsRef<Path>) -> Result<Self, ExactColorConfigFileError> {
        let path = path.as_ref();
        preflight_direct_regular_file(path)?;
        let mut file = open_without_following_or_blocking(path)?;
        let before = opened_regular_file_stamp(path, &file)?;

        #[cfg(any(unix, windows))]
        if let Some(snapshot) = snapshot_cache()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .get(&before)
        {
            return Ok(Self {
                path: path.to_path_buf(),
                bytes: Arc::clone(&snapshot.bytes),
                sha256: Arc::clone(&snapshot.sha256),
            });
        }

        let bytes = read_bounded(path, &mut file, before.length)?;
        let after = opened_regular_file_stamp(path, &file)?;
        if before != after || after.length != bytes.len() as u64 {
            return Err(ExactColorConfigFileError::ChangedWhileReading {
                path: path.to_path_buf(),
            });
        }

        let bytes: Arc<[u8]> = bytes.into();
        let sha256: Arc<str> = format!("{:x}", Sha256::digest(&bytes)).into();

        #[cfg(any(unix, windows))]
        snapshot_cache()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .insert(
                before,
                CachedSnapshot {
                    bytes: Arc::clone(&bytes),
                    sha256: Arc::clone(&sha256),
                },
            );

        Ok(Self {
            path: path.to_path_buf(),
            bytes,
            sha256,
        })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }

    pub fn sha256(&self) -> &str {
        &self.sha256
    }

    /// Verify a persisted exact-config identity against these same immutable
    /// bytes. No second read or hash occurs.
    pub fn verify_sha256(&self, expected: &str) -> Result<(), ExactColorConfigFileError> {
        let expected = normalize_sha256(expected)?;
        if self.sha256.eq_ignore_ascii_case(&expected) {
            Ok(())
        } else {
            Err(ExactColorConfigFileError::ChecksumMismatch {
                path: self.path.clone(),
                expected,
                actual: self.sha256.to_string(),
            })
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ExactColorConfigFileError {
    InvalidExpectedSha256(String),
    PathIo {
        path: PathBuf,
        detail: String,
    },
    Symlink {
        path: PathBuf,
    },
    NotRegularFile {
        path: PathBuf,
    },
    UnsafeLocator {
        path: PathBuf,
        detail: &'static str,
    },
    TooLarge {
        path: PathBuf,
        actual_bytes: u64,
        maximum_bytes: u64,
    },
    ChangedWhileReading {
        path: PathBuf,
    },
    ChecksumMismatch {
        path: PathBuf,
        expected: String,
        actual: String,
    },
}

impl fmt::Display for ExactColorConfigFileError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidExpectedSha256(value) => {
                write!(formatter, "expected config SHA-256 '{value}' is invalid")
            }
            Self::PathIo { path, detail } => write!(
                formatter,
                "cannot read OpenColorIO config '{}': {detail}",
                path.display()
            ),
            Self::Symlink { path } => write!(
                formatter,
                "OpenColorIO config '{}' is a symlink; a direct regular file is required",
                path.display()
            ),
            Self::NotRegularFile { path } => write!(
                formatter,
                "OpenColorIO config '{}' is not a regular file",
                path.display()
            ),
            Self::UnsafeLocator { path, detail } => write!(
                formatter,
                "OpenColorIO config locator '{}' is unsafe: {detail}",
                path.display()
            ),
            Self::TooLarge {
                path,
                actual_bytes,
                maximum_bytes,
            } => write!(
                formatter,
                "OpenColorIO config '{}' is {actual_bytes} bytes; the maximum is {maximum_bytes} bytes",
                path.display()
            ),
            Self::ChangedWhileReading { path } => write!(
                formatter,
                "OpenColorIO config '{}' changed while its exact bytes were being read",
                path.display()
            ),
            Self::ChecksumMismatch {
                path,
                expected,
                actual,
            } => write!(
                formatter,
                "OpenColorIO config '{}' expected SHA-256 '{expected}' but read '{actual}'",
                path.display()
            ),
        }
    }
}

impl std::error::Error for ExactColorConfigFileError {}

fn preflight_direct_regular_file(path: &Path) -> Result<(), ExactColorConfigFileError> {
    #[cfg(windows)]
    validate_local_windows_locator(path)?;

    let metadata = fs::symlink_metadata(path).map_err(|error| path_io(path, error))?;
    let file_type = metadata.file_type();
    if file_type.is_symlink() {
        return Err(ExactColorConfigFileError::Symlink {
            path: path.to_path_buf(),
        });
    }
    if !file_type.is_file() {
        return Err(ExactColorConfigFileError::NotRegularFile {
            path: path.to_path_buf(),
        });
    }
    if metadata.len() > MAX_EXACT_COLOR_CONFIG_BYTES {
        return Err(too_large(path, metadata.len()));
    }
    Ok(())
}

#[cfg(windows)]
fn validate_local_windows_locator(path: &Path) -> Result<(), ExactColorConfigFileError> {
    use std::path::{Component, Prefix};

    if let Some(Component::Prefix(prefix)) = path.components().next() {
        match prefix.kind() {
            Prefix::Disk(_) | Prefix::VerbatimDisk(_) => {}
            Prefix::UNC(_, _)
            | Prefix::VerbatimUNC(_, _)
            | Prefix::DeviceNS(_)
            | Prefix::Verbatim(_) => {
                return Err(ExactColorConfigFileError::UnsafeLocator {
                    path: path.to_path_buf(),
                    detail: "UNC, device-namespace, and generic verbatim paths are not local file locators",
                });
            }
        }
    }

    for component in path.components() {
        let Component::Normal(name) = component else {
            continue;
        };
        let name = name.to_string_lossy();
        if name.contains(':') {
            return Err(ExactColorConfigFileError::UnsafeLocator {
                path: path.to_path_buf(),
                detail: "alternate data streams are not direct regular-file locators",
            });
        }
        if is_reserved_windows_device_name(&name) {
            return Err(ExactColorConfigFileError::UnsafeLocator {
                path: path.to_path_buf(),
                detail: "DOS device names are not regular-file locators",
            });
        }
    }
    Ok(())
}

#[cfg(windows)]
fn is_reserved_windows_device_name(name: &str) -> bool {
    let trimmed = name.trim_end_matches([' ', '.']);
    let stem = trimmed
        .split_once('.')
        .map_or(trimmed, |(stem, _extension)| stem)
        .trim_end_matches(' ')
        .to_ascii_uppercase();
    matches!(
        stem.as_str(),
        "CON"
            | "PRN"
            | "AUX"
            | "NUL"
            | "CLOCK$"
            | "CONIN$"
            | "CONOUT$"
            | "COM1"
            | "COM2"
            | "COM3"
            | "COM4"
            | "COM5"
            | "COM6"
            | "COM7"
            | "COM8"
            | "COM9"
            | "COM¹"
            | "COM²"
            | "COM³"
            | "LPT1"
            | "LPT2"
            | "LPT3"
            | "LPT4"
            | "LPT5"
            | "LPT6"
            | "LPT7"
            | "LPT8"
            | "LPT9"
            | "LPT¹"
            | "LPT²"
            | "LPT³"
    )
}

fn open_without_following_or_blocking(path: &Path) -> Result<File, ExactColorConfigFileError> {
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;

        // O_NOFOLLOW makes final-component symlink rejection atomic with the
        // open. O_NONBLOCK ensures a regular-file -> FIFO replacement cannot
        // suspend the UI thread between preflight and opened-handle checks.
        options.custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK | libc::O_CLOEXEC);
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;

        // FILE_FLAG_OPEN_REPARSE_POINT asks Windows to open the link itself;
        // the opened-handle type check below then rejects it.
        const FILE_FLAG_OPEN_REPARSE_POINT: u32 = 0x0020_0000;
        options.custom_flags(FILE_FLAG_OPEN_REPARSE_POINT);
    }
    options.open(path).map_err(|error| path_io(path, error))
}

fn opened_regular_file_stamp(
    path: &Path,
    file: &File,
) -> Result<OpenedFileStamp, ExactColorConfigFileError> {
    let metadata = file.metadata().map_err(|error| path_io(path, error))?;
    if !metadata.file_type().is_file() {
        return Err(ExactColorConfigFileError::NotRegularFile {
            path: path.to_path_buf(),
        });
    }
    if metadata.len() > MAX_EXACT_COLOR_CONFIG_BYTES {
        return Err(too_large(path, metadata.len()));
    }
    OpenedFileStamp::from_opened_file(path, file, &metadata)
}

fn read_bounded(
    path: &Path,
    file: &mut File,
    declared_length: u64,
) -> Result<Vec<u8>, ExactColorConfigFileError> {
    let capacity = usize::try_from(declared_length)
        .unwrap_or(usize::MAX)
        .min(MAX_EXACT_COLOR_CONFIG_BYTES as usize);
    let mut bytes = Vec::with_capacity(capacity);
    let mut buffer = vec![0_u8; READ_BUFFER_BYTES].into_boxed_slice();
    loop {
        let count = file
            .read(&mut buffer)
            .map_err(|error| path_io(path, error))?;
        if count == 0 {
            break;
        }
        let next_length = bytes
            .len()
            .checked_add(count)
            .ok_or_else(|| too_large(path, u64::MAX))?;
        if next_length as u64 > MAX_EXACT_COLOR_CONFIG_BYTES {
            return Err(too_large(path, next_length as u64));
        }
        bytes.extend_from_slice(&buffer[..count]);
    }
    Ok(bytes)
}

fn normalize_sha256(value: &str) -> Result<String, ExactColorConfigFileError> {
    if value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        Ok(value.to_ascii_lowercase())
    } else {
        Err(ExactColorConfigFileError::InvalidExpectedSha256(
            value.to_string(),
        ))
    }
}

fn too_large(path: &Path, actual_bytes: u64) -> ExactColorConfigFileError {
    ExactColorConfigFileError::TooLarge {
        path: path.to_path_buf(),
        actual_bytes,
        maximum_bytes: MAX_EXACT_COLOR_CONFIG_BYTES,
    }
}

fn path_io(path: &Path, error: std::io::Error) -> ExactColorConfigFileError {
    ExactColorConfigFileError::PathIo {
        path: path.to_path_buf(),
        detail: error.to_string(),
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct OpenedFileStamp {
    length: u64,
    #[cfg(unix)]
    device: u64,
    #[cfg(unix)]
    inode: u64,
    #[cfg(unix)]
    modified_seconds: i64,
    #[cfg(unix)]
    modified_nanos: i64,
    #[cfg(unix)]
    change_seconds: i64,
    #[cfg(unix)]
    change_nanos: i64,
    #[cfg(windows)]
    volume_serial: u32,
    #[cfg(windows)]
    file_index: u64,
    #[cfg(windows)]
    creation_time: i64,
    #[cfg(windows)]
    last_write_time: i64,
    #[cfg(windows)]
    change_time: i64,
    #[cfg(all(not(unix), not(windows)))]
    path: PathBuf,
    #[cfg(all(not(unix), not(windows)))]
    modified: Option<std::time::SystemTime>,
}

impl OpenedFileStamp {
    fn from_opened_file(
        path: &Path,
        file: &File,
        metadata: &fs::Metadata,
    ) -> Result<Self, ExactColorConfigFileError> {
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;

            let _ = (path, file);
            Ok(Self {
                length: metadata.len(),
                device: metadata.dev(),
                inode: metadata.ino(),
                modified_seconds: metadata.mtime(),
                modified_nanos: metadata.mtime_nsec(),
                change_seconds: metadata.ctime(),
                change_nanos: metadata.ctime_nsec(),
            })
        }
        #[cfg(windows)]
        {
            let identity = windows_opened_file_identity(path, file)?;
            Ok(Self {
                length: metadata.len(),
                volume_serial: identity.volume_serial,
                file_index: identity.file_index,
                creation_time: identity.creation_time,
                last_write_time: identity.last_write_time,
                change_time: identity.change_time,
            })
        }
        #[cfg(all(not(unix), not(windows)))]
        {
            let _ = file;
            Ok(Self {
                length: metadata.len(),
                path: path.to_path_buf(),
                modified: metadata.modified().ok(),
            })
        }
    }
}

#[cfg(windows)]
struct WindowsOpenedFileIdentity {
    volume_serial: u32,
    file_index: u64,
    creation_time: i64,
    last_write_time: i64,
    change_time: i64,
}

#[cfg(windows)]
fn windows_opened_file_identity(
    path: &Path,
    file: &File,
) -> Result<WindowsOpenedFileIdentity, ExactColorConfigFileError> {
    use std::mem::size_of;
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::Storage::FileSystem::{
        BY_HANDLE_FILE_INFORMATION, FILE_ATTRIBUTE_REPARSE_POINT, FILE_BASIC_INFO, FILE_TYPE_DISK,
        FileBasicInfo, GetFileInformationByHandle, GetFileInformationByHandleEx, GetFileType,
    };

    let handle = file.as_raw_handle();
    // SAFETY: `handle` belongs to the live borrowed `File`; `GetFileType`
    // neither retains nor closes it.
    let file_type = unsafe { GetFileType(handle) };
    if file_type != FILE_TYPE_DISK {
        return Err(ExactColorConfigFileError::NotRegularFile {
            path: path.to_path_buf(),
        });
    }
    let mut information = BY_HANDLE_FILE_INFORMATION::default();
    // SAFETY: `handle` belongs to the live borrowed `File`, and `information`
    // is a writable value of the exact structure required by this API.
    let read_information = unsafe { GetFileInformationByHandle(handle, &mut information) };
    if read_information == 0 {
        return Err(path_io(path, std::io::Error::last_os_error()));
    }
    if information.dwFileAttributes & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
        return Err(ExactColorConfigFileError::Symlink {
            path: path.to_path_buf(),
        });
    }

    let mut basic = FILE_BASIC_INFO::default();
    let basic_size = u32::try_from(size_of::<FILE_BASIC_INFO>()).map_err(|error| {
        ExactColorConfigFileError::PathIo {
            path: path.to_path_buf(),
            detail: error.to_string(),
        }
    })?;
    // SAFETY: `handle` remains live; `basic` is writable for exactly
    // `basic_size` bytes and its type matches `FileBasicInfo`.
    let read_basic = unsafe {
        GetFileInformationByHandleEx(handle, FileBasicInfo, (&raw mut basic).cast(), basic_size)
    };
    if read_basic == 0 {
        return Err(path_io(path, std::io::Error::last_os_error()));
    }

    Ok(WindowsOpenedFileIdentity {
        volume_serial: information.dwVolumeSerialNumber,
        file_index: (u64::from(information.nFileIndexHigh) << 32)
            | u64::from(information.nFileIndexLow),
        creation_time: basic.CreationTime,
        last_write_time: basic.LastWriteTime,
        change_time: basic.ChangeTime,
    })
}

#[cfg(any(unix, windows))]
#[derive(Clone)]
struct CachedSnapshot {
    bytes: Arc<[u8]>,
    sha256: Arc<str>,
}

#[cfg(any(unix, windows))]
#[derive(Default)]
struct SnapshotCache {
    entries: VecDeque<(OpenedFileStamp, CachedSnapshot)>,
}

#[cfg(any(unix, windows))]
impl SnapshotCache {
    fn get(&mut self, stamp: &OpenedFileStamp) -> Option<CachedSnapshot> {
        let index = self
            .entries
            .iter()
            .position(|(candidate, _)| candidate == stamp)?;
        let entry = self.entries.remove(index)?;
        let snapshot = entry.1.clone();
        self.entries.push_back(entry);
        Some(snapshot)
    }

    fn insert(&mut self, stamp: OpenedFileStamp, snapshot: CachedSnapshot) {
        if let Some(index) = self
            .entries
            .iter()
            .position(|(candidate, _)| candidate == &stamp)
        {
            self.entries.remove(index);
        }
        self.entries.push_back((stamp, snapshot));
        while self.entries.len() > SNAPSHOT_CACHE_ENTRIES {
            self.entries.pop_front();
        }
    }
}

#[cfg(any(unix, windows))]
fn snapshot_cache() -> &'static Mutex<SnapshotCache> {
    static CACHE: OnceLock<Mutex<SnapshotCache>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(SnapshotCache::default()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Seek, SeekFrom, Write};

    #[test]
    fn regular_file_uses_one_exact_byte_snapshot_and_checksum() {
        let directory = tempfile::tempdir().expect("temp directory");
        let path = directory.path().join("config.ocio");
        fs::write(&path, b"exact-config").expect("write config");

        let snapshot = ExactColorConfigFile::read(&path).expect("read exact config");
        let checksum = format!("{:x}", Sha256::digest(b"exact-config"));
        assert_eq!(snapshot.bytes(), b"exact-config");
        assert_eq!(snapshot.sha256(), checksum);
        snapshot
            .verify_sha256(&checksum.to_ascii_uppercase())
            .expect("checksum is normalized");

        let mismatch = snapshot
            .verify_sha256(&"0".repeat(64))
            .expect_err("wrong checksum must fail");
        assert!(matches!(
            mismatch,
            ExactColorConfigFileError::ChecksumMismatch { actual, .. }
                if actual == checksum
        ));
    }

    #[test]
    fn oversized_regular_file_is_rejected_before_content_read() {
        let directory = tempfile::tempdir().expect("temp directory");
        let path = directory.path().join("oversized.ocio");
        let mut file = File::create(&path).expect("create config");
        file.seek(SeekFrom::Start(MAX_EXACT_COLOR_CONFIG_BYTES))
            .expect("seek to size boundary");
        file.write_all(&[0]).expect("extend beyond size boundary");

        assert!(matches!(
            ExactColorConfigFile::read(&path),
            Err(ExactColorConfigFileError::TooLarge {
                actual_bytes,
                maximum_bytes: MAX_EXACT_COLOR_CONFIG_BYTES,
                ..
            }) if actual_bytes == MAX_EXACT_COLOR_CONFIG_BYTES + 1
        ));
    }

    #[test]
    fn directory_is_rejected_as_non_regular() {
        let directory = tempfile::tempdir().expect("temp directory");

        assert!(matches!(
            ExactColorConfigFile::read(directory.path()),
            Err(ExactColorConfigFileError::NotRegularFile { .. })
        ));
    }

    #[cfg(unix)]
    #[test]
    fn symlink_is_rejected_instead_of_followed() {
        use std::os::unix::fs::symlink;

        let directory = tempfile::tempdir().expect("temp directory");
        let target = directory.path().join("target.ocio");
        let link = directory.path().join("link.ocio");
        fs::write(&target, b"target").expect("write target");
        symlink(&target, &link).expect("create symlink");

        assert!(matches!(
            ExactColorConfigFile::read(&link),
            Err(ExactColorConfigFileError::Symlink { .. })
        ));
    }

    #[cfg(unix)]
    #[test]
    fn fifo_is_rejected_without_waiting_for_a_writer() {
        use std::ffi::CString;
        use std::os::unix::ffi::OsStrExt;

        let directory = tempfile::tempdir().expect("temp directory");
        let path = directory.path().join("config.fifo");
        let c_path = CString::new(path.as_os_str().as_bytes()).expect("FIFO path has no NUL");
        // SAFETY: `c_path` is a live, NUL-terminated pathname and the mode has
        // no bits outside POSIX permission flags.
        let result = unsafe { libc::mkfifo(c_path.as_ptr(), 0o600) };
        assert_eq!(
            result,
            0,
            "mkfifo failed: {}",
            std::io::Error::last_os_error()
        );

        assert!(matches!(
            ExactColorConfigFile::read(&path),
            Err(ExactColorConfigFileError::NotRegularFile { .. })
        ));
    }

    #[cfg(unix)]
    #[test]
    fn character_device_is_rejected_before_content_read() {
        assert!(matches!(
            ExactColorConfigFile::read("/dev/null"),
            Err(ExactColorConfigFileError::NotRegularFile { .. })
        ));
    }

    #[cfg(unix)]
    #[test]
    fn unchanged_opened_identity_reuses_the_bounded_snapshot() {
        let directory = tempfile::tempdir().expect("temp directory");
        let path = directory.path().join("stable.ocio");
        fs::write(&path, b"stable revision").expect("write stable revision");
        let first = ExactColorConfigFile::read(&path).expect("read first snapshot");
        let second = ExactColorConfigFile::read(&path).expect("read cached snapshot");

        assert!(Arc::ptr_eq(&first.bytes, &second.bytes));
        assert!(Arc::ptr_eq(&first.sha256, &second.sha256));
    }

    #[cfg(unix)]
    #[test]
    fn opened_identity_cache_invalidates_after_in_place_change() {
        let directory = tempfile::tempdir().expect("temp directory");
        let path = directory.path().join("changing.ocio");
        fs::write(&path, b"aaaaaaaaaaaaaaaa").expect("write first revision");
        let first = ExactColorConfigFile::read(&path).expect("read first revision");
        fs::write(&path, b"bbbbbbbbbbbbbbbb").expect("write second revision");
        let second = ExactColorConfigFile::read(&path).expect("read second revision");

        assert_ne!(first.sha256(), second.sha256());
        assert_eq!(second.bytes(), b"bbbbbbbbbbbbbbbb");
    }

    #[cfg(windows)]
    #[test]
    fn windows_remote_and_device_namespaces_fail_before_file_io() {
        for locator in [
            r"\\server\share\config.ocio",
            r"\\?\UNC\server\share\config.ocio",
            r"\\.\pipe\ruvie-color",
            r"\\?\GLOBALROOT\Device\HarddiskVolume1\config.ocio",
        ] {
            assert!(matches!(
                validate_local_windows_locator(Path::new(locator)),
                Err(ExactColorConfigFileError::UnsafeLocator { .. })
            ));
        }
    }

    #[cfg(windows)]
    #[test]
    fn windows_local_disk_and_relative_locators_pass_lexical_preflight() {
        for locator in [
            r"C:\show\config.ocio",
            r"\\?\C:\show\config.ocio",
            r"show\config.ocio",
            r"\show\config.ocio",
        ] {
            validate_local_windows_locator(Path::new(locator))
                .expect("local locator must pass lexical preflight");
        }
    }

    #[cfg(windows)]
    #[test]
    fn windows_dos_devices_and_alternate_streams_fail_before_file_io() {
        for locator in [
            r"NUL",
            r"C:\show\CON.ocio",
            r"C:\show\lpt1 .txt",
            r"C:\show\config:payload.ocio",
        ] {
            assert!(matches!(
                validate_local_windows_locator(Path::new(locator)),
                Err(ExactColorConfigFileError::UnsafeLocator { .. })
            ));
        }
    }
}
