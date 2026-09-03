use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant};

use eframe::egui;
use egui_dock::{DockArea, DockState, NodeIndex, Style, TabViewer};
use library::core::binding_runtime::{resolve_published_numeric_value, SignalRuntimeValues};
use library::core::event_runtime::EventRuntime;
use library::model::authoring::{
    AttachmentId, AuthoringProject, BindingOperator, BindingScope, ConstraintKind, DataSourceId,
    DurationPolicy, EventBinding, EventBindingId, EventSource, InstancePath, MaskId, MaskMode,
    MatteMode, MatteRef, ModuleConnectionId, ModuleDefinitionId, ModuleInstanceId, OverrideId,
    PublishedParameterId, SignalBinding, SignalBindingId, SignalMapping, SignalSource, SourceRef,
    TimelineId, TimelineInterval, TimelineItemId, TimelineTrackId, TriggerPolicy,
};
use library::model::frame::color::Color;
use library::model::node::GeneratorContent;
use library::model::project::asset::{Asset, AssetKind};
use library::model::project::property::{Keyframe, KeyframeId, KeyframeUpdate, Property};
use library::model::project::property::{PropertyValue, Vec2};
use library::rendering::renderer::RenderOutput;
use library::{AuthoringRenderService, RenderDestination, SkiaRenderer, TimelineEditorService};
use ordered_float::OrderedFloat;
use pan_zoom_ui::{
    apply_navigation, navigation_delta, paint_canvas, AxisMask, CanvasState, CanvasTheme,
    GridConfig, InputPolicy, NavigationConfig, NavigationInput, ZoomPolicy,
};

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
    ImportAsset,
    PlaceAssetAt(uuid::Uuid, TimelineTrackId, f64),
    OpenSubtitleImport,
    AddText,
    AddSolid,
    AddReusableTitle,
    AddComposition,
    TogglePlayback,
    StopPlayback,
    Select(Option<TimelineItemId>),
    OpenTimeline(TimelineId, InstancePath),
    Rename(TimelineItemId, String),
    SetText(TimelineItemId, String),
    Move(TimelineItemId, TimelineTrackId, f64, i64),
    Trim(TimelineItemId, TimelineInterval),
    Property(TimelineItemId, String, PropertyValue),
    Split(TimelineItemId),
    AddEffect(TimelineItemId, String),
    RemoveAttachment(AttachmentId),
    MoveAttachment(AttachmentId, i32),
    ModuleParameter(ModuleInstanceId, PublishedParameterId, PropertyValue),
    ModuleNodeState(ModuleDefinitionId, uuid::Uuid, String, bool, bool),
    #[cfg(feature = "logic-editor")]
    ModuleNodePresentation(ModuleDefinitionId, uuid::Uuid, [f32; 2], [f32; 2], bool),
    #[cfg(feature = "logic-editor")]
    ConnectModulePorts(
        ModuleDefinitionId,
        library::model::authoring::ModulePortAddress,
        library::model::authoring::ModulePortAddress,
    ),
    AddModuleEffect(ModuleDefinitionId, String),
    RemoveModuleNode(ModuleDefinitionId, uuid::Uuid),
    SetModuleOutput(ModuleDefinitionId, uuid::Uuid),
    DisconnectModuleConnection(ModuleDefinitionId, ModuleConnectionId),
    AddSignalBinding(SignalBinding),
    AddEventBinding(EventBinding),
    SetParent(TimelineItemId, Option<TimelineItemId>),
    DurationPolicy(TimelineItemId, DurationPolicy),
    Delete(TimelineItemId, bool),
    Fade(TimelineItemId, f64),
    UpsertKeyframe(TimelineItemId, String, f64, PropertyValue),
    UpdateKeyframe(TimelineItemId, String, KeyframeId, KeyframeUpdate),
    RemoveKeyframe(TimelineItemId, String, KeyframeId),
    ImportData(std::path::PathBuf, TimelineTrackId),
    RefreshData(DataSourceId),
    DiscardOverride(OverrideId),
    ImportSubtitles(std::path::PathBuf, TimelineTrackId),
    CrossDissolve(TimelineItemId, f64),
    AddConstraint(TimelineItemId, TimelineItemId, ConstraintKind),
    AddRectangleMask(TimelineItemId),
    UpdateMask(TimelineItemId, MaskId, MaskMode, bool, f64, f64),
    SetMatte(TimelineItemId, Option<MatteRef>),
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum HistoryKey {
    Item(TimelineItemId, &'static str),
    Property(TimelineItemId, String),
    ModuleParameter(ModuleInstanceId, PublishedParameterId),
    ModuleNode(ModuleDefinitionId, uuid::Uuid),
    Attachment(AttachmentId),
    Binding,
    Data,
}

impl Edit {
    fn history_key(&self) -> Option<HistoryKey> {
        match self {
            Self::ImportAsset
            | Self::OpenSubtitleImport
            | Self::AddText
            | Self::AddSolid
            | Self::AddReusableTitle
            | Self::AddComposition
            | Self::TogglePlayback
            | Self::StopPlayback => None,
            Self::PlaceAssetAt(..) => Some(HistoryKey::Data),
            Self::Select(_) | Self::OpenTimeline(_, _) => None,
            Self::Rename(item, _) => Some(HistoryKey::Item(*item, "rename")),
            Self::SetText(item, _) => Some(HistoryKey::Item(*item, "text")),
            Self::Move(item, ..) => Some(HistoryKey::Item(*item, "move")),
            Self::Trim(item, _) => Some(HistoryKey::Item(*item, "trim")),
            Self::Property(item, key, _) => Some(HistoryKey::Property(*item, key.clone())),
            Self::Split(item) => Some(HistoryKey::Item(*item, "split")),
            Self::AddEffect(item, _) => Some(HistoryKey::Item(*item, "effect")),
            Self::RemoveAttachment(attachment) | Self::MoveAttachment(attachment, _) => {
                Some(HistoryKey::Attachment(*attachment))
            }
            Self::ModuleParameter(instance, parameter, _) => {
                Some(HistoryKey::ModuleParameter(*instance, *parameter))
            }
            Self::ModuleNodeState(definition, node, ..) => {
                Some(HistoryKey::ModuleNode(*definition, *node))
            }
            #[cfg(feature = "logic-editor")]
            Self::ModuleNodePresentation(definition, node, ..) => {
                Some(HistoryKey::ModuleNode(*definition, *node))
            }
            Self::AddModuleEffect(definition, _) => {
                Some(HistoryKey::ModuleNode(*definition, uuid::Uuid::nil()))
            }
            Self::RemoveModuleNode(definition, node) | Self::SetModuleOutput(definition, node) => {
                Some(HistoryKey::ModuleNode(*definition, *node))
            }
            Self::DisconnectModuleConnection(definition, connection) => {
                Some(HistoryKey::ModuleNode(*definition, connection.as_uuid()))
            }
            #[cfg(feature = "logic-editor")]
            Self::ConnectModulePorts(definition, from, _) => {
                Some(HistoryKey::ModuleNode(*definition, from.node_id))
            }
            Self::AddSignalBinding(_) | Self::AddEventBinding(_) => Some(HistoryKey::Binding),
            Self::SetParent(item, _) => Some(HistoryKey::Item(*item, "parent")),
            Self::DurationPolicy(item, _) => Some(HistoryKey::Item(*item, "duration-policy")),
            Self::Delete(item, _) => Some(HistoryKey::Item(*item, "delete")),
            Self::Fade(item, _) => Some(HistoryKey::Property(*item, "opacity".to_string())),
            Self::UpsertKeyframe(item, key, ..)
            | Self::UpdateKeyframe(item, key, ..)
            | Self::RemoveKeyframe(item, key, _) => Some(HistoryKey::Property(*item, key.clone())),
            Self::ImportData(..)
            | Self::RefreshData(_)
            | Self::DiscardOverride(_)
            | Self::ImportSubtitles(..) => Some(HistoryKey::Data),
            Self::CrossDissolve(item, _) => Some(HistoryKey::Item(*item, "transition")),
            Self::AddConstraint(item, ..) => Some(HistoryKey::Item(*item, "constraint")),
            Self::AddRectangleMask(item) => Some(HistoryKey::Item(*item, "mask")),
            Self::UpdateMask(item, ..) => Some(HistoryKey::Item(*item, "mask")),
            Self::SetMatte(item, _) => Some(HistoryKey::Item(*item, "matte")),
        }
    }
}

pub struct TimelineApp {
    editor: TimelineEditorService,
    plugins: Arc<library::plugin::PluginManager>,
    renderer: AuthoringRenderService<SkiaRenderer>,
    dock: DockState<Tab>,
    workspace: Workspace,
    open_timeline: TimelineId,
    instance_path: InstancePath,
    selected_item: Option<TimelineItemId>,
    current_time: f64,
    is_playing: bool,
    signal_runtime: SignalRuntimeValues,
    live_signal_sources: HashMap<SignalSource, f64>,
    event_runtime: EventRuntime,
    runtime_started_at: Instant,
    #[cfg(feature = "logic-editor")]
    logic_graph: crate::logic_graph_ui::LogicGraphState,
    last_playback_tick: Instant,
    preview: Option<egui::TextureHandle>,
    preview_canvas: CanvasState,
    preview_view_initialized: bool,
    preview_grid: bool,
    expanded_layers: HashSet<TimelineTrackId>,
    timeline_pixels_per_second: f32,
    preview_key: Option<(
        library::model::authoring::ProjectRevision,
        TimelineId,
        u64,
        u64,
        u64,
    )>,
    undo: Vec<AuthoringProject>,
    redo: Vec<AuthoringProject>,
    last_history_group: Option<(HistoryKey, Instant)>,
    status: String,
    qa: Option<crate::qa::QaBridge>,
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
        let mut app = Self {
            editor,
            plugins: plugins.clone(),
            renderer: AuthoringRenderService::new(skia, plugins, cache),
            dock: dock_for(Workspace::Edit),
            workspace: Workspace::Edit,
            open_timeline,
            instance_path,
            selected_item: None,
            current_time: 0.0,
            is_playing: false,
            signal_runtime: SignalRuntimeValues::default(),
            live_signal_sources: HashMap::new(),
            event_runtime: EventRuntime::default(),
            runtime_started_at: Instant::now(),
            #[cfg(feature = "logic-editor")]
            logic_graph: crate::logic_graph_ui::LogicGraphState::default(),
            last_playback_tick: Instant::now(),
            preview: None,
            preview_canvas: CanvasState::uniform(egui::Vec2::ZERO, 1.0),
            preview_view_initialized: false,
            preview_grid: true,
            expanded_layers: HashSet::new(),
            timeline_pixels_per_second: 80.0,
            preview_key: None,
            undo: Vec::new(),
            redo: Vec::new(),
            last_history_group: None,
            status: "Timeline-first project ready".to_string(),
            qa: crate::qa::QaBridge::from_env(&cc.egui_ctx),
        };
        if let Ok(path) = std::env::var("RUVIE_QA_ASSET") {
            let path = std::path::PathBuf::from(path);
            match asset_from_path(&path, &app.plugins).and_then(|asset| app.editor.add_asset(asset))
            {
                Ok(_) => app.status = "QA fixture imported into Assets".to_string(),
                Err(error) => app.status = format!("QA fixture failed: {error}"),
            }
        }
        Ok(app)
    }

    fn publish_qa_state(&self) {
        let Some(qa) = &self.qa else {
            return;
        };
        let Ok(project) = self.editor.snapshot() else {
            return;
        };
        let layers = project
            .items
            .values()
            .filter(|item| {
                project
                    .tracks
                    .get(&item.track_id)
                    .is_some_and(|track| track.timeline_id == self.open_timeline)
            })
            .map(|item| {
                serde_json::json!({
                    "id": item.id.to_string(),
                    "name": item.name,
                    "start": item.interval.start.into_inner(),
                    "duration": item.interval.duration.into_inner(),
                    "parent": item.parent.map(|id| id.to_string()),
                    "layer_expanded": self.expanded_layers.contains(&item.track_id),
                })
            })
            .collect::<Vec<_>>();
        let assets = project
            .assets
            .iter()
            .map(|asset| serde_json::json!({"id": asset.id, "name": asset.name, "kind": format!("{:?}", asset.kind)}))
            .collect::<Vec<_>>();
        qa.publish_state(serde_json::json!({
            "frame": {
                "timeline_id": self.open_timeline.to_string(),
                "current_time": self.current_time,
                "is_playing": self.is_playing,
                "selected_item": self.selected_item.map(|id| id.to_string()),
            },
            "assets": assets,
            "layers": layers,
            "preview": {
                "has_image": self.preview.is_some(),
                "pan": [self.preview_canvas.pan.x, self.preview_canvas.pan.y],
                "zoom": self.preview_canvas.zoom.x,
                "grid": self.preview_grid,
            },
            "runtime": {
                "signal_binding_count": project.signal_bindings.len(),
                "signal_generation": self.signal_runtime.generation(),
                "event_generation": self.event_runtime.generation(),
                "live_signal_source_count": self.live_signal_sources.len(),
            },
            "status": self.status,
        }));
    }

    fn new_project(&mut self) {
        match AuthoringProject::new("Untitled", 1920, 1080, 30.0, 60.0)
            .map_err(library::LibraryError::Validation)
            .and_then(|project| self.editor.replace_project(project))
        {
            Ok(()) => {
                self.status = match self.reset_project_ui() {
                    Ok(()) => "New project".to_string(),
                    Err(error) => error.to_string(),
                };
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
                self.status = match self.reset_project_ui() {
                    Ok(()) => format!("Opened {}", path.display()),
                    Err(error) => error.to_string(),
                };
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

    fn reset_project_ui(&mut self) -> Result<(), library::LibraryError> {
        self.open_timeline = self.editor.snapshot()?.root_timeline_id;
        self.instance_path = InstancePath::root(self.open_timeline);
        self.selected_item = None;
        self.current_time = 0.0;
        self.is_playing = false;
        self.signal_runtime = SignalRuntimeValues::default();
        self.live_signal_sources.clear();
        self.event_runtime.clear();
        self.runtime_started_at = Instant::now();
        self.preview_canvas = CanvasState::uniform(egui::Vec2::ZERO, 1.0);
        self.preview_view_initialized = false;
        self.expanded_layers.clear();
        self.undo.clear();
        self.redo.clear();
        self.last_history_group = None;
        self.invalidate_preview();
        Ok(())
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
        let runtime_events = match self.event_runtime.clone().snapshot_at(self.current_time) {
            Ok(snapshot) => snapshot,
            Err(error) => {
                self.status = error;
                return;
            }
        };
        let result = self.editor.compiled_project().and_then(|compiled| {
            let timeline = &compiled.project.timelines[&self.open_timeline];
            self.renderer.renderer_mut().resize_render_target(
                timeline.width as u32,
                timeline.height as u32,
                timeline.background_color.clone(),
            )?;
            let frame = self.editor.evaluate_compiled_frame_with_runtime(
                &compiled,
                self.open_timeline,
                &self.instance_path,
                self.current_time,
                1.0,
                None,
                &self.signal_runtime,
                &runtime_events,
            )?;
            let exported = self
                .renderer
                .render_export_frame(compiled.project.as_ref(), &frame)?;
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
        let compiled = match self.editor.compiled_project() {
            Ok(compiled) => compiled,
            Err(error) => {
                self.status = error.to_string();
                return;
            }
        };
        let project = &compiled.project;
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
        let mut settings =
            match library::plugin::ExportSettings::from_project(project.as_ref(), &timeline) {
                Ok(settings) => settings,
                Err(error) => {
                    self.status = error.to_string();
                    return;
                }
            };
        settings.container = "mp4".to_string();
        settings.codec = "libx264".to_string();
        settings.pixel_format = "yuv420p".to_string();
        let runtime_audio = match render_timeline_audio(project.as_ref(), self.open_timeline) {
            Ok(audio) => audio,
            Err(error) => {
                self.status = error;
                return;
            }
        };
        if let Some(audio_path) = runtime_audio.as_ref() {
            if let Err(error) = settings.bind_runtime_audio_source(
                audio_path.to_string_lossy().into_owned(),
                2,
                48_000,
            ) {
                self.status = error.to_string();
                return;
            }
        }
        let frame_count = match settings.frame_count_for_duration(timeline.duration.into_inner()) {
            Ok(count) => count,
            Err(error) => {
                self.status = error.to_string();
                return;
            }
        };
        if let Err(error) = self.renderer.renderer_mut().resize_render_target(
            timeline.width as u32,
            timeline.height as u32,
            timeline.background_color,
        ) {
            self.status = error.to_string();
            return;
        }
        let result = (|| {
            let mut event_runtime = self.event_runtime.clone();
            for frame_index in 0..frame_count {
                let time = settings.frame_time(frame_index)?;
                let runtime_events = event_runtime
                    .snapshot_at(time)
                    .map_err(library::LibraryError::Runtime)?;
                let frame = self.editor.evaluate_compiled_frame_with_runtime(
                    &compiled,
                    self.open_timeline,
                    &self.instance_path,
                    time,
                    1.0,
                    None,
                    &self.signal_runtime,
                    &runtime_events,
                )?;
                let frame = self
                    .renderer
                    .render_export_frame(compiled.project.as_ref(), &frame)?;
                self.plugins
                    .export_frame("ffmpeg_export", &output, &frame, &settings)?;
            }
            self.plugins
                .finish_export("ffmpeg_export", &output, &settings)
        })();
        if result.is_err() {
            drop(
                self.plugins
                    .finish_export("ffmpeg_export", &output, &settings),
            );
        }
        if let Some(audio_path) = runtime_audio {
            drop(std::fs::remove_file(audio_path));
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
        let result =
            asset_from_path(&path, &self.plugins).and_then(|asset| self.editor.add_asset(asset));
        match result {
            Ok(_) => {
                if let Some(before) = before {
                    self.record(before);
                }
                self.status = "Asset imported; add it from the Assets panel".to_string();
            }
            Err(error) => {
                if let Some(before) = before {
                    drop(self.editor.replace_project(before));
                }
                self.status = error.to_string();
            }
        }
    }

    fn place_asset(&mut self, asset_id: uuid::Uuid, track: TimelineTrackId, start: f64) {
        let before = self
            .editor
            .snapshot()
            .ok()
            .map(|project| project.as_ref().clone());
        let result = self.editor.snapshot().and_then(|project| {
            let asset = project
                .assets
                .iter()
                .find(|asset| asset.id == asset_id)
                .ok_or_else(|| library::LibraryError::Validation("Asset is missing".to_string()))?;
            let name = asset.name.clone();
            let duration = asset.duration.unwrap_or(5.0).max(1.0 / 30.0);
            drop(project);
            self.editor.place_asset(
                track,
                asset_id,
                name,
                TimelineInterval::new(start, duration)
                    .map_err(library::LibraryError::Validation)?,
                0,
            )
        });
        self.finish_add(result, before, "Asset added to Timeline");
    }

    fn import_subtitles(&mut self) {
        let target_track = self.editor.snapshot().ok().and_then(|project| {
            project
                .timelines
                .get(&self.open_timeline)
                .and_then(|timeline| timeline.track_order.first())
                .copied()
        });
        let path = rfd::FileDialog::new()
            .add_filter("SubRip subtitles", &["srt"])
            .pick_file();
        if let (Some(path), Some(track_id)) = (path, target_track) {
            self.apply(Edit::ImportSubtitles(path, track_id));
        }
    }

    fn add_text(&mut self) {
        let before = self
            .editor
            .snapshot()
            .ok()
            .map(|project| project.as_ref().clone());
        let result = self.editor.snapshot().and_then(|project| {
            let track = last_track(project.as_ref(), self.open_timeline)?;
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
            let track = last_track(project.as_ref(), self.open_timeline)?;
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

    fn add_reusable_title(&mut self) {
        let before = self
            .editor
            .snapshot()
            .ok()
            .map(|project| project.as_ref().clone());
        let result = self.editor.snapshot().and_then(|project| {
            let track = last_track(project.as_ref(), self.open_timeline)?;
            drop(project);
            self.editor.add_generator_module(
                track,
                "Reusable Title".to_string(),
                GeneratorContent::Text,
                TimelineInterval::new(self.current_time, 5.0)
                    .map_err(library::LibraryError::Validation)?,
                1,
                self.plugins.as_ref(),
            )
        });
        self.finish_add(result, before, "Reusable title added");
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
                let track = match last_track(project.as_ref(), self.open_timeline) {
                    Ok(track) => track,
                    Err(error) => {
                        self.status = error.to_string();
                        return;
                    }
                };
                drop(project);
                let interval = match TimelineInterval::new(self.current_time, 5.0) {
                    Ok(interval) => interval,
                    Err(error) => {
                        self.status = error;
                        return;
                    }
                };
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
                            drop(self.editor.replace_project(before));
                        }
                        self.status = error.to_string();
                    }
                }
            }
            Err(error) => {
                if let Some(before) = before {
                    drop(self.editor.replace_project(before));
                }
                self.status = error.to_string();
            }
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
                    drop(self.editor.replace_project(before));
                }
                self.status = error.to_string();
            }
        }
    }

    fn apply(&mut self, edit: Edit) {
        let edit = match edit {
            Edit::ImportAsset => {
                self.import_asset();
                return;
            }
            Edit::PlaceAssetAt(asset_id, track, start) => {
                self.place_asset(asset_id, track, start);
                return;
            }
            Edit::OpenSubtitleImport => {
                self.import_subtitles();
                return;
            }
            Edit::AddText => {
                self.add_text();
                return;
            }
            Edit::AddSolid => {
                self.add_solid();
                return;
            }
            Edit::AddReusableTitle => {
                self.add_reusable_title();
                return;
            }
            Edit::AddComposition => {
                self.add_nested_timeline();
                return;
            }
            Edit::TogglePlayback => {
                self.toggle_playback();
                return;
            }
            Edit::StopPlayback => {
                self.stop_playback();
                return;
            }
            edit => edit,
        };
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
                self.is_playing = false;
                self.invalidate_preview();
                return;
            }
            Edit::Rename(id, value) => self.editor.rename_item(id, value).map(|_| ()),
            Edit::SetText(id, value) => self.editor.set_text(id, value).map(|_| ()),
            Edit::Move(id, track, start, layer) => {
                self.editor.move_item(id, track, start, layer).map(|_| ())
            }
            Edit::Trim(id, interval) => self.editor.trim_item(id, interval).map(|_| ()),
            Edit::Property(id, key, value) => self.authored_item_time(id).and_then(|time| {
                self.editor
                    .update_item_property_value(id, key, time, value)
                    .map(|_| ())
            }),
            Edit::Split(id) => self.editor.split_item(id, self.current_time).map(|_| ()),
            Edit::AddEffect(id, effect_type) => self
                .editor
                .attach_effect(id, &effect_type, self.plugins.as_ref())
                .map(|_| ()),
            Edit::RemoveAttachment(attachment_id) => {
                self.editor.remove_attachment(attachment_id).map(|_| ())
            }
            Edit::MoveAttachment(attachment_id, direction) => self
                .editor
                .move_attachment(attachment_id, direction)
                .map(|_| ()),
            Edit::ModuleParameter(instance, parameter, value) => self
                .editor
                .set_module_parameter(instance, parameter, value)
                .map(|_| ()),
            Edit::ModuleNodeState(definition, node, name, enabled, bypassed) => self
                .editor
                .set_module_node_state(definition, node, name, enabled, bypassed)
                .map(|_| ()),
            #[cfg(feature = "logic-editor")]
            Edit::ModuleNodePresentation(definition, node, position, size, collapsed) => self
                .editor
                .set_module_node_presentation(definition, node, position, size, collapsed)
                .map(|_| ()),
            Edit::AddModuleEffect(definition, effect_type) => self
                .editor
                .add_effect_node_to_module(definition, &effect_type, self.plugins.as_ref())
                .map(|_| ()),
            Edit::RemoveModuleNode(definition, node) => {
                self.editor.remove_module_node(definition, node).map(|_| ())
            }
            Edit::SetModuleOutput(definition, node) => {
                self.editor.set_module_output(definition, node).map(|_| ())
            }
            #[cfg(feature = "logic-editor")]
            Edit::ConnectModulePorts(definition, from, to) => self
                .editor
                .connect_module_ports(definition, from, to)
                .map(|_| ()),
            Edit::DisconnectModuleConnection(definition, connection) => self
                .editor
                .disconnect_module_connection(definition, connection)
                .map(|_| ()),
            Edit::AddSignalBinding(binding) => self.editor.add_signal_binding(binding).map(|_| ()),
            Edit::AddEventBinding(binding) => self.editor.add_event_binding(binding).map(|_| ()),
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
            Edit::UpsertKeyframe(item, key, time, value) => self
                .editor
                .upsert_item_keyframe(item, key, time, value, None)
                .map(|_| ()),
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
            Edit::ImportSubtitles(path, track_id) => {
                self.editor.import_srt(&path, track_id).map(|_| ())
            }
            Edit::CrossDissolve(item_id, duration) => self
                .editor
                .add_cross_dissolve(item_id, duration)
                .map(|_| ()),
            Edit::AddConstraint(item_id, target_id, kind) => self
                .editor
                .add_constraint(item_id, target_id, kind)
                .map(|_| ()),
            Edit::AddRectangleMask(item_id) => self.editor.add_rectangle_mask(item_id).map(|_| ()),
            Edit::UpdateMask(item_id, mask_id, mode, inverted, feather, opacity) => self
                .editor
                .update_mask(
                    item_id,
                    mask_id,
                    self.current_time,
                    mode,
                    inverted,
                    feather,
                    opacity,
                )
                .map(|_| ()),
            Edit::SetMatte(item_id, matte) => self.editor.set_matte(item_id, matte).map(|_| ()),
            Edit::ImportAsset
            | Edit::PlaceAssetAt(..)
            | Edit::OpenSubtitleImport
            | Edit::AddText
            | Edit::AddSolid
            | Edit::AddReusableTitle
            | Edit::AddComposition
            | Edit::TogglePlayback
            | Edit::StopPlayback => Ok(()),
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

    fn authored_item_time(&self, item_id: TimelineItemId) -> Result<f64, library::LibraryError> {
        let project = self.editor.snapshot()?;
        let item = project.items.get(&item_id).ok_or_else(|| {
            library::LibraryError::Validation(format!("Timeline item {item_id} is missing"))
        })?;
        Ok(library::core::timeline_runtime::editable_item_local_time(
            item.interval,
            self.current_time,
        ))
    }

    fn invalidate_preview(&mut self) {
        self.preview_key = None;
    }

    fn toggle_playback(&mut self) {
        if !self.is_playing {
            let duration = self
                .editor
                .snapshot()
                .ok()
                .and_then(|project| project.timelines.get(&self.open_timeline).cloned())
                .map(|timeline| timeline.duration.into_inner())
                .unwrap_or(0.0);
            if self.current_time >= duration {
                self.current_time = 0.0;
            }
        }
        self.is_playing = !self.is_playing;
        self.last_playback_tick = Instant::now();
    }

    fn stop_playback(&mut self) {
        self.is_playing = false;
        self.current_time = 0.0;
        self.event_runtime.clear();
        self.last_playback_tick = Instant::now();
        self.invalidate_preview();
    }

    fn advance_playback(&mut self, ctx: &egui::Context) {
        let now = Instant::now();
        let elapsed = now.duration_since(self.last_playback_tick).as_secs_f64();
        self.last_playback_tick = now;
        if !self.is_playing {
            return;
        }
        let duration = self
            .editor
            .snapshot()
            .ok()
            .and_then(|project| project.timelines.get(&self.open_timeline).cloned())
            .map(|timeline| timeline.duration.into_inner())
            .unwrap_or(0.0);
        self.current_time = (self.current_time + elapsed.min(0.25)).min(duration);
        if self.current_time >= duration {
            self.is_playing = false;
        }
        self.invalidate_preview();
        ctx.request_repaint();
    }

    fn refresh_preview(&mut self, ctx: &egui::Context) {
        let Ok(compiled) = self.editor.compiled_project() else {
            return;
        };
        let project = &compiled.project;
        let Some(timeline) = project.timelines.get(&self.open_timeline) else {
            return;
        };
        let frame_number = (self.current_time * timeline.fps.into_inner()).floor() as u64;
        let runtime_events = match self.event_runtime.snapshot_at(self.current_time) {
            Ok(snapshot) => snapshot,
            Err(error) => {
                self.status = error;
                return;
            }
        };
        let key = (
            compiled.revision,
            self.open_timeline,
            frame_number,
            self.signal_runtime.generation(),
            runtime_events.generation(),
        );
        if self.preview_key == Some(key) {
            return;
        }
        let scale = (720.0 / timeline.width as f64).min(1.0);
        let width = (timeline.width as f64 * scale).round().max(1.0) as u32;
        let height = (timeline.height as f64 * scale).round().max(1.0) as u32;
        if let Err(error) = self.renderer.renderer_mut().resize_render_target(
            width,
            height,
            timeline.background_color.clone(),
        ) {
            self.status = error.to_string();
            return;
        }
        let rendered = self
            .editor
            .evaluate_compiled_frame_with_runtime(
                &compiled,
                self.open_timeline,
                &self.instance_path,
                self.current_time,
                scale,
                None,
                &self.signal_runtime,
                &runtime_events,
            )
            .and_then(|frame| {
                self.renderer.render_frame(
                    compiled.project.as_ref(),
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

fn render_timeline_audio(
    project: &AuthoringProject,
    timeline_id: TimelineId,
) -> Result<Option<std::path::PathBuf>, String> {
    let timeline = project
        .timelines
        .get(&timeline_id)
        .ok_or_else(|| format!("Missing Timeline {timeline_id}"))?;
    let mut clips = project
        .items
        .values()
        .filter_map(|item| {
            let track = project.tracks.get(&item.track_id)?;
            if track.timeline_id != timeline_id {
                return None;
            }
            let SourceRef::Asset { asset_id, time_map } = &item.source else {
                return None;
            };
            let asset = project.assets.iter().find(|asset| asset.id == *asset_id)?;
            matches!(asset.kind, AssetKind::Audio).then_some((item, asset, time_map))
        })
        .collect::<Vec<_>>();
    clips.sort_by_key(|(item, _, _)| (item.interval.start, item.layer));
    if clips.is_empty() {
        return Ok(None);
    }
    let output = std::env::temp_dir().join(format!("ruvie-audio-{}.f32le", uuid::Uuid::new_v4()));
    let mut command = std::process::Command::new("ffmpeg");
    command.arg("-hide_banner").arg("-loglevel").arg("error");
    for (_, asset, _) in &clips {
        command.arg("-i").arg(&asset.path);
    }
    let mut filters = Vec::new();
    let mut labels = String::new();
    for (index, (item, _, time_map)) in clips.iter().enumerate() {
        let rate = time_map.playback_rate.into_inner();
        if !rate.is_finite() || rate <= 0.0 {
            return Err(format!(
                "Audio item {} has an invalid playback rate",
                item.id
            ));
        }
        let volume = item
            .authored_properties
            .get("volume")
            .and_then(Property::get_static_value)
            .and_then(|value| match value {
                PropertyValue::Number(value) => Some(value.into_inner()),
                PropertyValue::Integer(value) => Some(*value as f64),
                _ => None,
            })
            .unwrap_or(1.0);
        let source_duration = item.interval.duration.into_inner() * rate;
        filters.push(format!(
            "[{index}:a]atrim=start={}:duration={},asetpts=PTS-STARTPTS,{},volume={},adelay={}:all=1[a{index}]",
            time_map.source_start,
            source_duration,
            atempo_filter(rate),
            volume,
            (item.interval.start.into_inner() * 1000.0).round() as u64
        ));
        labels.push_str(&format!("[a{index}]"));
    }
    filters.push(format!(
        "{labels}amix=inputs={}:normalize=0,atrim=duration={}[mix]",
        clips.len(),
        timeline.duration
    ));
    let status = command
        .arg("-filter_complex")
        .arg(filters.join(";"))
        .arg("-map")
        .arg("[mix]")
        .arg("-f")
        .arg("f32le")
        .arg("-acodec")
        .arg("pcm_f32le")
        .arg("-ar")
        .arg("48000")
        .arg("-ac")
        .arg("2")
        .arg("-y")
        .arg(&output)
        .status()
        .map_err(|error| format!("Cannot start FFmpeg audio mix: {error}"))?;
    if !status.success() {
        drop(std::fs::remove_file(&output));
        return Err(format!("FFmpeg audio mix failed with {status}"));
    }
    Ok(Some(output))
}

fn atempo_filter(mut rate: f64) -> String {
    let mut stages = Vec::new();
    while rate < 0.5 {
        stages.push("atempo=0.5".to_string());
        rate /= 0.5;
    }
    while rate > 100.0 {
        stages.push("atempo=100".to_string());
        rate /= 100.0;
    }
    stages.push(format!("atempo={rate}"));
    stages.join(",")
}

impl eframe::App for TimelineApp {
    fn raw_input_hook(&mut self, ctx: &egui::Context, raw_input: &mut egui::RawInput) {
        if let Some(qa) = &self.qa {
            qa.inject_for_frame(raw_input, ctx);
        }
    }

    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        if let Some(qa) = &self.qa {
            qa.begin_frame();
            qa.issue_capture(ctx);
        }
        if !ctx.wants_keyboard_input() && ctx.input(|input| input.key_pressed(egui::Key::Space)) {
            self.toggle_playback();
        }
        self.advance_playback(ctx);
        egui::TopBottomPanel::top("main-toolbar").show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.menu_button("File", |ui| {
                    if ui.button("New Project").clicked() {
                        self.new_project();
                        ui.close();
                    }
                    if ui.button("Open…").clicked() {
                        self.open_project();
                        ui.close();
                    }
                    if ui.button("Save").clicked() {
                        self.save(false);
                        ui.close();
                    }
                    if ui.button("Save As…").clicked() {
                        self.save(true);
                        ui.close();
                    }
                    ui.separator();
                    if ui.button("Export Frame…").clicked() {
                        self.export_frame();
                        ui.close();
                    }
                    if ui.button("Export Video…").clicked() {
                        self.export_video();
                        ui.close();
                    }
                });
                ui.menu_button("Edit", |ui| {
                    if ui
                        .add_enabled(!self.undo.is_empty(), egui::Button::new("Undo"))
                        .clicked()
                    {
                        self.undo();
                        ui.close();
                    }
                    if ui
                        .add_enabled(!self.redo.is_empty(), egui::Button::new("Redo"))
                        .clicked()
                    {
                        self.redo();
                        ui.close();
                    }
                });
                ui.separator();
                ui.strong("RuViE");
                ui.separator();
                for workspace in Workspace::ALL {
                    let workspace_button =
                        ui.selectable_label(self.workspace == workspace, workspace.label());
                    crate::qa::register_component(
                        format!("workspace.{:?}", workspace).to_lowercase(),
                        "workspace",
                        workspace_button.rect,
                        serde_json::json!({"workspace": workspace.label()}),
                    );
                    if workspace_button.clicked() {
                        self.workspace = workspace;
                        self.dock = dock_for(workspace);
                    }
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.small("Timeline-first");
                });
            });
        });
        self.refresh_preview(ctx);
        let Ok(compiled) = self.editor.compiled_project() else {
            return;
        };
        let project = &compiled.project;
        let mut edits = Vec::new();
        let signal_sample_time = self.runtime_started_at.elapsed().as_secs_f64();
        let mut viewer = Viewer {
            project: project.as_ref(),
            plugins: self.plugins.as_ref(),
            render_plan: compiled.render_plan.as_ref(),
            open_timeline: self.open_timeline,
            instance_path: &self.instance_path,
            selected_item: self.selected_item,
            current_time: &mut self.current_time,
            is_playing: self.is_playing,
            preview: self.preview.as_ref(),
            preview_canvas: &mut self.preview_canvas,
            preview_view_initialized: &mut self.preview_view_initialized,
            preview_grid: &mut self.preview_grid,
            expanded_layers: &mut self.expanded_layers,
            timeline_pixels_per_second: &mut self.timeline_pixels_per_second,
            workspace: self.workspace,
            signal_runtime: &mut self.signal_runtime,
            live_signal_sources: &mut self.live_signal_sources,
            signal_sample_time,
            event_runtime: &mut self.event_runtime,
            status: &mut self.status,
            #[cfg(feature = "logic-editor")]
            logic_graph: &mut self.logic_graph,
            edits: &mut edits,
        };
        DockArea::new(&mut self.dock)
            .style(Style::from_egui(ctx.style().as_ref()))
            .show(ctx, &mut viewer);
        drop(compiled);
        let had_edits = !edits.is_empty();
        for edit in edits {
            self.apply(edit);
        }
        if had_edits {
            ctx.request_repaint();
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
        self.publish_qa_state();
    }
}

struct Viewer<'a> {
    project: &'a AuthoringProject,
    plugins: &'a library::plugin::PluginManager,
    render_plan: &'a library::core::render_plan::RenderPlan,
    open_timeline: TimelineId,
    instance_path: &'a InstancePath,
    selected_item: Option<TimelineItemId>,
    current_time: &'a mut f64,
    is_playing: bool,
    preview: Option<&'a egui::TextureHandle>,
    preview_canvas: &'a mut CanvasState,
    preview_view_initialized: &'a mut bool,
    preview_grid: &'a mut bool,
    expanded_layers: &'a mut HashSet<TimelineTrackId>,
    timeline_pixels_per_second: &'a mut f32,
    workspace: Workspace,
    signal_runtime: &'a mut SignalRuntimeValues,
    live_signal_sources: &'a mut HashMap<SignalSource, f64>,
    signal_sample_time: f64,
    event_runtime: &'a mut EventRuntime,
    status: &'a mut String,
    #[cfg(feature = "logic-editor")]
    logic_graph: &'a mut crate::logic_graph_ui::LogicGraphState,
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
                self.preview_canvas,
                self.preview_view_initialized,
                self.preview_grid,
                self.edits,
            ),
            Tab::Timeline => timeline_ui(
                ui,
                self.project,
                self.open_timeline,
                self.instance_path,
                self.selected_item,
                self.current_time,
                self.is_playing,
                self.expanded_layers,
                self.timeline_pixels_per_second,
                self.edits,
            ),
            Tab::Inspector => {
                egui::ScrollArea::vertical()
                    .id_salt("inspector-scroll")
                    .show(ui, |ui| {
                        inspector_ui(
                            ui,
                            self.project,
                            self.plugins,
                            self.selected_item,
                            self.instance_path,
                            *self.current_time,
                            self.workspace,
                            self.signal_runtime,
                            self.edits,
                        );
                    });
            }
            Tab::Assets => assets_ui(ui, self.project, self.edits),
            Tab::Motion => motion_ui(ui, self.project, self.selected_item, self.edits),
            Tab::Data => data_ui(ui, self.project, self.open_timeline, self.edits),
            Tab::Logic => logic_ui(
                ui,
                self.project,
                self.plugins,
                self.render_plan,
                self.selected_item,
                self.instance_path,
                *self.current_time,
                self.signal_runtime,
                self.live_signal_sources,
                self.signal_sample_time,
                self.event_runtime,
                self.status,
                #[cfg(feature = "logic-editor")]
                self.logic_graph,
                self.edits,
            ),
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

#[expect(
    clippy::too_many_arguments,
    reason = "preview keeps canvas navigation and authored selection state explicit"
)]
fn preview_ui(
    ui: &mut egui::Ui,
    preview: Option<&egui::TextureHandle>,
    project: &AuthoringProject,
    selected: Option<TimelineItemId>,
    current_time: f64,
    canvas_state: &mut CanvasState,
    view_initialized: &mut bool,
    show_grid: &mut bool,
    edits: &mut Vec<Edit>,
) {
    let Some(texture) = preview else {
        ui.centered_and_justified(|ui| {
            ui.spinner();
        });
        return;
    };
    let texture_size = texture.size_vec2();
    let mut request_fit = false;
    ui.horizontal(|ui| {
        ui.strong("Canvas");
        let fit_button = ui.small_button("Fit");
        crate::qa::register_component(
            "preview.fit",
            "button",
            fit_button.rect,
            serde_json::json!({}),
        );
        if fit_button.clicked() {
            request_fit = true;
        }
        let grid = ui.toggle_value(show_grid, "Grid");
        crate::qa::register_component(
            "preview.grid",
            "toggle",
            grid.rect,
            serde_json::json!({"checked": *show_grid}),
        );
        ui.label(format!("{:.0}%", canvas_state.zoom.x * 100.0));
        ui.separator();
        ui.small("Wheel: pan  ·  Ctrl+wheel: zoom  ·  Middle-drag: pan  ·  Left-drag: move item");
    });
    let available = ui.available_size().max(egui::vec2(64.0, 64.0));
    let (viewport, response) = ui.allocate_exact_size(available, egui::Sense::click_and_drag());
    crate::qa::register_component("preview.canvas", "canvas", viewport, serde_json::json!({}));
    let fit = |state: &mut CanvasState| {
        let zoom = (viewport.width() / texture_size.x)
            .min(viewport.height() / texture_size.y)
            .max(0.01)
            * 0.92;
        *state = CanvasState::uniform((viewport.size() - texture_size * zoom) * 0.5, zoom);
    };
    if !*view_initialized || request_fit {
        fit(canvas_state);
        *view_initialized = true;
    }
    if response.double_clicked_by(egui::PointerButton::Middle) {
        fit(canvas_state);
    }
    let navigation = NavigationConfig {
        zoom_policy: ZoomPolicy::Uniform,
        input_policy: InputPolicy::AxisModifiers,
        pan_axes: AxisMask::BOTH,
        zoom_axes: AxisMask::BOTH,
        min_zoom: egui::Vec2::splat(0.05),
        max_zoom: egui::Vec2::splat(8.0),
        ..NavigationConfig::default()
    };
    let input = ui.input(|input| NavigationInput {
        anchor: input.pointer.hover_pos().map(|position| {
            let local = position - viewport.min;
            egui::pos2(local.x, local.y)
        }),
        hovered: response.hovered(),
        modifiers: input.modifiers,
        raw_scroll_delta: input.raw_scroll_delta,
        smooth_scroll_delta: input.smooth_scroll_delta,
        zoom_delta: input.zoom_delta(),
        drag_pan_delta: if response.hovered()
            && input.pointer.button_down(egui::PointerButton::Middle)
        {
            input.pointer.delta()
        } else {
            egui::Vec2::ZERO
        },
        scrub_zoom_delta: 0.0,
    });
    apply_navigation(
        canvas_state,
        navigation_delta(input, navigation),
        navigation,
    );
    let painter = ui.painter_at(viewport);
    if *show_grid {
        paint_canvas(
            &painter,
            viewport,
            viewport.min,
            *canvas_state,
            GridConfig {
                minor_spacing: egui::Vec2::splat(32.0),
                major_spacing: egui::Vec2::splat(160.0),
                ..GridConfig::default()
            },
            CanvasTheme::default(),
        );
    } else {
        painter.rect_filled(viewport, 0.0, CanvasTheme::default().background);
    }
    let image_min = viewport.min + canvas_state.pan;
    let image_rect = egui::Rect::from_min_size(image_min, texture_size * canvas_state.zoom);
    painter.image(
        texture.id(),
        image_rect,
        egui::Rect::from_min_max(egui::Pos2::ZERO, egui::pos2(1.0, 1.0)),
        egui::Color32::WHITE,
    );
    painter.rect_stroke(
        image_rect,
        0.0,
        egui::Stroke::new(1.0, egui::Color32::from_gray(105)),
        egui::StrokeKind::Outside,
    );
    if response.dragged_by(egui::PointerButton::Primary) {
        let Some(item) = selected.and_then(|id| project.items.get(&id)) else {
            return;
        };
        let local_time =
            library::core::timeline_runtime::editable_item_local_time(item.interval, current_time);
        let (x, y) = vec2_property_at(item, "position", local_time, (0.0, 0.0));
        let timeline_width = project.tracks.get(&item.track_id).and_then(|track| {
            project
                .timelines
                .get(&track.timeline_id)
                .map(|timeline| timeline.width as f32)
        });
        let source_per_texture_pixel = timeline_width.unwrap_or(texture_size.x) / texture_size.x;
        let delta = ui.input(|input| input.pointer.delta()) / canvas_state.zoom.x
            * source_per_texture_pixel;
        edits.push(Edit::Property(
            item.id,
            "position".to_string(),
            property_vec2(x + f64::from(delta.x), y + f64::from(delta.y)),
        ));
    }
}

