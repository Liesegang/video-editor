use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant};

use eframe::egui;
use egui_dock::{DockArea, DockState, NodeIndex, Style, TabViewer};
use library::model::authoring::{
    AuthoringProject, BindingOperator, BindingScope, DataSourceId, DurationPolicy, InstancePath,
    ModuleDefinitionId, ModuleInstanceId, OverrideId, PublishedParameterId, SignalBinding,
    SignalBindingId, SignalMapping, SignalSource, SourceRef, TimelineId, TimelineInterval,
    TimelineItemId, TimelineTrackId, TimelineTrackKind,
};
use library::model::frame::color::Color;
use library::model::project::asset::{Asset, AssetKind};
use library::model::project::property::{Keyframe, KeyframeId, KeyframeUpdate, Property};
use library::model::project::property::{PropertyValue, Vec2};
use library::rendering::renderer::RenderOutput;
use library::{RenderDestination, RenderService, SkiaRenderer, TimelineEditorService};
use ordered_float::OrderedFloat;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Workspace {
    Beginner,
    Edit,
    Motion,
    Data,
    Logic,
    Diagnostics,
}

impl Workspace {
    const ALL: [Self; 6] = [
        Self::Beginner,
        Self::Edit,
        Self::Motion,
        Self::Data,
        Self::Logic,
        Self::Diagnostics,
    ];

