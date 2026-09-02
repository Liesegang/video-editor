use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

use super::ProjectDocument;

pub struct ProjectFileStore;

impl ProjectFileStore {
    pub fn load(path: &Path) -> Result<ProjectDocument, String> {
        let source = fs::read_to_string(path)
            .map_err(|error| format!("Cannot read Project '{}': {error}", path.display()))?;
        ProjectDocument::from_json(&source)
    }

    pub fn save(path: &Path, document: &ProjectDocument) -> Result<(), String> {
        let source = document.to_json()?;
        let parent = path
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty());
        let directory = parent.unwrap_or_else(|| Path::new("."));
        let file_name = path
            .file_name()
            .ok_or_else(|| "Project path must name a file".to_string())?
            .to_string_lossy();
        let temporary = unique_temporary_path(directory, &file_name);
        let result = write_and_replace(&temporary, path, source.as_bytes());
        if result.is_err() {
            drop(fs::remove_file(&temporary));
        }
        result.map_err(|error| format!("Cannot save Project '{}': {error}", path.display()))
    }
}

fn unique_temporary_path(directory: &Path, file_name: &str) -> PathBuf {
    directory.join(format!(
        ".{file_name}.{}.tmp",
        uuid::Uuid::new_v4().as_simple()
    ))
}

fn write_and_replace(temporary: &Path, destination: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(temporary)?;
    file.write_all(bytes)?;
    file.sync_all()?;
    drop(file);
    replace_file(temporary, destination)
}

#[cfg(not(windows))]
fn replace_file(temporary: &Path, destination: &Path) -> std::io::Result<()> {
    fs::rename(temporary, destination)
}

#[cfg(windows)]
fn replace_file(temporary: &Path, destination: &Path) -> std::io::Result<()> {
    use std::os::windows::ffi::OsStrExt;

    use windows_sys::Win32::Storage::FileSystem::{
        MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH, MoveFileExW,
    };

    let temporary: Vec<u16> = temporary
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
    // the duration of the call, and the flags require no additional pointers.
    let replaced = unsafe {
        MoveFileExW(
            temporary.as_ptr(),
            destination.as_ptr(),
            MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
        )
    };
    if replaced == 0 {
        Err(std::io::Error::last_os_error())
    } else {
        Ok(())
    }
}