#[expect(
    clippy::too_many_arguments,
    reason = "timeline panel keeps layer projection, transport, and edit output explicit"
)]
fn timeline_ui(
    ui: &mut egui::Ui,
    project: &AuthoringProject,
    timeline_id: TimelineId,
    instance_path: &InstancePath,
    selected: Option<TimelineItemId>,
    time: &mut f64,
    is_playing: bool,
    expanded_layers: &mut HashSet<TimelineTrackId>,
    pixels_per_second: &mut f32,
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
        ui.strong(&timeline.name);
        ui.separator();
        let add_menu = ui.menu_button("+ Add", |ui| {
            for (label, edit) in [
                ("Import Media…", Edit::ImportAsset),
                ("Text", Edit::AddText),
                ("Solid", Edit::AddSolid),
                ("Composition", Edit::AddComposition),
                ("Reusable Title", Edit::AddReusableTitle),
                ("Subtitles…", Edit::OpenSubtitleImport),
            ] {
                let item = ui.button(label);
                crate::qa::register_component(
                    format!(
                        "timeline.add.{}",
                        label.to_ascii_lowercase().replace([' ', '…'], "_")
                    ),
                    "menu_item",
                    item.rect,
                    serde_json::json!({"label": label}),
                );
                if item.clicked() {
                    edits.push(edit);
                    ui.close();
                    break;
                }
            }
        });
        crate::qa::register_component(
            "timeline.add",
            "menu_button",
            add_menu.response.rect,
            serde_json::json!({}),
        );
        if let Some(id) = selected {
            ui.separator();
            if ui.small_button("Split").clicked() {
                edits.push(Edit::Split(id));
            }
            ui.menu_button("Delete", |ui| {
                if ui.button("Delete layer").clicked() {
                    edits.push(Edit::Delete(id, false));
                    ui.close();
                }
                if ui.button("Ripple delete").clicked() {
                    edits.push(Edit::Delete(id, true));
                    ui.close();
                }
            });
        }
    });
    timeline_transport_ui(
        ui,
        timeline.duration.into_inner(),
        time,
        is_playing,
        pixels_per_second,
        edits,
    );
    ui.separator();
    timeline_canvas_ui(
        ui,
        project,
        timeline,
        instance_path,
        selected,
        time,
        expanded_layers,
        *pixels_per_second,
        edits,
    );
}