    fn label(self) -> &'static str {
        match self {
            Self::Beginner => "Beginner",
            Self::Edit => "Edit",
            Self::Motion => "Motion",
            Self::Data => "Data",
            Self::Logic => "Logic",
            Self::Diagnostics => "Diagnostics",
        }
    }

    fn depth(self) -> u8 {
        match self {
            Self::Beginner => 0,
            Self::Edit => 1,
            Self::Motion | Self::Data => 2,
            Self::Logic => 3,
            Self::Diagnostics => 4,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Tab {
    Preview,
    Timeline,
    Inspector,
    Assets,
    Motion,
    Data,
    Logic,
    Diagnostics,
}

#[derive(Debug)]
enum Edit {
    Select(Option<TimelineItemId>),
    OpenTimeline(TimelineId, InstancePath),
    Rename(TimelineItemId, String),
    SetText(TimelineItemId, String),
    Move(TimelineItemId, TimelineTrackId, f64, i64),
    Trim(TimelineItemId, TimelineInterval),
    Property(TimelineItemId, String, PropertyValue),
    Keyframe(TimelineItemId, String, PropertyValue),
    Split(TimelineItemId),
    Blur(TimelineItemId),
    ModuleParameter(ModuleInstanceId, PublishedParameterId, PropertyValue),
    ModuleNodeState(ModuleDefinitionId, uuid::Uuid, String, bool, bool),
    AddSignalBinding(SignalBinding),
    SetParent(TimelineItemId, Option<TimelineItemId>),
    DurationPolicy(TimelineItemId, DurationPolicy),
    Delete(TimelineItemId, bool),
    Fade(TimelineItemId, f64),
    UpdateKeyframe(TimelineItemId, String, KeyframeId, KeyframeUpdate),
    RemoveKeyframe(TimelineItemId, String, KeyframeId),
    ImportData(std::path::PathBuf, TimelineTrackId),
    RefreshData(DataSourceId),
    DiscardOverride(OverrideId),
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum HistoryKey {
    Item(TimelineItemId, &'static str),
    Property(TimelineItemId, String),
    ModuleParameter(ModuleInstanceId, PublishedParameterId),
    ModuleNode(ModuleDefinitionId, uuid::Uuid),
    Binding,
    Data,
}

impl Edit {
    fn history_key(&self) -> Option<HistoryKey> {
        match self {
            Self::Select(_) | Self::OpenTimeline(_, _) => None,
            Self::Rename(item, _) => Some(HistoryKey::Item(*item, "rename")),
            Self::SetText(item, _) => Some(HistoryKey::Item(*item, "text")),
            Self::Move(item, ..) => Some(HistoryKey::Item(*item, "move")),
            Self::Trim(item, _) => Some(HistoryKey::Item(*item, "trim")),
            Self::Property(item, key, _) | Self::Keyframe(item, key, _) => {
                Some(HistoryKey::Property(*item, key.clone()))
            }
            Self::Split(item) => Some(HistoryKey::Item(*item, "split")),
            Self::Blur(item) => Some(HistoryKey::Item(*item, "effect")),
            Self::ModuleParameter(instance, parameter, _) => {
                Some(HistoryKey::ModuleParameter(*instance, *parameter))
            }
            Self::ModuleNodeState(definition, node, ..) => {
                Some(HistoryKey::ModuleNode(*definition, *node))
            }
            Self::AddSignalBinding(_) => Some(HistoryKey::Binding),
            Self::SetParent(item, _) => Some(HistoryKey::Item(*item, "parent")),
            Self::DurationPolicy(item, _) => Some(HistoryKey::Item(*item, "duration-policy")),
            Self::Delete(item, _) => Some(HistoryKey::Item(*item, "delete")),
            Self::Fade(item, _) => Some(HistoryKey::Property(*item, "opacity".to_string())),
            Self::UpdateKeyframe(item, key, ..) | Self::RemoveKeyframe(item, key, _) => {
                Some(HistoryKey::Property(*item, key.clone()))
            }
            Self::ImportData(..) | Self::RefreshData(_) | Self::DiscardOverride(_) => {
                Some(HistoryKey::Data)
            }
        }
    }
}

pub struct TimelineApp {
    editor: TimelineEditorService,
    plugins: Arc<library::plugin::PluginManager>,
    renderer: RenderService<SkiaRenderer>,
    dock: DockState<Tab>,
    workspace: Workspace,
    open_timeline: TimelineId,
    instance_path: InstancePath,
    selected_item: Option<TimelineItemId>,
    current_time: f64,
    preview: Option<egui::TextureHandle>,
    preview_key: Option<(library::model::authoring::ProjectRevision, TimelineId, u64)>,
    undo: Vec<AuthoringProject>,
    redo: Vec<AuthoringProject>,
    last_history_group: Option<(HistoryKey, Instant)>,
    status: String,
}

impl TimelineApp {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Result<Self, library::LibraryError> {
        egui_extras::install_image_loaders(&cc.egui_ctx);
        let editor = TimelineEditorService::create_default("Untitled")?;
        let open_timeline = editor.snapshot()?.root_timeline_id;
        let instance_path = InstancePath::root(open_timeline);
        let plugins = Arc::new(library::plugin::PluginManager::default());
        let cache = Arc::new(library::cache::CacheManager::new());
        let skia = SkiaRenderer::new(16, 16, Color::black(), false, None, Some(cache.clone()))?;
        Ok(Self {
            editor,
            plugins: plugins.clone(),
            renderer: RenderService::new(skia, plugins, cache),
            dock: dock_for(Workspace::Edit),
            workspace: Workspace::Edit,
            open_timeline,
            instance_path,
            selected_item: None,
            current_time: 0.0,
            preview: None,
            preview_key: None,
            undo: Vec::new(),
            redo: Vec::new(),
            last_history_group: None,
            status: "Timeline-first project ready".to_string(),
        })
    }

    fn new_project(&mut self) {
        match AuthoringProject::new("Untitled", 1920, 1080, 30.0, 60.0)
            .map_err(library::LibraryError::Validation)
            .and_then(|project| self.editor.replace_project(project))
        {
            Ok(()) => {
                self.open_timeline = self.editor.snapshot().unwrap().root_timeline_id;
                self.instance_path = InstancePath::root(self.open_timeline);
                self.selected_item = None;
                self.current_time = 0.0;
                self.undo.clear();
                self.redo.clear();
                self.last_history_group = None;
                self.invalidate_preview();
                self.status = "New project".to_string();
            }
            Err(error) => self.status = error.to_string(),
        }
    }

    fn open_project(&mut self) {
        let Some(path) = rfd::FileDialog::new()
            .add_filter("RuViE project", &["ruvie", "json"])
            .pick_file()
        else {
            return;
        };
        match TimelineEditorService::open(&path) {
            Ok(editor) => {
                self.editor = editor;
                self.open_timeline = self.editor.snapshot().unwrap().root_timeline_id;
                self.instance_path = InstancePath::root(self.open_timeline);
                self.selected_item = None;
                self.current_time = 0.0;
                self.undo.clear();
                self.redo.clear();
                self.last_history_group = None;
                self.invalidate_preview();
                self.status = format!("Opened {}", path.display());
            }
            Err(error) => self.status = error.to_string(),
        }
    }

    fn save(&mut self, save_as: bool) {
        let path = if save_as || self.editor.project_path().ok().flatten().is_none() {
            rfd::FileDialog::new()
                .add_filter("RuViE project", &["ruvie"])
                .set_file_name("project.ruvie")
                .save_file()
        } else {
            self.editor.project_path().ok().flatten()
        };
        let Some(path) = path else { return };
        match self.editor.save_as(&path) {
            Ok(()) => self.status = format!("Saved {}", path.display()),
            Err(error) => self.status = error.to_string(),
        }
    }

    fn record(&mut self, before: AuthoringProject) {
        self.undo.push(before);
        self.redo.clear();
        self.last_history_group = None;
    }

    fn undo(&mut self) {
        self.last_history_group = None;
        let Some(project) = self.undo.pop() else {
            return;
        };
        let Ok(current) = self.editor.snapshot() else {
            return;
        };
        self.redo.push(current.as_ref().clone());
        if let Err(error) = self.editor.replace_project(project) {
            self.status = error.to_string();
            return;
        }
        self.repair_navigation();
        self.status = "Undo".to_string();
    }

    fn redo(&mut self) {
        self.last_history_group = None;
        let Some(project) = self.redo.pop() else {
            return;
        };
        let Ok(current) = self.editor.snapshot() else {
            return;
        };
        self.undo.push(current.as_ref().clone());
        if let Err(error) = self.editor.replace_project(project) {
            self.status = error.to_string();
            return;
        }
        self.repair_navigation();
        self.status = "Redo".to_string();
    }

    fn repair_navigation(&mut self) {
        if let Ok(project) = self.editor.snapshot() {
            if !project.timelines.contains_key(&self.open_timeline) {
                self.open_timeline = project.root_timeline_id;
                self.instance_path = InstancePath::root(self.open_timeline);
            }
            if self
                .selected_item
                .is_some_and(|id| !project.items.contains_key(&id))
            {
                self.selected_item = None;
            }
        }
        self.invalidate_preview();
    }

    fn export_frame(&mut self) {
        let Some(path) = rfd::FileDialog::new()
            .add_filter("PNG image", &["png"])
            .set_file_name("frame.png")
            .save_file()
        else {
            return;
        };
        let result = self.editor.snapshot().and_then(|project| {
            let timeline = &project.timelines[&self.open_timeline];
            self.renderer.renderer.resize_render_target(
                timeline.width as u32,
                timeline.height as u32,
                timeline.background_color.clone(),
            )?;
            drop(project);
            let (project, frame) =
                self.editor
                    .evaluate_frame(self.open_timeline, self.current_time, 1.0, None)?;
            let exported = self
                .renderer
                .render_authoring_export_frame(project.as_ref(), &frame)?;
            let image = exported.image();
            image::save_buffer(
                &path,
                &image.data,
                image.width,
                image.height,
                image::ColorType::Rgba8,
            )
            .map_err(|error| library::LibraryError::Runtime(error.to_string()))
        });
        match result {
            Ok(()) => self.status = format!("Exported {}", path.display()),
            Err(error) => self.status = error.to_string(),
        }
        self.invalidate_preview();
    }

    fn export_video(&mut self) {
        let Some(path) = rfd::FileDialog::new()
            .add_filter("MP4 video", &["mp4"])
            .set_file_name("video.mp4")
            .save_file()
        else {
            return;
        };
        let output = path.to_string_lossy().into_owned();
        let project = match self.editor.snapshot() {
            Ok(project) => project,
            Err(error) => {
                self.status = error.to_string();
                return;
            }
        };
        if project.assets.iter().any(|asset| {
            !asset.path.is_empty()
                && Path::new(&asset.path)
                    .canonicalize()
                    .ok()
                    .zip(path.canonicalize().ok())
                    .is_some_and(|(source, destination)| source == destination)
        }) {
            self.status = "Export path cannot overwrite a source asset".to_string();
            return;
        }
        let timeline = project.timelines[&self.open_timeline].clone();
        let mut settings = match library::plugin::ExportSettings::from_authoring_project(
            project.as_ref(),
            &timeline,
        ) {
            Ok(settings) => settings,
            Err(error) => {
                self.status = error.to_string();
                return;
            }
        };
        settings.container = "mp4".to_string();
        settings.codec = "libx264".to_string();
        settings.pixel_format = "yuv420p".to_string();
        let frame_count = match settings.frame_count_for_duration(timeline.duration.into_inner()) {
            Ok(count) => count,
            Err(error) => {
                self.status = error.to_string();
                return;
            }
        };
        if let Err(error) = self.renderer.renderer.resize_render_target(
            timeline.width as u32,
            timeline.height as u32,
            timeline.background_color.clone(),
        ) {
            self.status = error.to_string();
            return;
        }
        drop(project);
        let result = (|| {
            for frame_index in 0..frame_count {
                let time = settings.frame_time(frame_index)?;
                let (project, frame) =
                    self.editor
                        .evaluate_frame(self.open_timeline, time, 1.0, None)?;
                let frame = self
                    .renderer
                    .render_authoring_export_frame(project.as_ref(), &frame)?;
                self.plugins
                    .export_frame("ffmpeg_export", &output, &frame, &settings)?;
            }
            self.plugins
                .finish_export("ffmpeg_export", &output, &settings)
        })();
        if result.is_err() {
            let _ = self
                .plugins
                .finish_export("ffmpeg_export", &output, &settings);
        }
        match result {
            Ok(()) => self.status = format!("Exported {output}"),
            Err(error) => self.status = error.to_string(),
        }
        self.invalidate_preview();
    }

    fn import_asset(&mut self) {
        let Some(path) = rfd::FileDialog::new().pick_file() else {
            return;
        };
        let before = self
            .editor
            .snapshot()
            .ok()
            .map(|project| project.as_ref().clone());
        let kind = asset_kind(&path);
        let name = path
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .into_owned();
        let mut asset = Asset::new(&name, &path.to_string_lossy(), kind);
        if let Ok(bytes) = std::fs::read(&path) {
            asset.verify_imported_content(&bytes);
        }
        let id = asset.id;
        let result = self.editor.add_asset(asset).and_then(|_| {
            let project = self.editor.snapshot()?;
            let track = *project.timelines[&self.open_timeline]
                .track_order
                .last()
                .unwrap();
            drop(project);
            self.editor.place_asset(
                track,
                id,
                name,
                TimelineInterval::new(self.current_time, 5.0)
                    .map_err(library::LibraryError::Validation)?,
                0,
            )
        });
        match result {
            Ok((id, _)) => {
                if let Some(before) = before {
                    self.record(before);
                }
                self.selected_item = Some(id);
                self.invalidate_preview();
                self.status = "Asset imported and placed".to_string();
            }
            Err(error) => {
                if let Some(before) = before {
                    let _ = self.editor.replace_project(before);
                }
                self.status = error.to_string();
            }
        }
    }

    fn add_text(&mut self) {
        let before = self
            .editor
            .snapshot()
            .ok()
            .map(|project| project.as_ref().clone());
        let result = self.editor.snapshot().and_then(|project| {
            let track = *project.timelines[&self.open_timeline]
                .track_order
                .last()
                .unwrap();
            drop(project);
            self.editor.add_text(
                track,
                "Text".to_string(),
                TimelineInterval::new(self.current_time, 5.0)
                    .map_err(library::LibraryError::Validation)?,
                1,
            )
        });
        self.finish_add(result, before, "Text added");
    }

    fn add_solid(&mut self) {
        let before = self
            .editor
            .snapshot()
            .ok()
            .map(|project| project.as_ref().clone());
        let result = self.editor.snapshot().and_then(|project| {
            let track = *project.timelines[&self.open_timeline]
                .track_order
                .last()
                .unwrap();
            drop(project);
            self.editor.add_solid(
                track,
                Color {
                    r: 55,
                    g: 86,
                    b: 160,
                    a: 255,
                },
                TimelineInterval::new(self.current_time, 5.0)
                    .map_err(library::LibraryError::Validation)?,
                0,
            )
        });
        self.finish_add(result, before, "Solid added");
    }

    fn add_nested_timeline(&mut self) {
        let before = self
            .editor
            .snapshot()
            .ok()
            .map(|project| project.as_ref().clone());
        let result = self
            .editor
            .add_timeline("Composition".to_string(), 1920, 1080, 30.0, 5.0);
        match result {
            Ok((timeline_id, _, _)) => {
                let project = match self.editor.snapshot() {
                    Ok(project) => project,
                    Err(error) => {
                        self.status = error.to_string();
                        return;
                    }
                };
                let track = *project.timelines[&self.open_timeline]
                    .track_order
                    .last()
                    .unwrap();
                drop(project);
                let interval = TimelineInterval::new(self.current_time, 5.0).unwrap();
                match self.editor.place_timeline(
                    track,
                    timeline_id,
                    "Composition".to_string(),
                    interval,
                    DurationPolicy::Fixed,
                    1,
                ) {
                    Ok((item, _)) => {
                        if let Some(before) = before {
                            self.record(before);
                        }
                        self.selected_item = Some(item);
                        self.invalidate_preview();
                        self.status = "Nested composition added; double-click to open".to_string();
                    }
                    Err(error) => {
                        if let Some(before) = before {
                            let _ = self.editor.replace_project(before);
                        }
                        self.status = error.to_string();
                    }
                }
            }
            Err(error) => {
                if let Some(before) = before {
                    let _ = self.editor.replace_project(before);
                }
                self.status = error.to_string();
            }
        }
    }

    fn add_track(&mut self) {
        let before = self
            .editor
            .snapshot()
            .ok()
            .map(|project| project.as_ref().clone());
        match self.editor.add_track(
            self.open_timeline,
            "Video".to_string(),
            TimelineTrackKind::AudioVisual,
        ) {
            Ok(_) => {
                if let Some(before) = before {
                    self.record(before);
                }
                self.status = "Track added".to_string();
            }
            Err(error) => self.status = error.to_string(),
        }
    }

    fn finish_add(
        &mut self,
        result: Result<
            (TimelineItemId, library::model::authoring::ChangeSet),
            library::LibraryError,
        >,
        before: Option<AuthoringProject>,
        message: &str,
    ) {
        match result {
            Ok((item, _)) => {
                if let Some(before) = before {
                    self.record(before);
                }
                self.selected_item = Some(item);
                self.invalidate_preview();
                self.status = message.to_string();
            }
            Err(error) => {
                if let Some(before) = before {
                    let _ = self.editor.replace_project(before);
                }
                self.status = error.to_string();
            }
        }
    }

    fn apply(&mut self, edit: Edit) {
        let history_key = edit.history_key();
        let now = Instant::now();
        let begins_group = history_key.as_ref().is_some_and(|key| {
            self.last_history_group
                .as_ref()
                .is_none_or(|(previous, at)| {
                    previous != key || now.duration_since(*at) > Duration::from_millis(750)
                })
        });
        let before = if begins_group {
            self.editor
                .snapshot()
                .ok()
                .map(|project| project.as_ref().clone())
        } else {
            None
        };
        let result = match edit {
            Edit::Select(item) => {
                self.last_history_group = None;
                self.selected_item = item;
                return;
            }
            Edit::OpenTimeline(id, path) => {
                self.last_history_group = None;
                self.open_timeline = id;
                self.instance_path = path;
                self.selected_item = None;
                self.current_time = 0.0;
                self.invalidate_preview();
                return;
            }
            Edit::Rename(id, value) => self.editor.rename_item(id, value).map(|_| ()),
            Edit::SetText(id, value) => self.editor.set_text(id, value).map(|_| ()),
            Edit::Move(id, track, start, layer) => {
                self.editor.move_item(id, track, start, layer).map(|_| ())
            }
            Edit::Trim(id, interval) => self.editor.trim_item(id, interval).map(|_| ()),
            Edit::Property(id, key, value) => self
                .editor
                .update_item_property_value(id, key, self.current_time, value)
                .map(|_| ()),
            Edit::Keyframe(id, key, value) => self
                .editor
                .upsert_item_keyframe(id, key, self.current_time, value, None)
                .map(|_| ()),
            Edit::Split(id) => self.editor.split_item(id, self.current_time).map(|_| ()),
            Edit::Blur(id) => self
                .editor
                .attach_effect(id, "blur", self.plugins.as_ref())
                .map(|_| ()),
            Edit::ModuleParameter(instance, parameter, value) => self
                .editor
                .set_module_parameter(instance, parameter, value)
                .map(|_| ()),
            Edit::ModuleNodeState(definition, node, name, enabled, bypassed) => self
                .editor
                .set_module_node_state(definition, node, name, enabled, bypassed)
                .map(|_| ()),
            Edit::AddSignalBinding(binding) => self.editor.add_signal_binding(binding).map(|_| ()),
            Edit::SetParent(item, parent) => self.editor.set_parent(item, parent).map(|_| ()),
            Edit::DurationPolicy(item, policy) => self
                .editor
                .set_composition_duration_policy(item, policy)
                .map(|_| ()),
            Edit::Delete(item, ripple) => self.editor.delete_item(item, ripple).map(|_| ()),
            Edit::Fade(item, seconds) => {
                let duration = self
                    .editor
                    .snapshot()
                    .ok()
                    .and_then(|project| {
                        project
                            .items
                            .get(&item)
                            .map(|item| item.interval.duration.into_inner())
                    })
                    .unwrap_or(0.0);
                let edge = seconds.min(duration / 2.0).max(0.0);
                let number = |value| PropertyValue::Number(OrderedFloat(value));
                self.editor
                    .set_item_property(
                        item,
                        "opacity".to_string(),
                        Property::keyframe(vec![
                            Keyframe::new(
                                0.0,
                                number(0.0),
                                library::animation::EasingFunction::Linear,
                            ),
                            Keyframe::new(
                                edge,
                                number(1.0),
                                library::animation::EasingFunction::EaseOutSine,
                            ),
                            Keyframe::new(
                                (duration - edge).max(edge),
                                number(1.0),
                                library::animation::EasingFunction::Linear,
                            ),
                            Keyframe::new(
                                duration,
                                number(0.0),
                                library::animation::EasingFunction::EaseInSine,
                            ),
                        ]),
                    )
                    .map(|_| ())
            }
            Edit::UpdateKeyframe(item, key, keyframe, update) => self
                .editor
                .update_item_keyframe(item, key, keyframe, update)
                .map(|_| ()),
            Edit::RemoveKeyframe(item, key, keyframe) => self
                .editor
                .remove_item_keyframe(item, key, keyframe)
                .map(|_| ()),
            Edit::ImportData(path, track_id) => {
                self.editor.import_data_source(&path, track_id).map(|_| ())
            }
            Edit::RefreshData(data_source_id) => {
                self.editor.refresh_data_source(data_source_id).map(|_| ())
            }
            Edit::DiscardOverride(override_id) => self
                .editor
                .remove_generated_override(override_id)
                .map(|_| ()),
        };
        match result {
            Ok(()) => {
                if let Some(before) = before {
                    self.record(before);
                }
                if let Some(key) = history_key {
                    self.last_history_group = Some((key, now));
                }
                self.invalidate_preview();
                self.status = "Edit applied".to_string();
            }
            Err(error) => self.status = error.to_string(),
        }
    }

    fn invalidate_preview(&mut self) {
        self.preview_key = None;
    }

    fn refresh_preview(&mut self, ctx: &egui::Context) {
        let Ok(revision) = self.editor.revision() else {
            return;
        };
        let Ok(project) = self.editor.snapshot() else {
            return;
        };
        let Some(timeline) = project.timelines.get(&self.open_timeline) else {
            return;
        };
        let frame_number = (self.current_time * timeline.fps.into_inner()).floor() as u64;
        let key = (revision, self.open_timeline, frame_number);
        if self.preview_key == Some(key) {
            return;
        }
        let scale = (720.0 / timeline.width as f64).min(1.0);
        let width = (timeline.width as f64 * scale).round().max(1.0) as u32;
        let height = (timeline.height as f64 * scale).round().max(1.0) as u32;
        if let Err(error) = self.renderer.renderer.resize_render_target(
            width,
            height,
            timeline.background_color.clone(),
        ) {
            self.status = error.to_string();
            return;
        }
        let rendered = self
            .editor
            .evaluate_frame(self.open_timeline, self.current_time, scale, None)
            .and_then(|(project, frame)| {
                self.renderer.render_authoring_frame(
                    project.as_ref(),
                    &frame,
                    RenderDestination::Preview,
                )
            });
        match rendered {
            Ok(RenderOutput::Image(image)) => {
                let color = egui::ColorImage::from_rgba_unmultiplied(
                    [image.width as usize, image.height as usize],
                    &image.data,
                );
                self.preview =
                    Some(ctx.load_texture("timeline-preview", color, egui::TextureOptions::LINEAR));
                self.preview_key = Some(key);
            }
            Ok(_) => self.status = "Preview renderer returned a non-image surface".to_string(),
            Err(error) => self.status = error.to_string(),
        }
    }
}

impl eframe::App for TimelineApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        egui::TopBottomPanel::top("main-toolbar").show(ctx, |ui| {
            ui.horizontal_wrapped(|ui| {
                if ui.button("New").clicked() {
                    self.new_project();
                }
                if ui.button("Open").clicked() {
                    self.open_project();
                }
                if ui.button("Save").clicked() {
                    self.save(false);
                }
                if ui.button("Save As").clicked() {
                    self.save(true);
                }
                if ui
                    .add_enabled(!self.undo.is_empty(), egui::Button::new("Undo"))
                    .clicked()
                {
                    self.undo();
                }
                if ui
                    .add_enabled(!self.redo.is_empty(), egui::Button::new("Redo"))
                    .clicked()
                {
                    self.redo();
                }
                if ui.button("Export Frame").clicked() {
                    self.export_frame();
                }
                if ui.button("Export Video").clicked() {
                    self.export_video();
                }
                ui.separator();
                if ui.button("Import").clicked() {
                    self.import_asset();
                }
                if ui.button("+ Text").clicked() {
                    self.add_text();
                }
                if ui.button("+ Solid").clicked() {
                    self.add_solid();
                }
                if ui.button("+ Composition").clicked() {
                    self.add_nested_timeline();
                }
                if ui.button("+ Track").clicked() {
                    self.add_track();
                }
                if ui.button("Split").clicked() {
                    if let Some(id) = self.selected_item {
                        self.apply(Edit::Split(id));
                    }
                }
                if ui.button("Delete").clicked() {
                    if let Some(id) = self.selected_item {
                        self.apply(Edit::Delete(id, false));
                        self.selected_item = None;
                    }
                }
                if ui.button("Ripple Delete").clicked() {
                    if let Some(id) = self.selected_item {
                        self.apply(Edit::Delete(id, true));
                        self.selected_item = None;
                    }
                }
                if ui.button("Blur").clicked() {
                    if let Some(id) = self.selected_item {
                        self.apply(Edit::Blur(id));
                    }
                }
                ui.separator();
                for workspace in Workspace::ALL {
                    if ui
                        .selectable_label(self.workspace == workspace, workspace.label())
                        .clicked()
                    {
                        self.workspace = workspace;
                        self.dock = dock_for(workspace);
                    }
                }
            });
        });
        self.refresh_preview(ctx);
        let Ok(project) = self.editor.snapshot() else {
            return;
        };
        let mut edits = Vec::new();
        let mut viewer = Viewer {
            project: project.as_ref(),
            open_timeline: self.open_timeline,
            instance_path: &self.instance_path,
            selected_item: self.selected_item,
            current_time: &mut self.current_time,
            preview: self.preview.as_ref(),
            workspace: self.workspace,
            edits: &mut edits,
        };
        DockArea::new(&mut self.dock)
            .style(Style::from_egui(ctx.style().as_ref()))
            .show(ctx, &mut viewer);
        drop(project);
        for edit in edits {
            self.apply(edit);
        }
        egui::TopBottomPanel::bottom("status").show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.label(&self.status);
                ui.separator();
                ui.label(format!(
                    "Depth {} · Timeline owns the edit",
                    self.workspace.depth()
                ));
            });
        });
    }
}

