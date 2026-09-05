//! Application-facing API for the authoritative Timeline editing model.
//!
//! The service owns the sole mutable [`AuthoringProject`] through an
//! [`AuthoringSession`]. UI code receives immutable snapshots and submits
//! exact-time commands; it never edits or synchronizes the legacy graph-backed
//! Project.

mod attachment;
mod authoring;
mod composition;
mod edit_plan;
mod interface;
mod item;
mod module;
mod node_clip_conversion;
mod palette;
mod shape_path;
mod text_ensemble;
mod transition;
mod transition_instance_controls;
mod transition_module_controls;
mod transition_parameter_automation;

#[cfg(test)]
mod attachment_tests;
#[cfg(test)]
mod edit_plan_tests;
#[cfg(test)]
mod item_tests;
#[cfg(test)]
mod module_presentation_tests;
#[cfg(test)]
mod module_removal_tests;
#[cfg(test)]
mod node_clip_conversion_tests;
#[cfg(test)]
mod palette_tests;
#[cfg(test)]
mod transition_input_coverage_tests;
#[cfg(test)]
mod transition_instance_dependency_tests;
#[cfg(test)]
mod transition_module_tests;
#[cfg(test)]
mod transition_parameter_automation_tests;
#[cfg(test)]
mod transition_tests;

use attachment::normalize_all_attachment_orders;
use module::remove_instance_and_private_definition;

pub use crate::model::authoring::TimelineEditPlanningIndex;
pub use authoring::{
    AuthoringKeyframeUpdate, AuthoringPropertyOwner, AuthoringPropertyValueTarget,
    AuthoringPropertyValueUpdate, TimelineSettingsUpdate,
};
pub use edit_plan::{
    EditPlan, EditPlanValidationScope, EditProjection, TimelineEditError, TimelineEditOperation,
    TimelineEditRequest, TimelineItemEditState, plan_timeline_edit, project_edit_plan,
};
pub use interface::{ModuleInterfaceCommand, ModuleInterfaceEditImpact, ModuleInterfaceEditResult};
pub use transition::TransitionPlacement;

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock, RwLockReadGuard, RwLockWriteGuard};

use crate::animation::EasingFunction;
use crate::error::LibraryError;
use crate::model::BlendMode;
use crate::model::authoring::{
    Attachment, AttachmentId, AttachmentOwner, AttachmentProcessor, AttachmentStage,
    AuthoringProject, AuthoringSession, AutomationTrack, BuiltinEffectInstance, ChangeSet,
    CompositionParameter, CompositionParameterId, CompositionParameterTarget, InstanceLocator,
    InstancePath, MediaInputBinding, MediaTime, ModuleConnectionId, ModuleDefinition,
    ModuleDefinitionId, ModuleInstance, ModuleInstanceId, ModuleInvocation, ModuleOutputId,
    ProjectDocument, ProjectFileStore, ProjectInvalidation, ProjectRevision, PublishedMediaInputId,
    PublishedParameterId, RationalRate, SourceRef, TimeMap, Timeline, TimelineId, TimelineInterval,
    TimelineItem, TimelineItemId, TimelineTrack, TimelineTrackId, TimelineTrackKind, TransitionId,
    ordered_track_item_ids, track_item_ids_after_placement,
};
use crate::model::frame::color::Color;
use crate::model::node::Node;
use crate::model::project::asset::Asset;
use crate::model::project::property::{KeyframeId, Property, PropertyMap, PropertyValue};
use crate::plugin::PluginManager;
use crate::util::output_path_identity::output_path_identity;

use super::asset_import::probe_assets_for_import;

/// Places one item at a stable z-order slot and rewrites the Track to
/// contiguous unique layers. Timeline rows therefore never depend on an
/// ambiguous duplicate layer value.
fn place_item_at_layer(
    project: &mut AuthoringProject,
    item_id: TimelineItemId,
    track_id: TimelineTrackId,
    requested_layer: i64,
) -> Result<(), String> {
    if !project.items.contains_key(&item_id) {
        return Err(format!("Missing Timeline item {item_id}"));
    }
    let item_ids = track_item_ids_after_placement(project, track_id, item_id, requested_layer);
    for (layer, ordered_item_id) in item_ids.into_iter().enumerate() {
        project
            .items
            .get_mut(&ordered_item_id)
            .ok_or_else(|| format!("Missing Timeline item {ordered_item_id}"))?
            .layer = i64::try_from(layer).map_err(|_| "Timeline layer overflow".to_string())?;
    }
    Ok(())
}