fn timeline_transport_ui(
    ui: &mut egui::Ui,
    duration: f64,
    time: &mut f64,
    is_playing: bool,
    pixels_per_second: &mut f32,
    edits: &mut Vec<Edit>,
) {
    ui.horizontal(|ui| {
        let play = ui
            .button(if is_playing { "Pause" } else { "Play" })
            .on_hover_text("Play/Pause (Space)");
        crate::qa::register_component(
            "timeline.play",
            "button",
            play.rect,
            serde_json::json!({"playing": is_playing}),
        );
        if play.clicked() {
            edits.push(Edit::TogglePlayback);
        }
        if ui.button("Stop").clicked() {
            edits.push(Edit::StopPlayback);
        }
        let minutes = (*time / 60.0).floor();
        let seconds = (*time % 60.0).floor();
        let centiseconds = ((*time % 1.0) * 100.0).floor();
        ui.monospace(format!("{minutes:02.0}:{seconds:02.0}.{centiseconds:02.0}"));
        ui.add(
            egui::Slider::new(time, 0.0..=duration)
                .show_value(false)
                .text("Time"),
        );
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui.small_button("Reset zoom").clicked() {
                *pixels_per_second = 80.0;
            }
            ui.add(
                egui::Slider::new(pixels_per_second, 20.0..=400.0)
                    .logarithmic(true)
                    .show_value(false)
                    .text("Timeline zoom"),
            );
            ui.label(format!("{:.1}×", *pixels_per_second / 80.0));
        });
    });
}

