use super::editor_service::EditorService;
use crate::error::LibraryError;
use crate::model::project::asset::Asset;
use crate::model::project::project::Composition;
use uuid::Uuid;

/// Project management, asset, and composition operations.
impl EditorService {
    pub fn load_project(&self, json_str: &str) -> Result<(), LibraryError> {
        let new_project = self.project_manager.load_project(json_str)?;

        // Hydrate Audio Cache (Orchestration logic)
        for asset in &new_project.assets {
            if asset.kind == crate::model::project::asset::AssetKind::Audio {
                self.audio_service
                    .trigger_audio_loading(asset.id, asset.path.clone());
            }
        }

        Ok(())
    }

    pub fn create_new_project(&self) -> Result<Uuid, LibraryError> {
        let (new_comp_id, _) = self.project_manager.create_new_project()?;
        Ok(new_comp_id)
    }

    pub fn save_project(&self) -> Result<String, LibraryError> {
        self.project_manager.save_project()
    }

    pub fn import_file(&self, path: &str) -> Result<Vec<Uuid>, LibraryError> {
        let asset_ids = self.project_manager.import_file(path)?;

        if let Ok(project) = self.project_manager.get_project().read() {
            for &asset_id in &asset_ids {
                if let Some(asset) = project.assets.iter().find(|a| a.id == asset_id) {
                    if asset.kind == crate::model::project::asset::AssetKind::Audio {
                        let path_clone = asset.path.clone();
                        self.audio_service
                            .trigger_audio_loading(asset_id, path_clone);
                    }
                }
            }
        }

        Ok(asset_ids)
    }

    pub fn load_project_from_path(&self, path: &std::path::Path) -> Result<(), LibraryError> {
        let content = std::fs::read_to_string(path)
            .map_err(|e| LibraryError::Runtime(format!("Failed to read project file: {}", e)))?;
        self.load_project(&content)
    }

    // --- Asset Operations ---

    pub fn add_asset(&self, asset: Asset) -> Result<Uuid, LibraryError> {
        self.project_manager.add_asset(asset)
    }

    pub fn is_asset_used(&self, asset_id: Uuid) -> bool {
        self.project_manager.is_asset_used(asset_id)
    }

    pub fn remove_asset(&self, asset_id: Uuid) -> Result<(), LibraryError> {
        self.project_manager.remove_asset(asset_id)
    }

    pub fn remove_asset_fully(&self, asset_id: Uuid) -> Result<(), LibraryError> {
        self.project_manager.remove_asset_fully(asset_id)
    }

    pub fn has_asset_with_path(&self, path: &str) -> bool {
        self.project_manager.has_asset_with_path(path)
    }

    // --- Composition Operations ---

    pub fn add_composition(
        &self,
        name: &str,
        width: u32,
        height: u32,
        fps: f64,
        duration: f64,
    ) -> Result<Uuid, LibraryError> {
        self.project_manager
            .add_composition(name, width, height, fps, duration)
    }

    pub fn update_composition(
        &self,
        id: Uuid,
        name: &str,
        width: u32,
        height: u32,
        fps: f64,
        duration: f64,
    ) -> Result<(), LibraryError> {
        self.project_manager
            .update_composition(id, name, width, height, fps, duration)
    }

    pub fn get_composition(&self, id: Uuid) -> Result<Composition, LibraryError> {
        self.project_manager.get_composition(id)
    }

    pub fn is_composition_used(&self, comp_id: Uuid) -> bool {
        self.project_manager.is_composition_used(comp_id)
    }

    pub fn remove_composition_fully(&self, comp_id: Uuid) -> Result<(), LibraryError> {
        self.project_manager.remove_composition_fully(comp_id)
    }
}