struct Viewer<'a> {
    project: &'a AuthoringProject,
    open_timeline: TimelineId,
    instance_path: &'a InstancePath,
    selected_item: Option<TimelineItemId>,
    current_time: &'a mut f64,
    preview: Option<&'a egui::TextureHandle>,
    workspace: Workspace,
    edits: &'a mut Vec<Edit>,
}

impl TabViewer for Viewer<'_> {
    type Tab = Tab;

    fn ui(&mut self, ui: &mut egui::Ui, tab: &mut Tab) {
        match tab {
            Tab::Preview => preview_ui(
                ui,
                self.preview,
                self.project,
                self.selected_item,
                *self.current_time,
                self.edits,
            ),
            Tab::Timeline => timeline_ui(
                ui,
                self.project,
                self.open_timeline,
                self.instance_path,
                self.selected_item,
                self.current_time,
                self.edits,
            ),
            Tab::Inspector => inspector_ui(
                ui,
                self.project,
                self.selected_item,
                self.instance_path,
                *self.current_time,
                self.edits,
            ),
            Tab::Assets => assets_ui(ui, self.project),
            Tab::Motion => motion_ui(ui, self.project, self.selected_item, self.edits),
            Tab::Data => data_ui(ui, self.project, self.open_timeline, self.edits),
            Tab::Logic => logic_ui(ui, self.project, self.selected_item, self.edits),
            Tab::Diagnostics => diagnostics_ui(ui, self.project, self.workspace),
        }
    }

    fn title(&mut self, tab: &mut Tab) -> egui::WidgetText {
        match tab {
            Tab::Preview => "Preview",
            Tab::Timeline => "Timeline",
            Tab::Inspector => "Inspector",
            Tab::Assets => "Assets",
            Tab::Motion => "Dope Sheet / Curve",
            Tab::Data => "Data",
            Tab::Logic => "Logic Module",
            Tab::Diagnostics => "Diagnostics",
        }
        .into()
    }
}

