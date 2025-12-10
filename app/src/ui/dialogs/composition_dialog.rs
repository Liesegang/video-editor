use egui::TextEdit;
use library::model::project::project::Composition;
use once_cell::sync::Lazy;
use uuid::Uuid;

use crate::model::ui_types::CompositionPreset;

static PRESETS: Lazy<Vec<CompositionPreset>> = Lazy::new(|| {
    vec![
        CompositionPreset {
            name: "Custom".to_string(),
            width: 0,
            height: 0,
            fps: 0.0,
        },
        // Social Media
        CompositionPreset {
            name: "Social Media Landscape HD".to_string(),
            width: 1920,
            height: 1080,
            fps: 29.97,
        },
        CompositionPreset {
            name: "Social Media Portrait HD".to_string(),
            width: 1080,
            height: 1920,
            fps: 29.97,
        },
        CompositionPreset {
            name: "Social Media Square HD".to_string(),
            width: 1080,
            height: 1080,
            fps: 29.97,
        },
        CompositionPreset {
            name: "Social Media Vertical 4:5".to_string(),
            width: 1080,
            height: 1350,
            fps: 29.97,
        },
        CompositionPreset {
            name: "Social Media Landscape 4K".to_string(),
            width: 3840,
            height: 2160,
            fps: 29.97,
        },
        CompositionPreset {
            name: "Social Media Portrait 4K".to_string(),
            width: 2160,
            height: 3840,
            fps: 29.97,
        },
        CompositionPreset {
            name: "Social Media Square 4K".to_string(),
            width: 2160,
            height: 2160,
            fps: 29.97,
        },
        // HDTV / UHD
        CompositionPreset {
            name: "HD 720 25".to_string(),
            width: 1280,
            height: 720,
            fps: 25.0,
        },
        CompositionPreset {
            name: "HD 720 29.97".to_string(),
            width: 1280,
            height: 720,
            fps: 29.97,
        },
        CompositionPreset {
            name: "HDTV 1080 24".to_string(),
            width: 1920,
            height: 1080,
            fps: 23.976,
        },
        CompositionPreset {
            name: "HDTV 1080 25".to_string(),
            width: 1920,
            height: 1080,
            fps: 25.0,
        },
        CompositionPreset {
            name: "HDTV 1080 29.97".to_string(),
            width: 1920,
            height: 1080,
            fps: 29.97,
        },
        CompositionPreset {
            name: "HDTV 1080 50".to_string(),
            width: 1920,
            height: 1080,
            fps: 50.0,
        },
        CompositionPreset {
            name: "HDTV 1080 60".to_string(),
            width: 1920,
            height: 1080,
            fps: 59.94,
        },
        CompositionPreset {
            name: "UHD 4K 23.976".to_string(),
            width: 3840,
            height: 2160,
            fps: 23.976,
        },
        CompositionPreset {
            name: "UHD 4K 25".to_string(),
            width: 3840,
            height: 2160,
            fps: 25.0,
        },
        CompositionPreset {
            name: "UHD 4K 29.97".to_string(),
            width: 3840,
            height: 2160,
            fps: 29.97,
        },
        CompositionPreset {
            name: "UHD 4K 50".to_string(),
            width: 3840,
            height: 2160,
            fps: 50.0,
        },
        CompositionPreset {
            name: "UHD 4K 59.94".to_string(),
            width: 3840,
            height: 2160,
            fps: 59.94,
        },
        CompositionPreset {
            name: "UHD 8K 23.976".to_string(),
            width: 7680,
            height: 4320,
            fps: 23.976,
        },
        CompositionPreset {
            name: "UHD 8K 25".to_string(),
            width: 7680,
            height: 4320,
            fps: 25.0,
        },
        CompositionPreset {
            name: "UHD 8K 29.97".to_string(),
            width: 7680,
            height: 4320,
            fps: 29.97,
        },
        CompositionPreset {
            name: "UHD 8K 50".to_string(),
            width: 7680,
            height: 4320,
            fps: 50.0,
        },
        CompositionPreset {
            name: "UHD 8K 59.94".to_string(),
            width: 7680,
            height: 4320,
            fps: 59.94,
        },
        // DCI
        CompositionPreset {
            name: "DCI 2K 23.976".to_string(),
            width: 2048,
            height: 1080,
            fps: 23.976,
        },
        CompositionPreset {
            name: "DCI 2K 24".to_string(),
            width: 2048,
            height: 1080,
            fps: 24.0,
        },
        CompositionPreset {
            name: "DCI 2K 25".to_string(),
            width: 2048,
            height: 1080,
            fps: 25.0,
        },
        CompositionPreset {
            name: "DCI 4K 23.976".to_string(),
            width: 4096,
            height: 2160,
            fps: 23.976,
        },
        CompositionPreset {
            name: "DCI 4K 24".to_string(),
            width: 4096,
            height: 2160,
            fps: 24.0,
        },
        CompositionPreset {
            name: "DCI 4K 25".to_string(),
            width: 4096,
            height: 2160,
            fps: 25.0,
        },
        CompositionPreset {
            name: "DCI 4K 50".to_string(),
            width: 4096,
            height: 2160,
            fps: 50.0,
        },
        CompositionPreset {
            name: "DCI 4K 59.94".to_string(),
            width: 4096,
            height: 2160,
            fps: 59.94,
        },
        CompositionPreset {
            name: "DCI 8K 23.976".to_string(),
            width: 8192,
            height: 4320,
            fps: 23.976,
        },
        CompositionPreset {
            name: "DCI 8K 24".to_string(),
            width: 8192,
            height: 4320,
            fps: 24.0,
        },
        CompositionPreset {
            name: "DCI 8K 25".to_string(),
            width: 8192,
            height: 4320,
            fps: 25.0,
        },
        // HDV
        CompositionPreset {
            name: "HDV 720/25".to_string(),
            width: 1280,
            height: 720,
            fps: 25.0,
        },
        CompositionPreset {
            name: "HDV 720/29.97".to_string(),
            width: 1280,
            height: 720,
            fps: 29.97,
        },
        // Film
        CompositionPreset {
            name: "Cineon Half".to_string(),
            width: 1828,
            height: 1332,
            fps: 24.0,
        },
        CompositionPreset {
            name: "Cineon Full".to_string(),
            width: 3656,
            height: 2664,
            fps: 24.0,
        },
        CompositionPreset {
            name: "Film (2K)".to_string(),
            width: 2048,
            height: 1556,
            fps: 24.0,
        },
        CompositionPreset {
            name: "Film (4K)".to_string(),
            width: 4096,
            height: 3112,
            fps: 24.0,
        },
    ]
});

