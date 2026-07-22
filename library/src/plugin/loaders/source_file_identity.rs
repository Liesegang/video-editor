use crate::error::LibraryError;
use crate::util::local_file::DirectRegularFile;
#[cfg(windows)]
use crate::util::local_file::WindowsFileIdentity;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

/// Filesystem identity used by loader and pixel caches.
///
/// Path alone is insufficient because media is routinely replaced in place.
#[derive(Clone, Debug, Hash, PartialEq, Eq)]
pub(crate) struct FileIdentity {
    pub(super) canonical_path: PathBuf,
    length: u64,
    modified_nanos: u128,
    #[cfg(unix)]
    device: u64,
    #[cfg(unix)]
    inode: u64,
    #[cfg(unix)]
    change_seconds: i64,
    #[cfg(unix)]
    change_nanos: i64,
    #[cfg(windows)]
    windows_identity: WindowsFileIdentity,
}

impl FileIdentity {
    pub(crate) fn read(path: &str) -> Result<Self, LibraryError> {
        let opened = DirectRegularFile::open(path)?;
        let canonical_path = opened.canonical_path().to_path_buf();
        let metadata = opened.file().metadata()?;
        let modified_nanos = metadata
            .modified()
            .ok()
            .and_then(|modified| modified.duration_since(UNIX_EPOCH).ok())
            .map(|duration| duration.as_nanos())
            .unwrap_or_default();
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            Ok(Self {
                canonical_path,
                length: metadata.len(),
                modified_nanos,
                device: metadata.dev(),
                inode: metadata.ino(),
                change_seconds: metadata.ctime(),
                change_nanos: metadata.ctime_nsec(),
            })
        }
        #[cfg(not(unix))]
        {
            Ok(Self {
                canonical_path,
                length: metadata.len(),
                modified_nanos,
                #[cfg(windows)]
                windows_identity: opened.windows_identity()?,
            })
        }
    }

    pub(crate) fn cache_token(&self) -> String {
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        self.hash(&mut hasher);
        format!("{:016x}", hasher.finish())
    }

    pub(crate) fn canonical_path(&self) -> &Path {
        &self.canonical_path
    }
}