fn preview_ui(
    ui: &mut egui::Ui,
    preview: Option<&egui::TextureHandle>,
    project: &AuthoringProject,
    selected: Option<TimelineItemId>,
    current_time: f64,
    edits: &mut Vec<Edit>,
) {
    ui.centered_and_justified(|ui| {
        if let Some(texture) = preview {
            let available = ui.available_size();
            let size = texture.size_vec2();
            let scale = (available.x / size.x).min(available.y / size.y).min(1.0);
            let response =
                ui.add(egui::Image::new((texture.id(), size * scale)).sense(egui::Sense::drag()));
            if response.dragged() {
                if let Some(item) = selected.and_then(|id| project.items.get(&id)) {
                    let (x, y) = vec2_property_at(item, "position", current_time, (0.0, 0.0));
                    let delta = ui.input(|input| input.pointer.delta()) / scale;
                    edits.push(Edit::Property(
                        item.id,
                        "position".to_string(),
                        property_vec2(x + f64::from(delta.x), y + f64::from(delta.y)),
                    ));
                }
            }
        } else {
            ui.spinner();
        }
    });
}

fn timeline_ui(
    ui: &mut egui::Ui,
    project: &AuthoringProject,
    timeline_id: TimelineId,
    instance_path: &InstancePath,
    selected: Option<TimelineItemId>,
    time: &mut f64,
    edits: &mut Vec<Edit>,
) {
    let timeline = &project.timelines[&timeline_id];
    ui.horizontal(|ui| {
        if timeline_id != project.root_timeline_id && ui.button("← Main").clicked() {
            edits.push(Edit::OpenTimeline(
                project.root_timeline_id,
                InstancePath::root(project.root_timeline_id),
            ));
        }
        ui.heading(&timeline.name);
        ui.add(egui::Slider::new(time, 0.0..=timeline.duration.into_inner()).text("time"));
    });
    timeline_canvas_ui(ui, project, timeline, instance_path, selected, time, edits);
}

