//! Race-resistant opening for regular files under distinct locator policies.
//!
//! Project documents can contain arbitrary strings. Automatic Preview/audio
//! paths must therefore reject URL-like locators and filesystem objects that
//! can block or act like devices before any decoder or third-party plugin sees
//! them. Explicit user-selected output paths may name a Windows network share,
//! but still reject device namespaces, alternate streams, links, and non-files.

use std::fs::{File, Metadata, OpenOptions};
use std::io;
use std::path::{Path, PathBuf};

/// An opened regular file whose final path component was not a symlink when
/// opened and whose directory entry matches the retained handle.
///
/// Keeping the handle alive lets callers validate before plugin dispatch and
/// closes the FIFO/device blocking class on Unix. Name-based plugin APIs still
/// need their own before/after identity checks for replacement races.
pub(crate) struct DirectRegularFile {
    file: File,
    canonical_path: PathBuf,
}

#[derive(Clone, Copy)]
enum RegularFileLocatorPolicy {
    AutomaticMedia,
    ExplicitOutput,
}

impl RegularFileLocatorPolicy {
    fn subject(self) -> &'static str {
        match self {
            Self::AutomaticMedia => "automatic media",
            Self::ExplicitOutput => "explicit output",
        }
    }

    fn requirement(self) -> &'static str {
        match self {
            Self::AutomaticMedia => "a direct local regular file",
            Self::ExplicitOutput => "a direct regular file",
        }
    }
}

#[cfg(windows)]
#[derive(Clone, Copy, Debug, Hash, PartialEq, Eq)]
pub(crate) struct WindowsFileIdentity {
    pub(crate) volume_serial: u32,
    pub(crate) file_index: u64,
}

impl DirectRegularFile {
    pub(crate) fn open(path: impl AsRef<Path>) -> io::Result<Self> {
        Self::open_with_policy(path.as_ref(), RegularFileLocatorPolicy::AutomaticMedia)
    }

    /// Open a host-reserved output file without applying the automatic-media
    /// ban on Windows UNC shares. The common no-follow and identity checks are
    /// deliberately identical to [`Self::open`].
    pub(crate) fn open_explicit_output(path: impl AsRef<Path>) -> io::Result<Self> {
        Self::open_with_policy(path.as_ref(), RegularFileLocatorPolicy::ExplicitOutput)
    }

    fn open_with_policy(path: &Path, policy: RegularFileLocatorPolicy) -> io::Result<Self> {
        validate_locator(path, policy)?;

        let before = std::fs::symlink_metadata(path)?;
        require_regular_nonsymlink(path, &before, policy)?;

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
        require_regular_nonsymlink(path, &opened, policy)?;
        if !same_file(&before, &opened) {
            return Err(rejected(
                path,
                policy,
                "the path changed while it was being opened",
            ));
        }

        let after = std::fs::symlink_metadata(path)?;
        require_regular_nonsymlink(path, &after, policy)?;
        if !same_file(&opened, &after) {
            return Err(rejected(
                path,
                policy,
                "the path changed after it was opened",
            ));
        }

        let canonical_path = path.canonicalize()?;
        let canonical_metadata = std::fs::metadata(&canonical_path)?;
        if !same_file(&opened, &canonical_metadata) {
            return Err(rejected(
                path,
                policy,
                "the canonical target changed while it was being verified",
            ));
        }
        #[cfg(windows)]
        verify_windows_identity(path, &canonical_path, &file, policy)?;

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

/// Validate a user-selected output locator before creating its sibling stage.
/// This is intentionally policy-only and performs no network I/O.
pub(crate) fn validate_explicit_output_path(path: &Path) -> io::Result<()> {
    validate_locator(path, RegularFileLocatorPolicy::ExplicitOutput)
}

fn validate_locator(path: &Path, policy: RegularFileLocatorPolicy) -> io::Result<()> {
    match policy {
        RegularFileLocatorPolicy::AutomaticMedia => {
            // Classify locators before Windows' colon/ADS validation so a URL
            // is rejected for the same reason on every platform.
            reject_uri_scheme(path)?;
            #[cfg(windows)]
            reject_automatic_windows_prefix(path)?;
        }
        RegularFileLocatorPolicy::ExplicitOutput => {
            #[cfg(windows)]
            reject_explicit_output_windows_prefix(path)?;
        }
    }
    #[cfg(windows)]
    reject_windows_reserved_components(path, policy)?;
    Ok(())
}

/// Compare two open handles using the strongest stable file identity exposed
/// by the current platform.
#[cfg(windows)]
pub(crate) fn file_handles_share_identity(left: &File, right: &File) -> io::Result<bool> {
    Ok(windows_file_identity(left)? == windows_file_identity(right)?)
}

/// Compare two open handles using the strongest stable file identity exposed
/// by the current platform.
#[cfg(not(windows))]
pub(crate) fn file_handles_share_identity(left: &File, right: &File) -> io::Result<bool> {
    Ok(same_file(&left.metadata()?, &right.metadata()?))
}

#[cfg(windows)]
fn reject_automatic_windows_prefix(path: &Path) -> io::Result<()> {
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
            RegularFileLocatorPolicy::AutomaticMedia,
            "UNC, network, device-namespace, and generic verbatim paths are not accepted",
        )),
    }
}

