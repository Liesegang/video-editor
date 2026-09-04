//! Filesystem-aware identity for an export destination or Project input.
//!
//! This does not open a destination. Existing paths are canonicalized and
//! compared by stable filesystem identity; paths that do not exist yet are
//! resolved through their nearest existing ancestor. Callers can therefore
//! compare relative, absolute, symlinked, and hard-linked aliases before an
//! exporter is allowed to create or truncate anything.

use crate::error::LibraryError;
use std::fs;
use std::path::{Component, Path, PathBuf};

const MAX_SYMLINK_DEPTH: usize = 32;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum ExistingFileIdentity {
    #[cfg(unix)]
    Unix { device: u64, inode: u64 },
    #[cfg(windows)]
    Windows { volume: u32, index: u64 },
}

/// Lexical/canonical identity plus stable identity for an existing file.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct OutputPathIdentity {
    resolved_path: PathBuf,
    path_key: PathBuf,
    existing_file: Option<ExistingFileIdentity>,
}

impl OutputPathIdentity {
    pub(crate) fn aliases(&self, other: &Self) -> bool {
        self.path_key == other.path_key
            || self
                .existing_file
                .zip(other.existing_file)
                .is_some_and(|(left, right)| left == right)
    }

    pub(crate) fn refresh_existing_file(&mut self) {
        self.existing_file = fs::metadata(&self.resolved_path)
            .ok()
            .and_then(|metadata| existing_file_identity(&self.resolved_path, &metadata));
    }
}

/// Resolve a path without opening its final destination.
pub(crate) fn output_path_identity(path: &str) -> Result<OutputPathIdentity, LibraryError> {
    if path.trim().is_empty() {
        return Err(LibraryError::Render(
            "export/input path must not be empty".to_string(),
        ));
    }
    let requested = PathBuf::from(path);
    let absolute = if requested.is_absolute() {
        requested
    } else {
        std::env::current_dir()?.join(requested)
    };
    let resolved_path = resolve_absolute(&absolute, 0)?;
    let existing_file = match fs::metadata(&resolved_path) {
        Ok(metadata) if metadata.file_type().is_file() => {
            existing_file_identity(&resolved_path, &metadata)
        }
        Ok(_) => {
            return Err(LibraryError::Render(format!(
                "path '{}' resolves to a directory, FIFO, socket, or device instead of a regular file",
                path
            )));
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
        Err(error) => return Err(error.into()),
    };
    Ok(OutputPathIdentity {
        path_key: platform_path(resolved_path.clone()),
        resolved_path,
        existing_file,
    })
}

#[cfg(unix)]
fn existing_file_identity(_path: &Path, metadata: &fs::Metadata) -> Option<ExistingFileIdentity> {
    use std::os::unix::fs::MetadataExt;

    Some(ExistingFileIdentity::Unix {
        device: metadata.dev(),
        inode: metadata.ino(),
    })
}

#[cfg(windows)]
fn existing_file_identity(path: &Path, _metadata: &fs::Metadata) -> Option<ExistingFileIdentity> {
    let file = fs::File::open(path).ok()?;
    let identity = crate::util::local_file::windows_file_identity(&file).ok()?;
    Some(ExistingFileIdentity::Windows {
        volume: identity.volume_serial,
        index: identity.file_index,
    })
}

#[cfg(not(any(unix, windows)))]
fn existing_file_identity(_path: &Path, _metadata: &fs::Metadata) -> Option<ExistingFileIdentity> {
    None
}

fn resolve_absolute(path: &Path, depth: usize) -> Result<PathBuf, LibraryError> {
    if depth >= MAX_SYMLINK_DEPTH {
        return Err(LibraryError::Render(format!(
            "path '{}' exceeds the symlink resolution limit",
            path.display()
        )));
    }

    // Walk every component instead of canonicalizing only the nearest
    // existing ancestor. A dangling symlink can legally be an intermediate
    // component (`alias/missing.png`); if its target is created later, a
    // lexical ancestor fallback would otherwise give the destination a false
    // identity and could miss an input-source alias.
    let components = path.components().collect::<Vec<_>>();
    let mut resolved = PathBuf::new();
    for (index, component) in components.iter().enumerate() {
        match component {
            Component::CurDir => continue,
            Component::ParentDir => {
                resolved.pop();
                continue;
            }
            Component::Prefix(_) | Component::RootDir => {
                resolved.push(component.as_os_str());
                continue;
            }
            Component::Normal(_) => {}
        }

        let candidate = resolved.join(component.as_os_str());
        match fs::symlink_metadata(&candidate) {
            Ok(metadata) if metadata.file_type().is_symlink() => {
                let target = fs::read_link(&candidate)?;
                let mut redirected = if target.is_absolute() {
                    target
                } else {
                    resolved.join(target)
                };
                for remaining in &components[index + 1..] {
                    redirected.push(remaining.as_os_str());
                }
                return resolve_absolute(&redirected, depth + 1);
            }
            Ok(_) => resolved = candidate,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                // Keep walking: later `..` components may return to an
                // existing prefix, whose following symlinks must still be
                // inspected. Treating that spelling as an alias is a safe
                // false positive even if the OS would currently reject it.
                resolved = candidate;
            }
            Err(error) => {
                return Err(LibraryError::Render(format!(
                    "cannot inspect path component '{}': {error}",
                    candidate.display()
                )));
            }
        }
    }

    match fs::canonicalize(&resolved) {
        Ok(canonical) => Ok(canonical),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            Ok(normalize_lexically(&resolved))
        }
        Err(error) => Err(LibraryError::Render(format!(
            "cannot resolve path '{}': {error}",
            resolved.display()
        ))),
    }
}