fn normalize_track_layers(
    project: &mut AuthoringProject,
    track_id: TimelineTrackId,
) -> Result<(), String> {
    for (layer, item_id) in ordered_track_item_ids(project, track_id, None)
        .into_iter()
        .enumerate()
    {
        project
            .items
            .get_mut(&item_id)
            .ok_or_else(|| format!("Missing Timeline item {item_id}"))?
            .layer = i64::try_from(layer).map_err(|_| "Timeline layer overflow".to_string())?;
    }
    Ok(())
}

#[derive(Clone)]
pub struct TimelineEditorService {
    session: Arc<RwLock<AuthoringSession>>,
    project_path: Arc<RwLock<Option<PathBuf>>>,
    timeline_edit_index: Arc<RwLock<Option<Arc<TimelineEditPlanningIndex>>>>,
}

#[derive(Clone, Debug)]
pub struct PreparedModuleDefinitionEdit {
    pub definition_id: ModuleDefinitionId,
    pub cloned: bool,
    pub changes: Option<ChangeSet>,
}

/// Editor command scope for a Timeline-owned Transition Module parameter.
/// Definition scope edits every placement; Instance scope persists a sparse
/// copy-on-write difference on one concrete nested Composition placement.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum TransitionAutomationOwner {
    Definition(TransitionId),
    Instance {
        transition_id: TransitionId,
        instance_path: InstancePath,
    },
}

#[derive(Clone, Debug)]
pub struct SharedModuleEdit<T> {
    pub value: T,
    pub affected_instance_count: usize,
    pub changes: ChangeSet,
}

/// A single Node presentation update used by an atomic Module layout edit.
#[derive(Clone, Debug, PartialEq)]
pub struct ModuleNodePresentationUpdate {
    pub node_id: uuid::Uuid,
    pub position: [f32; 2],
    pub size: [f32; 2],
    pub collapsed: bool,
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub enum ModuleInputHost {
    Item(TimelineItemId),
    Attachment(AttachmentId),
    Transition(TransitionId),
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub enum TimelineItemDependency {
    /// A first-class Timeline Transition uses this item as its A or B
    /// participant. The Transition must be removed before the item can be
    /// deleted; an explicit cascade performs both changes atomically.
    TransitionParticipant { transition_id: TransitionId },
    ModuleInput {
        host: ModuleInputHost,
        input_id: PublishedMediaInputId,
    },
    /// A persisted sparse control record is addressed through this item in
    /// its concrete nested Composition path.
    TransitionInstancePath {
        owner_item_id: TimelineItemId,
        transition_id: TransitionId,
    },
}

#[derive(Clone, Debug)]
pub struct ModuleItemPlacement {
    pub track_id: TimelineTrackId,
    pub name: String,
    pub output_id: ModuleOutputId,
    pub interval: TimelineInterval,
    pub layer: i64,
    pub parameter_overrides: HashMap<PublishedParameterId, PropertyValue>,
    pub input_bindings: HashMap<PublishedMediaInputId, MediaInputBinding>,
}

#[derive(Clone, Debug)]
pub struct ModuleAttachmentPlacement {
    pub owner: AttachmentOwner,
    pub stage: AttachmentStage,
    pub definition_id: ModuleDefinitionId,
    pub output_id: ModuleOutputId,
    pub parameter_overrides: HashMap<PublishedParameterId, PropertyValue>,
    pub input_bindings: HashMap<PublishedMediaInputId, MediaInputBinding>,
}

/// Result of one explicit source-island conversion. Presentation code uses
/// these stable identities to open the existing production Node Editor.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NodeClipConversionResult {
    pub item_id: TimelineItemId,
    pub definition_id: ModuleDefinitionId,
    pub instance_id: ModuleInstanceId,
    pub output_id: ModuleOutputId,
    pub moved_pre_transform_effects: usize,
    pub retained_post_transform_effects: usize,
    pub changes: ChangeSet,
}

impl TimelineEditorService {
    pub fn new(project: AuthoringProject) -> Result<Self, LibraryError> {
        Ok(Self {
            session: Arc::new(RwLock::new(
                AuthoringSession::new(project).map_err(LibraryError::Validation)?,
            )),
            project_path: Arc::new(RwLock::new(None)),
            timeline_edit_index: Arc::new(RwLock::new(None)),
        })
    }