#[cfg(windows)]
fn reject_explicit_output_windows_prefix(path: &Path) -> io::Result<()> {
    use std::path::{Component, Prefix};

    let Some(Component::Prefix(component)) = path.components().next() else {
        return Ok(());
    };
    match component.kind() {
        Prefix::Disk(_)
        | Prefix::VerbatimDisk(_)
        | Prefix::UNC(_, _)
        | Prefix::VerbatimUNC(_, _) => Ok(()),
        Prefix::DeviceNS(_) | Prefix::Verbatim(_) => Err(rejected(
            path,
            RegularFileLocatorPolicy::ExplicitOutput,
            "device-namespace and generic verbatim paths are not accepted",
        )),
    }
}

#[cfg(windows)]
fn reject_windows_reserved_components(
    path: &Path,
    policy: RegularFileLocatorPolicy,
) -> io::Result<()> {
    use std::path::Component;

    for component in path.components() {
        let Component::Normal(name) = component else {
            continue;
        };
        let name = name.to_string_lossy();
        if name.contains(':') {
            return Err(rejected(
                path,
                policy,
                "NTFS alternate-data-stream components are not accepted",
            ));
        }
        if is_windows_reserved_component(&name) {
            return Err(rejected(
                path,
                policy,
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
            RegularFileLocatorPolicy::AutomaticMedia,
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

fn require_regular_nonsymlink(
    path: &Path,
    metadata: &Metadata,
    policy: RegularFileLocatorPolicy,
) -> io::Result<()> {
    let file_type = metadata.file_type();
    if file_type.is_symlink() {
        return Err(rejected(path, policy, "symbolic links are not accepted"));
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        use windows_sys::Win32::Storage::FileSystem::FILE_ATTRIBUTE_REPARSE_POINT;
        if metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
            return Err(rejected(
                path,
                policy,
                "Windows reparse points are not accepted",
            ));
        }
    }
    if !file_type.is_file() {
        return Err(rejected(
            path,
            policy,
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
fn verify_windows_identity(
    path: &Path,
    canonical_path: &Path,
    opened: &File,
    policy: RegularFileLocatorPolicy,
) -> io::Result<()> {
    let expected = windows_file_identity(opened)?;
    for candidate_path in [path, canonical_path] {
        let mut options = OpenOptions::new();
        options.read(true);
        use std::os::windows::fs::OpenOptionsExt;
        use windows_sys::Win32::Storage::FileSystem::FILE_FLAG_OPEN_REPARSE_POINT;
        options.custom_flags(FILE_FLAG_OPEN_REPARSE_POINT);
        let candidate = options.open(candidate_path)?;
        require_regular_nonsymlink(candidate_path, &candidate.metadata()?, policy)?;
        if windows_file_identity(&candidate)? != expected {
            return Err(rejected(
                path,
                policy,
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
            "regular file identity requires a disk-file handle on Windows",
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

fn rejected(path: &Path, policy: RegularFileLocatorPolicy, reason: &str) -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidInput,
        format!(
            "{} requires {} at {:?}: {reason}",
            policy.subject(),
            policy.requirement(),
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
    fn windows_prefix_policy_distinguishes_automatic_media_from_explicit_output() {
        for locator in [r"\\server\share\clip.mp4", r"\\?\UNC\server\share\clip.mp4"] {
            let path = Path::new(locator);
            assert!(reject_automatic_windows_prefix(path).is_err());
            assert!(reject_explicit_output_windows_prefix(path).is_ok());
        }
        for locator in [
            r"\\.\pipe\ruvie-test",
            r"\\?\Volume{00000000-0000-0000-0000-000000000000}\clip.mp4",
        ] {
            let path = Path::new(locator);
            assert!(reject_automatic_windows_prefix(path).is_err());
            assert!(reject_explicit_output_windows_prefix(path).is_err());
        }
        for locator in [r"C:\media\clip.mp4", r"\\?\C:\media\clip.mp4"] {
            let path = Path::new(locator);
            assert!(reject_automatic_windows_prefix(path).is_ok());
            assert!(reject_explicit_output_windows_prefix(path).is_ok());
        }
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
            for policy in [
                RegularFileLocatorPolicy::AutomaticMedia,
                RegularFileLocatorPolicy::ExplicitOutput,
            ] {
                assert!(reject_windows_reserved_components(Path::new(locator), policy).is_err());
            }
        }
        assert!(
            reject_windows_reserved_components(
                Path::new(r"C:\media\compact1.wav"),
                RegularFileLocatorPolicy::ExplicitOutput,
            )
            .is_ok()
        );
    }
}
