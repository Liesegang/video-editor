//! Race-resistant opening for automatically consumed local media.
//!
//! Project documents can contain arbitrary strings. Automatic Preview/audio
//! paths must therefore reject URL-like locators and filesystem objects that
//! can block or act like devices before any decoder or third-party plugin sees
//! them. Explicit import/relink policy belongs to the editor layer and must not
//! be inferred from this helper.

use std::fs::{File, Metadata, OpenOptions};
use std::io;
use std::path::{Path, PathBuf};

/// An opened direct local regular file whose final path component was not a
/// symlink when opened.
///
/// Keeping the handle alive lets callers validate before plugin dispatch and
/// closes the FIFO/device blocking class on Unix. Name-based plugin APIs still
/// need their own before/after identity checks for replacement races.
pub(crate) struct DirectRegularFile {
    file: File,
    canonical_path: PathBuf,
}

#[cfg(windows)]
#[derive(Clone, Copy, Debug, Hash, PartialEq, Eq)]
pub(crate) struct WindowsFileIdentity {
    pub(crate) volume_serial: u32,
    pub(crate) file_index: u64,
}

impl DirectRegularFile {
    pub(crate) fn open(path: impl AsRef<Path>) -> io::Result<Self> {
        let path = path.as_ref();
        #[cfg(windows)]
        {
            reject_unsafe_windows_prefix(path)?;
            reject_windows_reserved_components(path)?;
        }
        reject_uri_scheme(path)?;

        let before = std::fs::symlink_metadata(path)?;
        require_regular_nonsymlink(path, &before)?;

        let mut options = OpenOptions::new();
        options.read(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.custom_flags(libc::O_CLOEXEC | libc::O_NOFOLLOW | libc::O_NONBLOCK);
        }
        #[cfg(windows)]
        {
            use std::os::windows::fs::OpenOptionsExt;
            use windows_sys::Win32::Storage::FileSystem::FILE_FLAG_OPEN_REPARSE_POINT;
            options.custom_flags(FILE_FLAG_OPEN_REPARSE_POINT);
        }
        let file = options.open(path)?;
        let opened = file.metadata()?;
        require_regular_nonsymlink(path, &opened)?;
        if !same_file(&before, &opened) {
            return Err(rejected(path, "the path changed while it was being opened"));
        }

        let after = std::fs::symlink_metadata(path)?;
        require_regular_nonsymlink(path, &after)?;
        if !same_file(&opened, &after) {
            return Err(rejected(path, "the path changed after it was opened"));
        }

        let canonical_path = path.canonicalize()?;
        let canonical_metadata = std::fs::metadata(&canonical_path)?;
        if !same_file(&opened, &canonical_metadata) {
            return Err(rejected(
                path,
                "the canonical target changed while it was being verified",
            ));
        }
        #[cfg(windows)]
        verify_windows_identity(path, &canonical_path, &file)?;

        Ok(Self {
            file,
            canonical_path,
        })
    }

    pub(crate) fn file(&self) -> &File {
        &self.file
    }

    pub(crate) fn canonical_path(&self) -> &Path {
        &self.canonical_path
    }

    pub(crate) fn into_file(self) -> File {
        self.file
    }

    #[cfg(windows)]
    pub(crate) fn windows_identity(&self) -> io::Result<WindowsFileIdentity> {
        windows_file_identity(&self.file)
    }
}

#[cfg(windows)]
fn reject_unsafe_windows_prefix(path: &Path) -> io::Result<()> {
    use std::path::{Component, Prefix};

    let Some(Component::Prefix(component)) = path.components().next() else {
        return Ok(());
    };
    match component.kind() {
        Prefix::Disk(_) | Prefix::VerbatimDisk(_) => Ok(()),
        Prefix::UNC(_, _)
        | Prefix::VerbatimUNC(_, _)
        | Prefix::DeviceNS(_)
        | Prefix::Verbatim(_) => Err(rejected(
            path,
            "UNC, network, device-namespace, and generic verbatim paths are not accepted",
        )),
    }
}

