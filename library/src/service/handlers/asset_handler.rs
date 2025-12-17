use crate::error::LibraryError;
use crate::model::project::asset::{Asset, AssetKind};
use crate::model::project::project::Project;
use std::path::{Path, PathBuf};
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
            for comp in &proj.compositions {
                for track in &comp.tracks {
                    for clip in &track.clips {
                        if let Some(ref r) = clip.reference_id {
                            if *r == asset_id {
                                return true;
                            }
                        }
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
            Err(LibraryError::Project("Entity not found".to_string()))
        }
    }

    pub fn import_file(project: &Arc<RwLock<Project>>, path: &str) -> Result<Uuid, LibraryError> {
        let path_obj = Path::new(path);
        let name = path_obj
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .to_string();

        let ext = path_obj
            .extension()
            .unwrap_or_default()
            .to_string_lossy()
            .to_lowercase();

        let kind = match ext.as_str() {
            "mp4" | "mov" | "avi" | "mkv" => AssetKind::Video,
            "png" | "jpg" | "jpeg" | "bmp" => AssetKind::Image,
            "mp3" | "wav" | "ogg" | "aac" => AssetKind::Audio,
            _ => AssetKind::Other,
        };

        let asset = Asset::new(&name, path, kind);

        Self::add_asset(project, asset)
    }
}
