use eframe::egui;
use log::{error, info};
use std::collections::HashMap;
use std::sync::mpsc::{channel, Receiver};
use std::sync::{Arc, RwLock};
use std::thread;

use library::cache::SharedCacheManager;
use library::framing::entity_converters::EntityConverterRegistry;
use library::model::project::project::Project;
use library::model::project::property::PropertyValue;
use library::plugin::{ExportSettings, PluginManager, PropertyDefinition, PropertyUiType};
use library::rendering::skia_renderer::SkiaRenderer;
use library::service::{ExportService, ProjectModel, RenderService};

pub struct ExportDialog {
    pub is_open: bool,
    selected_exporter_id: Option<String>,
    pub property_values: HashMap<String, PropertyValue>,
    output_path: String,

    // Dependencies
    plugin_manager: Arc<PluginManager>,
    cache_manager: SharedCacheManager,
    entity_converter_registry: Arc<EntityConverterRegistry>,

    // Export state
    is_exporting: bool,
    progress: f32,
    status_message: String,
    progress_rx: Option<Receiver<f32>>, // Receive progress updates
    pub cancellation_token: Option<Arc<std::sync::atomic::AtomicBool>>,

    // New Fields
    pub active_composition_id: Option<uuid::Uuid>, // Targeted composition
    pub export_range: ExportRange,
    pub custom_start_frame: u64,
    pub custom_end_frame: u64,
}

#[derive(PartialEq, Clone, Copy, Debug)]
pub enum ExportRange {
    EntireComposition,
    WorkArea,
    Custom,
}

impl ExportDialog {
    pub fn new(
        plugin_manager: Arc<PluginManager>,
        cache_manager: SharedCacheManager,
        entity_converter_registry: Arc<EntityConverterRegistry>,
    ) -> Self {
        Self {
            is_open: false,
            selected_exporter_id: None,
            property_values: HashMap::new(),
            output_path: "output".to_string(),
            plugin_manager,
            cache_manager,
            entity_converter_registry,
            is_exporting: false,
            progress: 0.0,
            status_message: String::new(),
            progress_rx: None,
            cancellation_token: None,
            active_composition_id: None,
            export_range: ExportRange::EntireComposition,
            custom_start_frame: 0,
            custom_end_frame: 0,
        }
    }

    pub fn open(&mut self) {
        self.is_open = true;
    }

    pub fn show(
        &mut self,
        ctx: &egui::Context,
        project: &Arc<RwLock<Project>>,
        active_composition_id: Option<uuid::Uuid>,
    ) {
        self.active_composition_id = active_composition_id;
        let mut is_open = self.is_open;

        // Poll progress
        if self.is_exporting {
            let mut latest_progress = None;
            let mut finished = false;

            if let Some(rx) = &self.progress_rx {
                while let Ok(p) = rx.try_recv() {
                    latest_progress = Some(p);
                    if p >= 1.0 {
                        finished = true;
                    }
                }
            }

            if let Some(p) = latest_progress {
                self.progress = p;
            }
            if finished {
                self.is_exporting = false;
                self.status_message = "Export complete!".to_string();
                self.progress_rx = None;
            }
        }

        egui::Window::new("Export")
            .open(&mut is_open)
            .collapsible(false)
            .resizable(true)
            .default_width(400.0)
            .show(ctx, |ui| {
                if self.is_exporting {
                    self.show_export_progress(ui);
                } else {
                    self.show_configuration(ui, project);
                }
            });

        self.is_open = is_open;
    }

    fn show_export_progress(&mut self, ui: &mut egui::Ui) {
        ui.heading("Exporting...");
        ui.add(egui::ProgressBar::new(self.progress).show_percentage());
        ui.label(&self.status_message);
        ui.spinner();

        if ui.button("Cancel").clicked() {
            if let Some(token) = &self.cancellation_token {
                token.store(true, std::sync::atomic::Ordering::Relaxed);
            }
            self.is_exporting = false;
            self.status_message = "Cancelled.".to_string();
            self.progress_rx = None;
        }
    }