#[cfg(windows)]
fn reject_windows_reserved_components(path: &Path) -> io::Result<()> {
    use std::path::Component;

    for component in path.components() {
        let Component::Normal(name) = component else {
            continue;
        };
        let name = name.to_string_lossy();
        if name.contains(':') {
            return Err(rejected(
                path,
                "NTFS alternate-data-stream components are not accepted",
            ));
        }
        if is_windows_reserved_component(&name) {
            return Err(rejected(
                path,
                "Windows reserved DOS/console device names are not accepted",
            ));
        }
    }
    Ok(())
}

#[cfg(windows)]
fn is_windows_reserved_component(component: &str) -> bool {
    let normalized = component.trim_end_matches([' ', '.']);
    let base = normalized
        .split('.')
        .next()
        .unwrap_or_default()
        .trim_end_matches([' ', '.'])
        .to_uppercase();
    if matches!(
        base.as_str(),
        "CON" | "PRN" | "AUX" | "NUL" | "CONIN$" | "CONOUT$" | "CLOCK$"
    ) {
        return true;
    }
    ["COM", "LPT"].into_iter().any(|prefix| {
        base.strip_prefix(prefix).is_some_and(|suffix| {
            matches!(
                suffix,
                "1" | "2" | "3" | "4" | "5" | "6" | "7" | "8" | "9" | "¹" | "²" | "³"
            )
        })
    })
}

fn reject_uri_scheme(path: &Path) -> io::Result<()> {
    let Some(locator) = path.to_str() else {
        return Ok(());
    };
    if has_uri_scheme(locator) {
        return Err(rejected(
            path,
            "URL and URI-scheme locators are not local files",
        ));
    }
    Ok(())
}

fn has_uri_scheme(locator: &str) -> bool {
    let Some(colon) = locator.find(':') else {
        return false;
    };
    #[cfg(windows)]
    if colon == 1
        && locator
            .as_bytes()
            .first()
            .is_some_and(u8::is_ascii_alphabetic)
    {
        return false;
    }
    let mut prefix = locator
        .as_bytes()
        .get(..colon)
        .unwrap_or_default()
        .iter()
        .copied();
    prefix.next().is_some_and(|byte| byte.is_ascii_alphabetic())
        && prefix.all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'+' | b'-' | b'.'))
}

fn require_regular_nonsymlink(path: &Path, metadata: &Metadata) -> io::Result<()> {
    let file_type = metadata.file_type();
    if file_type.is_symlink() {
        return Err(rejected(path, "symbolic links are not accepted"));
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        use windows_sys::Win32::Storage::FileSystem::FILE_ATTRIBUTE_REPARSE_POINT;
        if metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
            return Err(rejected(path, "Windows reparse points are not accepted"));
        }
    }
    if !file_type.is_file() {
        return Err(rejected(
            path,
            "directories, FIFOs, sockets, and devices are not accepted",
        ));
    }
    Ok(())
}

#[cfg(unix)]
fn same_file(left: &Metadata, right: &Metadata) -> bool {
    use std::os::unix::fs::MetadataExt;
    left.dev() == right.dev() && left.ino() == right.ino()
}

#[cfg(windows)]
fn verify_windows_identity(path: &Path, canonical_path: &Path, opened: &File) -> io::Result<()> {
    let expected = windows_file_identity(opened)?;
    for candidate_path in [path, canonical_path] {
        let mut options = OpenOptions::new();
        options.read(true);
        use std::os::windows::fs::OpenOptionsExt;
        use windows_sys::Win32::Storage::FileSystem::FILE_FLAG_OPEN_REPARSE_POINT;
        options.custom_flags(FILE_FLAG_OPEN_REPARSE_POINT);
        let candidate = options.open(candidate_path)?;
        require_regular_nonsymlink(candidate_path, &candidate.metadata()?)?;
        if windows_file_identity(&candidate)? != expected {
            return Err(rejected(
                path,
                "the Windows volume/file identity changed during verification",
            ));
        }
    }
    Ok(())
}