static RESOLUTION_PRESETS: Lazy<Vec<CompositionPreset>> = Lazy::new(|| {
    vec![
        CompositionPreset {
            name: "Custom".to_string(),
            width: 0,
            height: 0,
            fps: 0.0,
        },
        CompositionPreset {
            name: "1920x1080 (FHD)".to_string(),
            width: 1920,
            height: 1080,
            fps: 0.0,
        },
        CompositionPreset {
            name: "1080x1920 (Portrait FHD)".to_string(),
            width: 1080,
            height: 1920,
            fps: 0.0,
        },
        CompositionPreset {
            name: "1080x1080 (Square HD)".to_string(),
            width: 1080,
            height: 1080,
            fps: 0.0,
        },
        CompositionPreset {
            name: "1080x1350 (Vertical 4:5)".to_string(),
            width: 1080,
            height: 1350,
            fps: 0.0,
        },
        CompositionPreset {
            name: "3840x2160 (4K UHD)".to_string(),
            width: 3840,
            height: 2160,
            fps: 0.0,
        },
        CompositionPreset {
            name: "2160x3840 (Portrait 4K)".to_string(),
            width: 2160,
            height: 3840,
            fps: 0.0,
        },
        CompositionPreset {
            name: "2160x2160 (Square 4K)".to_string(),
            width: 2160,
            height: 2160,
            fps: 0.0,
        },
        CompositionPreset {
            name: "1280x720 (HD)".to_string(),
            width: 1280,
            height: 720,
            fps: 0.0,
        },
        CompositionPreset {
            name: "7680x4320 (8K UHD)".to_string(),
            width: 7680,
            height: 4320,
            fps: 0.0,
        },
        CompositionPreset {
            name: "2048x1080 (DCI 2K)".to_string(),
            width: 2048,
            height: 1080,
            fps: 0.0,
        },
        CompositionPreset {
            name: "4096x2160 (DCI 4K)".to_string(),
            width: 4096,
            height: 2160,
            fps: 0.0,
        },
        CompositionPreset {
            name: "8192x4320 (DCI 8K)".to_string(),
            width: 8192,
            height: 4320,
            fps: 0.0,
        },
        CompositionPreset {
            name: "1828x1332 (Cineon Half)".to_string(),
            width: 1828,
            height: 1332,
            fps: 0.0,
        },
        CompositionPreset {
            name: "3656x2664 (Cineon Full)".to_string(),
            width: 3656,
            height: 2664,
            fps: 0.0,
        },
        CompositionPreset {
            name: "2048x1556 (Film 2K)".to_string(),
            width: 2048,
            height: 1556,
            fps: 0.0,
        },
        CompositionPreset {
            name: "4096x3112 (Film 4K)".to_string(),
            width: 4096,
            height: 3112,
            fps: 0.0,
        },
    ]
});