fn timeline_canvas_ui(
    ui: &mut egui::Ui,
    project: &AuthoringProject,
    timeline: &library::model::authoring::Timeline,
    instance_path: &InstancePath,
    selected: Option<TimelineItemId>,
    time: &mut f64,
    edits: &mut Vec<Edit>,
) {
    const HEADER: f32 = 128.0;
    const ROW_HEIGHT: f32 = 52.0;
    const PX: f32 = 80.0;
    let tracks: Vec<_> = timeline.track_order.iter().rev().copied().collect();
    egui::ScrollArea::both().show(ui, |ui| {
        let width = HEADER + timeline.duration.into_inner() as f32 * PX;
        let height = 24.0 + tracks.len() as f32 * ROW_HEIGHT;
        let (canvas, ruler) = ui.allocate_exact_size(
            egui::vec2(width.max(ui.available_width()), height),
            egui::Sense::click(),
        );
        let painter = ui.painter_at(canvas);
        painter.rect_filled(canvas, 0.0, ui.visuals().extreme_bg_color);
        for second in 0..=timeline.duration.into_inner().ceil() as usize {
            let x = canvas.left() + HEADER + second as f32 * PX;
            painter.line_segment(
                [egui::pos2(x, canvas.top()), egui::pos2(x, canvas.bottom())],
                egui::Stroke::new(1.0, ui.visuals().widgets.noninteractive.bg_stroke.color),
            );
            painter.text(
                egui::pos2(x + 3.0, canvas.top() + 3.0),
                egui::Align2::LEFT_TOP,
                format!("{second}s"),
                egui::FontId::monospace(10.0),
                ui.visuals().weak_text_color(),
            );
        }
        if ruler.clicked() {
            if let Some(pointer) = ruler.interact_pointer_pos() {
                let raw = f64::from((pointer.x - canvas.left() - HEADER) / PX).max(0.0);
                *time = snap_time(raw, timeline.fps.into_inner(), &[])
                    .min(timeline.duration.into_inner());
            }
        }
        for (row, track_id) in tracks.iter().enumerate() {
            let track = &project.tracks[track_id];
            let top = canvas.top() + 24.0 + row as f32 * ROW_HEIGHT;
            let row_rect = egui::Rect::from_min_size(
                egui::pos2(canvas.left(), top),
                egui::vec2(canvas.width(), ROW_HEIGHT),
            );
            painter.rect_filled(
                row_rect,
                0.0,
                if row % 2 == 0 {
                    ui.visuals().faint_bg_color
                } else {
                    ui.visuals().extreme_bg_color
                },
            );
            painter.text(
                egui::pos2(row_rect.left() + 8.0, row_rect.center().y),
                egui::Align2::LEFT_CENTER,
                &track.name,
                egui::FontId::proportional(13.0),
                ui.visuals().text_color(),
            );
            let boundaries: Vec<f64> = project
                .items
                .values()
                .filter(|candidate| candidate.track_id == *track_id)
                .flat_map(|candidate| {
                    [
                        candidate.interval.start.into_inner(),
                        candidate.interval.start.into_inner()
                            + candidate.interval.duration.into_inner(),
                    ]
                })
                .collect();
            let mut items: Vec<_> = project
                .items
                .values()
                .filter(|item| item.track_id == *track_id)
                .collect();
            items.sort_by_key(|item| (item.layer, item.interval.start));
            for item in items {
                let left = canvas.left() + HEADER + item.interval.start.into_inner() as f32 * PX;
                let width = (item.interval.duration.into_inner() as f32 * PX).max(12.0);
                let rect = egui::Rect::from_min_size(
                    egui::pos2(left, top + 6.0),
                    egui::vec2(width, ROW_HEIGHT - 12.0),
                );
                let trim_rect = egui::Rect::from_min_max(
                    egui::pos2(rect.right() - 7.0, rect.top()),
                    rect.right_bottom(),
                );
                let trim = ui.interact(
                    trim_rect,
                    ui.make_persistent_id(("trim", item.id.as_uuid())),
                    egui::Sense::drag(),
                );
                let body = ui.interact(
                    egui::Rect::from_min_max(rect.min, egui::pos2(trim_rect.left(), rect.bottom())),
                    ui.make_persistent_id(("move", item.id.as_uuid())),
                    egui::Sense::click_and_drag(),
                );
                let visual = rect.translate(if body.dragged() {
                    egui::vec2(body.drag_delta().x, 0.0)
                } else {
                    egui::Vec2::ZERO
                });
                let color = if selected == Some(item.id) {
                    ui.visuals().selection.bg_fill
                } else {
                    item_color(&item.source)
                };
                painter.rect_filled(visual, 4.0, color);
                painter.rect_filled(
                    egui::Rect::from_min_max(
                        egui::pos2(visual.right() - 7.0, visual.top()),
                        visual.right_bottom(),
                    ),
                    2.0,
                    color.gamma_multiply(1.35),
                );
                painter.text(
                    visual.left_center() + egui::vec2(7.0, 0.0),
                    egui::Align2::LEFT_CENTER,
                    &item.name,
                    egui::FontId::proportional(12.0),
                    egui::Color32::WHITE,
                );
                if body.clicked() {
                    edits.push(Edit::Select(Some(item.id)));
                }
                if body.double_clicked() {
                    if let SourceRef::Composition(instance) = &item.source {
                        edits.push(Edit::OpenTimeline(
                            instance.timeline_id,
                            instance_path.clone().nested(item.id),
                        ));
                    }
                }
                if body.drag_stopped() {
                    let raw =
                        item.interval.start.into_inner() + f64::from(body.drag_delta().x / PX);
                    edits.push(Edit::Move(
                        item.id,
                        item.track_id,
                        snap_time(raw.max(0.0), timeline.fps.into_inner(), &boundaries),
                        item.layer,
                    ));
                }
                if trim.drag_stopped() {
                    let raw_end = item.interval.start.into_inner()
                        + item.interval.duration.into_inner()
                        + f64::from(trim.drag_delta().x / PX);
                    let end = snap_time(raw_end, timeline.fps.into_inner(), &boundaries);
                    if let Ok(interval) = TimelineInterval::new(
                        item.interval.start.into_inner(),
                        (end - item.interval.start.into_inner()).max(0.0),
                    ) {
                        edits.push(Edit::Trim(item.id, interval));
                    }
                }
            }
        }
        let playhead_x = canvas.left() + HEADER + *time as f32 * PX;
        painter.line_segment(
            [
                egui::pos2(playhead_x, canvas.top()),
                egui::pos2(playhead_x, canvas.bottom()),
            ],
            egui::Stroke::new(2.0, egui::Color32::from_rgb(245, 90, 75)),
        );
    });
}

