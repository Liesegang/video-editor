//! Application-facing service for the Timeline-first Project.
//!
//! Every operation in this service edits [`AuthoringProject`] directly. It
//! never constructs or synchronizes the former Composition/Track/Clip graph
//! Project.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, RwLock};

use crate::core::framing::evaluate_authoring_timeline_frame;
use crate::core::render_plan::{RenderPlan, RenderPlanCache, RenderPlanCacheStats};
use crate::error::LibraryError;
use crate::model::authoring::{
    AuthoringProject, AuthoringSession, ChangeSet, CompositionInstance, DataSource, DataSourceId,
    DurationPolicy, ModuleDefinition, ModuleGraph, ModuleInstance, ModuleRole, ProjectDocument,
    ProjectFileStore, ProjectRevision, SourceRef, TimeMap, TimelineId, TimelineInterval,
    TimelineItemId, TimelineTrackId, TimelineTrackKind,
};
use crate::model::authoring::{
    ModuleDefinitionId, ModuleInstanceId, PublishedParameterId, SignalBinding, SignalBindingId,
};
use crate::model::frame::color::Color;
use crate::model::frame::frame::{FrameInfo, Region};
use crate::model::project::asset::Asset;
use crate::model::project::property::Property;
use crate::model::project::property::PropertyValue;
use crate::plugin::PluginManager;

pub struct TimelineEditorService {
    session: RwLock<AuthoringSession>,
    render_plan_cache: Mutex<RenderPlanCache>,
    project_path: RwLock<Option<PathBuf>>,
}

impl TimelineEditorService {
    pub fn new(project: AuthoringProject) -> Result<Self, LibraryError> {
        Ok(Self {
            session: RwLock::new(AuthoringSession::new(project).map_err(LibraryError::Validation)?),
            render_plan_cache: Mutex::new(RenderPlanCache::default()),
            project_path: RwLock::new(None),
        })
    }

    pub fn create_default(name: impl Into<String>) -> Result<Self, LibraryError> {
        let project = AuthoringProject::new(name, 1920, 1080, 30.0, 60.0)
            .map_err(LibraryError::Validation)?;
        Self::new(project)
    }

    pub fn open(path: &Path) -> Result<Self, LibraryError> {
        let document = ProjectFileStore::load(path).map_err(LibraryError::Project)?;
        let service = Self::new(document.project)?;
        *service.write_path()? = Some(path.to_path_buf());
        Ok(service)
    }

    pub fn replace_project(&self, project: AuthoringProject) -> Result<(), LibraryError> {
        let session = AuthoringSession::new(project).map_err(LibraryError::Validation)?;
        *self.write_session()? = session;
        *self.lock_plan_cache()? = RenderPlanCache::default();
        Ok(())
    }

    pub fn snapshot(&self) -> Result<Arc<AuthoringProject>, LibraryError> {
        Ok(Arc::new(self.read_session()?.project().clone()))
    }

    pub fn revision(&self) -> Result<ProjectRevision, LibraryError> {
        Ok(self.read_session()?.revision())
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
        fps: f64,
        duration: f64,
    ) -> Result<(TimelineId, TimelineTrackId, ChangeSet), LibraryError> {
        self.write_session()?
            .add_timeline(name, width, height, fps, duration)
            .map_err(LibraryError::Validation)
    }

    pub fn add_track(
        &self,
        timeline_id: TimelineId,
        name: String,
        kind: TimelineTrackKind,
    ) -> Result<(TimelineTrackId, ChangeSet), LibraryError> {
        self.write_session()?
            .add_track(timeline_id, name, kind)
            .map_err(LibraryError::Validation)
    }

    pub fn add_asset(&self, asset: Asset) -> Result<ChangeSet, LibraryError> {
        self.write_session()?
            .add_asset(asset)
            .map_err(LibraryError::Validation)
    }

    pub fn place_asset(
        &self,
        track_id: TimelineTrackId,
        asset_id: uuid::Uuid,
        name: String,
        interval: TimelineInterval,
        layer: i64,
    ) -> Result<(TimelineItemId, ChangeSet), LibraryError> {
        self.add_item(
            track_id,
            name,
            SourceRef::Asset {
                asset_id,
                time_map: TimeMap::default(),
            },
            interval,
            layer,
        )
    }