static FPS_PRESETS: Lazy<Vec<CompositionPreset>> = Lazy::new(|| {
    vec![
        CompositionPreset {
            name: "Custom".to_string(),
            width: 0,
            height: 0,
            fps: 0.0,
        },
        CompositionPreset {
            name: "23.976 fps".to_string(),
            width: 0,
            height: 0,
            fps: 23.976,
        },
        CompositionPreset {
            name: "24 fps".to_string(),
            width: 0,
            height: 0,
            fps: 24.0,
        },
        CompositionPreset {
            name: "25 fps".to_string(),
            width: 0,
            height: 0,
            fps: 25.0,
        },
        CompositionPreset {
            name: "29.97 fps".to_string(),
            width: 0,
            height: 0,
            fps: 29.97,
        },
        CompositionPreset {
            name: "30 fps".to_string(),
            width: 0,
            height: 0,
            fps: 30.0,
        },
        CompositionPreset {
            name: "50 fps".to_string(),
            width: 0,
            height: 0,
            fps: 50.0,
        },
        CompositionPreset {
            name: "59.94 fps".to_string(),
            width: 0,
            height: 0,
            fps: 59.94,
        },
        CompositionPreset {
            name: "60 fps".to_string(),
            width: 0,
            height: 0,
            fps: 60.0,
        },
    ]
});

#[derive(Debug, Clone, PartialEq)]
pub enum ActivePreset {
    Custom,
    General(usize),
    Resolution(usize),
    Fps(usize),
}

pub struct CompositionDialog {
    pub is_open: bool,
    pub comp_id: Option<Uuid>, // Will be Some(id) in edit mode
    pub name: String,
    pub width: u64,
    pub height: u64,
    pub fps: f64,
    pub duration: f64,
    pub confirmed: bool,
    pub edit_mode: bool, // New flag

    active_preset: ActivePreset,
}

impl Default for CompositionDialog {
    fn default() -> Self {
        let mut s = Self {
            is_open: false,
            comp_id: None,
            name: "New Composition".to_string(),
            width: 1920,
            height: 1080,
            fps: 29.97,
            duration: 60.0,
            confirmed: false,
            edit_mode: false,
            active_preset: ActivePreset::Custom,
        };
        s.update_active_preset();
        s
    }
}

impl CompositionDialog {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn open_for_new(&mut self) {
        *self = Self::default(); // Reset to default values for a new composition
        self.is_open = true;
        self.edit_mode = false;
        self.confirmed = false;
    }

    pub fn open_for_edit(&mut self, composition: &Composition) {
        self.is_open = true;
        self.comp_id = Some(composition.id);
        self.name = composition.name.clone();
        self.width = composition.width;
        self.height = composition.height;
        self.fps = composition.fps;
        self.duration = composition.duration;
        self.confirmed = false;
        self.edit_mode = true;
        self.update_active_preset();
    }

