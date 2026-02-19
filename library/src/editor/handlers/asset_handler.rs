use crate::error::LibraryError;
use crate::model::project::asset::Asset;
use crate::model::project::project::Project;
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
            // Check all clips in the nodes registry
            for clip in proj.all_clips() {
                if let Some(ref r) = clip.reference_id {
                    if *r == asset_id {
                        return true;
                    }
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
            Err(LibraryError::project("Entity not found".to_string()))
        }
    }
}