    pub fn add_text(
        &self,
        track_id: TimelineTrackId,
        text: String,
        interval: TimelineInterval,
        layer: i64,
    ) -> Result<(TimelineItemId, ChangeSet), LibraryError> {
        self.add_item(
            track_id,
            "Text".to_string(),
            SourceRef::Text { text },
            interval,
            layer,
        )
    }

    pub fn add_solid(
        &self,
        track_id: TimelineTrackId,
        color: Color,
        interval: TimelineInterval,
        layer: i64,
    ) -> Result<(TimelineItemId, ChangeSet), LibraryError> {
        self.add_item(
            track_id,
            "Solid".to_string(),
            SourceRef::Solid { color },
            interval,
            layer,
        )
    }

    pub fn place_timeline(
        &self,
        track_id: TimelineTrackId,
        timeline_id: TimelineId,
        name: String,
        interval: TimelineInterval,
        duration_policy: DurationPolicy,
        layer: i64,
    ) -> Result<(TimelineItemId, ChangeSet), LibraryError> {
        self.add_item(
            track_id,
            name,
            SourceRef::Composition(CompositionInstance {
                timeline_id,
                time_map: TimeMap::default(),
                duration_policy,
                parameter_overrides: Default::default(),
            }),
            interval,
            layer,
        )
    }

    pub fn move_item(
        &self,
        item_id: TimelineItemId,
        track_id: TimelineTrackId,
        start: f64,
        layer: i64,
    ) -> Result<ChangeSet, LibraryError> {
        self.write_session()?
            .move_item(item_id, track_id, start, layer)
            .map_err(LibraryError::Validation)
    }

    pub fn trim_item(
        &self,
        item_id: TimelineItemId,
        interval: TimelineInterval,
    ) -> Result<ChangeSet, LibraryError> {
        self.write_session()?
            .trim_item(item_id, interval)
            .map_err(LibraryError::Validation)
    }

    pub fn split_item(
        &self,
        item_id: TimelineItemId,
        timeline_time: f64,
    ) -> Result<(TimelineItemId, ChangeSet), LibraryError> {
        self.write_session()?
            .split_item(item_id, timeline_time)
            .map_err(LibraryError::Validation)
    }

    pub fn delete_item(
        &self,
        item_id: TimelineItemId,
        ripple: bool,
    ) -> Result<ChangeSet, LibraryError> {
        self.write_session()?
            .delete_item(item_id, ripple)
            .map_err(LibraryError::Validation)
    }

    pub fn set_item_property(
        &self,
        item_id: TimelineItemId,
        key: String,
        property: Property,
    ) -> Result<ChangeSet, LibraryError> {
        self.write_session()?
            .set_item_property(item_id, key, property)
            .map_err(LibraryError::Validation)
    }

    pub fn update_item_property_value(
        &self,
        item_id: TimelineItemId,
        key: String,
        time: f64,
        value: PropertyValue,
    ) -> Result<ChangeSet, LibraryError> {
        self.write_session()?
            .update_item_property_value(item_id, key, time, value)
            .map_err(LibraryError::Validation)
    }

    pub fn upsert_item_keyframe(
        &self,
        item_id: TimelineItemId,
        key: String,
        time: f64,
        value: PropertyValue,
        easing: Option<crate::animation::EasingFunction>,
    ) -> Result<ChangeSet, LibraryError> {
        self.write_session()?
            .upsert_item_keyframe(item_id, key, time, value, easing)
            .map_err(LibraryError::Validation)
    }

    pub fn update_item_keyframe(
        &self,
        item_id: TimelineItemId,
        key: String,
        keyframe_id: crate::model::project::property::KeyframeId,
        update: crate::model::project::property::KeyframeUpdate,
    ) -> Result<ChangeSet, LibraryError> {
        self.write_session()?
            .update_item_keyframe(item_id, key, keyframe_id, update)
            .map_err(LibraryError::Validation)
    }

    pub fn remove_item_keyframe(
        &self,
        item_id: TimelineItemId,
        key: String,
        keyframe_id: crate::model::project::property::KeyframeId,
    ) -> Result<ChangeSet, LibraryError> {
        self.write_session()?
            .remove_item_keyframe(item_id, key, keyframe_id)
            .map_err(LibraryError::Validation)
    }

    pub fn rename_item(
        &self,
        item_id: TimelineItemId,
        name: String,
    ) -> Result<ChangeSet, LibraryError> {
        self.write_session()?
            .rename_item(item_id, name)
            .map_err(LibraryError::Validation)
    }