fn normalize_lexically(path: &Path) -> PathBuf {
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                if matches!(
                    normalized.components().next_back(),
                    Some(Component::Normal(_))
                ) {
                    normalized.pop();
                }
            }
            Component::Prefix(_) | Component::RootDir | Component::Normal(_) => {
                normalized.push(component.as_os_str());
            }
        }
    }
    normalized
}

#[cfg(any(windows, target_os = "macos"))]
fn platform_path(path: PathBuf) -> PathBuf {
    PathBuf::from(path.to_string_lossy().to_lowercase())
}

#[cfg(not(any(windows, target_os = "macos")))]
fn platform_path(path: PathBuf) -> PathBuf {
    path
}

#[cfg(test)]
mod tests {
    use super::output_path_identity;
    use std::fs;
    use uuid::Uuid;

    #[test]
    fn relative_and_absolute_paths_share_one_identity() {
        let name = format!("ruvie-destination-{}.mp4", Uuid::new_v4());
        let absolute = std::env::current_dir().unwrap().join(&name);
        assert_eq!(
            output_path_identity(&name).unwrap(),
            output_path_identity(absolute.to_str().unwrap()).unwrap()
        );
    }

    #[test]
    fn missing_nested_paths_resolve_dot_and_parent_aliases() {
        let directory = tempfile::tempdir().unwrap();
        let direct = directory.path().join("missing").join("out.mp4");
        let alias = directory
            .path()
            .join("unused")
            .join("..")
            .join("missing")
            .join("out.mp4");

        assert_eq!(
            output_path_identity(direct.to_str().unwrap()).unwrap(),
            output_path_identity(alias.to_str().unwrap()).unwrap()
        );
    }

    #[cfg(unix)]
    #[test]
    fn dangling_final_symlink_resolves_to_its_real_target() {
        use std::os::unix::fs::symlink;

        let directory = tempfile::tempdir().unwrap();
        let target = directory.path().join("target.mp4");
        let alias = directory.path().join("alias.mp4");
        symlink(&target, &alias).unwrap();

        assert_eq!(
            output_path_identity(target.to_str().unwrap()).unwrap(),
            output_path_identity(alias.to_str().unwrap()).unwrap()
        );
    }

    #[cfg(unix)]
    #[test]
    fn dangling_intermediate_symlink_resolves_the_missing_child_target() {
        use std::os::unix::fs::symlink;

        let directory = tempfile::tempdir().unwrap();
        let target_directory = directory.path().join("future-source");
        let target = target_directory.join("frame.png");
        let alias_directory = directory.path().join("alias");
        let alias = alias_directory.join("frame.png");
        symlink(&target_directory, &alias_directory).unwrap();

        assert_eq!(
            output_path_identity(target.to_str().unwrap()).unwrap(),
            output_path_identity(alias.to_str().unwrap()).unwrap()
        );
    }

    #[cfg(any(unix, windows))]
    #[test]
    fn existing_hard_links_share_one_file_identity() {
        let directory = tempfile::tempdir().unwrap();
        let first = directory.path().join("first.mp4");
        let second = directory.path().join("second.mp4");
        fs::write(&first, b"existing output").unwrap();
        fs::hard_link(&first, &second).unwrap();

        let first = output_path_identity(first.to_str().unwrap()).unwrap();
        let second = output_path_identity(second.to_str().unwrap()).unwrap();
        assert!(first.aliases(&second));
    }
}