    pub fn create_default(name: impl Into<String>) -> Result<Self, LibraryError> {
        Self::new(
            AuthoringProject::new(
                name,
                1920,
                1080,
                RationalRate::new(30, 1).map_err(LibraryError::Validation)?,
                MediaTime::new(60, 1).map_err(LibraryError::Validation)?,
            )
            .map_err(LibraryError::Validation)?,
        )
    }

    pub fn open(path: &Path) -> Result<Self, LibraryError> {
        let document = ProjectFileStore::load(path).map_err(LibraryError::Project)?;
        let service = Self::new(document.project)?;
        *service.write_path()? = Some(path.to_path_buf());
        Ok(service)
    }

    pub fn replace_project(&self, project: AuthoringProject) -> Result<(), LibraryError> {
        *self.write_session()? =
            AuthoringSession::new(project).map_err(LibraryError::Validation)?;
        *self.timeline_edit_index.write().map_err(|_| {
            LibraryError::Runtime("Timeline edit planning index lock poisoned".to_string())
        })? = None;
        *self.write_path()? = None;
        Ok(())
    }

    pub fn snapshot(&self) -> Result<Arc<AuthoringProject>, LibraryError> {
        Ok(Arc::new(self.read_session()?.project().clone()))
    }

    /// Captures content and revision under one read lock so Preview can never
    /// pair a newer revision with an older immutable Project snapshot.
    pub fn snapshot_with_revision(
        &self,
    ) -> Result<(Arc<AuthoringProject>, ProjectRevision), LibraryError> {
        let session = self.read_session()?;
        Ok((Arc::new(session.project().clone()), session.revision()))
    }

    pub fn document(&self) -> Result<ProjectDocument, LibraryError> {
        Ok(ProjectDocument::new(self.read_session()?.project().clone()))
    }

    pub fn revision(&self) -> Result<ProjectRevision, LibraryError> {
        Ok(self.read_session()?.revision())
    }

    pub fn can_undo(&self) -> Result<bool, LibraryError> {
        Ok(self.read_session()?.can_undo())
    }

    pub fn can_redo(&self) -> Result<bool, LibraryError> {
        Ok(self.read_session()?.can_redo())
    }

    pub fn undo(&self) -> Result<Option<ChangeSet>, LibraryError> {
        self.write_session()?
            .undo()
            .map_err(LibraryError::Validation)
    }

    pub fn redo(&self) -> Result<Option<ChangeSet>, LibraryError> {
        self.write_session()?
            .redo()
            .map_err(LibraryError::Validation)
    }

    pub fn project_path(&self) -> Result<Option<PathBuf>, LibraryError> {
        Ok(self.read_path()?.clone())
    }

    pub fn save(&self) -> Result<(), LibraryError> {
        let path = self
            .project_path()?
            .ok_or_else(|| LibraryError::Project("Project has no save path".to_string()))?;
        self.save_to(&path)
    }

    pub fn save_as(&self, path: &Path) -> Result<(), LibraryError> {
        self.save_to(path)?;
        *self.write_path()? = Some(path.to_path_buf());
        Ok(())
    }