    pub fn set_text(
        &self,
        item_id: TimelineItemId,
        text: String,
    ) -> Result<ChangeSet, LibraryError> {
        self.write_session()?
            .set_text(item_id, text)
            .map_err(LibraryError::Validation)
    }

    pub fn attach_effect(
        &self,
        item_id: TimelineItemId,
        effect_type: &str,
        plugins: &PluginManager,
    ) -> Result<(ModuleInstanceId, ChangeSet), LibraryError> {
        let node = plugins.create_effect_operation_node(effect_type)?;
        self.write_session()?
            .attach_effect_module(item_id, effect_type.to_string(), node)
            .map_err(LibraryError::Validation)
    }

    pub fn set_module_parameter(
        &self,
        instance_id: ModuleInstanceId,
        parameter_id: PublishedParameterId,
        value: PropertyValue,
    ) -> Result<ChangeSet, LibraryError> {
        self.write_session()?
            .set_module_parameter(instance_id, parameter_id, value)
            .map_err(LibraryError::Validation)
    }

    pub fn set_module_node_state(
        &self,
        definition_id: ModuleDefinitionId,
        node_id: uuid::Uuid,
        name: String,
        enabled: bool,
        bypassed: bool,
    ) -> Result<ChangeSet, LibraryError> {
        self.write_session()?
            .set_module_node_state(definition_id, node_id, name, enabled, bypassed)
            .map_err(LibraryError::Validation)
    }

    pub fn add_signal_binding(&self, binding: SignalBinding) -> Result<ChangeSet, LibraryError> {
        self.write_session()?
            .add_signal_binding(binding)
            .map_err(LibraryError::Validation)
    }

    pub fn remove_signal_binding(
        &self,
        binding_id: SignalBindingId,
    ) -> Result<ChangeSet, LibraryError> {
        self.write_session()?
            .remove_signal_binding(binding_id)
            .map_err(LibraryError::Validation)
    }

    pub fn set_parent(
        &self,
        item_id: TimelineItemId,
        parent: Option<TimelineItemId>,
    ) -> Result<ChangeSet, LibraryError> {
        self.write_session()?
            .set_parent(item_id, parent)
            .map_err(LibraryError::Validation)
    }

    pub fn set_composition_duration_policy(
        &self,
        item_id: TimelineItemId,
        policy: DurationPolicy,
    ) -> Result<ChangeSet, LibraryError> {
        self.write_session()?
            .set_composition_duration_policy(item_id, policy)
            .map_err(LibraryError::Validation)
    }

    pub fn add_cross_dissolve(
        &self,
        to_item_id: TimelineItemId,
        duration: f64,
    ) -> Result<ChangeSet, LibraryError> {
        self.write_session()?
            .add_cross_dissolve(to_item_id, duration)
            .map_err(LibraryError::Validation)
    }

    pub fn add_constraint(
        &self,
        item_id: TimelineItemId,
        target_item_id: TimelineItemId,
        kind: crate::model::authoring::ConstraintKind,
    ) -> Result<ChangeSet, LibraryError> {
        self.write_session()?
            .add_constraint(item_id, target_item_id, kind)
            .map_err(LibraryError::Validation)
    }

    pub fn add_rectangle_mask(
        &self,
        item_id: TimelineItemId,
    ) -> Result<(crate::model::authoring::MaskId, ChangeSet), LibraryError> {
        let mut session = self.write_session()?;
        let timeline_id = session
            .project()
            .items
            .get(&item_id)
            .and_then(|item| session.project().tracks.get(&item.track_id))
            .map(|track| track.timeline_id)
            .ok_or_else(|| LibraryError::Validation(format!("Missing Timeline item {item_id}")))?;
        let timeline = &session.project().timelines[&timeline_id];
        let left = timeline.width as f64 * 0.1;
        let top = timeline.height as f64 * 0.1;
        let right = timeline.width as f64 * 0.9;
        let bottom = timeline.height as f64 * 0.9;
        let path = crate::model::path::PathValue::new(
            crate::model::path::FillRule::NonZero,
            vec![crate::model::path::PathContour::new(
                crate::model::path::PathPoint::new(left, top),
                vec![
                    crate::model::path::PathSegment::line(crate::model::path::PathPoint::new(
                        right, top,
                    )),
                    crate::model::path::PathSegment::line(crate::model::path::PathPoint::new(
                        right, bottom,
                    )),
                    crate::model::path::PathSegment::line(crate::model::path::PathPoint::new(
                        left, bottom,
                    )),
                ],
                true,
            )],
        )
        .map_err(|error| LibraryError::Validation(error.to_string()))?;
        session
            .add_mask(item_id, path, crate::model::authoring::MaskMode::Add)
            .map_err(LibraryError::Validation)
    }