#[derive(Clone, Copy)]
enum TimelineDisplayRow<'a> {
    Layer {
        track: &'a library::model::authoring::TimelineTrack,
        expanded: bool,
    },
    Clip {
        item: &'a library::model::authoring::TimelineItem,
        depth: usize,
    },
}

fn flatten_timeline_layers<'a>(
    project: &'a AuthoringProject,
    timeline: &library::model::authoring::Timeline,
    expanded: &HashSet<TimelineTrackId>,
) -> Vec<TimelineDisplayRow<'a>> {
    let mut rows = Vec::new();
    for track_id in timeline.track_order.iter().rev() {
        let Some(track) = project.tracks.get(track_id) else {
            continue;
        };
        let is_expanded = expanded.contains(track_id);
        rows.push(TimelineDisplayRow::Layer {
            track,
            expanded: is_expanded,
        });
        if !is_expanded {
            continue;
        }
        let mut clips = project
            .items
            .values()
            .filter(|item| item.track_id == *track_id)
            .collect::<Vec<_>>();
        clips.sort_by_key(|item| (std::cmp::Reverse(item.layer), item.interval.start));
        for item in clips {
            let mut depth = 1;
            let mut parent = item.parent;
            while let Some(parent_id) = parent {
                let Some(parent_item) = project.items.get(&parent_id) else {
                    break;
                };
                depth += 1;
                parent = parent_item.parent;
            }
            rows.push(TimelineDisplayRow::Clip { item, depth });
        }
    }
    rows
}

