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
    AuthoringProject, AuthoringSession, ChangeSet, CompositionInstance, DurationPolicy,
    ProjectDocument, ProjectFileStore, ProjectRevision, SourceRef, TimeMap, TimelineId,
    TimelineInterval, TimelineItemId, TimelineTrackId, TimelineTrackKind,
};
use crate::model::authoring::{ModuleInstanceId, PublishedParameterId};
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

    pub fn set_parent(
        &self,
        item_id: TimelineItemId,
        parent: Option<TimelineItemId>,
    ) -> Result<ChangeSet, LibraryError> {
        self.write_session()?
            .set_parent(item_id, parent)
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
        service
            .add_solid(
                nested_track_id,
                Color::white(),
                TimelineInterval::new(0.0, 5.0).expect("interval"),
                0,
            )
            .expect("solid");

        let (_, frame) = service
            .evaluate_frame(nested_id, 1.0, 1.0, None)
            .expect("nested frame");

        assert_eq!((frame.width, frame.height), (640, 360));
        assert_eq!(frame.object_count(), 1);
    }
}