    pub fn update_mask(
        &self,
        item_id: TimelineItemId,
        mask_id: crate::model::authoring::MaskId,
        time: f64,
        mode: crate::model::authoring::MaskMode,
        inverted: bool,
        feather: f64,
        opacity: f64,
    ) -> Result<ChangeSet, LibraryError> {
        self.write_session()?
            .update_mask(item_id, mask_id, time, mode, inverted, feather, opacity)
            .map_err(LibraryError::Validation)
    }

    pub fn import_srt(
        &self,
        path: &Path,
        target_track_id: TimelineTrackId,
    ) -> Result<Vec<TimelineItemId>, LibraryError> {
        let source = std::fs::read_to_string(path).map_err(|error| {
            LibraryError::Project(format!("Cannot read subtitles {}: {error}", path.display()))
        })?;
        let cues =
            crate::core::subtitle_runtime::parse_srt(&source).map_err(LibraryError::Validation)?;
        let mut session = self.write_session()?;
        cues.into_iter()
            .enumerate()
            .map(|(index, cue)| {
                session
                    .add_item(
                        target_track_id,
                        format!("Subtitle {}", index + 1),
                        SourceRef::Text { text: cue.text },
                        TimelineInterval::new(cue.start, cue.end - cue.start)?,
                        index as i64,
                    )
                    .map(|(item_id, _)| item_id)
            })
            .collect::<Result<Vec<_>, String>>()
            .map_err(LibraryError::Validation)
    }

    pub fn import_data_source(
        &self,
        path: &Path,
        target_track_id: TimelineTrackId,
    ) -> Result<(DataSourceId, ChangeSet), LibraryError> {
        let source = std::fs::read_to_string(path).map_err(|error| {
            LibraryError::Project(format!(
                "Cannot read Data source {}: {error}",
                path.display()
            ))
        })?;
        let (source_ref, table) = crate::core::data_source_runtime::parse_table(path, &source)
            .map_err(LibraryError::Validation)?;
        let snapshot = self.snapshot()?;
        let timeline_id = snapshot
            .tracks
            .get(&target_track_id)
            .ok_or_else(|| LibraryError::Validation(format!("Missing Track {target_track_id}")))?
            .timeline_id;
        let duration = snapshot.timelines[&timeline_id].duration.into_inner();
        drop(snapshot);
        let data_source_id = DataSourceId::new();
        let definition_id = ModuleDefinitionId::new();
        let generator_id = ModuleInstanceId::new();
        let definition = ModuleDefinition {
            id: definition_id,
            name: format!(
                "{} row generator",
                path.file_stem()
                    .and_then(|name| name.to_str())
                    .unwrap_or("Data")
            ),
            role: ModuleRole::Generator,
            graph: ModuleGraph::default(),
            published_parameters: Vec::new(),
            published_signals: Vec::new(),
            published_actions: Vec::new(),
            version: 1,
        };
        let instance = ModuleInstance {
            id: generator_id,
            definition_id,
            parameter_overrides: Default::default(),
        };
        let generated = crate::core::data_source_runtime::generate_text_items(
            generator_id,
            &table,
            duration,
            self.revision()?.get().wrapping_add(1),
            data_source_id,
        )
        .map_err(LibraryError::Validation)?;
        let data_source = DataSource {
            id: data_source_id,
            generator_id,
            target_track_id,
            name: path
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or("Data")
                .to_string(),
            source: source_ref,
            stable_key_field: table.stable_key_field,
            cached_rows: table.rows,
        };
        let change = self
            .write_session()?
            .replace_data_source_generation(
                target_track_id,
                data_source,
                Some(definition),
                Some(instance),
                generated,
            )
            .map_err(LibraryError::Validation)?;
        Ok((data_source_id, change))
    }

