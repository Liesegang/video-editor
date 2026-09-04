use eframe::egui;
use log::error;
use std::collections::HashMap;
use std::sync::mpsc::{Receiver, channel};
use std::sync::{Arc, RwLock};
use std::thread::{self, JoinHandle};

use library::cache::SharedCacheManager;
use library::editor::render_service::RenderService;
// use library::framing::entity_converters::EntityConverterRegistry;
use library::model::project::Project;
use library::model::property::{PropertyUiType, PropertyValue};
use library::plugin::{ExportSettings, PluginManager};
use library::rendering::skia_renderer::SkiaRenderer;
use library::{EditorService, ExportService, ProjectModel};

use super::export_audio_temp::ExportAudioTempFile;
use crate::utils::lock::read_or_recover;

enum ExportUpdate {
    Progress(f32),
    Complete,
    Cancelled,
    Failed(String),
}

pub struct ExportDialog {
    pub is_open: bool,
    selected_exporter_id: Option<String>,
    pub property_values: HashMap<String, PropertyValue>,
    output_path: String,

    // Dependencies
    plugin_manager: Arc<PluginManager>,
    cache_manager: SharedCacheManager,

    // Export state
    is_exporting: bool,
    progress: f32,
    status_message: String,
    progress_rx: Option<Receiver<ExportUpdate>>,
    pub cancellation_token: Option<Arc<std::sync::atomic::AtomicBool>>,
    export_worker: Option<JoinHandle<()>>,

    // New Fields
    pub active_composition_id: Option<uuid::Uuid>, // Targeted composition
    pub export_range: ExportRange,
    pub custom_start_frame: u64,
    pub custom_end_frame: u64,

    // Overrides
    pub override_width: Option<u32>,
    pub override_height: Option<u32>,
    pub override_fps: Option<f64>,
}

#[derive(PartialEq, Clone, Copy, Debug)]
pub enum ExportRange {
    EntireComposition,
    WorkArea,
    Custom,
}

impl ExportDialog {
    pub fn new(plugin_manager: Arc<PluginManager>, cache_manager: SharedCacheManager) -> Self {
        Self {
            is_open: false,
            selected_exporter_id: None,
            property_values: HashMap::new(),
            output_path: "output".to_string(),
            plugin_manager,
            cache_manager,
            is_exporting: false,
            progress: 0.0,
            status_message: String::new(),
            progress_rx: None,
            cancellation_token: None,
            export_worker: None,
            active_composition_id: None,
            export_range: ExportRange::EntireComposition,
            custom_start_frame: 0,
            custom_end_frame: 0,
            override_width: None,
            override_height: None,
            override_fps: None,
        }
    }

    pub fn open(&mut self) {
        self.is_open = true;
    }

    fn poll_export_updates(&mut self) {
        if !self.is_exporting && self.export_worker.is_none() {
            return;
        }

        let mut latest_progress = None;
        let mut terminal_update = None;
        if let Some(rx) = &self.progress_rx {
            while let Ok(update) = rx.try_recv() {
                match update {
                    ExportUpdate::Progress(progress) => latest_progress = Some(progress),
                    terminal @ (ExportUpdate::Complete
                    | ExportUpdate::Cancelled
                    | ExportUpdate::Failed(_)) => terminal_update = Some(terminal),
                }
            }
        }

        if let Some(progress) = latest_progress {
            self.progress = progress;
        }
        if let Some(update) = terminal_update {
            self.finish_export_job(update);
        } else if self
            .export_worker
            .as_ref()
            .is_some_and(JoinHandle::is_finished)
        {
            let worker_panicked = self
                .export_worker
                .take()
                .is_some_and(|handle| handle.join().is_err());
            self.is_exporting = false;
            self.status_message = if worker_panicked {
                "Export failed because the worker panicked.".to_string()
            } else {
                "Export failed because the worker ended without a terminal status.".to_string()
            };
            self.progress_rx = None;
            self.cancellation_token = None;
        }
    }