    pub fn add_timeline(
        &self,
        name: String,
        width: u64,
        height: u64,
        fps: RationalRate,
        duration: MediaTime,
    ) -> Result<(TimelineId, TimelineTrackId, ChangeSet), LibraryError> {
        let mut session = self.write_session()?;
        session
            .transact(vec![ProjectInvalidation::ProjectStructure], |project| {
                let timeline_id = TimelineId::new();
                let track_id = TimelineTrackId::new();
                project.timelines.insert(
                    timeline_id,
                    Timeline {
                        id: timeline_id,
                        name,
                        width,
                        height,
                        fps,
                        duration,
                        background_color: Color::black(),
                        color_profile: "sRGB".to_string(),
                        track_order: vec![track_id],
                        authored_properties: PropertyMap::new(),
                        published_parameters: Vec::new(),
                    },
                );
                project.tracks.insert(
                    track_id,
                    TimelineTrack {
                        id: track_id,
                        timeline_id,
                        name: "Video 1".to_string(),
                        kind: TimelineTrackKind::AudioVisual,
                        authored_properties: PropertyMap::new(),
                    },
                );
                Ok((timeline_id, track_id))
            })
            .map(|((timeline_id, track_id), changes)| (timeline_id, track_id, changes))
            .map_err(LibraryError::Validation)
    }

    pub fn add_track(
        &self,
        timeline_id: TimelineId,
        name: String,
        kind: TimelineTrackKind,
    ) -> Result<(TimelineTrackId, ChangeSet), LibraryError> {
        let mut session = self.write_session()?;
        session
            .transact(
                vec![ProjectInvalidation::TimelineStructure { timeline_id }],
                |project| {
                    let timeline = project
                        .timelines
                        .get_mut(&timeline_id)
                        .ok_or_else(|| format!("Missing Timeline {timeline_id}"))?;
                    let track_id = TimelineTrackId::new();
                    timeline.track_order.push(track_id);
                    project.tracks.insert(
                        track_id,
                        TimelineTrack {
                            id: track_id,
                            timeline_id,
                            name,
                            kind,
                            authored_properties: PropertyMap::new(),
                        },
                    );
                    Ok(track_id)
                },
            )
            .map_err(LibraryError::Validation)
    }

    pub fn add_asset(&self, asset: Asset) -> Result<ChangeSet, LibraryError> {
        let asset_id = asset.id;
        let mut session = self.write_session()?;
        session
            .transact(vec![ProjectInvalidation::ProjectStructure], |project| {
                if project
                    .assets
                    .iter()
                    .any(|existing| existing.id == asset_id)
                {
                    return Err(format!("Asset {asset_id} already exists"));
                }
                project.assets.push(asset);
                Ok(())
            })
            .map(|(_, changes)| changes)
            .map_err(LibraryError::Validation)
    }

    pub fn has_asset_with_path(&self, path: &Path) -> Result<bool, LibraryError> {
        let path = path
            .to_str()
            .ok_or_else(|| LibraryError::Validation("Asset path is not valid UTF-8".to_string()))?;
        let requested = output_path_identity(path)?;
        for asset in &self.read_session()?.project().assets {
            if requested.aliases(&output_path_identity(&asset.path)?) {
                return Ok(true);
            }
        }
        Ok(false)
    }

    /// Probes every stream without creating any legacy Clip/Node state and
    /// atomically adds the resulting Assets to the authoring Project.
    pub fn import_file(
        &self,
        path: &Path,
        plugins: &PluginManager,
    ) -> Result<(Vec<uuid::Uuid>, ChangeSet), LibraryError> {
        if self.has_asset_with_path(path)? {
            return Err(LibraryError::Validation(format!(
                "Asset path '{}' is already imported",
                path.display()
            )));
        }
        let assets = probe_assets_for_import(path, plugins)?;
        let imported_path = assets
            .first()
            .map(|asset| asset.path.clone())
            .ok_or_else(|| {
                LibraryError::Validation("Asset probe returned no Assets".to_string())
            })?;
        let imported_identity = output_path_identity(&imported_path)?;
        let ids = assets.iter().map(|asset| asset.id).collect::<Vec<_>>();
        let mut session = self.write_session()?;
        session
            .transact(vec![ProjectInvalidation::ProjectStructure], |project| {
                for asset in &project.assets {
                    let identity = output_path_identity(&asset.path)
                        .map_err(|error| format!("Cannot compare Asset path identity: {error}"))?;
                    if imported_identity.aliases(&identity) {
                        return Err(format!("Asset path '{imported_path}' is already imported"));
                    }
                }
                project.assets.extend(assets);
                Ok(ids)
            })
            .map_err(LibraryError::Validation)
    }