    pub fn refresh_data_source(
        &self,
        data_source_id: DataSourceId,
    ) -> Result<ChangeSet, LibraryError> {
        let snapshot = self.snapshot()?;
        let previous = snapshot
            .data_sources
            .get(&data_source_id)
            .cloned()
            .ok_or_else(|| {
                LibraryError::Validation(format!("Missing Data source {data_source_id}"))
            })?;
        let path = match &previous.source {
            crate::model::authoring::DataSourceRef::Csv { path }
            | crate::model::authoring::DataSourceRef::Json { path } => PathBuf::from(path),
            crate::model::authoring::DataSourceRef::EmbeddedTable => {
                return Err(LibraryError::Validation(
                    "Embedded tables do not have an external file to refresh".to_string(),
                ));
            }
        };
        let source = std::fs::read_to_string(&path).map_err(|error| {
            LibraryError::Project(format!(
                "Cannot read Data source {}: {error}",
                path.display()
            ))
        })?;
        let (source_ref, table) = crate::core::data_source_runtime::parse_table(&path, &source)
            .map_err(LibraryError::Validation)?;
        if table.stable_key_field != previous.stable_key_field {
            return Err(LibraryError::Validation(format!(
                "Stable key changed from '{}' to '{}'; choose how to reconcile before refreshing",
                previous.stable_key_field, table.stable_key_field
            )));
        }
        let timeline_id = snapshot.tracks[&previous.target_track_id].timeline_id;
        let duration = snapshot.timelines[&timeline_id].duration.into_inner();
        drop(snapshot);
        let generated = crate::core::data_source_runtime::generate_text_items(
            previous.generator_id,
            &table,
            duration,
            self.revision()?.get().wrapping_add(1),
            data_source_id,
        )
        .map_err(LibraryError::Validation)?;
        let mut refreshed = previous;
        refreshed.source = source_ref;
        refreshed.cached_rows = table.rows;
        self.write_session()?
            .replace_data_source_generation(
                refreshed.target_track_id,
                refreshed,
                None,
                None,
                generated,
            )
            .map_err(LibraryError::Validation)
    }

    pub fn remove_generated_override(
        &self,
        override_id: crate::model::authoring::OverrideId,
    ) -> Result<ChangeSet, LibraryError> {
        self.write_session()?
            .remove_generated_override(override_id)
            .map_err(LibraryError::Validation)
    }

    pub fn compile_render_plan(&self) -> Result<(RenderPlan, RenderPlanCacheStats), LibraryError> {
        let project = self.snapshot()?;
        self.lock_plan_cache()?
            .compile(project.as_ref())
            .map_err(LibraryError::Validation)
    }

    pub fn evaluate_frame(
        &self,
        timeline_id: TimelineId,
        time: f64,
        render_scale: f64,
        region: Option<Region>,
    ) -> Result<(Arc<AuthoringProject>, FrameInfo), LibraryError> {
        let project = self.snapshot()?;
        let timeline = project
            .timelines
            .get(&timeline_id)
            .ok_or_else(|| LibraryError::Validation(format!("Missing Timeline {timeline_id}")))?;
        let frame_number = frame_number_at(time, timeline.fps.into_inner())?;
        let (plan, _) = self
            .lock_plan_cache()?
            .compile(project.as_ref())
            .map_err(LibraryError::Validation)?;
        let frame = evaluate_authoring_timeline_frame(
            project.as_ref(),
            &plan,
            timeline_id,
            frame_number,
            render_scale,
            region,
        )?;
        Ok((project, frame))
    }

    fn add_item(
        &self,
        track_id: TimelineTrackId,
        name: String,
        source: SourceRef,
        interval: TimelineInterval,
        layer: i64,
    ) -> Result<(TimelineItemId, ChangeSet), LibraryError> {
        self.write_session()?
            .add_item(track_id, name, source, interval, layer)
            .map_err(LibraryError::Validation)
    }

    fn save_to(&self, path: &Path) -> Result<(), LibraryError> {
        let project = self.read_session()?.project().clone();
        ProjectFileStore::save(path, &ProjectDocument::new(project)).map_err(LibraryError::Project)
    }