#[expect(
    clippy::too_many_arguments,
    reason = "Timeline surface keeps model, navigation, selection, and deferred edits explicit"
)]
fn timeline_canvas_ui(
    ui: &mut egui::Ui,
    project: &AuthoringProject,
    timeline: &library::model::authoring::Timeline,
    instance_path: &InstancePath,
    selected: Option<TimelineItemId>,
    time: &mut f64,
    expanded_layers: &mut HashSet<TimelineTrackId>,
    pixels_per_second: f32,
    edits: &mut Vec<Edit>,
) {
    const HEADER: f32 = 176.0;
    const RULER_HEIGHT: f32 = 26.0;
    const ROW_HEIGHT: f32 = 34.0;
    let px = pixels_per_second.max(1.0);
    let rows = flatten_timeline_layers(project, timeline, expanded_layers);
    egui::ScrollArea::both()
        .auto_shrink([false, false])
        .show(ui, |ui| {
            let width = HEADER + timeline.duration.into_inner() as f32 * px;
            let height = RULER_HEIGHT + (rows.len().max(1) as f32 * ROW_HEIGHT);
            let (canvas, _) = ui.allocate_exact_size(
                egui::vec2(width.max(ui.available_width()), height),
                egui::Sense::hover(),
            );
            let painter = ui.painter_at(canvas);
            painter.rect_filled(canvas, 0.0, ui.visuals().extreme_bg_color);
            let ruler_rect = egui::Rect::from_min_max(
                egui::pos2(canvas.left() + HEADER, canvas.top()),
                egui::pos2(canvas.right(), canvas.top() + RULER_HEIGHT),
            );
            let ruler = ui.interact(
                ruler_rect,
                ui.make_persistent_id(("timeline-ruler", timeline.id.as_uuid())),
                egui::Sense::click_and_drag(),
            );
            crate::qa::register_component(
                "timeline.ruler",
                "timeline_ruler",
                ruler_rect,
                serde_json::json!({"duration": timeline.duration.into_inner()}),
            );
            for second in 0..=timeline.duration.into_inner().ceil() as usize {
                let x = canvas.left() + HEADER + second as f32 * px;
                painter.line_segment(
                    [
                        egui::pos2(x, canvas.top() + RULER_HEIGHT),
                        egui::pos2(x, canvas.bottom()),
                    ],
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
            if ruler.clicked() || ruler.dragged() {
                if let Some(pointer) = ruler.interact_pointer_pos() {
                    let raw = f64::from((pointer.x - canvas.left() - HEADER) / px).max(0.0);
                    *time = snap_time(raw, timeline.fps.into_inner(), &[])
                        .min(timeline.duration.into_inner());
                }
            }
            let boundaries = project
                .items
                .values()
                .filter(|item| {
                    project
                        .tracks
                        .get(&item.track_id)
                        .is_some_and(|track| track.timeline_id == timeline.id)
                })
                .flat_map(|item| {
                    [
                        item.interval.start.into_inner(),
                        item.interval.start.into_inner() + item.interval.duration.into_inner(),
                    ]
                })
                .collect::<Vec<_>>();
            for (row_index, row) in rows.iter().enumerate() {
                let top = canvas.top() + RULER_HEIGHT + row_index as f32 * ROW_HEIGHT;
                let row_rect = egui::Rect::from_min_size(
                    egui::pos2(canvas.left(), top),
                    egui::vec2(canvas.width(), ROW_HEIGHT),
                );
                painter.rect_filled(
                    row_rect,
                    0.0,
                    if row_index.is_multiple_of(2) {
                        ui.visuals().faint_bg_color
                    } else {
                        ui.visuals().extreme_bg_color
                    },
                );
                match row {
                    TimelineDisplayRow::Layer { track, expanded } => {
                        let header_rect = egui::Rect::from_min_max(
                            row_rect.min,
                            egui::pos2(row_rect.left() + HEADER, row_rect.bottom()),
                        );
                        let expander_rect = egui::Rect::from_min_size(
                            header_rect.min,
                            egui::vec2(24.0, header_rect.height()),
                        );
                        let expander = ui.interact(
                            expander_rect,
                            ui.make_persistent_id(("layer-expander", track.id.as_uuid())),
                            egui::Sense::click(),
                        );
                        crate::qa::register_component(
                            format!("timeline.layer:{}", track.id),
                            "layer",
                            header_rect,
                            serde_json::json!({"name": track.name, "expanded": expanded}),
                        );
                        crate::qa::register_component(
                            format!("timeline.expand:{}", track.id),
                            "expander",
                            expander_rect,
                            serde_json::json!({"expanded": expanded}),
                        );
                        if expander.clicked() {
                            if *expanded {
                                expanded_layers.remove(&track.id);
                            } else {
                                expanded_layers.insert(track.id);
                            }
                        }
                        painter.text(
                            egui::pos2(header_rect.left() + 8.0, header_rect.center().y),
                            egui::Align2::LEFT_CENTER,
                            if *expanded { "v" } else { ">" },
                            egui::FontId::proportional(10.0),
                            ui.visuals().weak_text_color(),
                        );
                        painter.text(
                            egui::pos2(header_rect.left() + 25.0, header_rect.center().y),
                            egui::Align2::LEFT_CENTER,
                            &track.name,
                            egui::FontId::proportional(12.0),
                            ui.visuals().text_color(),
                        );
                        if !*expanded {
                            for item in project
                                .items
                                .values()
                                .filter(|item| item.track_id == track.id)
                            {
                                paint_timeline_clip(
                                    ui,
                                    &painter,
                                    canvas,
                                    row_rect,
                                    item,
                                    selected,
                                    timeline,
                                    instance_path,
                                    px,
                                    &boundaries,
                                    edits,
                                );
                            }
                        }
                    }
                    TimelineDisplayRow::Clip { item, depth } => {
                        let header_rect = egui::Rect::from_min_max(
                            row_rect.min,
                            egui::pos2(row_rect.left() + HEADER, row_rect.bottom()),
                        );
                        let header = ui.interact(
                            header_rect,
                            ui.make_persistent_id(("clip-header", item.id.as_uuid())),
                            egui::Sense::click(),
                        );
                        crate::qa::register_component(
                            format!("timeline.clip_label:{}", item.id),
                            "clip_label",
                            header_rect,
                            serde_json::json!({"name": item.name}),
                        );
                        painter.text(
                            egui::pos2(
                                header_rect.left() + 12.0 + *depth as f32 * 12.0,
                                header_rect.center().y,
                            ),
                            egui::Align2::LEFT_CENTER,
                            &item.name,
                            egui::FontId::proportional(12.0),
                            if selected == Some(item.id) {
                                ui.visuals().selection.stroke.color
                            } else {
                                ui.visuals().text_color()
                            },
                        );
                        if header.clicked() {
                            edits.push(Edit::Select(Some(item.id)));
                        }
                        let left =
                            canvas.left() + HEADER + item.interval.start.into_inner() as f32 * px;
                        let width = (item.interval.duration.into_inner() as f32 * px).max(12.0);
                        let clip_rect = egui::Rect::from_min_size(
                            egui::pos2(left, top + 4.0),
                            egui::vec2(width, ROW_HEIGHT - 8.0),
                        );
                        let trim_rect = egui::Rect::from_min_max(
                            egui::pos2(clip_rect.right() - 8.0, clip_rect.top()),
                            clip_rect.right_bottom(),
                        );
                        let trim = ui.interact(
                            trim_rect,
                            ui.make_persistent_id(("trim", item.id.as_uuid())),
                            egui::Sense::drag(),
                        );
                        let body = ui.interact(
                            egui::Rect::from_min_max(
                                clip_rect.min,
                                egui::pos2(trim_rect.left(), clip_rect.bottom()),
                            ),
                            ui.make_persistent_id(("move", item.id.as_uuid())),
                            egui::Sense::click_and_drag(),
                        );
                        let move_origin_id =
                            ui.make_persistent_id(("move-origin", item.id.as_uuid()));
                        let trim_origin_id =
                            ui.make_persistent_id(("trim-origin", item.id.as_uuid()));
                        let has_move_origin = ui
                            .data(|data| data.get_temp::<(f64, f32)>(move_origin_id))
                            .is_some();
                        if body.is_pointer_button_down_on() && !has_move_origin {
                            let pointer_x = body
                                .interact_pointer_pos()
                                .map_or(body.rect.center().x, |pointer| pointer.x);
                            ui.data_mut(|data| {
                                data.insert_temp(
                                    move_origin_id,
                                    (item.interval.start.into_inner(), pointer_x),
                                )
                            });
                        }
                        let has_trim_origin = ui
                            .data(|data| data.get_temp::<(f64, f32)>(trim_origin_id))
                            .is_some();
                        if trim.is_pointer_button_down_on() && !has_trim_origin {
                            let pointer_x = trim
                                .interact_pointer_pos()
                                .map_or(trim.rect.center().x, |pointer| pointer.x);
                            ui.data_mut(|data| {
                                data.insert_temp(
                                    trim_origin_id,
                                    (
                                        item.interval.start.into_inner()
                                            + item.interval.duration.into_inner(),
                                        pointer_x,
                                    ),
                                )
                            });
                        }
                        crate::qa::register_component(
                            format!("timeline.clip:{}", item.id),
                            "timeline_item",
                            body.rect,
                            serde_json::json!({
                                "name": item.name,
                                "start": item.interval.start.into_inner(),
                                "duration": item.interval.duration.into_inner(),
                            }),
                        );
                        crate::qa::register_component(
                            format!("timeline.trim:{}", item.id),
                            "trim_handle",
                            trim.rect,
                            serde_json::json!({}),
                        );
                        let visual = clip_rect.translate(if body.dragged() {
                            egui::vec2(body.drag_delta().x, 0.0)
                        } else {
                            egui::Vec2::ZERO
                        });
                        let color = if selected == Some(item.id) {
                            ui.visuals().selection.bg_fill
                        } else {
                            item_color(&item.source)
                        };
                        painter.rect_filled(visual, 3.0, color);
                        painter.rect_filled(
                            egui::Rect::from_min_max(
                                egui::pos2(visual.right() - 8.0, visual.top()),
                                visual.right_bottom(),
                            ),
                            2.0,
                            color.gamma_multiply(1.3),
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
                        let released = ui.input(|input| input.pointer.any_released());
                        let pointer_x =
                            ui.input(|input| input.pointer.latest_pos().map(|pos| pos.x));
                        if released {
                            if let (Some((origin, pressed_x)), Some(pointer_x)) = (
                                ui.data(|data| data.get_temp::<(f64, f32)>(move_origin_id)),
                                pointer_x,
                            ) {
                                let raw = origin + f64::from((pointer_x - pressed_x) / px);
                                edits.push(Edit::Move(
                                    item.id,
                                    item.track_id,
                                    snap_time(raw.max(0.0), timeline.fps.into_inner(), &boundaries),
                                    item.layer,
                                ));
                            }
                            if let (Some((origin, pressed_x)), Some(pointer_x)) = (
                                ui.data(|data| data.get_temp::<(f64, f32)>(trim_origin_id)),
                                pointer_x,
                            ) {
                                let raw_end = origin + f64::from((pointer_x - pressed_x) / px);
                                let end =
                                    snap_time(raw_end, timeline.fps.into_inner(), &boundaries);
                                if let Ok(interval) = TimelineInterval::new(
                                    item.interval.start.into_inner(),
                                    (end - item.interval.start.into_inner()).max(0.0),
                                ) {
                                    edits.push(Edit::Trim(item.id, interval));
                                }
                            }
                        }
                        if released {
                            ui.data_mut(|data| data.remove::<(f64, f32)>(move_origin_id));
                        }
                        if released {
                            ui.data_mut(|data| data.remove::<(f64, f32)>(trim_origin_id));
                        }
                    }
                }
            }
            let dragged_asset = ui.data(|data| data.get_temp::<uuid::Uuid>(asset_drag_id()));
            let pointer = ui.input(|input| input.pointer.latest_pos());
            if let (Some(asset_id), Some(pointer)) = (dragged_asset, pointer) {
                if canvas.contains(pointer) && pointer.y >= canvas.top() + RULER_HEIGHT {
                    let row_index = ((pointer.y - canvas.top() - RULER_HEIGHT) / ROW_HEIGHT)
                        .floor()
                        .max(0.0) as usize;
                    if let Some(row) = rows.get(row_index) {
                        let track_id = match row {
                            TimelineDisplayRow::Layer { track, .. } => track.id,
                            TimelineDisplayRow::Clip { item, .. } => item.track_id,
                        };
                        let target_rect = egui::Rect::from_min_size(
                            egui::pos2(
                                canvas.left(),
                                canvas.top() + RULER_HEIGHT + row_index as f32 * ROW_HEIGHT,
                            ),
                            egui::vec2(canvas.width(), ROW_HEIGHT),
                        );
                        painter.rect_stroke(
                            target_rect,
                            0.0,
                            egui::Stroke::new(2.0, ui.visuals().selection.stroke.color),
                            egui::StrokeKind::Inside,
                        );
                        let start = snap_time(
                            f64::from((pointer.x - canvas.left() - HEADER).max(0.0) / px),
                            timeline.fps.into_inner(),
                            &boundaries,
                        );
                        let marker_x = canvas.left() + HEADER + start as f32 * px;
                        painter.line_segment(
                            [
                                egui::pos2(marker_x, target_rect.top()),
                                egui::pos2(marker_x, target_rect.bottom()),
                            ],
                            egui::Stroke::new(2.0, ui.visuals().selection.stroke.color),
                        );
                        if ui.input(|input| input.pointer.any_released()) {
                            edits.push(Edit::PlaceAssetAt(asset_id, track_id, start));
                            ui.data_mut(|data| data.remove::<uuid::Uuid>(asset_drag_id()));
                        }
                    }
                } else if ui.input(|input| input.pointer.any_released()) {
                    ui.data_mut(|data| data.remove::<uuid::Uuid>(asset_drag_id()));
                }
            }
            let playhead_x = canvas.left() + HEADER + *time as f32 * px;
            painter.line_segment(
                [
                    egui::pos2(playhead_x, canvas.top()),
                    egui::pos2(playhead_x, canvas.bottom()),
                ],
                egui::Stroke::new(2.0, egui::Color32::from_rgb(245, 90, 75)),
            );
        });
}

#[expect(
    clippy::too_many_arguments,
    reason = "collapsed layers keep clips directly editable on their shared row"
)]
fn paint_timeline_clip(
    ui: &mut egui::Ui,
    painter: &egui::Painter,
    canvas: egui::Rect,
    row_rect: egui::Rect,
    item: &library::model::authoring::TimelineItem,
    selected: Option<TimelineItemId>,
    timeline: &library::model::authoring::Timeline,
    instance_path: &InstancePath,
    px: f32,
    boundaries: &[f64],
    edits: &mut Vec<Edit>,
) {
    const HEADER: f32 = 176.0;
    let left = canvas.left() + HEADER + item.interval.start.into_inner() as f32 * px;
    let width = (item.interval.duration.into_inner() as f32 * px).max(12.0);
    let clip_rect = egui::Rect::from_min_size(
        egui::pos2(left, row_rect.top() + 4.0),
        egui::vec2(width, row_rect.height() - 8.0),
    );
    let trim_rect = egui::Rect::from_min_max(
        egui::pos2(clip_rect.right() - 8.0, clip_rect.top()),
        clip_rect.right_bottom(),
    );
    let trim = ui.interact(
        trim_rect,
        ui.make_persistent_id(("collapsed-trim", item.id.as_uuid())),
        egui::Sense::drag(),
    );
    let body = ui.interact(
        egui::Rect::from_min_max(
            clip_rect.min,
            egui::pos2(trim_rect.left(), clip_rect.bottom()),
        ),
        ui.make_persistent_id(("collapsed-move", item.id.as_uuid())),
        egui::Sense::click_and_drag(),
    );
    crate::qa::register_component(
        format!("timeline.clip:{}", item.id),
        "timeline_item",
        body.rect,
        serde_json::json!({
            "name": item.name,
            "start": item.interval.start.into_inner(),
            "duration": item.interval.duration.into_inner(),
        }),
    );
    crate::qa::register_component(
        format!("timeline.trim:{}", item.id),
        "trim_handle",
        trim.rect,
        serde_json::json!({}),
    );
    let color = if selected == Some(item.id) {
        ui.visuals().selection.bg_fill
    } else {
        item_color(&item.source)
    };
    let visual = clip_rect.translate(if body.dragged() {
        egui::vec2(body.drag_delta().x, 0.0)
    } else {
        egui::Vec2::ZERO
    });
    painter.rect_filled(visual, 3.0, color);
    painter.rect_filled(
        egui::Rect::from_min_max(
            egui::pos2(visual.right() - 8.0, visual.top()),
            visual.right_bottom(),
        ),
        2.0,
        color.gamma_multiply(1.3),
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
    let move_origin_id = ui.make_persistent_id(("collapsed-move-origin", item.id.as_uuid()));
    let trim_origin_id = ui.make_persistent_id(("collapsed-trim-origin", item.id.as_uuid()));
    if body.is_pointer_button_down_on()
        && ui
            .data(|data| data.get_temp::<(f64, f32)>(move_origin_id))
            .is_none()
    {
        let pointer_x = body
            .interact_pointer_pos()
            .map_or(body.rect.center().x, |pointer| pointer.x);
        ui.data_mut(|data| {
            data.insert_temp(
                move_origin_id,
                (item.interval.start.into_inner(), pointer_x),
            )
        });
    }
    if trim.is_pointer_button_down_on()
        && ui
            .data(|data| data.get_temp::<(f64, f32)>(trim_origin_id))
            .is_none()
    {
        let pointer_x = trim
            .interact_pointer_pos()
            .map_or(trim.rect.center().x, |pointer| pointer.x);
        ui.data_mut(|data| {
            data.insert_temp(
                trim_origin_id,
                (
                    item.interval.start.into_inner() + item.interval.duration.into_inner(),
                    pointer_x,
                ),
            )
        });
    }
    if ui.input(|input| input.pointer.any_released()) {
        let pointer_x = ui.input(|input| input.pointer.latest_pos().map(|pos| pos.x));
        if let (Some((origin, pressed_x)), Some(pointer_x)) = (
            ui.data(|data| data.get_temp::<(f64, f32)>(move_origin_id)),
            pointer_x,
        ) {
            let raw = origin + f64::from((pointer_x - pressed_x) / px);
            edits.push(Edit::Move(
                item.id,
                item.track_id,
                snap_time(raw.max(0.0), timeline.fps.into_inner(), boundaries),
                item.layer,
            ));
        }
        if let (Some((origin, pressed_x)), Some(pointer_x)) = (
            ui.data(|data| data.get_temp::<(f64, f32)>(trim_origin_id)),
            pointer_x,
        ) {
            let end = snap_time(
                origin + f64::from((pointer_x - pressed_x) / px),
                timeline.fps.into_inner(),
                boundaries,
            );
            if let Ok(interval) = TimelineInterval::new(
                item.interval.start.into_inner(),
                (end - item.interval.start.into_inner()).max(0.0),
            ) {
                edits.push(Edit::Trim(item.id, interval));
            }
        }
        ui.data_mut(|data| {
            data.remove::<(f64, f32)>(move_origin_id);
            data.remove::<(f64, f32)>(trim_origin_id);
        });
    }
}

fn last_track(
    project: &AuthoringProject,
    timeline_id: TimelineId,
) -> Result<TimelineTrackId, library::LibraryError> {
    project
        .timelines
        .get(&timeline_id)
        .ok_or_else(|| {
            library::LibraryError::Validation(format!("Timeline {timeline_id} is missing"))
        })?
        .track_order
        .last()
        .copied()
        .ok_or_else(|| {
            library::LibraryError::Validation(format!("Timeline {timeline_id} has no Track"))
        })
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

fn asset_drag_id() -> egui::Id {
    egui::Id::new("ruvie-asset-panel-drag")
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

fn inspector_number(
    ui: &mut egui::Ui,
    id: impl std::hash::Hash,
    label: &str,
    value: &mut f64,
    speed: f64,
    suffix: &str,
) -> bool {
    ui.push_id(id, |ui| {
        ui.horizontal(|ui| {
            ui.add_sized([82.0, 20.0], egui::Label::new(label).selectable(false));
            ui.add_sized(
                [184.0, 20.0],
                egui::DragValue::new(value).speed(speed).suffix(suffix),
            )
            .changed()
        })
        .inner
    })
    .inner
}

fn inspector_integer(
    ui: &mut egui::Ui,
    id: impl std::hash::Hash,
    label: &str,
    value: &mut i64,
) -> bool {
    ui.push_id(id, |ui| {
        ui.horizontal(|ui| {
            ui.add_sized([82.0, 20.0], egui::Label::new(label).selectable(false));
            ui.add_sized([184.0, 20.0], egui::DragValue::new(value).speed(1.0))
                .changed()
        })
        .inner
    })
    .inner
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct InspectorPropertyResponse {
    value_changed: bool,
    toggle_keyframe: bool,
}

fn inspector_keyframe_control(
    ui: &mut egui::Ui,
    item_id: TimelineItemId,
    key: &str,
    property: Option<&Property>,
    local_time: f64,
) -> bool {
    const TOLERANCE: f64 = 0.001;
    let is_keyframed = property.is_some_and(|property| property.evaluator == "keyframe");
    let has_key = property.is_some_and(|property| property.has_keyframe_at(local_time, TOLERANCE));
    let (color, tooltip) = if has_key {
        (
            egui::Color32::from_rgb(244, 186, 88),
            "Remove keyframe at the current local time",
        )
    } else if is_keyframed {
        (
            egui::Color32::from_rgb(217, 166, 85),
            "Add keyframe at the current local time",
        )
    } else {
        (
            ui.visuals().weak_text_color(),
            "Enable animation and add a keyframe at the current local time",
        )
    };
    let (rect, response) = ui.allocate_exact_size(egui::vec2(20.0, 20.0), egui::Sense::click());
    if response.hovered() {
        ui.painter()
            .rect_filled(rect.shrink(2.0), 3.0, ui.visuals().widgets.hovered.bg_fill);
    }
    let center = rect.center();
    let radius = 4.0;
    let points = vec![
        center + egui::vec2(0.0, -radius),
        center + egui::vec2(radius, 0.0),
        center + egui::vec2(0.0, radius),
        center + egui::vec2(-radius, 0.0),
    ];
    if has_key {
        ui.painter().add(egui::Shape::convex_polygon(
            points,
            color,
            egui::Stroke::NONE,
        ));
    } else {
        ui.painter().add(egui::Shape::closed_line(
            points,
            egui::Stroke::new(1.25, color),
        ));
    }
    let response = response
        .on_hover_cursor(egui::CursorIcon::PointingHand)
        .on_hover_text(tooltip);
    crate::qa::register_component(
        format!("inspector.keyframe:{item_id}:{key}"),
        "property_keyframe_toggle",
        response.rect,
        serde_json::json!({
            "property": key,
            "keyframed": is_keyframed,
            "key_at_current_time": has_key,
            "local_time": local_time,
        }),
    );
    response.clicked()
}

#[expect(
    clippy::too_many_arguments,
    reason = "a compact Inspector number row keeps value and animation state together"
)]
fn inspector_property_number(
    ui: &mut egui::Ui,
    item_id: TimelineItemId,
    key: &str,
    label: &str,
    property: Option<&Property>,
    local_time: f64,
    value: &mut f64,
    speed: f64,
    suffix: &str,
) -> InspectorPropertyResponse {
    ui.push_id((item_id, key), |ui| {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 2.0;
            ui.add_sized([60.0, 20.0], egui::Label::new(label).selectable(false));
            let toggle_keyframe =
                inspector_keyframe_control(ui, item_id, key, property, local_time);
            let value_response = ui.add_sized(
                [184.0, 20.0],
                egui::DragValue::new(value).speed(speed).suffix(suffix),
            );
            crate::qa::register_component(
                format!("inspector.value:{item_id}:{key}"),
                "inspector_number",
                value_response.rect,
                serde_json::json!({"property": key, "value": *value}),
            );
            InspectorPropertyResponse {
                value_changed: value_response.changed(),
                toggle_keyframe,
            }
        })
        .inner
    })
    .inner
}

#[expect(
    clippy::too_many_arguments,
    reason = "a compact Inspector vector row keeps both axes and animation state together"
)]
fn inspector_property_vec2(
    ui: &mut egui::Ui,
    item_id: TimelineItemId,
    key: &str,
    label: &str,
    property: Option<&Property>,
    local_time: f64,
    x: &mut f64,
    y: &mut f64,
    speed: f64,
    suffix: &str,
) -> InspectorPropertyResponse {
    ui.push_id((item_id, key), |ui| {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 2.0;
            ui.add_sized([60.0, 20.0], egui::Label::new(label).selectable(false));
            let toggle_keyframe =
                inspector_keyframe_control(ui, item_id, key, property, local_time);
            let x_response = ui.add_sized(
                [90.0, 20.0],
                egui::DragValue::new(x).speed(speed).prefix("X "),
            );
            let y_response = ui.add_sized(
                [90.0, 20.0],
                egui::DragValue::new(y)
                    .speed(speed)
                    .prefix("Y ")
                    .suffix(suffix),
            );
            for (axis, response, value) in [("x", &x_response, *x), ("y", &y_response, *y)] {
                crate::qa::register_component(
                    format!("inspector.value:{item_id}:{key}:{axis}"),
                    "inspector_vector_component",
                    response.rect,
                    serde_json::json!({"property": key, "axis": axis, "value": value}),
                );
            }
            InspectorPropertyResponse {
                value_changed: x_response.changed() || y_response.changed(),
                toggle_keyframe,
            }
        })
        .inner
    })
    .inner
}