fn item_color(source: &SourceRef) -> egui::Color32 {
    match source {
        SourceRef::Text { .. } => egui::Color32::from_rgb(190, 92, 160),
        SourceRef::Composition(_) => egui::Color32::from_rgb(112, 92, 196),
        SourceRef::Asset { .. } => egui::Color32::from_rgb(55, 135, 185),
        SourceRef::Solid { .. } | SourceRef::Shape { .. } => egui::Color32::from_rgb(75, 145, 105),
        SourceRef::Module { .. } => egui::Color32::from_rgb(190, 125, 45),
    }
}

fn snap_time(raw: f64, fps: f64, boundaries: &[f64]) -> f64 {
    let frame = (raw * fps).round() / fps;
    boundaries
        .iter()
        .copied()
        .filter(|boundary| (*boundary - raw).abs() <= 0.12)
        .min_by(|left, right| (left - raw).abs().total_cmp(&(right - raw).abs()))
        .unwrap_or(frame)
}

fn inspector_ui(
    ui: &mut egui::Ui,
    project: &AuthoringProject,
    selected: Option<TimelineItemId>,
    instance_path: &InstancePath,
    current_time: f64,
    edits: &mut Vec<Edit>,
) {
    let Some(id) = selected else {
        ui.label("Select a Timeline item");
        return;
    };
    let Some(item) = project.items.get(&id) else {
        return;
    };
    ui.heading("Instance");
    let mut name = item.name.clone();
    if ui.text_edit_singleline(&mut name).changed() {
        edits.push(Edit::Rename(id, name));
    }
    if let SourceRef::Text { text } = &item.source {
        let mut value = text.clone();
        ui.label("Text");
        if ui.text_edit_multiline(&mut value).changed() {
            edits.push(Edit::SetText(id, value));
        }
    }
    let mut start = item.interval.start.into_inner();
    let mut duration = item.interval.duration.into_inner();
    let mut layer = item.layer;
    if ui
        .add(
            egui::DragValue::new(&mut start)
                .speed(0.05)
                .prefix("Start "),
        )
        .changed()
    {
        edits.push(Edit::Move(id, item.track_id, start.max(0.0), layer));
    }
    if ui
        .add(
            egui::DragValue::new(&mut duration)
                .speed(0.05)
                .prefix("Duration "),
        )
        .changed()
    {
        if let Ok(interval) = TimelineInterval::new(start, duration.max(0.0)) {
            edits.push(Edit::Trim(id, interval));
        }
    }
    if ui
        .add(egui::DragValue::new(&mut layer).prefix("Layer "))
        .changed()
    {
        edits.push(Edit::Move(id, item.track_id, start, layer));
    }
    let timeline_id = project.tracks[&item.track_id].timeline_id;
    egui::ComboBox::from_label("Track")
        .selected_text(&project.tracks[&item.track_id].name)
        .show_ui(ui, |ui| {
            for track_id in &project.timelines[&timeline_id].track_order {
                let track = &project.tracks[track_id];
                if ui
                    .selectable_label(*track_id == item.track_id, &track.name)
                    .clicked()
                {
                    edits.push(Edit::Move(id, *track_id, start, layer));
                }
            }
        });
    egui::ComboBox::from_label("Parent")
        .selected_text(
            item.parent
                .and_then(|parent| project.items.get(&parent))
                .map(|parent| parent.name.as_str())
                .unwrap_or("None"),
        )
        .show_ui(ui, |ui| {
            if ui.selectable_label(item.parent.is_none(), "None").clicked() {
                edits.push(Edit::SetParent(id, None));
            }
            for candidate in project.items.values().filter(|candidate| {
                candidate.id != id && project.tracks[&candidate.track_id].timeline_id == timeline_id
            }) {
                if ui
                    .selectable_label(item.parent == Some(candidate.id), &candidate.name)
                    .clicked()
                {
                    edits.push(Edit::SetParent(id, Some(candidate.id)));
                }
            }
        });
    if let SourceRef::Composition(instance) = &item.source {
        egui::ComboBox::from_label("Duration")
            .selected_text(format!("{:?}", instance.duration_policy))
            .show_ui(ui, |ui| {
                for (label, policy) in [
                    ("Fixed", DurationPolicy::Fixed),
                    ("Scale", DurationPolicy::Scale),
                    ("Loop", DurationPolicy::Loop),
                ] {
                    if ui
                        .selectable_label(instance.duration_policy == policy, label)
                        .clicked()
                    {
                        edits.push(Edit::DurationPolicy(id, policy));
                    }
                }
            });
    }
    if ui.button("Fade In / Out").clicked() {
        edits.push(Edit::Fade(id, 0.5));
    }
    ui.separator();
    ui.heading("Transform");
    let (mut x, mut y) = vec2_property_at(item, "position", current_time, (0.0, 0.0));
    let x_changed = ui
        .add(egui::DragValue::new(&mut x).speed(1.0).prefix("X "))
        .changed();
    let y_changed = ui
        .add(egui::DragValue::new(&mut y).speed(1.0).prefix("Y "))
        .changed();
    if x_changed || y_changed {
        edits.push(Edit::Property(
            id,
            "position".to_string(),
            property_vec2(x, y),
        ));
    }
    if ui.button("Add position keyframe").clicked() {
        edits.push(Edit::Keyframe(
            id,
            "position".to_string(),
            property_vec2(x, y),
        ));
    }
    let mut opacity = number_property_at(item, "opacity", current_time, 1.0);
    if ui
        .add(egui::Slider::new(&mut opacity, 0.0..=1.0).text("Opacity"))
        .changed()
    {
        edits.push(Edit::Property(
            id,
            "opacity".to_string(),
            PropertyValue::Number(OrderedFloat(opacity)),
        ));
    }
    if ui.button("Add opacity keyframe").clicked() {
        edits.push(Edit::Keyframe(
            id,
            "opacity".to_string(),
            PropertyValue::Number(OrderedFloat(opacity)),
        ));
    }
    ui.indent("opacity-provenance", |ui| {
        if let Some(property) = item.authored_properties.get("opacity") {
            if let Some(base) = property.value() {
                ui.small(format!("Base: {:?}", base));
            }
            if property.evaluator == "keyframe" {
                ui.small(format!("Keyframe at {:.3}s: {:.3}", current_time, opacity));
            }
        } else {
            ui.small("Base: 1.0");
        }
        if let Some(parent) = item.parent.and_then(|parent| project.items.get(&parent)) {
            let inherited = number_property_at(parent, "opacity", current_time, 1.0);
            ui.small(format!("Parent {}: x{inherited:.3}", parent.name));
        }
    });
    ui.separator();
    ui.heading("Effect Stack");
    let attachments: Vec<_> = project.attachments.values().filter(|attachment| matches!(attachment.owner, library::model::authoring::AttachmentOwner::Item { item_id } if item_id == id)).collect();
    if attachments.is_empty() {
        ui.label("No effects");
    }
    for attachment in attachments {
        let instance = &project.module_instances[&attachment.module_instance_id];
        let definition = &project.module_definitions[&instance.definition_id];
        ui.label(format!("{} · {:?}", definition.name, attachment.stage));
        for parameter in &definition.published_parameters {
            let value = instance
                .parameter_overrides
                .get(&parameter.id)
                .unwrap_or(&parameter.default_value);
            match value {
                PropertyValue::Number(number) => {
                    let mut number = number.into_inner();
                    if ui
                        .add(
                            egui::DragValue::new(&mut number)
                                .speed(0.1)
                                .prefix(format!("{} ", parameter.name)),
                        )
                        .changed()
                    {
                        edits.push(Edit::ModuleParameter(
                            instance.id,
                            parameter.id,
                            PropertyValue::Number(OrderedFloat(number)),
                        ));
                    }
                }
                _ => {
                    ui.small(format!("{} = {:?}", parameter.name, value));
                }
            }
            ui.indent((instance.id, parameter.id), |ui| {
                ui.small(format!("Base: {:?}", parameter.default_value));
                if let Some(value) = instance.parameter_overrides.get(&parameter.id) {
                    ui.small(format!("Instance override: {:?}", value));
                }
                let bindings: Vec<_> = project
                    .signal_bindings
                    .values()
                    .filter(|binding| {
                        binding.target_parameter_id == parameter.id
                            && match &binding.scope {
                                BindingScope::Definition { definition_id } => {
                                    *definition_id == definition.id
                                }
                                BindingScope::Instance {
                                    instance_path: target_path,
                                    module_instance_id,
                                } => {
                                    *module_instance_id == instance.id
                                        && target_path == instance_path
                                }
                                BindingScope::Query { .. } => false,
                            }
                    })
                    .collect();
                for binding in &bindings {
                    ui.small(format!(
                        "Automation: {:?} via {:?}",
                        binding.source, binding.operator
                    ));
                }
                if bindings.is_empty() && ui.button("Bind audio envelope").clicked() {
                    let binding_id = SignalBindingId::new();
                    edits.push(Edit::AddSignalBinding(SignalBinding {
                        id: binding_id,
                        source: SignalSource::AudioEnvelope {
                            channel: "master".to_string(),
                        },
                        scope: BindingScope::Instance {
                            instance_path: instance_path.clone(),
                            module_instance_id: instance.id,
                        },
                        target_parameter_id: parameter.id,
                        mapping: SignalMapping {
                            input_min: OrderedFloat(0.0),
                            input_max: OrderedFloat(1.0),
                            output_min: OrderedFloat(0.0),
                            output_max: OrderedFloat(1.0),
                            clamp: true,
                        },
                        operator: BindingOperator::Multiply,
                        smoothing_seconds: OrderedFloat(0.05),
                        priority: 0,
                    }));
                }
            });
        }
    }
}

