use uuid::Uuid;

use super::super::lifecycle::ProjectManager;
use super::{absorb_legacy_transform, insert_transform, resolve_graph_owners, validate_candidate};
use crate::error::LibraryError;
use crate::model::project::NodeContainer;

impl ProjectManager {
    /// Materializes the unique semantic Shape/Image Transform without adding
    /// Image Opacity. Existing authored Transform state is retained exactly.
    pub fn ensure_semantic_container_transform(
        &self,
        owner: NodeContainer,
    ) -> Result<Uuid, LibraryError> {
        let mut project = self
            .project
            .write()
            .map_err(|_| LibraryError::Runtime("Lock Poisoned".to_string()))?;
        let mut candidate = project.clone();
        let mut resolved = resolve_graph_owners(&candidate, owner)?;
        if resolved.transform.is_none() {
            resolved.transform = Some(insert_transform(
                &mut candidate,
                owner,
                &self.plugin_manager,
            )?);
        }
        absorb_legacy_transform(&mut candidate, owner, resolved)?;
        validate_candidate(&candidate, owner)?;
        let transform_id = resolved
            .transform
            .map(|(node_id, _)| node_id)
            .ok_or_else(|| LibraryError::Project(format!("{owner:?} has no semantic Transform")))?;
        *project = candidate;
        Ok(transform_id)
    }
}