    fn finish_export_job(&mut self, update: ExportUpdate) {
        let worker_panicked = self
            .export_worker
            .take()
            .is_some_and(|handle| handle.join().is_err());
        self.is_exporting = false;
        self.status_message = if worker_panicked {
            "Export failed because the worker panicked.".to_string()
        } else {
            match update {
                ExportUpdate::Complete => "Export complete!".to_string(),
                ExportUpdate::Cancelled => "Cancelled.".to_string(),
                ExportUpdate::Failed(message) => message,
                ExportUpdate::Progress(_) => {
                    "Export failed because an invalid terminal status was received.".to_string()
                }
            }
        };
        self.progress_rx = None;
        self.cancellation_token = None;
    }

    fn request_cancel(&mut self) {
        if let Some(token) = &self.cancellation_token {
            token.store(true, std::sync::atomic::Ordering::Relaxed);
            self.status_message = "Cancelling; finalizing active output...".to_string();
        }
    }

    fn can_start_export(&self) -> bool {
        !self.is_exporting && self.export_worker.is_none()
    }

    pub fn show(
        &mut self,
        ctx: &egui::Context,
        project: &Arc<RwLock<Project>>,
        project_service: &EditorService,
        active_composition_id: Option<uuid::Uuid>,
    ) {
        self.active_composition_id = active_composition_id;
        let mut is_open = self.is_open;

        self.poll_export_updates();

        let result = crate::ui::widgets::modal::Modal::new("Export")
            .open(&mut is_open)
            .collapsible(false)
            .resizable(true)
            .default_width(400.0)
            .show(ctx, |ui| {
                let mut should_close = false;
                if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                    if self.is_exporting {
                        self.request_cancel();
                    } else {
                        should_close = true;
                    }
                }

                if self.is_exporting {
                    self.show_export_progress(ui);
                } else {
                    if self.show_configuration(ui, project, project_service) {
                        should_close = true;
                    }
                }
                should_close
            });

        if let Some(inner) = result {
            if inner.inner.unwrap_or(false) {
                is_open = false;
            }
        }