fn assets_ui(ui: &mut egui::Ui, project: &AuthoringProject) {
    ui.heading("Project Assets");
    if project.assets.is_empty() {
        ui.label("Use Import to add media");
    }
    for asset in &project.assets {
        ui.label(format!("{} · {:?}", asset.name, asset.kind));
    }
    ui.separator();
    ui.label("Compositions");
    for timeline in project.timelines.values() {
        ui.label(&timeline.name);
    }
}

fn motion_ui(
    ui: &mut egui::Ui,
    project: &AuthoringProject,
    selected: Option<TimelineItemId>,
    edits: &mut Vec<Edit>,
) {
    ui.heading("Timeline-owned animation");
    let Some(item) = selected.and_then(|id| project.items.get(&id)) else {
        ui.label("Select an item");
        return;
    };
    for (name, property) in item.authored_properties.iter() {
        ui.collapsing(name, |ui| {
            if property.evaluator == "keyframe" {
                let keyframes = property.keyframes();
                draw_numeric_curve(ui, &keyframes, item.interval.duration.into_inner());
                for keyframe in keyframes {
                    ui.horizontal(|ui| {
                        let mut time = keyframe.time.into_inner();
                        let time_changed = ui
                            .add(egui::DragValue::new(&mut time).speed(0.01).suffix("s"))
                            .changed();
                        let mut value_update = None;
                        match &keyframe.value {
                            PropertyValue::Number(value) => {
                                let mut value = value.into_inner();
                                if ui
                                    .add(egui::DragValue::new(&mut value).speed(0.01))
                                    .changed()
                                {
                                    value_update = Some(PropertyValue::Number(OrderedFloat(value)));
                                }
                            }
                            PropertyValue::Vec2(value) => {
                                let (mut x, mut y) = (value.x.into_inner(), value.y.into_inner());
                                let changed = ui
                                    .add(egui::DragValue::new(&mut x).prefix("x "))
                                    .changed()
                                    | ui.add(egui::DragValue::new(&mut y).prefix("y ")).changed();
                                if changed {
                                    value_update = Some(property_vec2(x, y));
                                }
                            }
                            value => {
                                ui.label(format!("{value:?}"));
                            }
                        }
                        ui.label(format!("{:?}", keyframe.easing));
                        if time_changed || value_update.is_some() {
                            edits.push(Edit::UpdateKeyframe(
                                item.id,
                                name.clone(),
                                keyframe.id,
                                KeyframeUpdate {
                                    time: time_changed.then_some(time.max(0.0)),
                                    value: value_update,
                                    easing: None,
                                },
                            ));
                        }
                        if ui.small_button("Delete").clicked() {
                            edits.push(Edit::RemoveKeyframe(item.id, name.clone(), keyframe.id));
                        }
                    });
                }
            } else {
                ui.label(format!("Base: {:?}", property.value()));
            }
        });
    }
}

fn draw_numeric_curve(ui: &mut egui::Ui, keyframes: &[Keyframe], duration: f64) {
    let points: Vec<_> = keyframes
        .iter()
        .filter_map(|keyframe| match keyframe.value {
            PropertyValue::Number(value) => Some((keyframe.time.into_inner(), value.into_inner())),
            PropertyValue::Integer(value) => Some((keyframe.time.into_inner(), value as f64)),
            _ => None,
        })
        .collect();
    if points.is_empty() || duration <= 0.0 {
        return;
    }
    let min = points
        .iter()
        .map(|point| point.1)
        .fold(f64::INFINITY, f64::min);
    let max = points
        .iter()
        .map(|point| point.1)
        .fold(f64::NEG_INFINITY, f64::max);
    let span = (max - min).max(0.000_001);
    let (rect, _) = ui.allocate_exact_size(
        egui::vec2(ui.available_width(), 110.0),
        egui::Sense::hover(),
    );
    ui.painter()
        .rect_filled(rect, 3.0, ui.visuals().extreme_bg_color);
    let positions: Vec<_> = points
        .iter()
        .map(|(time, value)| {
            egui::pos2(
                egui::lerp(rect.x_range(), (*time / duration).clamp(0.0, 1.0) as f32),
                egui::lerp(rect.y_range(), (1.0 - (*value - min) / span) as f32),
            )
        })
        .collect();
    for segment in positions.windows(2) {
        ui.painter().line_segment(
            [segment[0], segment[1]],
            egui::Stroke::new(2.0, ui.visuals().selection.bg_fill),
        );
    }
    for point in positions {
        ui.painter()
            .circle_filled(point, 4.0, ui.visuals().selection.stroke.color);
    }
}

