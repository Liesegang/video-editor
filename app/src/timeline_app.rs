use std::path::Path;
use std::sync::Arc;

use eframe::egui;
use egui_dock::{DockArea, DockState, NodeIndex, Style, TabViewer};
use library::model::authoring::{
    AuthoringProject, DurationPolicy, ModuleInstanceId, PublishedParameterId, SourceRef,
    TimelineId, TimelineInterval, TimelineItemId,
};
use library::model::frame::color::Color;
use library::model::project::asset::{Asset, AssetKind};
use library::model::project::property::{Property, PropertyValue, Vec2};
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
    OpenTimeline(TimelineId),
    Rename(TimelineItemId, String),
    SetText(TimelineItemId, String),
    Move(TimelineItemId, f64, i64),
    Trim(TimelineItemId, TimelineInterval),
    Property(TimelineItemId, String, PropertyValue),
    Split(TimelineItemId),
    Blur(TimelineItemId),
    ModuleParameter(ModuleInstanceId, PublishedParameterId, PropertyValue),
}

pub struct TimelineApp {
    editor: TimelineEditorService,
    plugins: Arc<library::plugin::PluginManager>,
    renderer: RenderService<SkiaRenderer>,
    dock: DockState<Tab>,
    workspace: Workspace,
    open_timeline: TimelineId,
    selected_item: Option<TimelineItemId>,
    current_time: f64,
    preview: Option<egui::TextureHandle>,
    preview_key: Option<(library::model::authoring::ProjectRevision, TimelineId, u64)>,
    undo: Vec<AuthoringProject>,
    redo: Vec<AuthoringProject>,
    status: String,
}

impl TimelineApp {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Result<Self, library::LibraryError> {
        egui_extras::install_image_loaders(&cc.egui_ctx);
        let editor = TimelineEditorService::create_default("Untitled")?;
        let open_timeline = editor.snapshot()?.root_timeline_id;
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
            selected_item: None,
            current_time: 0.0,
            preview: None,
            preview_key: None,
            undo: Vec::new(),
            redo: Vec::new(),
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
                self.selected_item = None;
                self.current_time = 0.0;
                self.undo.clear();
                self.redo.clear();
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
                self.selected_item = None;
                self.current_time = 0.0;
                self.undo.clear();
                self.redo.clear();
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
    }