    fn show_configuration(&mut self, ui: &mut egui::Ui, project: &Arc<RwLock<Project>>) {
        ui.heading("Export Settings");

        // 1. Composition Selection
        ui.horizontal(|ui| {
            ui.label("Composition:");
            let project_read = project.read().unwrap();
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
            let known_exporters = ["ffmpeg_export", "png_export"];
            let current_selection = self
                .selected_exporter_id
                .clone()
                .unwrap_or_else(|| "Select...".to_string());

            egui::ComboBox::from_id_salt("exporter_select")
                .selected_text(current_selection)
                .show_ui(ui, |ui| {
                    for id in known_exporters {
                        if ui
                            .selectable_label(self.selected_exporter_id.as_deref() == Some(id), id)
                            .clicked()
                        {
                            self.selected_exporter_id = Some(id.to_string());
                            self.property_values.clear();
                            if let Ok(()) = self.load_defaults(id) {
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
            let project_read = project.read().unwrap();
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

        // 4. Output Path
        ui.horizontal(|ui| {
            ui.label("Output Path:");
            ui.text_edit_singleline(&mut self.output_path);
            if ui.button("Browse...").clicked() {
                let mut dialog = rfd::FileDialog::new().set_file_name(&self.output_path);
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
                            ui.label(&def.label);

                            let value = self
                                .property_values
                                .entry(def.name.clone())
                                .or_insert(def.default_value.clone());

                            match &def.ui_type {
                                PropertyUiType::Text | PropertyUiType::MultilineText => {
                                    if let PropertyValue::String(s) = value {
                                        ui.text_edit_singleline(s);
                                    }
                                }
                                PropertyUiType::Integer { min, max, suffix } => {
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
                                        if ui.checkbox(b, &def.name).changed() {
                                            // Update
                                        }
                                    }
                                }
                                PropertyUiType::Dropdown { options } => {
                                    if let PropertyValue::String(s) = value {
                                        egui::ComboBox::from_id_salt(&def.name)
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

        ui.separator();

        ui.horizontal(|ui| {
            let enabled = self.selected_exporter_id.is_some() && !self.output_path.is_empty();
            if ui
                .add_enabled(enabled, egui::Button::new("Export"))
                .clicked()
            {
                self.start_export(project);
            }
            if ui.button("Close").clicked() {
                self.is_open = false;
            }
        });
    }

    fn load_defaults(&mut self, exporter_id: &str) -> Result<(), ()> {
        if let Some(defs) = self
            .plugin_manager
            .get_export_plugin_properties(exporter_id)
        {
            for def in defs {
                self.property_values.insert(def.name, def.default_value);
            }
        }
        Ok(())
    }

    fn start_export(&mut self, project_lock: &Arc<RwLock<Project>>) {
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
        let project_snapshot = project_lock.read().unwrap().clone();
        let exporter_id_owned = exporter_id.clone();
        let output_path_owned = self.output_path.clone();
        let property_values_owned = self.property_values.clone();
        let plugin_manager = self.plugin_manager.clone();
        let cache_manager = self.cache_manager.clone();
        let entity_converter_registry = self.entity_converter_registry.clone();
        let export_range = self.export_range;
        let custom_start = self.custom_start_frame;
        let custom_end = self.custom_end_frame;

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

        thread::spawn(move || {
            // Initialize Renderer inside thread (requires context)
            let composition = &project_snapshot.compositions[comp_index];
            let mut renderer = SkiaRenderer::new(
                composition.width as u32,
                composition.height as u32,
                composition.background_color.clone(),
                false,
                None,
            );

            let render_service_plugin_manager = plugin_manager.clone();
            let mut render_service = RenderService::new(
                renderer,
                render_service_plugin_manager,
                cache_manager,
                entity_converter_registry,
            );

            // Construct ProjectModel
            let project_model =
                match ProjectModel::new(Arc::new(project_snapshot.clone()), comp_index) {
                    Ok(pm) => pm,
                    Err(e) => {
                        error!("Failed to create project model: {}", e);
                        return; // Should report error to UI
                    }
                };

            // Build ExportSettings
            let mut settings = ExportSettings::for_dimensions(
                composition.width as u32,
                composition.height as u32,
                composition.fps,
            );

            // Map properties
            let mut json_params = HashMap::new();
            for (k, v) in &property_values_owned {
                let json_val = match v {
                    PropertyValue::String(s) => serde_json::Value::String(s.clone()),
                    PropertyValue::Number(n) => {
                        serde_json::Value::Number(serde_json::Number::from_f64(n.0).unwrap())
                    }
                    PropertyValue::Boolean(b) => serde_json::Value::Bool(*b),
                    _ => serde_json::Value::Null,
                };
                json_params.insert(k.clone(), json_val);
            }
            settings.parameters = json_params;
            settings.container = match property_values_owned.get("container") {
                Some(library::model::project::property::PropertyValue::String(s)) => s.clone(),
                _ => {
                    if exporter_id_owned == "png_export" {
                        "png".to_string()
                    } else {
                        "mp4".to_string()
                    }
                }
            };

            settings.codec = match property_values_owned.get("codec") {
                Some(library::model::project::property::PropertyValue::String(s)) => s.clone(),
                _ => {
                    if exporter_id_owned == "png_export" {
                        "png".to_string()
                    } else {
                        "libx264".to_string()
                    }
                }
            };

            settings.pixel_format = match property_values_owned.get("pixel_format") {
                Some(library::model::project::property::PropertyValue::String(s)) => s.clone(),
                _ => "rgba".to_string(),
            };

            let settings_arc = Arc::new(settings.clone());

            let mut export_service = ExportService::new(
                plugin_manager.clone(),
                exporter_id_owned.clone(),
                settings_arc,
                4,
            );

            // Range Calculation
            let (start_frame, end_frame_total) = match export_range {
                ExportRange::EntireComposition => {
                    (0, (composition.duration * composition.fps).ceil() as u64)
                }
                ExportRange::WorkArea => (composition.work_area_in, composition.work_area_out),
                ExportRange::Custom => (custom_start, custom_end),
            };
            let duration_frames = end_frame_total.saturating_sub(start_frame).max(1);

            // Prepare path helpers
            let mut stem_path = std::path::PathBuf::from(&output_path_owned);
            if stem_path.extension().is_some() {
                stem_path.set_extension("");
            }
            let stem_str = stem_path.to_str().unwrap_or("output");

            // Construct absolute final path for finish_export key
            let final_output_path = if settings.container.is_empty() {
                output_path_owned.clone()
            } else {
                format!("{}.{}", stem_str, settings.container)
            };

            let chunk_size = 10;
            let mut current_frame = start_frame;

            while current_frame < end_frame_total {
                if cancel_token.load(std::sync::atomic::Ordering::Relaxed) {
                    let _ = plugin_manager.finish_export(&exporter_id_owned, &final_output_path);
                    break;
                }

                let end = (current_frame + chunk_size).min(end_frame_total);
                let range = current_frame..end;

                if let Err(e) = export_service.render_range(
                    &mut render_service,
                    &project_model,
                    range,
                    stem_str,
                ) {
                    error!("Export failed: {}", e);
                    let _ = plugin_manager.finish_export(&exporter_id_owned, &final_output_path);
                    break;
                }

                current_frame = end;
                let pct =
                    (current_frame.saturating_sub(start_frame)) as f32 / duration_frames as f32;
                let _ = tx.send(pct);
            }

            let _ = export_service.shutdown();

            // Finalize export
            if let Err(e) = plugin_manager.finish_export(&exporter_id_owned, &final_output_path) {
                error!("Failed to finalize export: {}", e);
            }

            let _ = tx.send(1.0); // Done
        });
    }
}