fn keyframe_toggle_edit(
    item_id: TimelineItemId,
    key: &str,
    property: Option<&Property>,
    local_time: f64,
    value: PropertyValue,
) -> Edit {
    const TOLERANCE: f64 = 0.001;
    property
        .and_then(|property| property.keyframe_id_at(local_time, TOLERANCE))
        .map_or_else(
            || Edit::UpsertKeyframe(item_id, key.to_string(), local_time, value),
            |keyframe_id| Edit::RemoveKeyframe(item_id, key.to_string(), keyframe_id),
        )
}

fn duration_policy_label(policy: &DurationPolicy) -> &'static str {
    match policy {
        DurationPolicy::Fixed => "Fixed",
        DurationPolicy::Scale => "Scale",
        DurationPolicy::Loop => "Loop",
        DurationPolicy::Responsive { .. } => "Responsive",
    }
}

#[expect(
    clippy::too_many_arguments,
    reason = "the immediate-mode Inspector receives explicit immutable editor context"
)]
fn inspector_ui(
    ui: &mut egui::Ui,
    project: &AuthoringProject,
    plugins: &library::plugin::PluginManager,
    selected: Option<TimelineItemId>,
    instance_path: &InstancePath,
    current_time: f64,
    workspace: Workspace,
    signal_runtime: &SignalRuntimeValues,
    edits: &mut Vec<Edit>,
) {
    let Some(id) = selected else {
        ui.label("Select a Timeline item");
        return;
    };
    let Some(item) = project.items.get(&id) else {
        return;
    };
    let local_time =
        library::core::timeline_runtime::editable_item_local_time(item.interval, current_time);
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
    if inspector_number(ui, (id, "start"), "Start", &mut start, 0.05, " s") {
        edits.push(Edit::Move(id, item.track_id, start.max(0.0), layer));
    }
    if inspector_number(ui, (id, "duration"), "Duration", &mut duration, 0.05, " s") {
        if let Ok(interval) = TimelineInterval::new(start, duration.max(0.0)) {
            edits.push(Edit::Trim(id, interval));
        }
    }
    if inspector_integer(ui, (id, "layer"), "Layer", &mut layer) {
        edits.push(Edit::Move(id, item.track_id, start, layer));
    }
    let timeline_id = project.tracks[&item.track_id].timeline_id;
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
        let definition_duration = project
            .timelines
            .get(&instance.timeline_id)
            .map(|timeline| timeline.duration.into_inner())
            .unwrap_or(item.interval.duration.into_inner());
        let responsive_edge = (definition_duration * 0.2)
            .min(item.interval.duration.into_inner() * 0.5)
            .max(0.0);
        egui::ComboBox::from_label("Duration")
            .selected_text(duration_policy_label(&instance.duration_policy))
            .show_ui(ui, |ui| {
                for (label, policy) in [
                    ("Fixed", DurationPolicy::Fixed),
                    ("Scale", DurationPolicy::Scale),
                    ("Loop", DurationPolicy::Loop),
                    (
                        "Responsive",
                        DurationPolicy::Responsive {
                            intro_end: OrderedFloat(responsive_edge),
                            outro_start: OrderedFloat(definition_duration - responsive_edge),
                        },
                    ),
                ] {
                    if ui
                        .selectable_label(
                            duration_policy_label(&instance.duration_policy) == label,
                            label,
                        )
                        .clicked()
                    {
                        edits.push(Edit::DurationPolicy(id, policy));
                    }
                }
            });
        if let DurationPolicy::Responsive {
            intro_end,
            outro_start,
        } = &instance.duration_policy
        {
            let mut intro_end = intro_end.into_inner();
            let mut outro_start = outro_start.into_inner();
            let intro_changed = inspector_number(
                ui,
                (id, "responsive-intro"),
                "Intro end",
                &mut intro_end,
                0.05,
                " s",
            );
            let outro_changed = inspector_number(
                ui,
                (id, "responsive-outro"),
                "Outro start",
                &mut outro_start,
                0.05,
                " s",
            );
            if intro_changed || outro_changed {
                let placement_duration = item.interval.duration.into_inner();
                let intro_end = intro_end
                    .clamp(0.0, definition_duration)
                    .min(placement_duration);
                let minimum_outro_start =
                    (definition_duration + intro_end - placement_duration).max(intro_end);
                let outro_start = outro_start
                    .clamp(intro_end, definition_duration)
                    .max(minimum_outro_start);
                edits.push(Edit::DurationPolicy(
                    id,
                    DurationPolicy::Responsive {
                        intro_end: OrderedFloat(intro_end),
                        outro_start: OrderedFloat(outro_start),
                    },
                ));
            }
            ui.small("Intro and Outro keep their timing; the middle section adapts to the clip.");
        }
    }
    if ui.button("Fade In / Out").clicked() {
        edits.push(Edit::Fade(id, 0.5));
    }
    if ui
        .add_enabled(
            item.transition_in.is_none(),
            egui::Button::new("Cross Dissolve from Previous"),
        )
        .clicked()
    {
        edits.push(Edit::CrossDissolve(id, 0.5));
    }
    if workspace.depth() >= 2 {
        ui.horizontal(|ui| {
            ui.label(format!("Masks: {}", item.mask_ids.len()));
            if ui.small_button("Add Rectangle Mask").clicked() {
                edits.push(Edit::AddRectangleMask(id));
            }
        });
        for mask_id in &item.mask_ids {
            let Some(mask) = project.masks.get(mask_id) else {
                continue;
            };
            let numeric = |property: &Property, fallback: f64| {
                property
                    .evaluate_at(local_time)
                    .ok()
                    .and_then(|value| match value {
                        PropertyValue::Number(value) => Some(value.into_inner()),
                        PropertyValue::Integer(value) => Some(value as f64),
                        _ => None,
                    })
                    .unwrap_or(fallback)
            };
            let mut mode = mask.mode;
            let mut inverted = mask.inverted;
            let mut feather = numeric(&mask.feather, 0.0);
            let mut opacity = numeric(&mask.opacity, 1.0);
            let mut changed = false;
            ui.horizontal(|ui| {
                egui::ComboBox::from_id_salt(("mask-mode", mask_id))
                    .selected_text(format!("{mode:?}"))
                    .show_ui(ui, |ui| {
                        for candidate in [
                            MaskMode::Add,
                            MaskMode::Subtract,
                            MaskMode::Intersect,
                            MaskMode::Difference,
                        ] {
                            changed |= ui
                                .selectable_value(&mut mode, candidate, format!("{candidate:?}"))
                                .changed();
                        }
                    });
                changed |= ui.checkbox(&mut inverted, "Invert").changed();
            });
            changed |= ui
                .add(egui::Slider::new(&mut feather, 0.0..=100.0).text("Feather"))
                .changed();
            changed |= ui
                .add(egui::Slider::new(&mut opacity, 0.0..=1.0).text("Mask opacity"))
                .changed();
            if changed {
                edits.push(Edit::UpdateMask(
                    id, *mask_id, mode, inverted, feather, opacity,
                ));
            }
        }
        ui.collapsing("Track Matte", |ui| {
            if let Some(matte) = item.matte {
                let source_name = project
                    .items
                    .get(&matte.item_id)
                    .map(|item| item.name.as_str())
                    .unwrap_or("Missing");
                ui.label(format!("{source_name} · {:?}", matte.mode));
                if ui.small_button("Clear Matte").clicked() {
                    edits.push(Edit::SetMatte(id, None));
                }
            } else {
                ui.label("Choose another item in this composition as the matte source.");
            }
            for candidate in project.items.values().filter(|candidate| {
                candidate.id != id && project.tracks[&candidate.track_id].timeline_id == timeline_id
            }) {
                ui.horizontal(|ui| {
                    ui.label(&candidate.name);
                    for (label, mode) in [
                        ("Alpha", MatteMode::Alpha),
                        ("Alpha Invert", MatteMode::AlphaInverted),
                        ("Luma", MatteMode::Luma),
                        ("Luma Invert", MatteMode::LumaInverted),
                    ] {
                        if ui.small_button(label).clicked() {
                            edits.push(Edit::SetMatte(
                                id,
                                Some(MatteRef {
                                    item_id: candidate.id,
                                    mode,
                                }),
                            ));
                        }
                    }
                });
            }
        });
        ui.collapsing("Constraints", |ui| {
            for constraint in &item.constraints {
                let target = project
                    .items
                    .get(&constraint.target_item_id)
                    .map(|item| item.name.as_str())
                    .unwrap_or("Missing");
                ui.label(format!("{:?} → {target}", constraint.kind));
            }
            for candidate in project.items.values().filter(|candidate| {
                candidate.id != id && project.tracks[&candidate.track_id].timeline_id == timeline_id
            }) {
                ui.horizontal(|ui| {
                    ui.label(&candidate.name);
                    if ui.small_button("Copy Position").clicked() {
                        edits.push(Edit::AddConstraint(
                            id,
                            candidate.id,
                            ConstraintKind::CopyPosition,
                        ));
                    }
                    if ui.small_button("Look At").clicked() {
                        edits.push(Edit::AddConstraint(
                            id,
                            candidate.id,
                            ConstraintKind::LookAt,
                        ));
                    }
                    if matches!(
                        &candidate.source,
                        SourceRef::Shape { shape }
                            if shape.shape_kind == library::model::authoring::ShapeKind::Path
                    ) && ui.small_button("Follow Path").clicked()
                    {
                        edits.push(Edit::AddConstraint(
                            id,
                            candidate.id,
                            ConstraintKind::FollowPath,
                        ));
                    }
                });
            }
        });
    }
    let is_audio = match &item.source {
        SourceRef::Asset { asset_id, .. } => project
            .assets
            .iter()
            .find(|asset| asset.id == *asset_id)
            .is_some_and(|asset| matches!(asset.kind, AssetKind::Audio)),
        _ => false,
    };
    if is_audio {
        let mut volume = number_property_at(item, "volume", local_time, 1.0);
        if ui
            .add(egui::Slider::new(&mut volume, 0.0..=2.0).text("Volume"))
            .changed()
        {
            edits.push(Edit::Property(
                id,
                "volume".to_string(),
                PropertyValue::Number(OrderedFloat(volume)),
            ));
        }
    }
    ui.separator();
    ui.heading("Transform");
    let (mut x, mut y) = vec2_property_at(item, "position", local_time, (0.0, 0.0));
    let position = property_vec2(x, y);
    let response = inspector_property_vec2(
        ui,
        id,
        "position",
        "Position",
        item.authored_properties.get("position"),
        local_time,
        &mut x,
        &mut y,
        1.0,
        " px",
    );
    if response.value_changed {
        edits.push(Edit::Property(
            id,
            "position".to_string(),
            property_vec2(x, y),
        ));
    }
    if response.toggle_keyframe {
        edits.push(keyframe_toggle_edit(
            id,
            "position",
            item.authored_properties.get("position"),
            local_time,
            position,
        ));
    }

    let (scale_x, scale_y) = vec2_property_at(item, "scale", local_time, (1.0, 1.0));
    let (mut scale_x_percent, mut scale_y_percent) = (scale_x * 100.0, scale_y * 100.0);
    let scale = property_vec2(scale_x, scale_y);
    let response = inspector_property_vec2(
        ui,
        id,
        "scale",
        "Scale",
        item.authored_properties.get("scale"),
        local_time,
        &mut scale_x_percent,
        &mut scale_y_percent,
        0.1,
        "%",
    );
    if response.value_changed {
        edits.push(Edit::Property(
            id,
            "scale".to_string(),
            property_vec2(
                scale_x_percent.max(0.0) / 100.0,
                scale_y_percent.max(0.0) / 100.0,
            ),
        ));
    }
    if response.toggle_keyframe {
        edits.push(keyframe_toggle_edit(
            id,
            "scale",
            item.authored_properties.get("scale"),
            local_time,
            scale,
        ));
    }

    let mut rotation = number_property_at(item, "rotation", local_time, 0.0);
    let rotation_value = PropertyValue::Number(OrderedFloat(rotation));
    let response = inspector_property_number(
        ui,
        id,
        "rotation",
        "Rotation",
        item.authored_properties.get("rotation"),
        local_time,
        &mut rotation,
        1.0,
        "°",
    );
    if response.value_changed {
        edits.push(Edit::Property(
            id,
            "rotation".to_string(),
            PropertyValue::Number(OrderedFloat(rotation)),
        ));
    }
    if response.toggle_keyframe {
        edits.push(keyframe_toggle_edit(
            id,
            "rotation",
            item.authored_properties.get("rotation"),
            local_time,
            rotation_value,
        ));
    }

    let (mut anchor_x, mut anchor_y) = vec2_property_at(item, "anchor", local_time, (0.0, 0.0));
    let anchor = property_vec2(anchor_x, anchor_y);
    let response = inspector_property_vec2(
        ui,
        id,
        "anchor",
        "Anchor",
        item.authored_properties.get("anchor"),
        local_time,
        &mut anchor_x,
        &mut anchor_y,
        1.0,
        " px",
    );
    if response.value_changed {
        edits.push(Edit::Property(
            id,
            "anchor".to_string(),
            property_vec2(anchor_x, anchor_y),
        ));
    }
    if response.toggle_keyframe {
        edits.push(keyframe_toggle_edit(
            id,
            "anchor",
            item.authored_properties.get("anchor"),
            local_time,
            anchor,
        ));
    }

    let mut opacity = number_property_at(item, "opacity", local_time, 1.0);
    let opacity_value = PropertyValue::Number(OrderedFloat(opacity));
    let response = inspector_property_number(
        ui,
        id,
        "opacity",
        "Opacity",
        item.authored_properties.get("opacity"),
        local_time,
        &mut opacity,
        0.01,
        "",
    );
    if response.value_changed {
        opacity = opacity.clamp(0.0, 1.0);
        edits.push(Edit::Property(
            id,
            "opacity".to_string(),
            PropertyValue::Number(OrderedFloat(opacity)),
        ));
    }
    if response.toggle_keyframe {
        edits.push(keyframe_toggle_edit(
            id,
            "opacity",
            item.authored_properties.get("opacity"),
            local_time,
            opacity_value,
        ));
    }
    ui.indent("opacity-provenance", |ui| {
        if let Some(property) = item.authored_properties.get("opacity") {
            if let Some(base) = property.value() {
                ui.small(format!("Base: {:?}", base));
            }
            if property.evaluator == "keyframe" {
                ui.small(format!(
                    "Keyframe at {:.3}s local: {:.3}",
                    local_time, opacity
                ));
            }
        } else {
            ui.small("Base: 1.0");
        }
        if let Some(parent) = item.parent.and_then(|parent| project.items.get(&parent)) {
            let parent_time = library::core::timeline_runtime::editable_item_local_time(
                parent.interval,
                current_time,
            );
            let inherited = number_property_at(parent, "opacity", parent_time, 1.0);
            ui.small(format!("Parent {}: x{inherited:.3}", parent.name));
        }
    });
    ui.separator();
    ui.heading("Effect Stack");
    let mut available_effects = plugins.get_available_effects();
    available_effects.sort_by(|left, right| (&left.2, &left.1).cmp(&(&right.2, &right.1)));
    let add_effect = ui.menu_button("+ Add Effect", |ui| {
        if available_effects.is_empty() {
            ui.label("No effects are installed");
        }
        let mut current_category = None;
        for (effect_id, name, category) in &available_effects {
            if current_category.as_ref() != Some(category) {
                if current_category.is_some() {
                    ui.separator();
                }
                ui.strong(category);
                current_category = Some(category.clone());
            }
            let option = ui.button(name);
            crate::qa::register_component(
                format!("inspector.effects.option:{effect_id}"),
                "effect_option",
                option.rect,
                serde_json::json!({"effect_id": effect_id, "name": name, "category": category}),
            );
            if option.clicked() {
                edits.push(Edit::AddEffect(id, effect_id.clone()));
                ui.close();
            }
        }
    });
    crate::qa::register_component(
        "inspector.effects.add",
        "menu_button",
        add_effect.response.rect,
        serde_json::json!({"effect_count": available_effects.len()}),
    );
    let mut attachments: Vec<_> = project
        .attachments
        .values()
        .filter(|attachment| matches!(attachment.owner, library::model::authoring::AttachmentOwner::Item { item_id } if item_id == id))
        .collect();
    attachments.sort_by_key(|attachment| (attachment.order, attachment.id));
    if attachments.is_empty() {
        ui.label("No effects");
    }
    let attachment_count = attachments.len();
    for (index, attachment) in attachments.into_iter().enumerate() {
        let instance = &project.module_instances[&attachment.module_instance_id];
        let definition = &project.module_definitions[&instance.definition_id];
        ui.group(|ui| {
            ui.horizontal(|ui| {
                ui.strong(&definition.name);
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let remove = ui.small_button("Remove");
                    crate::qa::register_component(
                        format!("inspector.effects.remove:{}", attachment.id),
                        "effect_remove",
                        remove.rect,
                        serde_json::json!({}),
                    );
                    if remove.clicked() {
                        edits.push(Edit::RemoveAttachment(attachment.id));
                    }
                    let down = ui.add_enabled(
                        index + 1 < attachment_count,
                        egui::Button::new("Down").small(),
                    );
                    crate::qa::register_component(
                        format!("inspector.effects.down:{}", attachment.id),
                        "effect_move_down",
                        down.rect,
                        serde_json::json!({"enabled": down.enabled()}),
                    );
                    if down.clicked() {
                        edits.push(Edit::MoveAttachment(attachment.id, 1));
                    }
                    let up = ui.add_enabled(index > 0, egui::Button::new("Up").small());
                    crate::qa::register_component(
                        format!("inspector.effects.up:{}", attachment.id),
                        "effect_move_up",
                        up.rect,
                        serde_json::json!({"enabled": up.enabled()}),
                    );
                    if up.clicked() {
                        edits.push(Edit::MoveAttachment(attachment.id, -1));
                    }
                });
            });
            ui.small(format!("{:?}", attachment.stage));
            for parameter in &definition.published_parameters {
                effect_parameter_ui(
                    ui,
                    project,
                    definition,
                    instance,
                    instance_path,
                    parameter,
                    signal_runtime,
                    edits,
                );
            }
        });
    }
}