    fn undo(&mut self) {
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
            let track = project.timelines[&self.open_timeline].track_order[0];
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
            let track = project.timelines[&self.open_timeline].track_order[0];
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
            let track = project.timelines[&self.open_timeline].track_order[0];
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
                let track = project.timelines[&self.open_timeline].track_order[0];
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
        let before = self
            .editor
            .snapshot()
            .ok()
            .map(|project| project.as_ref().clone());
        let result = match edit {
            Edit::Select(item) => {
                self.selected_item = item;
                return;
            }
            Edit::OpenTimeline(id) => {
                self.open_timeline = id;
                self.selected_item = None;
                self.current_time = 0.0;
                self.invalidate_preview();
                return;
            }
            Edit::Rename(id, value) => self.editor.rename_item(id, value).map(|_| ()),
            Edit::SetText(id, value) => self.editor.set_text(id, value).map(|_| ()),
            Edit::Move(id, start, layer) => self
                .editor
                .move_item(
                    id,
                    self.editor.snapshot().unwrap().items[&id].track_id,
                    start,
                    layer,
                )
                .map(|_| ()),
            Edit::Trim(id, interval) => self.editor.trim_item(id, interval).map(|_| ()),
            Edit::Property(id, key, value) => self
                .editor
                .update_item_property_value(id, key, self.current_time, value)
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
        };
        match result {
            Ok(()) => {
                if let Some(before) = before {
                    self.record(before);
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
                if ui.button("Split").clicked() {
                    if let Some(id) = self.selected_item {
                        self.apply(Edit::Split(id));
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
                self.edits,
            ),
            Tab::Timeline => timeline_ui(
                ui,
                self.project,
                self.open_timeline,
                self.selected_item,
                self.current_time,
                self.edits,
            ),
            Tab::Inspector => inspector_ui(ui, self.project, self.selected_item, self.edits),
            Tab::Assets => assets_ui(ui, self.project),
            Tab::Motion => motion_ui(ui, self.project, self.selected_item),
            Tab::Data => data_ui(ui, self.project),
            Tab::Logic => logic_ui(ui, self.project, self.selected_item),
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
                    let (x, y) = vec2_property(item, "position", (0.0, 0.0));
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
    selected: Option<TimelineItemId>,
    time: &mut f64,
    edits: &mut Vec<Edit>,
) {
    let timeline = &project.timelines[&timeline_id];
    ui.horizontal(|ui| {
        if timeline_id != project.root_timeline_id && ui.button("← Main").clicked() {
            edits.push(Edit::OpenTimeline(project.root_timeline_id));
        }
        ui.heading(&timeline.name);
        ui.add(egui::Slider::new(time, 0.0..=timeline.duration.into_inner()).text("time"));
    });
    egui::ScrollArea::both().show(ui, |ui| {
        for track_id in timeline.track_order.iter().rev() {
            let track = &project.tracks[track_id];
            ui.horizontal(|ui| {
                ui.label(format!("{}", track.name));
                let mut items: Vec<_> = project
                    .items
                    .values()
                    .filter(|item| item.track_id == *track_id)
                    .collect();
                items.sort_by_key(|item| (item.interval.start, item.layer));
                for item in items {
                    let response = ui.selectable_label(
                        selected == Some(item.id),
                        format!(
                            "{}  {:.2}–{:.2}",
                            item.name,
                            item.interval.start,
                            item.interval.start + item.interval.duration
                        ),
                    );
                    if response.clicked() {
                        edits.push(Edit::Select(Some(item.id)));
                    }
                    if response.double_clicked() {
                        if let SourceRef::Composition(instance) = &item.source {
                            edits.push(Edit::OpenTimeline(instance.timeline_id));
                        }
                    }
                }
            });
            ui.separator();
        }
    });
}

fn inspector_ui(
    ui: &mut egui::Ui,
    project: &AuthoringProject,
    selected: Option<TimelineItemId>,
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
        edits.push(Edit::Move(id, start.max(0.0), layer));
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
        edits.push(Edit::Move(id, start, layer));
    }
    ui.separator();
    ui.heading("Transform");
    let (mut x, mut y) = vec2_property(item, "position", (0.0, 0.0));
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
    let mut opacity = number_property(item, "opacity", 1.0);
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

fn motion_ui(ui: &mut egui::Ui, project: &AuthoringProject, selected: Option<TimelineItemId>) {
    ui.heading("Timeline-owned animation");
    let Some(item) = selected.and_then(|id| project.items.get(&id)) else {
        ui.label("Select an item");
        return;
    };
    for (name, property) in item.authored_properties.iter() {
        ui.collapsing(name, |ui| {
            if property.evaluator == "keyframe" {
                for key in property.keyframes() {
                    ui.label(format!("{:.3}s  {:?}", key.time, key.value));
                }
            } else {
                ui.label(format!("Base: {:?}", property.value()));
            }
        });
    }
}

fn data_ui(ui: &mut egui::Ui, project: &AuthoringProject) {
    ui.heading("Data and generated items");
    ui.label(format!("Data sources: {}", project.data_sources.len()));
    ui.label(format!(
        "Generated items: {}",
        project.generated_items.len()
    ));
    ui.label(format!("Overrides: {}", project.overrides.len()));
    ui.small("Stable provenance keeps manual corrections across regeneration.");
}

fn logic_ui(ui: &mut egui::Ui, project: &AuthoringProject, selected: Option<TimelineItemId>) {
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

fn vec2_property(
    item: &library::model::authoring::TimelineItem,
    name: &str,
    default: (f64, f64),
) -> (f64, f64) {
    match item.authored_properties.get(name).and_then(Property::value) {
        Some(PropertyValue::Vec2(value)) => (value.x.into_inner(), value.y.into_inner()),
        _ => default,
    }
}

fn number_property(
    item: &library::model::authoring::TimelineItem,
    name: &str,
    default: f64,
) -> f64 {
    match item.authored_properties.get(name).and_then(Property::value) {
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
}
