use std::fs;
use std::path::Path;

use crate::util::atomic_file::atomic_write;

use super::ProjectDocument;

/// Atomic format-v1 storage. There is deliberately no pre-v1 reader/writer or
/// bidirectional compatibility model.
pub struct ProjectFileStore;

impl ProjectFileStore {
    pub fn load(path: &Path) -> Result<ProjectDocument, String> {
        let source = fs::read_to_string(path)
            .map_err(|error| format!("Cannot read Project '{}': {error}", path.display()))?;
        ProjectDocument::from_json(&source)
    }

    pub fn save(path: &Path, document: &ProjectDocument) -> Result<(), String> {
        let source = document.to_json()?;
        atomic_write(path, source.as_bytes())
            .map_err(|error| format!("Cannot save Project '{}': {error}", path.display()))
    }
}