    fn read_session(
        &self,
    ) -> Result<std::sync::RwLockReadGuard<'_, AuthoringSession>, LibraryError> {
        self.session
            .read()
            .map_err(|_| LibraryError::Runtime("Timeline editor lock was poisoned".to_string()))
    }

    fn write_session(
        &self,
    ) -> Result<std::sync::RwLockWriteGuard<'_, AuthoringSession>, LibraryError> {
        self.session
            .write()
            .map_err(|_| LibraryError::Runtime("Timeline editor lock was poisoned".to_string()))
    }

    fn lock_plan_cache(&self) -> Result<std::sync::MutexGuard<'_, RenderPlanCache>, LibraryError> {
        self.render_plan_cache
            .lock()
            .map_err(|_| LibraryError::Runtime("RenderPlan cache lock was poisoned".to_string()))
    }

    fn read_path(&self) -> Result<std::sync::RwLockReadGuard<'_, Option<PathBuf>>, LibraryError> {
        self.project_path
            .read()
            .map_err(|_| LibraryError::Runtime("Project path lock was poisoned".to_string()))
    }

    fn write_path(&self) -> Result<std::sync::RwLockWriteGuard<'_, Option<PathBuf>>, LibraryError> {
        self.project_path
            .write()
            .map_err(|_| LibraryError::Runtime("Project path lock was poisoned".to_string()))
    }
}

