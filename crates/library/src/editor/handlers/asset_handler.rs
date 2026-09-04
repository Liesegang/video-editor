use crate::error::LibraryError;
use crate::model::asset::Asset;
use crate::model::project::Project;
use std::sync::{Arc, RwLock};
use uuid::Uuid;

pub struct AssetHandler;

impl AssetHandler {
    pub fn add_asset(project: &Arc<RwLock<Project>>, asset: Asset) -> Result<Uuid, LibraryError> {
        let mut proj = project
            .write()
            .map_err(|_| LibraryError::Runtime("Lock Poisoned".to_string()))?;
        let id = asset.id;
        proj.assets.push(asset);
        Ok(id)
    }

    pub fn is_asset_used(project: &Arc<RwLock<Project>>, asset_id: Uuid) -> bool {
        if let Ok(proj) = project.read() {
            for node in proj.nodes.values() {
                if matches!(
                    node.content(),
                    crate::model::NodeContent::Media(media) if media.asset_id == asset_id
                ) {
                    return true;
                }
            }
        }
        false
    }

    pub fn remove_asset(
        project: &Arc<RwLock<Project>>,
        asset_id: Uuid,
    ) -> Result<(), LibraryError> {
        let mut proj = project
            .write()
            .map_err(|_| LibraryError::Runtime("Lock Poisoned".to_string()))?;
        if let Some(pos) = proj.assets.iter().position(|a| a.id == asset_id) {
            proj.assets.remove(pos);
            Ok(())
        } else {
            Err(LibraryError::Project("Entity not found".to_string()))
        }
    }
}
