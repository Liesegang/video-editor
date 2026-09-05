//! Asset import, usage queries, and removal commands.

use super::lifecycle::ProjectManager;
use crate::editor::asset_import::probe_assets_for_import;
use crate::editor::handlers;
use crate::error::LibraryError;
use crate::model::asset::{Asset, AssetKind, SourceColorDescription};
use crate::model::project::{ColorConfigIdentity, NodeContainer, ResolvedColorManagementConfig};
use crate::model::{NodeContent, active_legacy_media_color_properties};
use std::collections::BTreeSet;
use uuid::Uuid;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AssetSourceColorInspectorInterpretation {
    /// Decoder metadata or the explicit persisted import assumption remains
    /// authoritative. An empty description is not permission to guess.
    Automatic(SourceColorDescription),
    AuthoredDescription(SourceColorDescription),
    Assigned {
        color_space: String,
        exact_active_config: bool,
    },
    Malformed {
        detail: String,
    },
}

/// Ephemeral Timeline/Preview Inspector projection for one Media Asset.
/// Nothing in this structure is a second authored model.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AssetSourceColorInspector {
    pub asset_id: Uuid,
    pub asset_name: String,
    pub source_node_ids: Vec<Uuid>,
    pub interpretation: AssetSourceColorInspectorInterpretation,
    /// Exact config-local choices. A complete list is exposed only when the
    /// active Project config has a matching trusted enumeration API.
    pub assignable_color_spaces: Vec<String>,
    pub assignment_list_complete: bool,
    pub diagnostic: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SourceColorMetadataRefresh {
    pub asset_id: Uuid,
    pub changed: bool,
    pub diagnostic: Option<String>,
}

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

    /// Projects Asset source-color authority into a Clip/Track/Composition
    /// semantic Inspector without copying it into Media Node properties.
    pub fn asset_source_color_inspectors(
        &self,
        owner: NodeContainer,
    ) -> Result<Vec<AssetSourceColorInspector>, LibraryError> {
        let project = self.project.read().map_err(|_| {
            LibraryError::Runtime("Failed to acquire Project for source color Inspector".into())
        })?;
        let node_ids = container_node_ids(&project, owner)?;
        let mut by_asset = std::collections::BTreeMap::<Uuid, Vec<Uuid>>::new();
        for node_id in node_ids {
            if let Some(NodeContent::Media(media)) =
                project.get_node(*node_id).map(|node| node.content())
                && project
                    .get_asset(media.asset_id)
                    .is_some_and(|asset| matches!(asset.kind, AssetKind::Image | AssetKind::Video))
            {
                by_asset.entry(media.asset_id).or_default().push(*node_id);
            }
        }
        Ok(by_asset
            .into_iter()
            .filter_map(|(asset_id, source_node_ids)| {
                let asset = project.get_asset(asset_id)?;
                Some(source_color_inspector(&project, asset, source_node_ids))
            })
            .collect())
    }

    /// Assign one of the trusted, enumerated spaces from the exact active
    /// Project config. Callers cannot pass a free config-less string.
    pub fn assign_asset_source_color_space(
        &self,
        asset_id: Uuid,
        color_space: &str,
    ) -> Result<(), LibraryError> {
        let mut project = self.project.write().map_err(|_| {
            LibraryError::Runtime("Failed to acquire Project for source color assignment".into())
        })?;
        let resolved = project.resolved_color_management();
        let ResolvedColorManagementConfig::Ready(intent) = resolved else {
            return Err(LibraryError::Validation(
                "Cannot assign an Asset source space while the Project color configuration is unavailable"
                    .into(),
            ));
        };
        let choices = exact_assignable_color_spaces(intent.config())?;
        if !choices.iter().any(|choice| choice == color_space) {
            return Err(LibraryError::Validation(format!(
                "Source color space {color_space:?} was not issued by the exact active Project config"
            )));
        }
        let binding = intent
            .config()
            .source_space_binding(color_space)
            .map_err(|error| LibraryError::Validation(error.to_string()))?;
        let asset = project
            .assets
            .iter_mut()
            .find(|asset| asset.id == asset_id)
            .ok_or_else(|| LibraryError::Validation(format!("Asset {asset_id} was not found")))?;
        if !matches!(asset.kind, AssetKind::Image | AssetKind::Video) {
            return Err(LibraryError::Validation(format!(
                "Asset {asset_id} is not an Image or Video source"
            )));
        }
        asset.source_color.assign_space(binding);
        Ok(())
    }

    pub fn clear_asset_source_color_space(&self, asset_id: Uuid) -> Result<(), LibraryError> {
        let mut project = self.project.write().map_err(|_| {
            LibraryError::Runtime("Failed to acquire Project for source color repair".into())
        })?;
        let asset = project
            .assets
            .iter_mut()
            .find(|asset| asset.id == asset_id)
            .ok_or_else(|| LibraryError::Validation(format!("Asset {asset_id} was not found")))?;
        asset.source_color.clear_assigned_space();
        Ok(())
    }

    /// Explicitly returns the Asset to fresh decoded/import metadata. This is
    /// the only UI action that clears both a config-bound assignment and a
    /// complete authored CICP/profile override.
    pub fn use_detected_asset_source_color(&self, asset_id: Uuid) -> Result<(), LibraryError> {
        let mut project = self.project.write().map_err(|_| {
            LibraryError::Runtime("Failed to acquire Project for source color repair".into())
        })?;
        let asset = project
            .assets
            .iter_mut()
            .find(|asset| asset.id == asset_id)
            .ok_or_else(|| LibraryError::Validation(format!("Asset {asset_id} was not found")))?;
        asset.source_color.clear_assigned_space();
        asset.source_color.clear_override();
        Ok(())
    }

    /// Explicitly removes only the retired Node-local color fields. Asset
    /// authority is intentionally left untouched so clearing cannot silently
    /// manufacture a replacement interpretation.
    pub fn clear_legacy_media_node_color_properties(
        &self,
        node_id: Uuid,
    ) -> Result<(), LibraryError> {
        let mut project = self.project.write().map_err(|_| {
            LibraryError::Runtime("Failed to acquire Project for legacy color repair".into())
        })?;
        let node = project
            .get_node_mut(node_id)
            .ok_or_else(|| LibraryError::Validation(format!("Node {node_id} was not found")))?;
        if active_legacy_media_color_properties(node).is_empty() {
            return Err(LibraryError::Validation(format!(
                "Node {node_id} has no active deprecated color fields"
            )));
        }
        if !node.clear_legacy_media_color_properties() {
            return Err(LibraryError::Validation(format!(
                "Node {node_id} is not a Media Node"
            )));
        }
        Ok(())
    }

    /// Fresh metadata repair used by the Timeline Inspector and relink flows.
    /// Failure never mutates the Asset and never substitutes a runtime guess.
    pub fn refresh_asset_source_color_metadata(
        &self,
        asset_id: Uuid,
    ) -> Result<SourceColorMetadataRefresh, LibraryError> {
        let asset = self
            .project
            .read()
            .map_err(|_| LibraryError::Runtime("Failed to acquire Project Asset".into()))?
            .get_asset(asset_id)
            .cloned()
            .ok_or_else(|| LibraryError::Validation(format!("Asset {asset_id} was not found")))?;
        let detected = match probe_source_color(self.plugin_manager.as_ref(), &asset) {
            Ok(detected) => detected,
            Err(diagnostic) => {
                return Ok(SourceColorMetadataRefresh {
                    asset_id,
                    changed: false,
                    diagnostic: Some(diagnostic),
                });
            }
        };
        let mut project = self.project.write().map_err(|_| {
            LibraryError::Runtime("Failed to acquire Project for source color refresh".into())
        })?;
        let current = project
            .assets
            .iter_mut()
            .find(|candidate| candidate.id == asset_id)
            .ok_or_else(|| LibraryError::Validation(format!("Asset {asset_id} was removed")))?;
        if current.path != asset.path
            || current.kind != asset.kind
            || current.stream_index != asset.stream_index
        {
            return Err(LibraryError::Validation(format!(
                "Asset {asset_id} changed while its source metadata was being probed"
            )));
        }
        let changed = current.source_color.detected() != &detected;
        current.source_color.replace_detected(detected);
        Ok(SourceColorMetadataRefresh {
            asset_id,
            changed,
            diagnostic: None,
        })
    }

    pub fn import_file(&self, path: &str) -> Result<Vec<Uuid>, LibraryError> {
        let assets_to_add =
            probe_assets_for_import(std::path::Path::new(path), &self.plugin_manager)?;
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

fn container_node_ids(
    project: &crate::model::Project,
    owner: NodeContainer,
) -> Result<&[Uuid], LibraryError> {
    match owner {
        NodeContainer::Composition(id) => project
            .get_composition(id)
            .map(|composition| composition.node_ids.as_slice()),
        NodeContainer::Track(id) => project.get_track(id).map(|track| track.node_ids.as_slice()),
        NodeContainer::Clip(id) => project.get_clip(id).map(|clip| clip.node_ids.as_slice()),
    }
    .ok_or_else(|| LibraryError::Validation(format!("Container {} was not found", owner.id())))
}

fn source_color_inspector(
    project: &crate::model::Project,
    asset: &Asset,
    source_node_ids: Vec<Uuid>,
) -> AssetSourceColorInspector {
    use crate::model::asset::AssetSourceInterpretation;

    let (interpretation, mut diagnostic) = match asset.source_color.authoritative_interpretation()
    {
        AssetSourceInterpretation::Assigned(binding) => {
            let exact = project
                .requested_color_management_config()
                .is_some_and(|config| config.config() == binding.config());
            (
                AssetSourceColorInspectorInterpretation::Assigned {
                    color_space: binding.color_space().to_string(),
                    exact_active_config: exact,
                },
                (!exact).then(|| {
                    "Assigned source space belongs to a different Project color config; this Asset is fail-closed until cleared or reassigned"
                        .to_string()
                }),
            )
        }
        AssetSourceInterpretation::Description(description) => {
            let interpretation = if asset.source_color.user_override().is_some() {
                AssetSourceColorInspectorInterpretation::AuthoredDescription(description.clone())
            } else {
                AssetSourceColorInspectorInterpretation::Automatic(description.clone())
            };
            (
                interpretation,
                needs_fresh_source_color_metadata(asset).then(|| {
                    "No complete persisted source color identity or assumption is available. Re-probe or assign a verified source space; a loader must otherwise provide an explicit typed policy and rendering will not guess"
                        .to_string()
                }),
            )
        }
        AssetSourceInterpretation::Malformed { detail, .. } => (
            AssetSourceColorInspectorInterpretation::Malformed {
                detail: detail.to_string(),
            },
            Some(
                "The persisted source-space binding is malformed; clear it or assign a verified replacement"
                    .to_string(),
            ),
        ),
    };

    let (assignable_color_spaces, assignment_list_complete) =
        match project.resolved_color_management() {
            ResolvedColorManagementConfig::Ready(intent) => {
                match exact_assignable_color_spaces(intent.config()) {
                    Ok(spaces) => (spaces, true),
                    Err(error) => {
                        diagnostic.get_or_insert_with(|| error.to_string());
                        (Vec::new(), false)
                    }
                }
            }
            ResolvedColorManagementConfig::Unavailable { diagnostics, .. } => {
                diagnostic.get_or_insert_with(|| {
                    format!(
                        "Project color config is unavailable: {}",
                        diagnostics
                            .iter()
                            .map(ToString::to_string)
                            .collect::<Vec<_>>()
                            .join("; ")
                    )
                });
                (Vec::new(), false)
            }
        };

    AssetSourceColorInspector {
        asset_id: asset.id,
        asset_name: asset.name.clone(),
        source_node_ids,
        interpretation,
        assignable_color_spaces,
        assignment_list_complete,
        diagnostic,
    }
}

fn needs_fresh_source_color_metadata(asset: &Asset) -> bool {
    matches!(asset.kind, AssetKind::Image | AssetKind::Video)
        && asset.source_color.user_override().is_none()
        && asset.source_color.assigned_space().is_none()
        && asset.source_color.malformed_assigned_space().is_none()
        && !automatic_source_is_actionable(asset.source_color.detected())
}

fn automatic_source_is_actionable(source: &SourceColorDescription) -> bool {
    source.assumption.is_some()
        || source.profile.is_some()
        || (source.primaries.is_some() && source.transfer.is_some())
}

fn exact_assignable_color_spaces(
    config: &crate::model::project::ColorManagementConfig,
) -> Result<Vec<String>, LibraryError> {
    use ruvie_color_management::ColorTransformBackend;

    let spaces = match config.config() {
        identity if identity == &ColorConfigIdentity::default() => {
            crate::color_management::available_color_spaces()
                .map_err(|error| LibraryError::Validation(error.to_string()))?
        }
        ColorConfigIdentity::Bundled { id }
            if id == crate::model::project::LEGACY_BUNDLED_COLOR_CONFIG_V1_ID =>
        {
            ruvie_color_management::LegacySrgbV1ColorTransform
                .available_color_spaces()
                .map_err(|error| LibraryError::Validation(error.to_string()))?
        }
        _ => {
            return Err(LibraryError::Validation(
                "This build cannot enumerate source spaces from the exact active custom color config; free config-less names are disabled"
                    .into(),
            ));
        }
    };
    let unique = spaces
        .into_iter()
        .filter(|space| !space.is_data)
        .map(|space| space.id)
        .collect::<BTreeSet<_>>();
    Ok(unique.into_iter().collect())
}

fn probe_source_color(
    plugin_manager: &crate::plugin::PluginManager,
    asset: &Asset,
) -> Result<SourceColorDescription, String> {
    if !matches!(asset.kind, AssetKind::Image | AssetKind::Video) {
        return Err(format!(
            "Asset {} is not an Image or Video source",
            asset.id
        ));
    }
    let path = std::path::Path::new(&asset.path);
    let metadata = std::fs::symlink_metadata(path).map_err(|error| {
        format!(
            "Explicit source metadata probe requires an existing local regular file {:?}: {error}",
            asset.path
        )
    })?;
    if metadata.file_type().is_symlink() || !metadata.file_type().is_file() {
        return Err(format!(
            "Explicit source metadata probe accepts only a direct local regular file, not a URL, symlink, FIFO, device, or directory: {:?}",
            asset.path
        ));
    }
    let streams = plugin_manager
        .get_available_streams(&asset.path)
        .map_err(|error| format!("Fresh source metadata probe failed: {error}"))?
        .ok_or_else(|| {
            format!(
                "No load plugin can fresh-probe source metadata for {:?}",
                asset.path
            )
        })?;
    let mut candidates = streams
        .into_iter()
        .filter(|stream| stream.kind == asset.kind)
        .filter(|stream| {
            asset
                .stream_index
                .is_none_or(|expected| stream.stream_index == Some(expected))
        });
    let selected = candidates.next().ok_or_else(|| {
        format!(
            "Fresh probe found no {:?} stream {:?} for {:?}",
            asset.kind, asset.stream_index, asset.path
        )
    })?;
    if asset.stream_index.is_none() && candidates.next().is_some() {
        return Err(format!(
            "Fresh probe found multiple {:?} streams for {:?}; assign a stream before source color can be repaired",
            asset.kind, asset.path
        ));
    }
    Ok(selected.source_color)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::editor::project_service::MediaNodeRequest;
    use crate::model::asset::{SourceColorAssumption, SourceMatrixCoefficients};
    use crate::model::property::{Property, PropertyValue};
    use crate::model::{
        Clip, Project, active_legacy_media_color_properties, is_legacy_media_color_property,
    };
    use crate::plugin::PluginManager;
    use std::sync::{Arc, RwLock};

    fn fixture() -> (ProjectManager, Arc<RwLock<Project>>, Uuid, Uuid) {
        let project = Arc::new(RwLock::new(Project::new("source color Inspector")));
        let plugins = Arc::new(PluginManager::default());
        let manager = ProjectManager::new(Arc::clone(&project), plugins);
        let mut asset = Asset::new("source", "fixture.mp4", AssetKind::Video);
        asset.source_color.replace_detected(SourceColorDescription {
            assumption: Some(SourceColorAssumption::UntaggedYuvBt709LimitedV1),
            matrix: Some(SourceMatrixCoefficients::Bt709),
            bit_depth: Some(8),
            ..SourceColorDescription::default()
        });
        let asset_id = asset.id;
        project.write().unwrap().assets.push(asset);

        let node = manager
            .create_media_node(
                "Video",
                MediaNodeRequest::Video {
                    asset_id,
                    file_path: "fixture.mp4".to_string(),
                    stream_index: None,
                    audio_stream_index: None,
                    outputs: crate::model::MediaOutputSelection::ImageAndAudio,
                },
                1920,
                1080,
                1920,
                1080,
            )
            .unwrap();
        let node_id = node.id;
        let clip = Clip::new("Clip", 0.0, 1.0);
        let clip_id = clip.id;
        let mut project_write = project.write().unwrap();
        project_write.add_clip(clip);
        project_write.add_node(node);
        project_write
            .attach_node_to_container(NodeContainer::Clip(clip_id), node_id)
            .unwrap();
        project_write
            .set_output_node(NodeContainer::Clip(clip_id), Some(node_id))
            .unwrap();
        drop(project_write);
        (manager, project, asset_id, clip_id)
    }

    #[test]
    fn fresh_probe_targets_incomplete_legacy_detection_not_authored_authority() {
        let mut legacy = Asset::new("legacy", "untagged.mp4", AssetKind::Video);
        legacy
            .source_color
            .replace_detected(SourceColorDescription {
                bit_depth: Some(8),
                ..SourceColorDescription::default()
            });
        assert!(needs_fresh_source_color_metadata(&legacy));

        legacy
            .source_color
            .replace_detected(SourceColorDescription {
                assumption: Some(SourceColorAssumption::UntaggedYuvBt709LimitedV1),
                bit_depth: Some(8),
                ..SourceColorDescription::default()
            });
        assert!(!needs_fresh_source_color_metadata(&legacy));

        legacy
            .source_color
            .replace_complete_override(SourceColorDescription::default());
        assert!(
            !needs_fresh_source_color_metadata(&legacy),
            "an explicit untagged user override must never be replaced by probing"
        );
    }

    #[test]
    fn clip_projection_and_assignment_edit_the_asset_with_exact_config_identity() {
        let (manager, project, asset_id, clip_id) = fixture();
        let projected = manager
            .asset_source_color_inspectors(NodeContainer::Clip(clip_id))
            .unwrap();
        assert_eq!(projected.len(), 1);
        assert!(matches!(
            &projected[0].interpretation,
            AssetSourceColorInspectorInterpretation::Automatic(description)
                if description.assumption
                    == Some(SourceColorAssumption::UntaggedYuvBt709LimitedV1)
        ));
        assert!(projected[0].assignment_list_complete);
        assert!(
            projected[0]
                .assignable_color_spaces
                .iter()
                .any(|space| space == ruvie_color_management::DISPLAY_P3_SPACE_ID)
        );

        manager
            .assign_asset_source_color_space(asset_id, ruvie_color_management::DISPLAY_P3_SPACE_ID)
            .unwrap();
        let project_read = project.read().unwrap();
        let asset = project_read.get_asset(asset_id).unwrap();
        let binding = asset.source_color.assigned_space().unwrap();
        assert_eq!(
            binding.config(),
            project_read
                .requested_color_management_config()
                .unwrap()
                .config()
        );
        assert_eq!(
            binding.color_space(),
            ruvie_color_management::DISPLAY_P3_SPACE_ID
        );
        drop(project_read);

        assert!(
            manager
                .assign_asset_source_color_space(asset_id, "free config-less typo")
                .is_err()
        );
        manager.clear_asset_source_color_space(asset_id).unwrap();
        assert!(
            project
                .read()
                .unwrap()
                .get_asset(asset_id)
                .unwrap()
                .source_color
                .assigned_space()
                .is_none()
        );
    }

    #[test]
    fn legacy_builtin_project_enumerates_only_its_frozen_two_space_catalog() {
        let (manager, project, _asset_id, clip_id) = fixture();
        let legacy_identity = ColorConfigIdentity::Bundled {
            id: crate::model::project::LEGACY_BUNDLED_COLOR_CONFIG_V1_ID.to_string(),
        };
        project
            .write()
            .unwrap()
            .set_color_management(crate::model::project::ColorManagementConfig::new(
                legacy_identity,
                ruvie_color_management::LINEAR_SRGB_SPACE_ID,
                crate::model::project::PreviewColorConfig::direct(
                    ruvie_color_management::SRGB_SPACE_ID,
                ),
                crate::model::project::ExportColorConfig::new(
                    ruvie_color_management::SRGB_SPACE_ID,
                ),
            ))
            .unwrap();
        let projected = manager
            .asset_source_color_inspectors(NodeContainer::Clip(clip_id))
            .unwrap();
        assert!(projected[0].assignment_list_complete);
        assert_eq!(
            projected[0].assignable_color_spaces,
            vec![
                ruvie_color_management::LINEAR_SRGB_SPACE_ID.to_string(),
                ruvie_color_management::SRGB_SPACE_ID.to_string(),
            ]
        );
    }

    #[test]
    fn explicit_repair_removes_nonempty_legacy_fields_without_touching_asset_authority() {
        let (manager, project, asset_id, clip_id) = fixture();
        let node_id = project.read().unwrap().get_clip(clip_id).unwrap().node_ids[0];
        let original_detected = project
            .read()
            .unwrap()
            .get_asset(asset_id)
            .unwrap()
            .source_color
            .detected()
            .clone();
        let original = project.read().unwrap().get_node(node_id).unwrap().clone();
        let mut persisted = serde_json::to_value(original).unwrap();
        persisted["properties"]["input_color_space"] = serde_json::to_value(Property::constant(
            PropertyValue::String("ACEScg".to_string()),
        ))
        .unwrap();
        let legacy = serde_json::from_value(persisted).unwrap();
        project.write().unwrap().nodes.insert(node_id, legacy);
        assert_eq!(
            active_legacy_media_color_properties(
                project.read().unwrap().get_node(node_id).unwrap()
            )
            .len(),
            1
        );
        let stack = manager
            .semantic_container_property_stack(NodeContainer::Clip(clip_id))
            .unwrap();
        assert!(stack.sections().iter().any(|section| {
            section
                .diagnostics()
                .iter()
                .any(|diagnostic| diagnostic.contains("deprecated config-less color fields"))
        }));
        assert!(stack.sections().iter().all(|section| {
            section
                .properties()
                .iter()
                .all(|property| !is_legacy_media_color_property(property.key()))
        }));

        manager
            .clear_legacy_media_node_color_properties(node_id)
            .unwrap();
        let project_read = project.read().unwrap();
        let repaired = project_read.get_node(node_id).unwrap();
        assert!(active_legacy_media_color_properties(repaired).is_empty());
        assert!(repaired.properties().get("input_color_space").is_none());
        assert_eq!(
            project_read
                .get_asset(asset_id)
                .unwrap()
                .source_color
                .detected(),
            &original_detected
        );
    }
}