fn frame_number_at(time: f64, fps: f64) -> Result<u64, LibraryError> {
    if !time.is_finite() || time < 0.0 || !fps.is_finite() || fps <= 0.0 {
        return Err(LibraryError::Validation(
            "Preview time and Timeline FPS must be finite and non-negative".to_string(),
        ));
    }
    let frame = (time * fps).floor();
    if !frame.is_finite() || frame >= u64::MAX as f64 {
        return Err(LibraryError::Validation(
            "Preview frame is outside the supported range".to_string(),
        ));
    }
    Ok(frame as u64)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn basic_edit_save_open_and_frame_use_only_timeline_project() {
        let service = TimelineEditorService::create_default("Vertical slice").expect("service");
        let project = service.snapshot().expect("snapshot");
        let timeline_id = project.root_timeline_id;
        let track_id = project.timelines[&timeline_id].track_order[0];
        drop(project);
        let (item_id, _) = service
            .add_text(
                track_id,
                "Hello".to_string(),
                TimelineInterval::new(0.0, 2.0).expect("interval"),
                0,
            )
            .expect("text");
        let plugins = PluginManager::default();
        let (effect_instance, _) = service
            .attach_effect(item_id, "blur", &plugins)
            .expect("Blur effect");
        let snapshot = service.snapshot().expect("effect snapshot");
        let definition_id = snapshot.module_instances[&effect_instance].definition_id;
        let sigma_x = snapshot.module_definitions[&definition_id]
            .published_parameters
            .iter()
            .find(|parameter| parameter.name == "sigma_x")
            .expect("published sigma_x")
            .id;
        drop(snapshot);
        service
            .set_module_parameter(
                effect_instance,
                sigma_x,
                PropertyValue::Number(ordered_float::OrderedFloat(8.0)),
            )
            .expect("effect parameter");
        let binding_id = SignalBindingId::new();
        service
            .add_signal_binding(SignalBinding {
                id: binding_id,
                source: crate::model::authoring::SignalSource::AudioEnvelope {
                    channel: "master".to_string(),
                },
                scope: crate::model::authoring::BindingScope::Instance {
                    instance_path: crate::model::authoring::InstancePath::root(timeline_id),
                    module_instance_id: effect_instance,
                },
                target_parameter_id: sigma_x,
                mapping: crate::model::authoring::SignalMapping {
                    input_min: ordered_float::OrderedFloat(0.0),
                    input_max: ordered_float::OrderedFloat(1.0),
                    output_min: ordered_float::OrderedFloat(0.0),
                    output_max: ordered_float::OrderedFloat(1.0),
                    clamp: true,
                },
                operator: crate::model::authoring::BindingOperator::Multiply,
                smoothing_seconds: ordered_float::OrderedFloat(0.05),
                priority: 0,
            })
            .expect("Published parameter Binding");
        assert!(
            service
                .snapshot()
                .expect("binding snapshot")
                .signal_bindings
                .contains_key(&binding_id)
        );
        let before_logic_edit = service.snapshot().expect("logic snapshot");
        let placement = before_logic_edit.items[&item_id].clone();
        let definition = &before_logic_edit.module_definitions[&definition_id];
        let definition_version = definition.version;
        let node = definition.graph.nodes.values().next().expect("Blur Node");
        let node_id = node.id;
        let enabled = node.enabled;
        let bypassed = node.bypassed;
        drop(before_logic_edit);
        service
            .set_module_node_state(
                definition_id,
                node_id,
                "Blur Core".to_string(),
                enabled,
                bypassed,
            )
            .expect("Logic edit");
        let after_logic_edit = service.snapshot().expect("edited snapshot");
        assert_eq!(after_logic_edit.items[&item_id], placement);
        assert_eq!(
            after_logic_edit.module_definitions[&definition_id].version,
            definition_version + 1
        );
        drop(after_logic_edit);
        service.split_item(item_id, 1.0).expect("split");
        let (_, frame) = service
            .evaluate_frame(timeline_id, 0.5, 1.0, None)
            .expect("frame");
        assert_eq!(frame.object_count(), 1);
        let crate::model::frame::entity::FrameItem::Group(track) = &frame.items[0] else {
            panic!("Track group expected");
        };
        let crate::model::frame::entity::FrameItem::Group(item) = &track.items[0] else {
            panic!("Item group expected");
        };
        assert_eq!(item.effects[0].effect_type, "blur");

        let directory = tempfile::tempdir().expect("directory");
        let path = directory.path().join("vertical-slice.ruvie");
        service.save_as(&path).expect("save");
        let reopened = TimelineEditorService::open(&path).expect("open");
        assert_eq!(reopened.snapshot().expect("snapshot").items.len(), 2);
    }

    #[test]
    fn an_open_nested_timeline_is_a_preview_entry_point() {
        let service = TimelineEditorService::create_default("Nested preview").expect("service");
        let (nested_id, nested_track_id, _) = service
            .add_timeline("Title".to_string(), 640, 360, 24.0, 5.0)
            .expect("nested Timeline");
        let (solid_id, _) = service
            .add_solid(
                nested_track_id,
                Color::white(),
                TimelineInterval::new(0.0, 5.0).expect("interval"),
                0,
            )
            .expect("solid");
        service
            .upsert_item_keyframe(
                solid_id,
                "position".to_string(),
                1.0,
                PropertyValue::Vec2(crate::model::project::property::Vec2 {
                    x: ordered_float::OrderedFloat(40.0),
                    y: ordered_float::OrderedFloat(20.0),
                }),
                None,
            )
            .expect("position keyframe");
        let keyframe_id = service.snapshot().expect("keyframe snapshot").items[&solid_id]
            .authored_properties
            .get("position")
            .unwrap()
            .keyframes()[0]
            .id;
        service
            .update_item_keyframe(
                solid_id,
                "position".to_string(),
                keyframe_id,
                crate::model::project::property::KeyframeUpdate {
                    value: Some(PropertyValue::Vec2(crate::model::project::property::Vec2 {
                        x: ordered_float::OrderedFloat(40.0),
                        y: ordered_float::OrderedFloat(25.0),
                    })),
                    ..Default::default()
                },
            )
            .expect("edit persistent Keyframe");

        let (_, frame) = service
            .evaluate_frame(nested_id, 1.0, 1.0, None)
            .expect("nested frame");

        assert_eq!((frame.width, frame.height), (640, 360));
        assert_eq!(frame.object_count(), 1);
        let crate::model::frame::entity::FrameItem::Group(track) = &frame.items[0] else {
            panic!("Track group expected");
        };
        let crate::model::frame::entity::FrameItem::Group(item) = &track.items[0] else {
            panic!("Item group expected");
        };
        assert_eq!(item.transform.position.x, 40.0);
    }

    #[test]
    fn csv_refresh_keeps_direct_timeline_corrections_and_reports_removed_rows() {
        let service = TimelineEditorService::create_default("Data-driven").expect("service");
        let snapshot = service.snapshot().expect("snapshot");
        let track_id = snapshot.timelines[&snapshot.root_timeline_id].track_order[0];
        drop(snapshot);
        let directory = tempfile::tempdir().expect("directory");
        let path = directory.path().join("labels.csv");
        std::fs::write(&path, "id,text,x\nhero,Generated,10\n").expect("fixture");
        let (data_source_id, _) = service
            .import_data_source(&path, track_id)
            .expect("import data");
        let snapshot = service.snapshot().expect("generated snapshot");
        let item_id = *snapshot.items.keys().next().expect("materialized item");
        drop(snapshot);
        service
            .set_text(item_id, "Manual correction".to_string())
            .expect("direct edit");

        std::fs::write(&path, "id,text,x\nhero,Updated source,20\n").expect("updated fixture");
        service
            .refresh_data_source(data_source_id)
            .expect("refresh data");
        let snapshot = service.snapshot().expect("refreshed snapshot");
        assert!(matches!(
            &snapshot.items[&item_id].source,
            SourceRef::Text { text } if text == "Manual correction"
        ));
        assert_eq!(
            snapshot.overrides.values().next().expect("override").status,
            crate::model::authoring::OverrideStatus::Active
        );
        drop(snapshot);

        std::fs::write(&path, "id,text,x\n").expect("empty fixture");
        service
            .refresh_data_source(data_source_id)
            .expect("refresh removed row");
        let snapshot = service.snapshot().expect("orphan snapshot");
        assert!(!snapshot.items.contains_key(&item_id));
        assert_eq!(
            snapshot.overrides.values().next().expect("override").status,
            crate::model::authoring::OverrideStatus::Orphaned
        );
        let override_id = *snapshot.overrides.keys().next().expect("override id");
        drop(snapshot);
        service
            .remove_generated_override(override_id)
            .expect("discard orphaned correction");
        assert!(
            service
                .snapshot()
                .expect("resolved snapshot")
                .overrides
                .is_empty()
        );
    }

    #[test]
    fn cross_dissolve_creates_overlap_without_a_node() {
        let service = TimelineEditorService::create_default("Transition").expect("service");
        let snapshot = service.snapshot().expect("snapshot");
        let track_id = snapshot.timelines[&snapshot.root_timeline_id].track_order[0];
        drop(snapshot);
        service
            .add_solid(
                track_id,
                Color::black(),
                TimelineInterval::new(0.0, 2.0).expect("interval"),
                0,
            )
            .expect("first");
        let (second, _) = service
            .add_solid(
                track_id,
                Color::white(),
                TimelineInterval::new(2.0, 2.0).expect("interval"),
                1,
            )
            .expect("second");
        service
            .add_cross_dissolve(second, 0.5)
            .expect("cross dissolve");
        let first = service
            .snapshot()
            .expect("item snapshot")
            .items
            .values()
            .find(|item| item.id != second)
            .expect("first item")
            .id;
        service
            .add_constraint(
                second,
                first,
                crate::model::authoring::ConstraintKind::CopyPosition,
            )
            .expect("constraint");
        let snapshot = service.snapshot().expect("transition snapshot");
        assert_eq!(snapshot.items[&second].interval.start.into_inner(), 1.5);
        assert_eq!(snapshot.transitions.len(), 1);
        assert_eq!(snapshot.items[&second].constraints.len(), 1);
        assert!(snapshot.module_definitions.is_empty());
        drop(snapshot);
        service.delete_item(first, false).expect("delete endpoint");
        service
            .snapshot()
            .expect("cleaned transition snapshot")
            .validate()
            .expect("transition references remain valid");
    }

    #[test]
    fn rectangle_mask_is_timeline_owned_and_reaches_frame_plan() {
        let service = TimelineEditorService::create_default("Mask").expect("service");
        let snapshot = service.snapshot().expect("snapshot");
        let track_id = snapshot.timelines[&snapshot.root_timeline_id].track_order[0];
        drop(snapshot);
        let (item_id, _) = service
            .add_solid(
                track_id,
                Color::white(),
                TimelineInterval::new(0.0, 2.0).expect("interval"),
                0,
            )
            .expect("solid");
        let (mask_id, _) = service.add_rectangle_mask(item_id).expect("mask");
        service
            .update_mask(
                item_id,
                mask_id,
                0.0,
                crate::model::authoring::MaskMode::Difference,
                true,
                12.0,
                0.75,
            )
            .expect("mask controls");
        let (project, frame) = service
            .evaluate_frame(
                service.snapshot().expect("snapshot").root_timeline_id,
                0.0,
                1.0,
                None,
            )
            .expect("frame");
        assert!(project.masks.contains_key(&mask_id));
        assert_eq!(
            project.masks[&mask_id].mode,
            crate::model::authoring::MaskMode::Difference
        );
        assert!(project.masks[&mask_id].inverted);
        assert!(project.module_definitions.is_empty());
        let crate::model::frame::entity::FrameItem::Group(track) = &frame.items[0] else {
            panic!("Track group expected");
        };
        let crate::model::frame::entity::FrameItem::Group(item) = &track.items[0] else {
            panic!("Item group expected");
        };
        assert_eq!(item.masks.len(), 1);
    }
}