    fn update_active_preset(&mut self) {
        // Check general presets first
        for (i, preset) in PRESETS.iter().enumerate() {
            if preset.width == self.width && preset.height == self.height && preset.fps == self.fps
            {
                self.active_preset = ActivePreset::General(i);
                return;
            }
        }

        // Check resolution presets
        for (i, preset) in RESOLUTION_PRESETS.iter().enumerate() {
            if preset.width == self.width && preset.height == self.height {
                self.active_preset = ActivePreset::Resolution(i);
                return;
            }
        }

        // Check FPS presets
        for (i, preset) in FPS_PRESETS.iter().enumerate() {
            if preset.fps == self.fps {
                self.active_preset = ActivePreset::Fps(i);
                return;
            }
        }

        self.active_preset = ActivePreset::Custom;
    }

    fn apply_preset(&mut self, preset_idx: usize) {
        let preset = &PRESETS[preset_idx];
        self.width = preset.width;
        self.height = preset.height;
        self.fps = preset.fps;
        self.active_preset = ActivePreset::General(preset_idx);
    }

    fn apply_resolution_preset(&mut self, preset_idx: usize) {
        let preset = &RESOLUTION_PRESETS[preset_idx];
        self.width = preset.width;
        self.height = preset.height;
        self.update_active_preset(); // Update active preset based on new values
    }

    fn apply_fps_preset(&mut self, preset_idx: usize) {
        let preset = &FPS_PRESETS[preset_idx];
        self.fps = preset.fps;
        self.update_active_preset(); // Update active preset based on new values
    }

