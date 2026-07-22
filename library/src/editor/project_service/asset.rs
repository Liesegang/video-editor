//! Asset import, usage queries, and removal commands.

use super::lifecycle::ProjectManager;
use crate::editor::handlers;
use crate::error::LibraryError;
use crate::model::NodeContent;
use crate::model::asset::Asset;
use uuid::Uuid;

impl ProjectManager {
    pub fn add_asset(&self, asset: Asset) -> Result<Uuid, LibraryError> {
        handlers::asset_handler::AssetHandler::add_asset(&self.project, asset)
    }

    pub fn is_asset_used(&self, asset_id: Uuid) -> bool {
        handlers::asset_handler::AssetHandler::is_asset_used(&self.project, asset_id)
    }

    pub fn remove_asset(&self, asset_id: Uuid) -> Result<(), LibraryError> {
        handlers::asset_handler::AssetHandler::remove_asset(&self.project, asset_id)
    }

    pub fn remove_asset_fully(&self, asset_id: Uuid) -> Result<(), LibraryError> {
        let mut project_write = self.project.write().map_err(|e| {
            LibraryError::Runtime(format!("Failed to acquire project write lock: {}", e))
        })?;

        let media_node_ids: Vec<Uuid> = project_write
            .nodes
            .values()
            .filter_map(|node| match node.content() {
                NodeContent::Media(media) if media.asset_id == asset_id => Some(node.id),
                _ => None,
            })
            .collect();
        let clip_ids_to_remove: std::collections::HashSet<_> = media_node_ids
            .iter()
            .filter_map(|node_id| project_write.find_parent_clip(*node_id))
            .collect();
        for clip_id in clip_ids_to_remove {
            project_write.remove_clip(clip_id);
        }
        for node_id in media_node_ids {
            project_write
                .remove_node(node_id)
                .map_err(|error| LibraryError::Project(error.to_string()))?;
        }

        // Remove the asset itself
        project_write.assets.retain(|a| a.id != asset_id);
        Ok(())
    }

    pub fn import_file(&self, path: &str) -> Result<Vec<Uuid>, LibraryError> {
        let path_obj = std::path::Path::new(path);
        let base_name = path_obj
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .to_string();

        let mut assets_to_add = Vec::new();

        // 1. Try to get all streams
        if let Some(streams) = self.plugin_manager.get_available_streams(path)? {
            for stream in streams {
                let suffix = if let Some(idx) = stream.stream_index {
                    format!(" [Stream {}: {:?}]", idx, stream.kind)
                } else {
                    "".to_string()
                };
                let name = format!("{}{}", base_name, suffix);

                let mut asset = crate::model::asset::Asset::new(&name, path, stream.kind);
                asset.duration = stream.duration;
                asset.fps = stream.fps;
                asset.width = stream.width;
                asset.height = stream.height;
                asset.stream_index = stream.stream_index;
                asset.source_color.replace_detected(stream.source_color);
                if asset.kind == crate::model::asset::AssetKind::Video {
                    asset.frame_count = stream.frame_count;
                }

                assets_to_add.push(asset);
            }
        }

        // 2. Fallback if no streams returned (or empty list)
        if assets_to_add.is_empty() {
            // 1. Get Metadata (Single call)
            let (mut kind, duration, fps, width, height, frame_count, source_color) =
                if let Some(meta) = self.plugin_manager.get_metadata(path)? {
                    (
                        meta.kind,
                        meta.duration,
                        meta.fps,
                        meta.width,
                        meta.height,
                        meta.frame_count,
                        meta.source_color,
                    )
                } else {
                    (
                        crate::model::asset::AssetKind::Other,
                        None,
                        None,
                        None,
                        None,
                        None,
                        crate::model::asset::SourceColorDescription::default(),
                    )
                };

            // 2. Fallback for Kind if Unknown
            if kind == crate::model::asset::AssetKind::Other {
                // Fallback to extension if plugin didn't detect it
                let ext = path_obj
                    .extension()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .to_lowercase();
                kind = match ext.as_str() {
                    "mp4" | "mov" | "avi" | "mkv" | "webm" => crate::model::asset::AssetKind::Video,
                    "png" | "jpg" | "jpeg" | "bmp" | "webp" => {
                        crate::model::asset::AssetKind::Image
                    }
                    "mp3" | "wav" | "ogg" | "aac" | "flac" => crate::model::asset::AssetKind::Audio,
                    "obj" | "gltf" | "glb" => crate::model::asset::AssetKind::Model3D,
                    _ => crate::model::asset::AssetKind::Other,
                };
            }

            // 3. Create Asset
            let mut asset = crate::model::asset::Asset::new(&base_name, path, kind);
            asset.duration = duration;
            asset.fps = fps;
            asset.width = width;
            asset.height = height;
            asset.source_color.replace_detected(source_color);
            if asset.kind == crate::model::asset::AssetKind::Video {
                asset.frame_count = frame_count;
            }
            // stream_index remains None

            assets_to_add.push(asset);
        }

        let mut added_ids = Vec::new();
        for asset in assets_to_add {
            let id = self.add_asset(asset)?;
            added_ids.push(id);
        }

        Ok(added_ids)
    }

    pub fn has_asset_with_path(&self, path: &str) -> bool {
        if let Ok(project) = self.project.read() {
            let path_norm = std::path::Path::new(path).to_string_lossy().to_string();
            project.assets.iter().any(|asset| {
                let asset_norm = std::path::Path::new(&asset.path)
                    .to_string_lossy()
                    .to_string();
                asset_norm == path_norm
            })
        } else {
            false
        }
    }
}