        self.is_open = is_open;
    }

    fn show_export_progress(&mut self, ui: &mut egui::Ui) {
        ui.heading("Exporting...");
        ui.add(egui::ProgressBar::new(self.progress).show_percentage());
        ui.label(&self.status_message);
        ui.spinner();

        let cancelling = self
            .cancellation_token
            .as_ref()
            .is_some_and(|token| token.load(std::sync::atomic::Ordering::Relaxed));
        if ui
            .add_enabled(!cancelling, egui::Button::new("Cancel"))
            .clicked()
        {
            self.request_cancel();
        }
    }

    fn show_configuration(
        &mut self,
        ui: &mut egui::Ui,
        project: &Arc<RwLock<Project>>,
        project_service: &EditorService,
    ) -> bool {
        let mut close_dialog = false;
        ui.heading("Export Settings");
        if !self.status_message.is_empty() {
            let color = if self.status_message.starts_with("Export failed") {
                ui.visuals().error_fg_color
            } else {
                ui.visuals().text_color()
            };
            ui.colored_label(color, &self.status_message);
        }

        // 1. Composition Selection
        ui.horizontal(|ui| {
            ui.label("Composition:");
            let project_read = read_or_recover(project.as_ref());
            let current_comp_name = self
                .active_composition_id
                .and_then(|id| project_read.compositions.iter().find(|c| c.id == id))
                .map(|c| c.name.clone())
                .unwrap_or_else(|| "Select...".to_string());

            egui::ComboBox::from_id_salt("comp_select")
                .selected_text(current_comp_name)
                .show_ui(ui, |ui| {
                    for comp in &project_read.compositions {
                        if ui
                            .selectable_label(
                                self.active_composition_id == Some(comp.id),
                                &comp.name,
                            )
                            .clicked()
                        {
                            self.active_composition_id = Some(comp.id);
                        }
                    }
                });
        });

        // 2. Exporter Selection
        ui.horizontal(|ui| {
            ui.label("Exporter:");
            let known_exporters = self.plugin_manager.get_available_exporters();
            let current_selection_name = self
                .selected_exporter_id
                .as_ref()
                .and_then(|id| {
                    known_exporters
                        .iter()
                        .find(|(e_id, _)| e_id == id)
                        .map(|(_, name)| name.clone())
                })
                .unwrap_or_else(|| "Select...".to_string());

            egui::ComboBox::from_id_salt("exporter_select")
                .selected_text(current_selection_name)
                .show_ui(ui, |ui| {
                    for (id, name) in known_exporters {
                        if ui
                            .selectable_label(
                                self.selected_exporter_id.as_deref() == Some(&id),
                                &name,
                            )
                            .clicked()
                        {
                            self.selected_exporter_id = Some(id.clone());
                            self.property_values.clear();
                            if let Ok(()) = self.load_defaults(&id) {
                                // Defaults loaded
                            }
                        }
                    }
                });
        });

        // 3. Render Settings
        ui.separator();
        ui.heading("Render Settings");
        ui.horizontal(|ui| {
            ui.label("Range:");
            egui::ComboBox::from_id_salt("export_range")
                .selected_text(format!("{:?}", self.export_range))
                .show_ui(ui, |ui| {
                    ui.selectable_value(
                        &mut self.export_range,
                        ExportRange::EntireComposition,
                        "Entire Composition",
                    );
                    ui.selectable_value(&mut self.export_range, ExportRange::WorkArea, "Work Area");
                    ui.selectable_value(&mut self.export_range, ExportRange::Custom, "Custom");
                });
        });

        if self.export_range == ExportRange::Custom {
            ui.horizontal(|ui| {
                ui.label("Start Frame:");
                ui.add(egui::DragValue::new(&mut self.custom_start_frame));
                ui.label("End Frame:");
                ui.add(egui::DragValue::new(&mut self.custom_end_frame));
            });
        } else {
            // Show info about selected range
            let project_read = read_or_recover(project.as_ref());
            if let Some(comp_id) = self.active_composition_id {
                if let Some(comp) = project_read.compositions.iter().find(|c| c.id == comp_id) {
                    let (start, end) = match self.export_range {
                        ExportRange::EntireComposition => {
                            (0, (comp.duration * comp.fps).ceil() as u64)
                        }
                        ExportRange::WorkArea => (comp.work_area_in, comp.work_area_out),
                        _ => (0, 0),
                    };
                    ui.label(format!(
                        "Frames: {} to {} (Duration: {})",
                        start,
                        end,
                        end.saturating_sub(start)
                    ));
                }
            }
        }

        ui.separator();
        ui.heading("Video Settings");
        ui.horizontal(|ui| {
            let mut override_res = self.override_width.is_some() || self.override_height.is_some();
            if ui
                .checkbox(&mut override_res, "Override Resolution")
                .changed()
            {
                if override_res {
                    // Initialize with current composition limits if available, or valid defaults
                    if let Some(comp_id) = self.active_composition_id {
                        let project_read = read_or_recover(project.as_ref());
                        if let Some(comp) =
                            project_read.compositions.iter().find(|c| c.id == comp_id)
                        {
                            self.override_width = Some(comp.width as u32);
                            self.override_height = Some(comp.height as u32);
                        } else {
                            self.override_width = Some(1920);
                            self.override_height = Some(1080);
                        }
                    } else {
                        self.override_width = Some(1920);
                        self.override_height = Some(1080);
                    }
                } else {
                    self.override_width = None;
                    self.override_height = None;
                }
            }
            if override_res {
                if let Some(w) = &mut self.override_width {
                    ui.add(egui::DragValue::new(w).prefix("W: "));
                }
                if let Some(h) = &mut self.override_height {
                    ui.add(egui::DragValue::new(h).prefix("H: "));
                }
            }
        });
        ui.horizontal(|ui| {
            let mut override_fps = self.override_fps.is_some();
            if ui.checkbox(&mut override_fps, "Override FPS").changed() {
                if override_fps {
                    if let Some(comp_id) = self.active_composition_id {
                        let project_read = read_or_recover(project.as_ref());
                        if let Some(comp) =
                            project_read.compositions.iter().find(|c| c.id == comp_id)
                        {
                            self.override_fps = Some(comp.fps);
                        } else {
                            self.override_fps = Some(30.0);
                        }
                    } else {
                        self.override_fps = Some(30.0);
                    }
                } else {
                    self.override_fps = None;
                }
            }
            if let Some(fps) = &mut self.override_fps {
                ui.add(egui::DragValue::new(fps).speed(0.1));
            }
        });

        ui.separator();

        // 4. Output Path
        ui.horizontal(|ui| {
            ui.label("Output Path:");
            ui.text_edit_singleline(&mut self.output_path);
            if ui.button("Browse...").clicked() {
                let dialog = rfd::FileDialog::new().set_file_name(&self.output_path);
                if let Some(path) = dialog.save_file() {
                    let path_str = path.display().to_string();
                    self.output_path = path_str;
                }
            }
        });

        ui.separator();

        // 5. Properties
        if let Some(exporter_id) = &self.selected_exporter_id {
            if let Some(definitions) = self
                .plugin_manager
                .get_export_plugin_properties(exporter_id)
            {
                egui::Grid::new("export_properties")
                    .num_columns(2)
                    .show(ui, |ui| {
                        for def in definitions {
                            ui.label(def.label());

                            let value = self
                                .property_values
                                .entry(def.name().to_string())
                                .or_insert(def.default_value().clone());

                            match def.ui_type() {
                                PropertyUiType::Text | PropertyUiType::MultilineText => {
                                    if let PropertyValue::String(s) = value {
                                        ui.text_edit_singleline(s);
                                    }
                                }
                                PropertyUiType::Integer {
                                    min, max, suffix, ..
                                } => {
                                    if let PropertyValue::Number(n) = value {
                                        let mut v = n.0 as i64;
                                        if ui
                                            .add(
                                                egui::Slider::new(&mut v, *min..=*max).text(suffix),
                                            )
                                            .changed()
                                        {
                                            *n = ordered_float::OrderedFloat(v as f64);
                                        }
                                    } else if let PropertyValue::String(s) = value {
                                        // Handle stringified number default
                                        if let Ok(mut v) = s.parse::<i64>() {
                                            if ui
                                                .add(
                                                    egui::Slider::new(&mut v, *min..=*max)
                                                        .text(suffix),
                                                )
                                                .changed()
                                            {
                                                *value = PropertyValue::Number(
                                                    ordered_float::OrderedFloat(v as f64),
                                                );
                                            }
                                        }
                                    }
                                }
                                PropertyUiType::Float {
                                    min,
                                    max,
                                    step,
                                    suffix,
                                    ..
                                } => {
                                    if let PropertyValue::Number(n) = value {
                                        let mut v = n.0;
                                        if ui
                                            .add(
                                                egui::Slider::new(&mut v, *min..=*max)
                                                    .step_by(*step)
                                                    .text(suffix),
                                            )
                                            .changed()
                                        {
                                            *n = ordered_float::OrderedFloat(v);
                                        }
                                    }
                                }
                                PropertyUiType::Bool => {
                                    if let PropertyValue::Boolean(b) = value {
                                        if ui.checkbox(b, def.name()).changed() {
                                            // Update
                                        }
                                    }
                                }
                                PropertyUiType::Dropdown { options } => {
                                    if let PropertyValue::String(s) = value {
                                        egui::ComboBox::from_id_salt(def.name())
                                            .selected_text(s.clone())
                                            .show_ui(ui, |ui| {
                                                for opt in options {
                                                    ui.selectable_value(s, opt.clone(), opt);
                                                }
                                            });
                                    }
                                }
                                _ => {
                                    ui.label("Unsupported type");
                                }
                            }
                            ui.end_row();
                        }
                    });
            }
        }

        super::dialog_footer(ui, |ui| {
            let enabled = self.selected_exporter_id.is_some() && !self.output_path.is_empty();

            if ui
                .add_enabled(enabled, egui::Button::new("Export"))
                .clicked()
            {
                self.start_export(project, project_service);
            }

            if ui.button("Close").clicked() {
                close_dialog = true;
            }
        });

        close_dialog
    }

    fn load_defaults(&mut self, exporter_id: &str) -> Result<(), ()> {
        if let Some(defs) = self
            .plugin_manager
            .get_export_plugin_properties(exporter_id)
        {
            for def in defs {
                self.property_values
                    .insert(def.name().to_string(), def.default_value().clone());
            }
        }
        Ok(())
    }

    fn start_export(
        &mut self,
        project_lock: &Arc<RwLock<Project>>,
        project_service: &EditorService,
    ) {
        if !self.can_start_export() {
            self.status_message =
                "The previous export is still cancelling or finalizing.".to_string();
            return;
        }
        let exporter_id = if let Some(id) = &self.selected_exporter_id {
            id.clone()
        } else {
            return;
        };

        let target_comp_id = if let Some(id) = self.active_composition_id {
            id
        } else {
            self.status_message = "No active composition selected.".to_string();
            return;
        };

        self.is_exporting = true;
        self.status_message = "Starting export...".to_string();
        self.progress = 0.0;

        // Prepare data for thread
        let project_snapshot = Arc::new(read_or_recover(project_lock.as_ref()).clone());
        let exporter_id_owned = exporter_id;
        let output_path_owned = self.output_path.clone();
        let property_values_owned = self.property_values.clone();
        let plugin_manager = self.plugin_manager.clone();
        let cache_manager = self.cache_manager.clone();

        let export_range = self.export_range;
        let custom_start = self.custom_start_frame;
        let custom_end = self.custom_end_frame;

        let override_width = self.override_width;
        let override_height = self.override_height;
        let override_fps = self.override_fps;

        // Capture Audio Engine Sample Rate
        let engine_sample_rate = project_service.get_audio_engine().get_sample_rate();

        // Find composition index
        let comp_index = match project_snapshot
            .compositions
            .iter()
            .position(|c| c.id == target_comp_id)
        {
            Some(idx) => idx,
            None => {
                self.status_message = "Composition not found.".to_string();
                self.is_exporting = false;
                return;
            }
        };

        let (tx, rx) = channel();
        self.progress_rx = Some(rx);

        let cancel_token = Arc::new(std::sync::atomic::AtomicBool::new(false));
        self.cancellation_token = Some(cancel_token.clone());

        self.export_worker = Some(thread::spawn(move || {
            // Initialize Renderer inside thread (requires context)
            let composition = &project_snapshot.compositions[comp_index];
            let renderer = match SkiaRenderer::new(
                composition.width as u32,
                composition.height as u32,
                composition.background_color.clone(),
                false,
                None,
                Some(cache_manager.clone()),
            ) {
                Ok(renderer) => renderer,
                Err(error) => {
                    let message = format!(
                        "Export failed to initialize renderer: {error}. No output was written."
                    );
                    error!("{message}");
                    if tx.send(ExportUpdate::Failed(message)).is_err() {
                        log::debug!("export progress receiver was dropped");
                    }
                    return;
                }
            };

            let render_service_plugin_manager = plugin_manager.clone();
            let mut render_service = RenderService::new(
                renderer,
                render_service_plugin_manager,
                cache_manager.clone(),
            );

            // Construct ProjectModel
            let project_model = match ProjectModel::new(Arc::clone(&project_snapshot), comp_index) {
                Ok(pm) => pm,
                Err(e) => {
                    let message = format!(
                        "Export failed to create project model: {e}. No output was written."
                    );
                    error!("{message}");
                    if tx.send(ExportUpdate::Failed(message)).is_err() {
                        log::debug!("export progress receiver was dropped");
                    }
                    return;
                }
            };

            // Build ExportSettings
            let mut settings = match ExportSettings::from_project(&project_snapshot, composition) {
                Ok(settings) => settings,
                Err(error) => {
                    let message = format!(
                        "Export cannot establish Project color authority: {error}. No output was written."
                    );
                    error!("{message}");
                    if tx.send(ExportUpdate::Failed(message)).is_err() {
                        log::debug!("export progress receiver was dropped");
                    }
                    return;
                }
            };
            settings.width = override_width.unwrap_or(composition.width as u32);
            settings.height = override_height.unwrap_or(composition.height as u32);
            settings.fps = override_fps.unwrap_or(composition.fps);

            // Map properties
            let mut json_params = HashMap::new();
            for (k, v) in &property_values_owned {
                let json_val = match v {
                    PropertyValue::String(s) => serde_json::Value::String(s.clone()),
                    PropertyValue::Number(n) => serde_json::Number::from_f64(n.0)
                        .map_or(serde_json::Value::Null, serde_json::Value::Number),
                    PropertyValue::Boolean(b) => serde_json::Value::Bool(*b),
                    _ => serde_json::Value::Null,
                };
                json_params.insert(k.clone(), json_val);
            }
            settings.parameters = json_params;
            settings.container = match property_values_owned.get("container") {
                Some(PropertyValue::String(s)) => s.clone(),
                _ => {
                    if exporter_id_owned == "png_export" {
                        "png".to_string()
                    } else {
                        "mp4".to_string()
                    }
                }
            };

            settings.codec = match property_values_owned.get("codec") {
                Some(PropertyValue::String(s)) => s.clone(),
                _ => {
                    if exporter_id_owned == "png_export" {
                        "png".to_string()
                    } else {
                        "libx264".to_string()
                    }
                }
            };

            settings.pixel_format = match property_values_owned.get("pixel_format") {
                Some(PropertyValue::String(s)) => s.clone(),
                _ if exporter_id_owned == "png_export" => "rgba".to_string(),
                _ => "yuv420p".to_string(),
            };

            // Range inputs are authored in composition frames. Convert them
            // once to output-frame indices so video sampling, duration, audio,
            // and encoder timestamps all share ExportSettings::fps.
            let output_range = match export_range {
                ExportRange::EntireComposition => settings
                    .frame_count_for_duration(composition.duration)
                    .map(|end| 0..end),
                ExportRange::WorkArea => settings.resample_timeline_frame_range(
                    composition.work_area_in..composition.work_area_out,
                    composition.fps,
                ),
                ExportRange::Custom => settings
                    .resample_timeline_frame_range(custom_start..custom_end, composition.fps),
            };
            let output_range = match output_range {
                Ok(range) => range,
                Err(error) => {
                    let message = format!("Export frame-rate/range is invalid: {error}");
                    error!("{message}");
                    if tx.send(ExportUpdate::Failed(message)).is_err() {
                        log::debug!("export progress receiver was dropped");
                    }
                    return;
                }
            };
            let output_range =
                match settings.frame_range_within_duration(output_range, composition.duration) {
                    Ok(range) => range,
                    Err(error) => {
                        let message = format!("Export range is empty or out of bounds: {error}");
                        error!("{message}");
                        if tx.send(ExportUpdate::Failed(message)).is_err() {
                            log::debug!("export progress receiver was dropped");
                        }
                        return;
                    }
                };
            let (start_frame, end_frame_total) = (output_range.start, output_range.end);
            let duration_frames = end_frame_total - start_frame;

            // Resolve and verify the complete selected destination set before
            // audio rendering, temporary-file creation, save-worker startup,
            // frame rendering, or any exporter callback.
            let mut stem_path = std::path::PathBuf::from(&output_path_owned);
            if stem_path.extension().is_some() {
                stem_path.set_extension("");
            }
            let stem_str = stem_path.to_str().unwrap_or("output");
            let verified_plan =
                match ExportService::verify_plan(&project_model, &settings, output_range, stem_str)
                {
                    Ok(plan) => plan,
                    Err(error) => {
                        let message =
                            format!("Export destination is unsafe or cannot be verified: {error}");
                        error!("{message}");
                        if tx.send(ExportUpdate::Failed(message)).is_err() {
                            log::debug!("export progress receiver was dropped");
                        }
                        return;
                    }
                };
            let output_container = settings.container.clone();

            // Construct absolute final path for finish_export key
            let final_output_path = if output_container.is_empty() {
                output_path_owned.clone()
            } else {
                format!("{}.{}", stem_str, output_container)
            };

            // Audio Pre-rendering
            let mut audio_temp_file: Option<ExportAudioTempFile> = None;
            if matches!(
                settings.export_format(),
                library::plugin::ExportFormat::Video
            ) {
                let fps = settings.fps;
                let start_time = start_frame as f64 / fps;
                let duration = duration_frames as f64 / fps;
                let sample_rate = engine_sample_rate; // Use correct rate
                let start_sample = (start_time * sample_rate as f64).round() as u64;
                let frames = (duration * sample_rate as f64).round() as usize;

                let audio_data = library::audio::mixer::render_samples(
                    &project_model.project().assets,
                    project_model.project(),
                    project_model.composition(),
                    &cache_manager,
                    start_sample,
                    frames,
                    sample_rate,
                    2,
                    plugin_manager.as_ref(),
                );

                if !audio_data.is_empty() {
                    match ExportAudioTempFile::from_samples(&audio_data) {
                        Ok(temp_file) => {
                            if let Err(error) = settings.bind_runtime_audio_source(
                                temp_file.path_string().to_string(),
                                2,
                                sample_rate,
                            ) {
                                let message = format!(
                                    "Export failed to bind temporary audio safely: {error}"
                                );
                                error!("{message}");
                                if tx.send(ExportUpdate::Failed(message)).is_err() {
                                    log::debug!("export progress receiver was dropped");
                                }
                                return;
                            }
                            audio_temp_file = Some(temp_file);
                        }
                        Err(error) => {
                            let message = format!(
                                "Export failed to create a safe temporary audio file: {error}"
                            );
                            error!("{message}");
                            if tx.send(ExportUpdate::Failed(message)).is_err() {
                                log::debug!("export progress receiver was dropped");
                            }
                            return;
                        }
                    }
                }
            }

            let settings_arc = Arc::new(settings);

            let mut export_service = match ExportService::new(
                plugin_manager.clone(),
                exporter_id_owned.clone(),
                settings_arc,
                verified_plan,
                4,
            ) {
                Ok(service) => service,
                Err(error) => {
                    let message =
                        format!("Export settings changed after destination verification: {error}");
                    error!("{message}");
                    if tx.send(ExportUpdate::Failed(message)).is_err() {
                        log::debug!("export progress receiver was dropped");
                    }
                    return;
                }
            };

            // Cancellation is observed between frames. A one-frame chunk
            // prevents an expensive renderer from running nine extra frames
            // after the user has requested cancellation.
            let chunk_size = 1;
            let mut current_frame = start_frame;
            let mut export_error = None;
            let mut receiver_dropped = false;
            let mut cancelled = false;

            while current_frame < end_frame_total {
                if cancel_token.load(std::sync::atomic::Ordering::Relaxed) {
                    cancelled = true;
                    break;
                }

                let end = (current_frame + chunk_size).min(end_frame_total);
                let range = current_frame..end;

                if let Err(error) =
                    export_service.render_range(&mut render_service, &project_model, range)
                {
                    let message = format!(
                        "Export failed while rendering: {error}. Partial output may remain at {final_output_path}."
                    );
                    error!("{message}");
                    export_error = Some(message);
                    break;
                }

                current_frame = end;
                let pct =
                    (current_frame.saturating_sub(start_frame)) as f32 / duration_frames as f32;
                if tx.send(ExportUpdate::Progress(pct)).is_err() {
                    log::debug!("export progress receiver was dropped");
                    receiver_dropped = true;
                    break;
                }
            }

            let shutdown_result = if cancelled {
                export_service.cancel()
            } else {
                export_service.shutdown()
            };
            if let Err(error) = shutdown_result {
                error!("Failed to shut down export workers: {error}");
                export_error.get_or_insert_with(|| {
                    format!(
                        "Export failed while shutting down workers: {error}. Partial output may remain at {final_output_path}."
                    )
                });
            }

            // Keep the path alive until every exporter has closed it. Drop is
            // the cleanup authority on success, failure, cancellation, and unwind.
            drop(audio_temp_file);

            if receiver_dropped {
                return;
            }
            let update = export_error.map_or_else(
                || {
                    if cancelled {
                        ExportUpdate::Cancelled
                    } else {
                        ExportUpdate::Complete
                    }
                },
                ExportUpdate::Failed,
            );
            if tx.send(update).is_err() {
                log::debug!("export completion receiver was dropped");
            }
        }));
    }
}

impl Drop for ExportDialog {
    fn drop(&mut self) {
        self.request_cancel();
        if let Some(handle) = self.export_worker.take() {
            if handle.join().is_err() {
                error!("export worker panicked during dialog shutdown");
            }
        }
    }
}

#[cfg(test)]
#[path = "export_dialog_tests.rs"]
mod tests;