    pub fn show(&mut self, ctx: &egui::Context) {
        if !self.is_open {
            return;
        }

        let mut is_open_local = self.is_open;
        let window_title = if self.edit_mode {
            "Edit Composition Properties"
        } else {
            "New Composition Properties"
        };

        egui::Window::new(window_title)
            .open(&mut is_open_local)
            .collapsible(false)
            .resizable(false)
            .fixed_size([380.0, 300.0])
            .show(ctx, |ui| {
                // Store initial values to detect changes for "(Edited)" suffix
                let initial_values = (self.width, self.height, self.fps);
                let initial_active_preset = self.active_preset.clone();

                ui.vertical_centered_justified(|ui| {
                    ui.label("Composition Name:");
                    ui.add(TextEdit::singleline(&mut self.name));
                    ui.add_space(10.0);

                    egui::Grid::new("comp_props_grid")
                        .num_columns(2)
                        .spacing([40.0, 8.0])
                        .show(ui, |ui| {
                            // General Presets
                            ui.label("General Preset:");
                            let mut general_preset_selection = match self.active_preset {
                                ActivePreset::General(idx) => idx,
                                _ => 0, // Default to "Custom"
                            };
                            egui::ComboBox::from_id_salt("general_preset_combo")
                                .selected_text(self.get_active_preset_name_for_display(
                                    general_preset_selection,
                                    &PRESETS,
                                ))
                                .show_ui(ui, |ui| {
                                    for (i, preset) in PRESETS.iter().enumerate() {
                                        if ui
                                            .selectable_value(
                                                &mut general_preset_selection,
                                                i,
                                                preset.name.clone(),
                                            )
                                            .changed()
                                        {
                                            if i > 0 {
                                                // "Custom" preset (index 0) doesn't apply values
                                                self.apply_preset(i);
                                            } else {
                                                self.active_preset = ActivePreset::Custom;
                                                // Explicitly set to Custom
                                            }
                                        }
                                    }
                                });
                            ui.end_row();

                            // Resolution Presets
                            ui.label("Resolution Preset:");
                            let mut resolution_preset_selection = match self.active_preset {
                                ActivePreset::Resolution(idx) => idx,
                                _ => 0, // Default to "Custom"
                            };
                            egui::ComboBox::from_id_salt("resolution_preset_combo")
                                .selected_text(self.get_active_preset_name_for_display(
                                    resolution_preset_selection,
                                    &RESOLUTION_PRESETS,
                                ))
                                .show_ui(ui, |ui| {
                                    for (i, preset) in RESOLUTION_PRESETS.iter().enumerate() {
                                        if ui
                                            .selectable_value(
                                                &mut resolution_preset_selection,
                                                i,
                                                preset.name.clone(),
                                            )
                                            .changed()
                                        {
                                            if i > 0 {
                                                self.apply_resolution_preset(i);
                                            } else {
                                                self.update_active_preset(); // Re-evaluate if custom
                                            }
                                        }
                                    }
                                });
                            ui.end_row();

                            // FPS Presets
                            ui.label("FPS Preset:");
                            let mut fps_preset_selection = match self.active_preset {
                                ActivePreset::Fps(idx) => idx,
                                _ => 0, // Default to "Custom"
                            };
                            egui::ComboBox::from_id_salt("fps_preset_combo")
                                .selected_text(self.get_active_preset_name_for_display(
                                    fps_preset_selection,
                                    &FPS_PRESETS,
                                ))
                                .show_ui(ui, |ui| {
                                    for (i, preset) in FPS_PRESETS.iter().enumerate() {
                                        if ui
                                            .selectable_value(
                                                &mut fps_preset_selection,
                                                i,
                                                preset.name.clone(),
                                            )
                                            .changed()
                                        {
                                            if i > 0 {
                                                self.apply_fps_preset(i);
                                            } else {
                                                self.update_active_preset(); // Re-evaluate if custom
                                            }
                                        }
                                    }
                                });
                            ui.end_row();

                            // Width
                            ui.label("Width:");
                            if ui
                                .add(
                                    egui::DragValue::new(&mut self.width)
                                        .speed(1.0)
                                        .suffix("px"),
                                )
                                .changed()
                            {
                                self.update_active_preset();
                            }
                            ui.end_row();

                            // Height
                            ui.label("Height:");
                            if ui
                                .add(
                                    egui::DragValue::new(&mut self.height)
                                        .speed(1.0)
                                        .suffix("px"),
                                )
                                .changed()
                            {
                                self.update_active_preset();
                            }
                            ui.end_row();

                            // FPS
                            ui.label("FPS:");
                            if ui
                                .add(egui::DragValue::new(&mut self.fps).speed(0.1))
                                .changed()
                            {
                                self.update_active_preset();
                            }
                            ui.end_row();

                            // Duration
                            ui.label("Duration:");
                            ui.add(
                                egui::DragValue::new(&mut self.duration)
                                    .speed(0.1)
                                    .suffix("s"),
                            );
                            ui.end_row();
                        });

                    ui.add_space(10.0);

                    ui.horizontal(|ui| {
                        if ui.button("OK").clicked() {
                            self.confirmed = true;
                        }
                        if ui.button("Cancel").clicked() {
                            self.confirmed = false;
                        }
                    });
                });

                // Detect changes after interaction
                if initial_values != (self.width, self.height, self.fps) {
                    self.update_active_preset();
                } else if let ActivePreset::General(_idx) = initial_active_preset {
                    // Special handling for general preset 'Edited' status
                    if initial_values != (self.width, self.height, self.fps) {
                        // Force update to Custom if values diverge
                        self.active_preset = ActivePreset::Custom;
                    }
                }
            });

        self.is_open = is_open_local;
    }

    fn get_active_preset_name_for_display(
        &self,
        current_selection_idx: usize,
        presets_list: &[CompositionPreset],
    ) -> String {
        let preset_ref = &presets_list[current_selection_idx];
        let mut name = preset_ref.name.clone();
        if self.is_customized_from_preset(preset_ref) {
            name.push_str(" (Edited)");
        }
        name
    }

    fn is_customized_from_preset(&self, preset: &CompositionPreset) -> bool {
        (preset.width != 0 && preset.width != self.width)
            || (preset.height != 0 && preset.height != self.height)
            || (preset.fps != 0.0 && preset.fps != self.fps)
    }
}