    /// Creates a detached Node for insertion into a Module graph. This never
    /// constructs or mutates the legacy graph-backed Project.
    pub fn create_module_node(
        &self,
        plugins: &PluginManager,
        request: super::authoring_factory::ModuleNodeRequest,
        canvas_width: u64,
        canvas_height: u64,
    ) -> Result<Node, LibraryError> {
        super::authoring_factory::AuthoringNodeFactory::create(
            plugins,
            request,
            canvas_width,
            canvas_height,
        )
    }

    /// Resolves a descriptor-backed lightweight Effect entry without asking
    /// UI code to construct a persisted port/parameter contract.
    pub fn create_builtin_effect(
        &self,
        plugins: &PluginManager,
        effect_id: &str,
    ) -> Result<BuiltinEffectInstance, LibraryError> {
        super::authoring_factory::BuiltinEffectFactory::create(plugins, effect_id)
    }

    /// Adds an ordinary authored item. Module items must use
    /// [`Self::place_module_item`] so instance ownership is atomic.
    pub fn add_item(
        &self,
        track_id: TimelineTrackId,
        name: String,
        source: SourceRef,
        interval: TimelineInterval,
        layer: i64,
    ) -> Result<(TimelineItemId, ChangeSet), LibraryError> {
        if matches!(source, SourceRef::Module(_)) {
            return Err(LibraryError::Validation(
                "Use place_module_item to create a Node Clip".to_string(),
            ));
        }
        let mut session = self.write_session()?;
        let timeline_id = timeline_for_track(session.project(), track_id)?;
        session
            .transact(
                vec![ProjectInvalidation::TimelineStructure { timeline_id }],
                |project| {
                    let item_id = TimelineItemId::new();
                    project.items.insert(
                        item_id,
                        TimelineItem {
                            id: item_id,
                            track_id,
                            name,
                            source,
                            interval,
                            time_map: TimeMap::default(),
                            layer,
                            parent: None,
                            blend_mode: BlendMode::Normal,
                            authored_properties: PropertyMap::new(),
                        },
                    );
                    place_item_at_layer(project, item_id, track_id, layer)?;
                    Ok(item_id)
                },
            )
            .map_err(LibraryError::Validation)
    }

    fn save_to(&self, path: &Path) -> Result<(), LibraryError> {
        ProjectFileStore::save(path, &self.document()?).map_err(LibraryError::Project)
    }

    fn read_session(&self) -> Result<RwLockReadGuard<'_, AuthoringSession>, LibraryError> {
        self.session
            .read()
            .map_err(|_| LibraryError::Runtime("Authoring session lock poisoned".to_string()))
    }

    fn write_session(&self) -> Result<RwLockWriteGuard<'_, AuthoringSession>, LibraryError> {
        self.session
            .write()
            .map_err(|_| LibraryError::Runtime("Authoring session lock poisoned".to_string()))
    }

    fn read_path(&self) -> Result<RwLockReadGuard<'_, Option<PathBuf>>, LibraryError> {
        self.project_path
            .read()
            .map_err(|_| LibraryError::Runtime("Project path lock poisoned".to_string()))
    }

    fn write_path(&self) -> Result<RwLockWriteGuard<'_, Option<PathBuf>>, LibraryError> {
        self.project_path
            .write()
            .map_err(|_| LibraryError::Runtime("Project path lock poisoned".to_string()))
    }
}

fn timeline_for_track(
    project: &AuthoringProject,
    track_id: TimelineTrackId,
) -> Result<TimelineId, LibraryError> {
    project
        .tracks
        .get(&track_id)
        .map(|track| track.timeline_id)
        .ok_or_else(|| LibraryError::Validation(format!("Missing Timeline Track {track_id}")))
}

fn timeline_for_item(
    project: &AuthoringProject,
    item_id: TimelineItemId,
) -> Result<TimelineId, LibraryError> {
    let item = project
        .items
        .get(&item_id)
        .ok_or_else(|| LibraryError::Validation(format!("Missing Timeline item {item_id}")))?;
    timeline_for_track(project, item.track_id)
}