#[cfg(windows)]
pub(crate) fn windows_file_identity(file: &File) -> io::Result<WindowsFileIdentity> {
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::Storage::FileSystem::{
        BY_HANDLE_FILE_INFORMATION, FILE_TYPE_DISK, GetFileInformationByHandle, GetFileType,
    };

    let handle = file.as_raw_handle();
    // SAFETY: `file` owns a valid live handle for this synchronous query.
    let handle_type = unsafe { GetFileType(handle) };
    if handle_type != FILE_TYPE_DISK {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "automatic media requires a disk-file handle on Windows",
        ));
    }
    let mut information = BY_HANDLE_FILE_INFORMATION::default();
    // SAFETY: `file` owns a valid live handle and `information` is writable
    // for the duration of this synchronous Win32 call.
    let success =
        unsafe { GetFileInformationByHandle(handle, std::ptr::from_mut(&mut information)) };
    if success == 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(WindowsFileIdentity {
        volume_serial: information.dwVolumeSerialNumber,
        file_index: u64::from(information.nFileIndexHigh) << 32
            | u64::from(information.nFileIndexLow),
    })
}

#[cfg(not(unix))]
fn same_file(left: &Metadata, right: &Metadata) -> bool {
    left.file_type().is_file()
        && right.file_type().is_file()
        && left.len() == right.len()
        && left.modified().ok() == right.modified().ok()
}

fn rejected(path: &Path, reason: &str) -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidInput,
        format!(
            "automatic media requires a direct local regular file at {:?}: {reason}",
            path
        ),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uri_schemes_are_distinct_from_local_paths() {
        assert!(has_uri_scheme("https://example.invalid/media.png"));
        assert!(has_uri_scheme("file:///tmp/media.png"));
        assert!(!has_uri_scheme("relative/media.png"));
        assert!(!has_uri_scheme("/tmp/media.png"));
    }

    #[test]
    fn regular_file_is_opened_by_verified_handle() -> Result<(), Box<dyn std::error::Error>> {
        let directory = tempfile::tempdir()?;
        let path = directory.path().join("fixture.bin");
        std::fs::write(&path, b"fixture")?;

        let opened = DirectRegularFile::open(&path)?;
        assert_eq!(opened.file().metadata()?.len(), 7);
        assert!(opened.canonical_path().is_absolute());
        Ok(())
    }

    #[cfg(windows)]
    #[test]
    fn windows_network_and_device_prefixes_are_rejected_before_filesystem_access() {
        for locator in [
            r"\\server\share\clip.mp4",
            r"\\?\UNC\server\share\clip.mp4",
            r"\\.\pipe\ruvie-test",
            r"\\?\Volume{00000000-0000-0000-0000-000000000000}\clip.mp4",
        ] {
            assert!(reject_unsafe_windows_prefix(Path::new(locator)).is_err());
        }
        assert!(reject_unsafe_windows_prefix(Path::new(r"C:\media\clip.mp4")).is_ok());
        assert!(reject_unsafe_windows_prefix(Path::new(r"\\?\C:\media\clip.mp4")).is_ok());
    }

    #[cfg(windows)]
    #[test]
    fn windows_reserved_device_components_are_rejected_before_filesystem_access() {
        for locator in [
            r"C:\media\NUL.mp4",
            r"C:\media\con .mov",
            r"C:\media\AUX...",
            r"C:\media\COM1.wav",
            r"C:\media\com¹.wav",
            r"C:\media\LPT9.png",
            r"C:\media\CONIN$",
            r"C:\media\CONOUT$.wav",
            r"C:\media\CLOCK$.wav",
            r"C:\media\clip.mp4:payload",
        ] {
            assert!(reject_windows_reserved_components(Path::new(locator)).is_err());
        }
        assert!(reject_windows_reserved_components(Path::new(r"C:\media\compact1.wav")).is_ok());
    }
}