#[expect(
    clippy::too_many_arguments,
    reason = "an Effect parameter row receives its published-interface and runtime provenance context"
)]
fn effect_parameter_ui(
    ui: &mut egui::Ui,
    project: &AuthoringProject,
    definition: &library::model::authoring::ModuleDefinition,
    instance: &library::model::authoring::ModuleInstance,
    instance_path: &InstancePath,
    parameter: &library::model::authoring::PublishedParameter,
    signal_runtime: &SignalRuntimeValues,
    edits: &mut Vec<Edit>,
) {
    let value = instance
        .parameter_overrides
        .get(&parameter.id)
        .unwrap_or(&parameter.default_value);
    match value {
        PropertyValue::Number(number) => {
            let mut number = number.into_inner();
            if inspector_number(
                ui,
                (instance.id, parameter.id),
                &parameter.name,
                &mut number,
                0.1,
                "",
            ) {
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
        if let Some(effective) = resolve_published_numeric_value(
            definition.id,
            instance,
            instance_path,
            parameter,
            project.signal_bindings.values(),
            signal_runtime,
        ) {
            ui.strong(format!("Effective: {:?}", effective.value));
            for contribution in effective.contributions {
                ui.small(format!("{}: {:?}", contribution.label, contribution.value));
            }
        } else {
            ui.small(format!("Base: {:?}", parameter.default_value));
            if let Some(value) = instance.parameter_overrides.get(&parameter.id) {
                ui.small(format!("Instance override: {:?}", value));
            }
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
                        } => *module_instance_id == instance.id && target_path == instance_path,
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
        let bind_audio = ui.add_enabled(
            bindings.is_empty(),
            egui::Button::new("Bind audio envelope"),
        );
        crate::qa::register_component(
            format!("inspector.binding.audio:{}:{}", instance.id, parameter.id),
            "add_signal_binding",
            bind_audio.rect,
            serde_json::json!({"enabled": bindings.is_empty()}),
        );
        if bind_audio.clicked() {
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

fn assets_ui(ui: &mut egui::Ui, project: &AuthoringProject, edits: &mut Vec<Edit>) {
    ui.heading("Project Assets");
    let import = ui.button("Import Media...");
    crate::qa::register_component(
        "assets.import",
        "button",
        import.rect,
        serde_json::json!({}),
    );
    if import.clicked() {
        edits.push(Edit::ImportAsset);
    }
    if project.assets.is_empty() {
        ui.label("Import media to build the project library.");
    } else {
        ui.small("Drag an asset onto a Timeline layer to place it.");
    }
    for asset in &project.assets {
        let asset_row = ui
            .add(
                egui::Label::new(format!("{}  ·  {:?}", asset.name, asset.kind))
                    .sense(egui::Sense::drag()),
            )
            .on_hover_cursor(egui::CursorIcon::Grab);
        crate::qa::register_component(
            format!("assets.asset:{}", asset.id),
            "draggable_asset",
            asset_row.rect,
            serde_json::json!({"name": asset.name}),
        );
        if asset_row.drag_started() {
            ui.data_mut(|data| data.insert_temp(asset_drag_id(), asset.id));
        }
        if asset_row.dragged() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
        }
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
    let mut subtitles: Vec<_> = project
        .items
        .values()
        .filter(|item| {
            project
                .tracks
                .get(&item.track_id)
                .is_some_and(|track| track.timeline_id == open_timeline)
                && project.transcript_links.contains_key(&item.id)
                && matches!(item.source, SourceRef::Text { .. })
        })
        .collect();
    subtitles.sort_by_key(|item| (item.interval.start, item.layer));
    ui.heading(format!("Subtitles ({})", subtitles.len()));
    ui.label("Edit imported subtitle text in one list; cue timing remains attached to each item.");
    egui::ScrollArea::vertical()
        .id_salt("subtitle-list")
        .max_height(260.0)
        .show(ui, |ui| {
            for item in subtitles {
                let SourceRef::Text { text } = &item.source else {
                    continue;
                };
                let link = project.transcript_links.get(&item.id);
                let mut edited = text.clone();
                ui.horizontal(|ui| {
                    let timing = ui.monospace(format!(
                        "{:.2}–{:.2}",
                        item.interval.start,
                        item.interval.start.into_inner() + item.interval.duration.into_inner()
                    ));
                    if let Some(link) = link {
                        if let Some(document) = project.transcript_documents.get(&link.document_id)
                        {
                            let original = document
                                .text
                                .get(link.text_start..link.text_end)
                                .unwrap_or("<invalid transcript range>");
                            timing.on_hover_text(format!(
                                "{} · source {:.2}–{:.2}\nOriginal transcript: {}",
                                document.name,
                                link.source_time.start,
                                link.source_time.start.into_inner()
                                    + link.source_time.duration.into_inner(),
                                original
                            ));
                        }
                    }
                    if ui.text_edit_singleline(&mut edited).changed() {
                        edits.push(Edit::SetText(item.id, edited));
                    }
                });
            }
        });
    ui.separator();
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

#[expect(
    clippy::too_many_arguments,
    reason = "Logic workspace keeps Project, compiled routes, instance path, and runtime state explicit"
)]
fn logic_ui(
    ui: &mut egui::Ui,
    project: &AuthoringProject,
    plugins: &library::plugin::PluginManager,
    render_plan: &library::core::render_plan::RenderPlan,
    selected: Option<TimelineItemId>,
    instance_path: &InstancePath,
    current_time: f64,
    signal_runtime: &mut SignalRuntimeValues,
    live_signal_sources: &mut HashMap<SignalSource, f64>,
    signal_sample_time: f64,
    event_runtime: &mut EventRuntime,
    status: &mut String,
    #[cfg(feature = "logic-editor")] logic_graph: &mut crate::logic_graph_ui::LogicGraphState,
    edits: &mut Vec<Edit>,
) {
    ui.heading("Logic Module");
    ui.label("Only reusable ModuleDefinitions appear here. Timeline items are never expanded into nodes.");
    if !project.signal_bindings.is_empty() {
        let signal_preview = ui.collapsing("Live signal preview", |ui| {
            ui.small("Each runtime-only source sample is routed through the compiled Bindings; it is not saved in the project.");
            let mut sources = project
                .signal_bindings
                .values()
                .map(|binding| binding.source.clone())
                .collect::<Vec<_>>();
            sources.sort_by_key(|source| format!("{source:?}"));
            sources.dedup();
            for source in sources {
                let mut value = *live_signal_sources.get(&source).unwrap_or(&0.0);
                let response = ui.add(
                    egui::Slider::new(&mut value, 0.0..=1.0)
                        .text(format!("{source:?}")),
                );
                crate::qa::register_component(
                    format!("logic.signal_source:{source:?}"),
                    "signal_source",
                    response.rect,
                    serde_json::json!({"source": format!("{source:?}"), "value": value}),
                );
                if response.changed() {
                    live_signal_sources.insert(source.clone(), value);
                    match signal_runtime.sample_source(
                        &render_plan.bindings,
                        &source,
                        value,
                        signal_sample_time,
                    ) {
                        Ok(routes) => {
                            *status = format!("Signal sample routed to {} Binding(s)", routes.len());
                        }
                        Err(error) => *status = error,
                    }
                }
            }
        });
        crate::qa::register_component(
            "logic.signal_preview",
            "collapsing_header",
            signal_preview.header_response.rect,
            serde_json::json!({"open": signal_preview.body_response.is_some()}),
        );
    }
    if !project.event_bindings.is_empty() {
        ui.collapsing("Live event preview", |ui| {
            ui.small("Runtime-only occurrences; Stop clears every queue and overlap.");
            let mut sources = project
                .event_bindings
                .values()
                .map(|binding| binding.source.clone())
                .collect::<Vec<_>>();
            sources.sort_by_key(|source| format!("{source:?}"));
            sources.dedup();
            let duration = selected
                .and_then(|item_id| project.items.get(&item_id))
                .map(|item| item.interval.duration.into_inner())
                .unwrap_or(1.0)
                .max(f64::EPSILON);
            for source in sources {
                let trigger = ui.button(format!("Trigger {source:?}"));
                crate::qa::register_component(
                    format!("logic.event_source:{source:?}"),
                    "event_source",
                    trigger.rect,
                    serde_json::json!({"source": format!("{source:?}")}),
                );
                if trigger.clicked() {
                    match event_runtime.trigger_source(
                        &render_plan.bindings,
                        &source,
                        current_time,
                        duration,
                    ) {
                        Ok(outcomes) => {
                            *status = format!("Event routed to {} Binding(s)", outcomes.len());
                        }
                        Err(error) => *status = error,
                    }
                }
            }
            let active = event_runtime.active_at(current_time);
            ui.small(format!("{} active reactive occurrence(s)", active.len()));
            for invocation in active {
                ui.small(format!(
                    "{:?} at {:.3}s (local {:.3}s)",
                    invocation.action_id,
                    invocation.scheduled_at,
                    invocation.local_time(current_time)
                ));
            }
        });
    }
    let selected_instances: Vec<_> = selected.into_iter().flat_map(|id| project.attachments.values().filter(move |attachment| matches!(attachment.owner, library::model::authoring::AttachmentOwner::Item { item_id } if item_id == id))).collect();
    #[cfg(feature = "logic-editor")]
    if let Some(attachment) = selected_instances.first() {
        let instance = &project.module_instances[&attachment.module_instance_id];
        let definition = &project.module_definitions[&instance.definition_id];
        egui::CollapsingHeader::new("Node Graph")
            .default_open(true)
            .show(ui, |ui| {
                ui.small("This canvas edits only the selected reusable ModuleDefinition.");
                ui.allocate_ui(egui::vec2(ui.available_width(), 360.0), |ui| {
                    for intent in crate::logic_graph_ui::show(ui, definition, logic_graph) {
                        use crate::logic_graph_ui::LogicGraphEdit;
                        match intent {
                            LogicGraphEdit::MoveNode {
                                definition_id,
                                node_id,
                                position,
                                size,
                                collapsed,
                            } => edits.push(Edit::ModuleNodePresentation(
                                definition_id,
                                node_id,
                                position,
                                size,
                                collapsed,
                            )),
                            LogicGraphEdit::Connect {
                                definition_id,
                                from,
                                to,
                            } => edits.push(Edit::ConnectModulePorts(definition_id, from, to)),
                            LogicGraphEdit::Disconnect {
                                definition_id,
                                connection_id,
                            } => edits.push(Edit::DisconnectModuleConnection(
                                definition_id,
                                connection_id,
                            )),
                            LogicGraphEdit::DeleteNode {
                                definition_id,
                                node_id,
                            } => edits.push(Edit::RemoveModuleNode(definition_id, node_id)),
                        }
                    }
                });
            });
    }
    #[cfg(not(feature = "logic-editor"))]
    ui.small("This build omits the optional graphical Logic editor.");
    for attachment in selected_instances {
        let instance = &project.module_instances[&attachment.module_instance_id];
        let definition = &project.module_definitions[&instance.definition_id];
        egui::CollapsingHeader::new(&definition.name)
            .default_open(true)
            .show(ui, |ui| {
            let instance_count = project
                .module_instances
                .values()
                .filter(|candidate| candidate.definition_id == definition.id)
                .count();
            ui.colored_label(
                ui.visuals().warn_fg_color,
                format!(
                    "Editing shared ModuleDefinition — changes affect {instance_count} instance(s)."
                ),
            );
            ui.label(format!(
                "Role: {:?} · Version {}",
                definition.role, definition.version
            ));
            ui.label(format!("Internal nodes: {}", definition.graph.nodes.len()));
            ui.label(format!(
                "Published parameters: {}",
                definition.published_parameters.len()
            ));
            for action in &definition.published_actions {
                let exists = project.event_bindings.values().any(|binding| {
                    binding.target_action_id == action.id
                        && matches!(
                            &binding.scope,
                            BindingScope::Instance {
                                instance_path: target_path,
                                module_instance_id,
                            } if target_path == instance_path && *module_instance_id == instance.id
                        )
                });
                if ui
                    .add_enabled(
                        !exists,
                        egui::Button::new(format!("Add Marker Trigger: {}", action.name)),
                    )
                    .clicked()
                {
                    edits.push(Edit::AddEventBinding(EventBinding {
                        id: EventBindingId::new(),
                        source: EventSource::Marker {
                            name: format!("{}/{}", definition.name, action.name),
                        },
                        scope: BindingScope::Instance {
                            instance_path: instance_path.clone(),
                            module_instance_id: instance.id,
                        },
                        target_action_id: action.id,
                        trigger_policy: TriggerPolicy::Restart,
                        priority: 0,
                    }));
                }
            }
            if definition.role == library::model::authoring::ModuleRole::Effect {
                let mut effects = plugins.get_available_effects();
                effects.sort_by(|left, right| (&left.2, &left.1).cmp(&(&right.2, &right.1)));
                let add_operation = ui.menu_button("+ Add operation", |ui| {
                    let mut current_category = None;
                    for (effect_id, name, category) in &effects {
                        if current_category.as_ref() != Some(category) {
                            if current_category.is_some() {
                                ui.separator();
                            }
                            ui.strong(category);
                            current_category = Some(category.clone());
                        }
                        let option = ui.button(name);
                        crate::qa::register_component(
                            format!("logic.operation.option:{}:{effect_id}", definition.id),
                            "module_operation_option",
                            option.rect,
                            serde_json::json!({"effect_id": effect_id, "name": name, "category": category}),
                        );
                        if option.clicked() {
                            edits.push(Edit::AddModuleEffect(definition.id, effect_id.clone()));
                            ui.close();
                        }
                    }
                });
                crate::qa::register_component(
                    format!("logic.operation.add:{}", definition.id),
                    "module_operation_menu",
                    add_operation.response.rect,
                    serde_json::json!({"operation_count": effects.len()}),
                );
            }
            ui.collapsing("Advanced node details", |ui| {
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
                        if definition.output_node_id == Some(node.id) {
                            ui.strong("Module output");
                        } else if ui.small_button("Use as output").clicked() {
                            edits.push(Edit::SetModuleOutput(definition.id, node.id));
                        }
                        if name_changed || enabled_changed || bypass_changed {
                            edits.push(Edit::ModuleNodeState(
                                definition.id,
                                node.id,
                                name,
                                enabled,
                                bypassed,
                            ));
                        }
                        if ui.small_button("Delete node").clicked() {
                            edits.push(Edit::RemoveModuleNode(definition.id, node.id));
                        }
                    });
                }
                ui.collapsing(
                    format!("Connections ({})", definition.graph.connections.len()),
                    |ui| {
                    for connection in &definition.graph.connections {
                        let from = definition
                            .graph
                            .nodes
                            .get(&connection.from.node_id)
                            .map(|node| node.name.as_str())
                            .unwrap_or("Missing");
                        let to = definition
                            .graph
                            .nodes
                            .get(&connection.to.node_id)
                            .map(|node| node.name.as_str())
                            .unwrap_or("Missing");
                        ui.horizontal(|ui| {
                            ui.label(format!("{from} → {to}"));
                            if ui.small_button("Disconnect").clicked() {
                                edits.push(Edit::DisconnectModuleConnection(
                                    definition.id,
                                    connection.id,
                                ));
                            }
                        });
                    }
                    },
                );
            });
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

fn asset_from_path(
    path: &Path,
    plugins: &library::plugin::PluginManager,
) -> Result<Asset, library::LibraryError> {
    let path_string = path.to_string_lossy().into_owned();
    let metadata = plugins.get_metadata(&path_string)?;
    let kind = metadata
        .as_ref()
        .map_or_else(|| asset_kind(path), |value| value.kind.clone());
    let name = path
        .file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .into_owned();
    let mut asset = Asset::new(&name, &path_string, kind);
    if let Some(metadata) = metadata {
        asset.duration = metadata.duration;
        asset.width = metadata.width;
        asset.height = metadata.height;
        asset.fps = metadata.fps;
        asset.frame_count = metadata.frame_count;
        asset.stream_index = metadata.stream_index;
        asset.source_color.replace_detected(metadata.source_color);
    }
    if let Ok(bytes) = std::fs::read(path) {
        asset.verify_imported_content(&bytes);
    }
    Ok(asset)
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

    #[test]
    fn compact_property_control_adds_and_removes_the_current_keyframe() {
        let item = TimelineItemId::new();
        let value = property_vec2(12.0, 34.0);
        let constant = Property::constant(value.clone());
        assert!(matches!(
            keyframe_toggle_edit(item, "position", Some(&constant), 1.25, value.clone()),
            Edit::UpsertKeyframe(id, key, time, keyed_value)
                if id == item && key == "position" && time == 1.25 && keyed_value == value
        ));

        let keyframe = Keyframe::new(
            1.25,
            value.clone(),
            library::animation::EasingFunction::Linear,
        );
        let keyframe_id = keyframe.id;
        let animated = Property::keyframe(vec![keyframe]);
        assert!(matches!(
            keyframe_toggle_edit(item, "position", Some(&animated), 1.25, value),
            Edit::RemoveKeyframe(id, key, id_at_time)
                if id == item && key == "position" && id_at_time == keyframe_id
        ));
    }

    #[test]
    fn timeline_layer_expands_to_clip_rows_not_property_rows() {
        let project = AuthoringProject::new("Layers", 320, 180, 30.0, 10.0).expect("project");
        let timeline_id = project.root_timeline_id;
        let track_id = project.timelines[&timeline_id].track_order[0];
        let mut session =
            library::model::authoring::AuthoringSession::new(project).expect("session");
        for (name, start) in [("A", 0.0), ("B", 2.0)] {
            session
                .add_item(
                    track_id,
                    name.to_string(),
                    SourceRef::Solid {
                        color: Color::white(),
                    },
                    TimelineInterval::new(start, 1.0).expect("interval"),
                    0,
                )
                .expect("clip");
        }
        let project = session.into_project();
        let timeline = &project.timelines[&timeline_id];
        let collapsed = flatten_timeline_layers(&project, timeline, &HashSet::new());
        assert_eq!(collapsed.len(), 1);
        assert!(matches!(collapsed[0], TimelineDisplayRow::Layer { .. }));

        let expanded = flatten_timeline_layers(&project, timeline, &HashSet::from([track_id]));
        assert_eq!(expanded.len(), 3);
        assert!(matches!(expanded[0], TimelineDisplayRow::Layer { .. }));
        assert!(expanded[1..]
            .iter()
            .all(|row| matches!(row, TimelineDisplayRow::Clip { .. })));
    }

    #[test]
    fn imported_png_becomes_a_real_project_asset() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let path = directory.path().join("asset.png");
        image::RgbaImage::from_pixel(8, 6, image::Rgba([20, 80, 160, 255]))
            .save(&path)
            .expect("PNG fixture");
        let plugins = library::plugin::PluginManager::default();

        let asset = asset_from_path(&path, &plugins).expect("asset import");

        assert_eq!(asset.kind, AssetKind::Image);
        assert_eq!(asset.width, Some(8));
        assert_eq!(asset.height, Some(6));
        assert!(asset.imported_content_sha256().is_some());
    }

    #[test]
    fn audio_tempo_chain_covers_extreme_playback_rates() {
        assert_eq!(atempo_filter(1.0), "atempo=1");
        assert_eq!(atempo_filter(0.25), "atempo=0.5,atempo=0.5");
        assert_eq!(atempo_filter(200.0), "atempo=100,atempo=2");
    }

    #[test]
    fn timeline_audio_mix_produces_runtime_pcm() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let source = directory.path().join("tone.wav");
        let status = std::process::Command::new("ffmpeg")
            .args(["-hide_banner", "-loglevel", "error", "-f", "lavfi", "-i"])
            .arg("sine=frequency=440:duration=0.2")
            .args(["-y"])
            .arg(&source)
            .status()
            .expect("FFmpeg fixture");
        assert!(status.success());
        let mut project = AuthoringProject::new("Audio", 320, 180, 30.0, 1.0).expect("project");
        let track_id = project.timelines[&project.root_timeline_id].track_order[0];
        let asset = Asset::new("Tone", &source.to_string_lossy(), AssetKind::Audio);
        let asset_id = asset.id;
        project.assets.push(asset);
        let mut session =
            library::model::authoring::AuthoringSession::new(project).expect("session");
        session
            .add_item(
                track_id,
                "Tone".to_string(),
                SourceRef::Asset {
                    asset_id,
                    time_map: Default::default(),
                },
                TimelineInterval::new(0.1, 0.2).expect("interval"),
                0,
            )
            .expect("audio item");
        let project = session.into_project();
        let output = render_timeline_audio(&project, project.root_timeline_id)
            .expect("audio mix")
            .expect("PCM output");
        assert!(std::fs::metadata(&output).expect("PCM metadata").len() > 0);
        std::fs::remove_file(output).expect("remove PCM");
    }
}