fn data_ui(
    ui: &mut egui::Ui,
    project: &AuthoringProject,
    open_timeline: TimelineId,
    edits: &mut Vec<Edit>,
) {
    ui.heading("Data and generated items");
    ui.label("Import a CSV or JSON table. Each stable row becomes an ordinary Timeline item.");
    let target_track = project
        .timelines
        .get(&open_timeline)
        .and_then(|timeline| timeline.track_order.first())
        .copied();
    if ui
        .add_enabled(
            target_track.is_some(),
            egui::Button::new("Import CSV / JSON"),
        )
        .clicked()
    {
        if let (Some(path), Some(track_id)) = (
            rfd::FileDialog::new()
                .add_filter("Table data", &["csv", "json"])
                .pick_file(),
            target_track,
        ) {
            edits.push(Edit::ImportData(path, track_id));
        }
    }
    ui.separator();
    for data_source in project.data_sources.values() {
        ui.group(|ui| {
            ui.horizontal(|ui| {
                ui.strong(&data_source.name);
                if ui.button("Refresh").clicked() {
                    edits.push(Edit::RefreshData(data_source.id));
                }
            });
            ui.label(format!(
                "{} rows  ·  stable key: {}",
                data_source.cached_rows.len(),
                data_source.stable_key_field
            ));
        });
    }
    ui.label(format!(
        "Generated items: {}",
        project.generated_items.len()
    ));
    let active = project
        .overrides
        .values()
        .filter(|authored_override| {
            matches!(
                authored_override.status,
                library::model::authoring::OverrideStatus::Active
            )
        })
        .count();
    let orphaned = project
        .overrides
        .values()
        .filter(|authored_override| {
            matches!(
                authored_override.status,
                library::model::authoring::OverrideStatus::Orphaned
            )
        })
        .count();
    let conflicts = project.overrides.len().saturating_sub(active + orphaned);
    ui.label(format!(
        "Manual corrections: {active} active · {orphaned} orphaned · {conflicts} conflicts"
    ));
    for authored_override in project.overrides.values() {
        match &authored_override.status {
            library::model::authoring::OverrideStatus::Orphaned => {
                ui.horizontal(|ui| {
                    ui.colored_label(
                        ui.visuals().warn_fg_color,
                        format!(
                            "Orphaned correction: {}",
                            authored_override.generated_item_id
                        ),
                    );
                    if ui.button("Discard correction").clicked() {
                        edits.push(Edit::DiscardOverride(authored_override.id));
                    }
                });
            }
            library::model::authoring::OverrideStatus::Conflict { reason } => {
                ui.horizontal(|ui| {
                    ui.colored_label(
                        ui.visuals().error_fg_color,
                        format!("Conflicting correction: {reason}"),
                    );
                    if ui.button("Use generated value").clicked() {
                        edits.push(Edit::DiscardOverride(authored_override.id));
                    }
                });
            }
            library::model::authoring::OverrideStatus::Active => {}
        }
    }
    ui.small("Canvas and Inspector edits are stored as overrides and survive data refresh.");
}

fn logic_ui(
    ui: &mut egui::Ui,
    project: &AuthoringProject,
    selected: Option<TimelineItemId>,
    edits: &mut Vec<Edit>,
) {
    ui.heading("Logic Module");
    ui.label("Only reusable ModuleDefinitions appear here. Timeline items are never expanded into nodes.");
    let selected_instances: Vec<_> = selected.into_iter().flat_map(|id| project.attachments.values().filter(move |attachment| matches!(attachment.owner, library::model::authoring::AttachmentOwner::Item { item_id } if item_id == id))).collect();
    for attachment in selected_instances {
        let instance = &project.module_instances[&attachment.module_instance_id];
        let definition = &project.module_definitions[&instance.definition_id];
        ui.collapsing(&definition.name, |ui| {
            ui.label(format!(
                "Role: {:?} · Version {}",
                definition.role, definition.version
            ));
            ui.label(format!("Internal nodes: {}", definition.graph.nodes.len()));
            ui.label(format!(
                "Published parameters: {}",
                definition.published_parameters.len()
            ));
            for node in definition.graph.nodes.values() {
                ui.group(|ui| {
                    let mut name = node.name.clone();
                    let mut enabled = node.enabled;
                    let mut bypassed = node.bypassed;
                    let name_changed = ui.text_edit_singleline(&mut name).changed();
                    let enabled_changed = ui.checkbox(&mut enabled, "Enabled").changed();
                    let bypass_changed = if node.supports_bypass() {
                        ui.checkbox(&mut bypassed, "Bypass").changed()
                    } else {
                        false
                    };
                    ui.small(format!("{:?}", node.content()));
                    if name_changed || enabled_changed || bypass_changed {
                        edits.push(Edit::ModuleNodeState(
                            definition.id,
                            node.id,
                            name,
                            enabled,
                            bypassed,
                        ));
                    }
                });
            }
        });
    }
}

fn diagnostics_ui(ui: &mut egui::Ui, project: &AuthoringProject, workspace: Workspace) {
    ui.heading("Derived runtime state");
    ui.label(format!("Workspace depth: {}", workspace.depth()));
    ui.label(format!(
        "Timelines {} · Items {} · Shared modules {} · Instances {}",
        project.timelines.len(),
        project.items.len(),
        project.module_definitions.len(),
        project.module_instances.len()
    ));
    ui.small("RenderPlan is compiled and cached; it is not editable or persisted.");
}

fn dock_for(workspace: Workspace) -> DockState<Tab> {
    let mut dock = DockState::new(vec![Tab::Preview]);
    let surface = dock.main_surface_mut();
    let [main, _] = surface.split_below(NodeIndex::root(), 0.68, vec![Tab::Timeline]);
    let main = if workspace.depth() >= 1 {
        surface.split_right(main, 0.78, vec![Tab::Inspector])[0]
    } else {
        main
    };
    surface.split_left(main, 0.20, vec![Tab::Assets]);
    if workspace.depth() >= 2 {
        let tab = if workspace == Workspace::Data {
            Tab::Data
        } else {
            Tab::Motion
        };
        surface.push_to_focused_leaf(tab);
    }
    if workspace.depth() >= 3 {
        surface.push_to_focused_leaf(Tab::Logic);
    }
    if workspace.depth() >= 4 {
        surface.push_to_focused_leaf(Tab::Diagnostics);
    }
    dock
}

fn property_vec2(x: f64, y: f64) -> PropertyValue {
    PropertyValue::Vec2(Vec2 {
        x: OrderedFloat(x),
        y: OrderedFloat(y),
    })
}

fn vec2_property_at(
    item: &library::model::authoring::TimelineItem,
    name: &str,
    time: f64,
    default: (f64, f64),
) -> (f64, f64) {
    match item
        .authored_properties
        .get(name)
        .and_then(|property| property.evaluate_at(time).ok())
        .as_ref()
    {
        Some(PropertyValue::Vec2(value)) => (value.x.into_inner(), value.y.into_inner()),
        _ => default,
    }
}

fn number_property_at(
    item: &library::model::authoring::TimelineItem,
    name: &str,
    time: f64,
    default: f64,
) -> f64 {
    match item
        .authored_properties
        .get(name)
        .and_then(|property| property.evaluate_at(time).ok())
        .as_ref()
    {
        Some(PropertyValue::Number(value)) => value.into_inner(),
        Some(PropertyValue::Integer(value)) => *value as f64,
        _ => default,
    }
}

fn asset_kind(path: &Path) -> AssetKind {
    match path
        .extension()
        .and_then(|extension| extension.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase()
        .as_str()
    {
        "png" | "jpg" | "jpeg" | "webp" | "gif" => AssetKind::Image,
        "wav" | "mp3" | "flac" | "ogg" | "m4a" => AssetKind::Audio,
        "mp4" | "mov" | "mkv" | "webm" | "avi" => AssetKind::Video,
        _ => AssetKind::Other,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn basic_workspaces_never_surface_logic() {
        assert!(dock_for(Workspace::Beginner)
            .find_tab(&Tab::Logic)
            .is_none());
        assert!(dock_for(Workspace::Edit).find_tab(&Tab::Logic).is_none());
        assert!(dock_for(Workspace::Logic).find_tab(&Tab::Logic).is_some());
    }

    #[test]
    fn timeline_snap_prefers_nearby_item_edges_then_frames() {
        assert_eq!(snap_time(1.94, 30.0, &[2.0]), 2.0);
        assert_eq!(snap_time(1.26, 10.0, &[]), 1.3);
    }

    #[test]
    fn continuous_property_updates_share_one_history_key() {
        let item = TimelineItemId::new();
        let first = Edit::Property(item, "position".to_string(), property_vec2(10.0, 20.0));
        let second = Edit::Property(item, "position".to_string(), property_vec2(30.0, 40.0));
        assert_eq!(first.history_key(), second.history_key());
    }
}
