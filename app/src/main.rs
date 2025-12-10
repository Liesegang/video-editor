use eframe::egui;
use egui_dock::{DockArea, DockState, NodeIndex, Style};
use library::model::project::project::{Composition, Project};
use library::model::project::property::{Property, PropertyMap, PropertyValue};
use library::model::project::Track;
use library::service::project_service::ProjectService;
use serde::{Deserialize, Serialize};
use std::fs;
use std::io::Write; // Only keep Write trait if needed
use std::sync::{Arc, RwLock};
use uuid::Uuid;

// --- 1. データ定義 ---

#[derive(Debug, Clone)]
enum Tab {
    Preview,
    Timeline,
    Inspector,
    Assets,
}

#[derive(Clone, Serialize, Deserialize)]
struct Asset {
    name: String,
    duration: f32,
    #[serde(with = "ColorDef")]
    color: egui::Color32,
    kind: AssetKind,
    composition_id: Option<Uuid>, // Added for AssetKind::Composition
}

impl Asset {
    // Helper to generate a unique ID for egui, especially useful after composition_id is added
    fn id(&self) -> egui::Id {
        match self.kind {
            AssetKind::Composition(id) => egui::Id::new(id),
            // For other asset kinds, generate a stable ID from a combination of name and kind
            _ => egui::Id::new((&self.name, &self.kind)),
        }
    }
}

#[derive(Clone, PartialEq, Serialize, Deserialize, Hash)] // Added #[derive(Hash)]
enum AssetKind {
    Video,
    Audio,
    Composition(Uuid), // Added: a composition asset links to its actual composition
}

// GUI-specific Clip representation (derived from TrackEntity for display)
// This struct holds display-only properties, actual data resides in Project
#[derive(Debug, Clone)]
struct GuiClip {
    id: Uuid,
    name: String,
    track_id: Uuid,
    start_time: f32,
    duration: f32,
    color: egui::Color32,
    position: [f32; 2],
    scale: f32,
    opacity: f32,
    rotation: f32,
    asset_index: usize, // To link back to local assets for display info
}

// Serde helper for egui::Color32
#[derive(Serialize, Deserialize)]
#[serde(remote = "egui::Color32")]
struct ColorDef(#[serde(getter = "get_color_tuple")] (u8, u8, u8, u8));
fn get_color_tuple(color: &egui::Color32) -> (u8, u8, u8, u8) {
    color.to_tuple()
}
impl From<ColorDef> for egui::Color32 {
    fn from(def: ColorDef) -> egui::Color32 {
        egui::Color32::from_rgba_premultiplied(def.0 .0, def.0 .1, def.0 .2, def.0 .3)
    }
}

// Serde helper for egui::Vec2
#[derive(Serialize, Deserialize)]
#[serde(remote = "egui::Vec2")]
struct Vec2Def {
    x: f32,
    y: f32,
}

// フォント設定関数
fn setup_custom_fonts(ctx: &egui::Context) {
    let mut fonts = egui::FontDefinitions::default();

    // Windowsの標準日本語フォント "MS Gothic" を読み込む
    // ※ 他のOSの場合は適宜パスを変更するか、フォントファイルを同梱してください
    let font_path = "C:\\Windows\\Fonts\\msgothic.ttc";

    if let Ok(font_data) = fs::read(font_path) {
        fonts.font_data.insert(
            "my_font".to_owned(),
            egui::FontData::from_owned(font_data).tweak(egui::FontTweak {
                scale: 1.2,
                ..Default::default()
            }),
        );

        // 優先順位の先頭に追加
        fonts
            .families
            .entry(egui::FontFamily::Proportional)
            .or_default()
            .insert(0, "my_font".to_owned());
        fonts
            .families
            .entry(egui::FontFamily::Monospace)
            .or_default()
            .insert(0, "my_font".to_owned());

        ctx.set_fonts(fonts);
    } else {
        eprintln!("Warning: Failed to load font from {}", font_path);
    }
}

#[derive(Serialize, Deserialize)]
struct EditorContext {
    assets: Vec<Asset>,
    current_time: f32,
    is_playing: bool,
    timeline_pixels_per_second: f32,

    // --- キャンバス用の状態 ---
    #[serde(with = "Vec2Def")]
    view_pan: egui::Vec2,
    view_zoom: f32,

    #[serde(skip)]
    dragged_asset: Option<usize>,

    // --- タイムライン用の状態 ---
    #[serde(skip)]
    timeline_v_zoom: f32,
    #[serde(skip)]
    timeline_h_zoom: f32,
    #[serde(skip)]
    timeline_scroll_offset: egui::Vec2,

    #[serde(skip)]
    selected_composition_id: Option<Uuid>,
    #[serde(skip)] // Marking selected_track_id as transient UI state
    selected_track_id: Option<Uuid>,
    #[serde(skip)] // Marking selected_entity_id as transient UI state
    selected_entity_id: Option<Uuid>, 
    #[serde(skip)]
    // Cache for the selected entity's properties for the Inspector panel.
    // Stores (entity_id, entity_type, properties, start_time, end_time)
    inspector_entity_cache: Option<(Uuid, String, PropertyMap, f64, f64)>,

}

impl EditorContext {
    fn new(default_comp_id: Uuid) -> Self {
        // Modified to accept default_comp_id
        let assets = vec![
            Asset {
                name: "Intro_Seq.mp4".into(),
                duration: 5.0,
                color: egui::Color32::from_rgb(100, 150, 255),
                kind: AssetKind::Video,
                composition_id: None,
            },
            Asset {
                name: "Main_Cam.mov".into(),
                duration: 15.0,
                color: egui::Color32::from_rgb(80, 120, 200),
                kind: AssetKind::Video,
                composition_id: None,
            },
            Asset {
                name: "BGM_Happy.mp3".into(),
                duration: 30.0,
                color: egui::Color32::from_rgb(100, 255, 150),
                kind: AssetKind::Audio,
                composition_id: None,
            },
            Asset {
                name: "Text_Overlay.png".into(),
                duration: 5.0,
                color: egui::Color32::from_rgb(255, 100, 150),
                kind: AssetKind::Video,
                composition_id: None,
            },
            Asset {
                name: "Logo.png".into(),
                duration: 5.0,
                color: egui::Color32::from_rgb(255, 200, 100),
                kind: AssetKind::Video,
                composition_id: None,
            },
            Asset {
                name: "Main Composition".into(),
                duration: 60.0,
                color: egui::Color32::from_rgb(255, 150, 255),
                kind: AssetKind::Composition(default_comp_id),
                composition_id: Some(default_comp_id),
            },
        ];

        Self {
            assets,
            current_time: 0.0,
            is_playing: false,
            timeline_pixels_per_second: 50.0,

            view_pan: egui::vec2(20.0, 20.0),
            view_zoom: 0.3,
            dragged_asset: None,

            timeline_v_zoom: 1.0,
            timeline_h_zoom: 1.0,
            timeline_scroll_offset: egui::Vec2::ZERO,

            selected_composition_id: Some(default_comp_id), // Initialize with default composition
            selected_track_id: None,
            selected_entity_id: None,
            inspector_entity_cache: None,
        }
    }

    // Helper to get selected composition
    fn get_current_composition<'a>(&self, project: &'a Project) -> Option<&'a Composition> {
        self.selected_composition_id
            .and_then(|id| project.compositions.iter().find(|&c| c.id == id))
    }

    // --- UI実装 ---

    // 1. Canvasプレビュー
    fn show_preview(
        &mut self,
        ui: &mut egui::Ui,
        project_service: &ProjectService,
        project: &Arc<RwLock<Project>>,
    ) {
        let (rect, response) =
            ui.allocate_exact_size(ui.available_size(), egui::Sense::click_and_drag());

        let pointer_pos = ui.input(|i| i.pointer.hover_pos());
        let space_down = ui.input(|i| i.key_down(egui::Key::Space));
        // Corrected access to mouse button state using ctx.input()
        let middle_down = ui
            .ctx()
            .input(|i| i.pointer.button_down(egui::PointerButton::Middle));
        let is_panning_input = space_down || middle_down;

        if is_panning_input && response.dragged() {
            self.view_pan += response.drag_delta();
        }

        if response.hovered() {
            let scroll_delta = ui.input(|i| i.raw_scroll_delta.y);
            if scroll_delta != 0.0 {
                let zoom_factor = if scroll_delta > 0.0 { 1.1 } else { 0.9 };
                let old_zoom = self.view_zoom;
                self.view_zoom *= zoom_factor;

                if let Some(mouse_pos) = pointer_pos {
                    let mouse_in_canvas = mouse_pos - rect.min;
                    self.view_pan = mouse_in_canvas
                        - (mouse_in_canvas - self.view_pan) * (self.view_zoom / old_zoom);
                }
            }
        }

        let view_offset = rect.min + self.view_pan;
        let view_zoom = self.view_zoom;

        let to_screen =
            |pos: egui::Pos2| -> egui::Pos2 { view_offset + (pos.to_vec2() * view_zoom) };
        let to_world = |pos: egui::Pos2| -> egui::Pos2 {
            let vec = pos - view_offset;
            egui::pos2(vec.x / view_zoom, vec.y / view_zoom)
        };

        let painter = ui.painter().with_clip_rect(rect);

        // 背景
        painter.rect_filled(rect, 0.0, egui::Color32::from_gray(30));

        // グリッド
        let grid_size = 100.0 * self.view_zoom;
        let offset = self.view_pan;
        if grid_size > 10.0 {
            let cols = (rect.width() / grid_size).ceil() as usize + 2;
            let rows = (rect.height() / grid_size).ceil() as usize + 2;
            let start_x = (offset.x % grid_size) - grid_size;
            let start_y = (offset.y % grid_size) - grid_size;
            let grid_color = egui::Color32::from_gray(50);

            for i in 0..cols {
                let x = rect.min.x + start_x + (i as f32) * grid_size;
                painter.line_segment(
                    [egui::pos2(x, rect.min.y), egui::pos2(x, rect.max.y)],
                    egui::Stroke::new(1.0, grid_color),
                );
            }
            for i in 0..rows {
                let y = rect.min.y + start_y + (i as f32) * grid_size;
                painter.line_segment(
                    [egui::pos2(rect.min.x, y), egui::pos2(rect.max.x, y)],
                    egui::Stroke::new(1.0, grid_color),
                );
            }
        }

        // フルHDフレーム
        let frame_rect = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1920.0, 1080.0));
        let screen_frame_min = to_screen(frame_rect.min);
        let screen_frame_max = to_screen(frame_rect.max);
        painter.rect_stroke(
            egui::Rect::from_min_max(screen_frame_min, screen_frame_max),
            0.0,
            egui::Stroke::new(2.0 * self.view_zoom.max(1.0), egui::Color32::WHITE),
        );

        let mut hovered_entity_id = None;
        let mut gui_clips: Vec<GuiClip> = Vec::new();

        if let Ok(proj_read) = project.read() {
            if let Some(comp) = self.get_current_composition(&proj_read) {
                // Collect GuiClips from current composition's tracks
                for track in &comp.tracks {
                    for entity in &track.entities {
                        // For simplicity, hardcode asset_index 0 (first asset) for now.
                        // In a real app, this would be determined by entity_type or asset property.
                        let asset_index = 0;
                        let asset = self.assets.get(asset_index);

                        if let Some(a) = asset {
                            let gc = GuiClip {
                                id: entity.id,
                                name: entity.entity_type.clone(), // Use entity_type as name for now
                                track_id: track.id,
                                start_time: entity.start_time as f32,
                                duration: (entity.end_time - entity.start_time) as f32,
                                color: a.color,
                                position: [
                                    entity.properties.get_f32("position_x").unwrap_or(960.0),
                                    entity.properties.get_f32("position_y").unwrap_or(540.0),
                                ],
                                scale: entity.properties.get_f32("scale").unwrap_or(100.0),
                                opacity: entity.properties.get_f32("opacity").unwrap_or(100.0),
                                rotation: entity.properties.get_f32("rotation").unwrap_or(0.0),
                                asset_index,
                            };
                            gui_clips.push(gc);
                        }
                    }
                }

                // Clip hit test
                if let Some(mouse_screen_pos) = pointer_pos {
                    if rect.contains(mouse_screen_pos) {
                        let mut sorted_clips: Vec<&GuiClip> = gui_clips
                            .iter()
                            .filter(|gc| {
                                self.current_time >= gc.start_time
                                    && self.current_time < gc.start_time + gc.duration
                            })
                            .collect();
                        // Sort by track index for consistent Z-order hit testing
                        sorted_clips.sort_by_key(|gc| {
                            comp.tracks
                                .iter()
                                .position(|t| t.id == gc.track_id)
                                .unwrap_or(0)
                        });

                        for gc in sorted_clips.iter().rev() {
                            // Iterate in reverse to hit top-most clips first
                            let is_audio = self
                                .assets
                                .get(gc.asset_index)
                                .map(|a| a.kind == AssetKind::Audio)
                                .unwrap_or(false);
                            if is_audio {
                                continue;
                            }

                            let mouse_world_pos = to_world(mouse_screen_pos);
                            let center = egui::pos2(gc.position[0], gc.position[1]);

                            let vec = mouse_world_pos - center;
                            let angle_rad = -gc.rotation.to_radians();
                            let cos = angle_rad.cos();
                            let sin = angle_rad.sin();
                            let local_x = vec.x * cos - vec.y * sin;
                            let local_y = vec.x * sin + vec.y * cos;

                            let base_w = 640.0;
                            let base_h = 360.0;
                            let half_w = (base_w * gc.scale / 100.0) / 2.0;
                            let half_h = (base_h * gc.scale / 100.0) / 2.0;

                            if local_x.abs() <= half_w && local_y.abs() <= half_h {
                                hovered_entity_id = Some(gc.id);
                                break;
                            }
                        }
                    }
                }
            }
        }

        // Clip drawing
        if let Ok(proj_read) = project.read() {
            if let Some(comp) = self.get_current_composition(&proj_read) {
                // Re-collect visible GuiClips after potential modifications in Inspector
                let mut visible_clips: Vec<GuiClip> = Vec::new();
                for track in &comp.tracks {
                    for entity in &track.entities {
                        let asset_index = 0; // Temporary: should derive from entity properties
                        let asset = self.assets.get(asset_index);

                        if self.current_time >= entity.start_time as f32
                            && self.current_time < entity.end_time as f32
                        {
                            if let Some(a) = asset {
                                let gc = GuiClip {
                                    id: entity.id,
                                    name: entity.entity_type.clone(),
                                    track_id: track.id,
                                    start_time: entity.start_time as f32,
                                    duration: (entity.end_time - entity.start_time) as f32,
                                    color: a.color,
                                    position: [
                                        entity.properties.get_f32("position_x").unwrap_or(960.0),
                                        entity.properties.get_f32("position_y").unwrap_or(540.0),
                                    ],
                                    scale: entity.properties.get_f32("scale").unwrap_or(100.0),
                                    opacity: entity.properties.get_f32("opacity").unwrap_or(100.0),
                                    rotation: entity.properties.get_f32("rotation").unwrap_or(0.0),
                                    asset_index,
                                };
                                visible_clips.push(gc);
                            }
                        }
                    }
                }
                // Sort by track index for consistent Z-order drawing
                visible_clips.sort_by_key(|gc| {
                    comp.tracks
                        .iter()
                        .position(|t| t.id == gc.track_id)
                        .unwrap_or(0)
                });

                for clip in visible_clips {
                    let is_audio = self
                        .assets
                        .get(clip.asset_index)
                        .map(|a| a.kind == AssetKind::Audio)
                        .unwrap_or(false);
                    if is_audio {
                        continue;
                    }

                    let opacity = (clip.opacity / 100.0).clamp(0.0, 1.0);
                    let color = clip.color.linear_multiply(opacity);

                    let base_w = 640.0;
                    let base_h = 360.0;
                    let scale = clip.scale / 100.0;
                    let w = base_w * scale;
                    let h = base_h * scale;

                    let center = egui::pos2(clip.position[0], clip.position[1]);
                    let rot = clip.rotation.to_radians();
                    let cos_r = rot.cos();
                    let sin_r = rot.sin();

                    let local_corners = [
                        egui::vec2(-w / 2.0, -h / 2.0),
                        egui::vec2(w / 2.0, -h / 2.0),
                        egui::vec2(w / 2.0, h / 2.0),
                        egui::vec2(-w / 2.0, h / 2.0),
                    ];

                    let screen_points: Vec<egui::Pos2> = local_corners
                        .iter()
                        .map(|corner| {
                            let rot_x = corner.x * cos_r - corner.y * sin_r;
                            let rot_y = corner.x * sin_r + corner.y * cos_r;
                            to_screen(center + egui::vec2(rot_x, rot_y))
                        })
                        .collect();

                    painter.add(egui::Shape::convex_polygon(
                        screen_points.clone(),
                        color,
                        egui::Stroke::NONE,
                    ));
                }
            }
        }

        let interacted_with_gizmo = false; // Placeholder

        // 選択・移動
        if !is_panning_input && !interacted_with_gizmo {
            if response.clicked() {
                self.selected_entity_id = hovered_entity_id;
            } else if response.dragged() {
                if let Some(entity_id) = self.selected_entity_id {
                    let current_zoom = self.view_zoom;
                    if let Some(comp_id) = self.selected_composition_id {
                        if let Some(track_id) = self.selected_track_id {
                            // Need track_id to update entity properties
                            let world_delta = response.drag_delta() / current_zoom;

                            // Update properties via ProjectService
                            project_service
                                .update_entity_property(
                                    comp_id,
                                    track_id,
                                    entity_id,
                                    "position_x",
                                    PropertyValue::Number(
                                        project_service
                                            .with_track_mut(comp_id, track_id, |track| {
                                                track
                                                    .entities
                                                    .iter()
                                                    .find(|e| e.id == entity_id)
                                                    .and_then(|e| {
                                                        e.properties.get_f64("position_x")
                                                    })
                                                    .unwrap_or(0.0)
                                            })
                                            .unwrap_or(0.0)
                                            + world_delta.x as f64,
                                    ),
                                )
                                .ok(); // Handle error
                            project_service
                                .update_entity_property(
                                    comp_id,
                                    track_id,
                                    entity_id,
                                    "position_y",
                                    PropertyValue::Number(
                                        project_service
                                            .with_track_mut(comp_id, track_id, |track| {
                                                track
                                                    .entities
                                                    .iter()
                                                    .find(|e| e.id == entity_id)
                                                    .and_then(|e| {
                                                        e.properties.get_f64("position_y")
                                                    })
                                                    .unwrap_or(0.0)
                                            })
                                            .unwrap_or(0.0)
                                            + world_delta.y as f64,
                                    ),
                                )
                                .ok(); // Handle error
                        }
                    }
                }
            }
        }

        // 情報
        let info_text = format!(
            "Time: {:.2}\nZoom: {:.0}%",
            self.current_time,
            self.view_zoom * 100.0
        );
        painter.text(
            rect.left_top() + egui::vec2(10.0, 10.0),
            egui::Align2::LEFT_TOP,
            info_text,
            egui::FontId::monospace(14.0),
            egui::Color32::WHITE,
        );
    }

    // 2. アセット
    fn show_assets(&mut self, ui: &mut egui::Ui) {
        ui.heading("Assets");
        ui.separator();
        egui::ScrollArea::vertical().show(ui, |ui| {
            for (index, asset) in self.assets.iter().enumerate() {
                ui.push_id(asset.id(), |ui_in_scope| {
                    // Correct usage of push_id with closure
                    let label_text = format!("{} ({:.1}s)", asset.name, asset.duration);
                    let icon = match asset.kind {
                        AssetKind::Video => "📹",
                        AssetKind::Audio => "🎵",
                        AssetKind::Composition(_) => "📄", // New icon for composition
                    };

                    let item_response = ui_in_scope
                        .add(
                            egui::Label::new(
                                egui::RichText::new(format!("{} {}", icon, label_text))
                                    .background_color(asset.color)
                                    .color(egui::Color32::BLACK),
                            )
                            .sense(egui::Sense::drag()),
                        )
                        .on_hover_text(format!("Asset ID: {:?}", asset.id()));

                    if item_response.drag_started() {
                        self.dragged_asset = Some(index);
                    }
                    ui_in_scope.add_space(5.0);
                });
            }
        });
    }

    fn show_timeline_ruler(&mut self, ui: &mut egui::Ui) {
        let (outer_rect, _) = ui.allocate_exact_size(
            egui::vec2(ui.available_width(), ui.available_height()),
            egui::Sense::hover(),
        );

        ui.horizontal(|ui| {
            // Spacer for the track list
            let (spacer_rect, _) = ui
                .allocate_exact_size(egui::vec2(100.0, outer_rect.height()), egui::Sense::hover());
            ui.painter_at(spacer_rect).rect_filled(
                spacer_rect,
                0.0,
                ui.style().visuals.widgets.noninteractive.bg_fill,
            );

            // The actual ruler
            let (rect, _) = ui.allocate_at_least(
                egui::vec2(ui.available_width(), outer_rect.height()),
                egui::Sense::hover(),
            );
            let painter = ui.painter_at(rect);
            painter.rect_filled(rect, 0.0, ui.style().visuals.widgets.noninteractive.bg_fill);

            let time_scale = self.timeline_pixels_per_second * self.timeline_h_zoom;
            let start_x = rect.min.x + self.timeline_scroll_offset.x;

            // Determine the density of markers based on zoom
            let (major_interval, minor_interval) = if time_scale > 150.0 {
                (1.0, 0.5)
            } else if time_scale > 50.0 {
                (1.0, 1.0) // No minor
            } else if time_scale > 15.0 {
                (5.0, 1.0)
            } else {
                (10.0, 5.0)
            };

            let first_second = (-self.timeline_scroll_offset.x / time_scale).floor() as i32;
            let last_second =
                ((-self.timeline_scroll_offset.x + rect.width()) / time_scale).ceil() as i32;

            for sec in first_second..=last_second {
                let s = sec as f32;
                if s % minor_interval == 0.0 {
                    let x = start_x + s * time_scale;
                    if x >= rect.min.x && x <= rect.max.x {
                        let is_major = s % major_interval == 0.0;
                        let _height = if is_major {
                            rect.height()
                        } else {
                            rect.height() * 0.5
                        };
                        painter.line_segment(
                            [egui::pos2(x, rect.min.y), egui::pos2(x, rect.max.y)],
                            ui.style().visuals.widgets.noninteractive.bg_stroke,
                        );
                        if is_major {
                            painter.text(
                                egui::pos2(x + 2.0, rect.center().y),
                                egui::Align2::LEFT_CENTER,
                                format!("{}s", s),
                                egui::FontId::monospace(10.0),
                                ui.style().visuals.text_color(),
                            );
                        }
                    }
                }
            }
        });
    }

    fn show_timeline_controls(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            // Play button
            if ui.button(if self.is_playing { "⏸" } else { "▶" }).clicked() {
                self.is_playing = !self.is_playing;
            }

            // Time display
            let minutes = (self.current_time / 60.0).floor();
            let seconds = (self.current_time % 60.0).floor();
            let ms = ((self.current_time % 1.0) * 100.0).floor();
            let time_text = format!("{:02}:{:02}.{:02}", minutes, seconds, ms);
            ui.label(egui::RichText::new(time_text).monospace());

            // Spacer
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                // Zoom reset button
                if ui.button("1:1").clicked() {
                    self.timeline_h_zoom = 1.0;
                    self.timeline_v_zoom = 1.0;
                }

                // Zoom information
                let zoom_text = format!("H-Zoom: {:.1}x", self.timeline_h_zoom);
                ui.label(zoom_text);
            });
        });
    }

    // 3. タイムライン
    fn show_timeline(
        &mut self,
        ui: &mut egui::Ui,
        project_service: &ProjectService,
        project: &Arc<RwLock<Project>>,
    ) {
        // Use panels to divide the space correctly
        egui::TopBottomPanel::top("timeline_ruler_panel")
            .exact_height(20.0)
            .show_inside(ui, |ui| {
                self.show_timeline_ruler(ui);
            });

        egui::TopBottomPanel::bottom("timeline_controls_panel")
            .exact_height(40.0)
            .show_inside(ui, |ui| {
                ui.separator();
                self.show_timeline_controls(ui);
            });

        egui::CentralPanel::default().show_inside(ui, |ui| {
            // Main timeline content
            ui.with_layout(egui::Layout::left_to_right(egui::Align::TOP), |ui| {
                // --- Track list sidebar ---
                let (track_list_rect, track_list_response) = ui.allocate_exact_size(
                    egui::vec2(100.0, ui.available_height()),
                    egui::Sense::click_and_drag(),
                );
                let track_list_painter = ui.painter_at(track_list_rect);
                track_list_painter.rect_filled(
                    track_list_rect,
                    0.0,
                    ui.style().visuals.window_fill(),
                ); // Fill entire sidebar background

                let row_height = 30.0;
                let track_spacing = 2.0;

                let mut current_tracks: Vec<Track> = Vec::new();
                if let Ok(proj_read) = project.read() {
                    if let Some(comp) = self.get_current_composition(&proj_read) {
                        current_tracks = comp.tracks.clone();
                    }
                }
                let num_tracks = current_tracks.len();

                for (i, track) in current_tracks.iter().enumerate() {
                    let y = track_list_rect.min.y
                        + (i as f32 * (row_height + track_spacing))
                        + self.timeline_scroll_offset.y;
                    let track_label_rect = egui::Rect::from_min_size(
                        egui::pos2(track_list_rect.min.x, y),
                        egui::vec2(track_list_rect.width(), row_height),
                    );

                    if track_list_rect.intersects(track_label_rect) {
                        let track_interaction_response = ui
                            .interact(
                                track_label_rect,
                                egui::Id::new(track.id).with("track_label_interact"),
                                egui::Sense::click(),
                            )
                            .on_hover_text(format!("Track ID: {}", track.id));
                        if track_interaction_response.clicked() {
                            self.selected_track_id = Some(track.id);
                        }

                        // Draw alternating background for this row
                        track_list_painter.rect_filled(
                            track_label_rect,
                            0.0,
                            if self.selected_track_id == Some(track.id) {
                                egui::Color32::from_rgb(50, 80, 120)
                            }
                            // use self.selected_track_id
                            else if i % 2 == 0 {
                                egui::Color32::from_gray(50)
                            } else {
                                egui::Color32::from_gray(60)
                            },
                        );
                        // Draw text label
                        track_list_painter.text(
                            track_label_rect.left_center() + egui::vec2(5.0, 0.0),
                            egui::Align2::LEFT_CENTER,
                            format!("Track {}", track.name), // Display track name
                            egui::FontId::monospace(10.0),
                            egui::Color32::GRAY,
                        );
                    }
                }

                // Track list right-click context menu
                track_list_response.context_menu(|ui| {
                    if let Some(comp_id) = self.selected_composition_id {
                        if ui.button("Add Track").clicked() {
                            project_service
                                .add_track(comp_id, "New Track")
                                .expect("Failed to add track");
                            ui.close_menu();
                        }
                        if let Some(track_id) = self.selected_track_id {
                            if ui.button("Remove Selected Track").clicked() {
                                project_service
                                    .remove_track(comp_id, track_id)
                                    .expect("Failed to remove track");
                                self.selected_track_id = None;
                                self.selected_entity_id = None;
                                ui.close_menu();
                            }
                        } else {
                            ui.label("Select a track to remove");
                        }
                    } else {
                        ui.label("Select a Composition first");
                    }
                });

                ui.separator();

                // --- Clip area ---
                let (content_rect, response) =
                    ui.allocate_at_least(ui.available_size(), egui::Sense::click_and_drag());

                // --- Interaction ---
                if response.hovered() {
                    let scroll_delta = ui.input(|i| i.raw_scroll_delta);
                    if ui.input(|i| i.modifiers.ctrl) && scroll_delta.y != 0.0 {
                        let zoom_factor = if scroll_delta.y > 0.0 { 1.1 } else { 0.9 };
                        self.timeline_h_zoom =
                            (self.timeline_h_zoom * zoom_factor).clamp(0.1, 10.0);
                    } else if scroll_delta.y != 0.0 {
                        self.timeline_scroll_offset.y -= scroll_delta.y;
                    }

                    if scroll_delta.x != 0.0 {
                        self.timeline_scroll_offset.x -= scroll_delta.x;
                    }
                }
                if response.dragged() {
                    self.timeline_scroll_offset.x += response.drag_delta().x;
                    self.timeline_scroll_offset.y += response.drag_delta().y;
                }

                // --- Drawing ---
                let painter = ui.painter_at(content_rect);
                let time_scale = self.timeline_pixels_per_second * self.timeline_h_zoom;

                // Constrain scroll offset
                let max_scroll_y =
                    (num_tracks as f32 * (row_height + track_spacing)) - content_rect.height();
                self.timeline_scroll_offset.y = self
                    .timeline_scroll_offset
                    .y
                    .clamp(-max_scroll_y.max(0.0), 0.0);
                self.timeline_scroll_offset.x = self.timeline_scroll_offset.x.min(0.0);

                for i in 0..num_tracks {
                    let y = content_rect.min.y
                        + (i as f32 * (row_height + track_spacing))
                        + self.timeline_scroll_offset.y;
                    let track_rect = egui::Rect::from_min_size(
                        egui::pos2(content_rect.min.x, y),
                        egui::vec2(content_rect.width(), row_height),
                    );
                    painter.rect_filled(
                        track_rect,
                        0.0,
                        if i % 2 == 0 {
                            egui::Color32::from_gray(50)
                        } else {
                            egui::Color32::from_gray(60)
                        },
                    );
                }

                // Logic for adding entity to track on drag-drop
                if ui.input(|i| i.pointer.any_released()) {
                    if let Some(asset_index) = self.dragged_asset {
                        if let Some(mouse_pos) = response.hover_pos() {
                            let drop_time = ((mouse_pos.x
                                - content_rect.min.x
                                - self.timeline_scroll_offset.x)
                                / time_scale)
                                .max(0.0);
                            let drop_track_index =
                                ((mouse_pos.y - content_rect.min.y - self.timeline_scroll_offset.y)
                                    / (row_height + track_spacing))
                                    .floor() as usize;

                            if let Some(comp_id) = self.selected_composition_id {
                                if let Some(track) = current_tracks.get(drop_track_index) {
                                    if let Some(asset) = self.assets.get(asset_index) {
                                        // Handle dropping a Composition asset
                                        if let AssetKind::Composition(_nested_comp_id) = asset.kind
                                        {
                                            // For now, create a generic entity representing the nested composition
                                            if let Err(e) = project_service.add_entity_to_track(
                                                comp_id,
                                                track.id,
                                                &format!("Nested Comp: {}", asset.name),
                                                drop_time as f64,
                                                (drop_time + asset.duration) as f64,
                                            ) {
                                                eprintln!(
                                                    "Failed to add nested composition entity: {:?}",
                                                    e
                                                );
                                            }
                                        } else {
                                            // Add entity via ProjectService for other asset kinds
                                            if let Err(e) = project_service.add_entity_to_track(
                                                comp_id,
                                                track.id,
                                                &asset.name, // Use asset name as entity type for now
                                                drop_time as f64,
                                                (drop_time + asset.duration) as f64,
                                            ) {
                                                eprintln!("Failed to add entity to track: {:?}", e);
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }

                let is_dragging_asset = self.dragged_asset.is_some();
                let mut clicked_on_entity = false;

                if !is_dragging_asset && response.drag_stopped() {
                    if let Some(pos) = response.interact_pointer_pos() {
                        self.current_time =
                            ((pos.x - content_rect.min.x - self.timeline_scroll_offset.x)
                                / time_scale)
                                .max(0.0);
                    }
                }

                // Draw entities (clips) from the Project model
                if let Ok(proj_read) = project.read() {
                    if let Some(comp) = self.get_current_composition(&proj_read) {
                        for track in &comp.tracks {
                            let clip_track_index = comp
                                .tracks
                                .iter()
                                .position(|t| t.id == track.id)
                                .map(|idx| idx as f32)
                                .unwrap_or(0.0);

                            for entity in &track.entities {
                                let asset_index = 0; // Temporary: should derive from entity properties
                                let asset = self.assets.get(asset_index);

                                if let Some(a) = asset {
                                    let gc = GuiClip {
                                        id: entity.id,
                                        name: entity.entity_type.clone(),
                                        track_id: track.id,
                                        start_time: entity.start_time as f32,
                                        duration: (entity.end_time - entity.start_time) as f32,
                                        color: a.color,
                                        position: [
                                            entity
                                                .properties
                                                .get_f32("position_x")
                                                .unwrap_or(960.0),
                                            entity
                                                .properties
                                                .get_f32("position_y")
                                                .unwrap_or(540.0),
                                        ],
                                        scale: entity.properties.get_f32("scale").unwrap_or(100.0),
                                        opacity: entity
                                            .properties
                                            .get_f32("opacity")
                                            .unwrap_or(100.0),
                                        rotation: entity
                                            .properties
                                            .get_f32("rotation")
                                            .unwrap_or(0.0),
                                        asset_index,
                                    };

                                    let x = content_rect.min.x
                                        + self.timeline_scroll_offset.x
                                        + gc.start_time * time_scale;
                                    let y = content_rect.min.y
                                        + self.timeline_scroll_offset.y
                                        + clip_track_index * (row_height + track_spacing);
                                    let clip_rect = egui::Rect::from_min_size(
                                        egui::pos2(x, y),
                                        egui::vec2(gc.duration * time_scale, row_height),
                                    );

                                    let clip_resp = ui.interact(
                                        clip_rect,
                                        egui::Id::new(gc.id),
                                        egui::Sense::click_and_drag(),
                                    );
                                    if clip_resp.clicked() {
                                        self.selected_entity_id = Some(gc.id);
                                        self.selected_track_id = Some(gc.track_id);
                                        clicked_on_entity = true;
                                    }

                                    if clip_resp.drag_started() {
                                        self.selected_entity_id = Some(gc.id);
                                        self.selected_track_id = Some(gc.track_id);
                                    }
                                    if clip_resp.dragged() && self.selected_entity_id == Some(gc.id)
                                    {
                                        let dt = clip_resp.drag_delta().x / time_scale;
                                        // Update entity's start_time in ProjectService
                                        if let Some(comp_id) = self.selected_composition_id {
                                            if let Some(track_id) = self.selected_track_id {
                                                project_service
                                                    .with_track_mut(
                                                        comp_id,
                                                        track_id,
                                                        |track_mut| {
                                                            if let Some(entity_mut) = track_mut
                                                                .entities
                                                                .iter_mut()
                                                                .find(|e| e.id == gc.id)
                                                            {
                                                                entity_mut.start_time = (entity_mut
                                                                    .start_time
                                                                    + dt as f64)
                                                                    .max(0.0);
                                                                entity_mut.end_time = (entity_mut
                                                                    .end_time
                                                                    + dt as f64)
                                                                    .max(entity_mut.start_time);
                                                            }
                                                        },
                                                    )
                                                    .ok();
                                                // No need to rebuild entity cache here, ProjectService's track operations handle it.
                                            }
                                        }
                                    }

                                    let is_sel = self.selected_entity_id == Some(gc.id);
                                    let color = gc.color;
                                    let transparent_color = egui::Color32::from_rgba_premultiplied(
                                        color.r(),
                                        color.g(),
                                        color.b(),
                                        150,
                                    );

                                    painter.rect_filled(clip_rect, 4.0, transparent_color);
                                    if is_sel {
                                        painter.rect_stroke(
                                            clip_rect,
                                            4.0,
                                            egui::Stroke::new(2.0, egui::Color32::WHITE),
                                        );
                                    }
                                    painter.text(
                                        clip_rect.center(),
                                        egui::Align2::CENTER_CENTER,
                                        &gc.name,
                                        egui::FontId::default(),
                                        egui::Color32::BLACK,
                                    );
                                }
                            }
                        }
                    }
                }

                if clicked_on_entity {
                    // Selected entity is already set
                } else if response.clicked() && !is_dragging_asset {
                    self.selected_entity_id = None;
                }

                let cx = content_rect.min.x
                    + self.timeline_scroll_offset.x
                    + self.current_time * time_scale;
                if cx > content_rect.min.x && cx < content_rect.max.x {
                    painter.line_segment(
                        [
                            egui::pos2(cx, content_rect.min.y),
                            egui::pos2(cx, content_rect.max.y),
                        ],
                        egui::Stroke::new(2.0, egui::Color32::RED),
                    );
                }
            });
        });
    }

    // 4. インスペクタ
    fn show_inspector(
        &mut self,
        ui: &mut egui::Ui,
        project_service: &ProjectService,
        project: &Arc<RwLock<Project>>,
    ) {
        let mut needs_refresh = false;

        // Display compositions
        ui.heading("Compositions");
        ui.separator();
        egui::ScrollArea::vertical()
            .id_source("compositions_scroll_area")
            .max_height(200.0)
            .show(ui, |ui| {
                if let Ok(proj_read) = project.read() {
                    for comp in &proj_read.compositions {
                        ui.push_id(comp.id, |ui_in_scope| {
                            // Correct usage of push_id with closure
                            let is_selected = self.selected_composition_id == Some(comp.id);
                            let response = ui_in_scope
                                .selectable_label(is_selected, &comp.name)
                                .on_hover_text(format!("Comp ID: {}", comp.id));
                            if response.clicked() {
                                self.selected_composition_id = Some(comp.id);
                                self.selected_track_id = None; // Deselect track when composition changes
                                self.selected_entity_id = None; // Deselect entity when composition changes
                            }
                        });
                    }
                }
            });

        ui.horizontal(|ui| {
            if ui.button("Add Comp").clicked() {
                let new_comp_id = project_service
                    .add_composition("New Composition", 1920, 1080, 30.0, 60.0)
                    .expect("Failed to add composition");
                self.selected_composition_id = Some(new_comp_id);
                // Also add a corresponding asset
                self.assets.push(Asset {
                    name: format!("Comp: New Composition"),
                    duration: 60.0,
                    color: egui::Color32::from_rgb(255, 150, 255),
                    kind: AssetKind::Composition(new_comp_id),
                    composition_id: Some(new_comp_id),
                });
                needs_refresh = true;
            }
            if ui.button("Remove Comp").clicked() {
                if let Some(comp_id) = self.selected_composition_id {
                    project_service
                        .remove_composition(comp_id)
                        .expect("Failed to remove composition");
                    // Also remove the corresponding asset
                    self.assets.retain(
                        |asset| !matches!(asset.kind, AssetKind::Composition(id) if id == comp_id),
                    );
                    self.selected_composition_id = None;
                    self.selected_track_id = None;
                    self.selected_entity_id = None;
                    needs_refresh = true;
                }
            }
        });

        ui.add_space(10.0);

        // Display tracks for selected composition
        if let Some(comp_id) = self.selected_composition_id {
            ui.heading(format!("Tracks in Comp: {}", comp_id)); // Displaying ID for now
            ui.separator();
            egui::ScrollArea::vertical()
                .id_source("tracks_scroll_area")
                .max_height(200.0)
                .show(ui, |ui| {
                    if let Ok(proj_read) = project.read() {
                        if let Some(comp) = proj_read.compositions.iter().find(|c| c.id == comp_id)
                        {
                            for track in &comp.tracks {
                                ui.push_id(track.id, |ui_in_scope| {
                                    // Correct usage of push_id with closure
                                    let is_selected = self.selected_track_id == Some(track.id);
                                    let response = ui_in_scope
                                        .selectable_label(is_selected, &track.name)
                                        .on_hover_text(format!("Track ID: {}", track.id));
                                    if response.clicked() {
                                        self.selected_track_id = Some(track.id);
                                        self.selected_entity_id = None; // Deselect entity when track changes
                                    }
                                });
                            }
                        }
                    }
                });

            // Removed "Add Track" and "Remove Track" buttons from here.
            ui.add_space(10.0);
        }

        // Display properties of selected entity
        if let Some(selected_entity_id) = self.selected_entity_id {
            if let Some(comp_id) = self.selected_composition_id {
                if let Some(track_id) = self.selected_track_id {
                    // Use inspector_entity_cache
                    if let Some((cached_entity_id, cached_entity_type, cached_properties, cached_start_time, cached_end_time)) =
                        self.inspector_entity_cache.as_mut()
                    {
                        // Ensure the cached entity matches the actually selected entity
                        if *cached_entity_id == selected_entity_id {
                            ui.heading("Entity Properties");
                            ui.separator();

                            let mut current_entity_type = cached_entity_type.clone();
                            ui.horizontal(|ui| {
                                ui.label("Type");
                                if ui.text_edit_singleline(&mut current_entity_type).changed() {
                                    *cached_entity_type = current_entity_type.clone();
                                    project_service
                                        .with_track_mut(comp_id, track_id, |track_mut| {
                                            if let Some(entity_mut) = track_mut
                                                .entities
                                                .iter_mut()
                                                .find(|e| e.id == selected_entity_id)
                                            {
                                                entity_mut.entity_type = cached_entity_type.clone();
                                            }
                                        })
                                        .ok();
                                    needs_refresh = true;
                                }
                            });

                            egui::Grid::new("entity_props")
                                .striped(true)
                                .show(ui, |ui| {
                                    // position_x
                                    let mut pos_x = cached_properties.get_f32("position_x").unwrap_or(960.0);
                                    ui.label("Position X");
                                    if ui
                                        .add(egui::DragValue::new(&mut pos_x).speed(1.0).suffix("px"))
                                        .changed()
                                    {
                                        cached_properties.set("position_x".to_string(), Property::constant(PropertyValue::Number(pos_x as f64)));
                                        project_service
                                            .update_entity_property(
                                                comp_id,
                                                track_id,
                                                selected_entity_id,
                                                "position_x",
                                                PropertyValue::Number(pos_x as f64),
                                            )
                                            .ok();
                                        needs_refresh = true;
                                    }
                                    ui.end_row();

                                    // position_y
                                    let mut pos_y = cached_properties.get_f32("position_y").unwrap_or(540.0);
                                    ui.label("Position Y");
                                    if ui
                                        .add(egui::DragValue::new(&mut pos_y).speed(1.0).suffix("px"))
                                        .changed()
                                    {
                                        cached_properties.set("position_y".to_string(), Property::constant(PropertyValue::Number(pos_y as f64)));
                                        project_service
                                            .update_entity_property(
                                                comp_id,
                                                track_id,
                                                selected_entity_id,
                                                "position_y",
                                                PropertyValue::Number(pos_y as f64),
                                            )
                                            .ok();
                                        needs_refresh = true;
                                    }
                                    ui.end_row();

                                    // scale
                                    let mut scale = cached_properties.get_f32("scale").unwrap_or(100.0);
                                    ui.label("Scale");
                                    if ui
                                        .add(egui::Slider::new(&mut scale, 0.0..=200.0).suffix("%"))
                                        .changed()
                                    {
                                        cached_properties.set("scale".to_string(), Property::constant(PropertyValue::Number(scale as f64)));
                                        project_service
                                            .update_entity_property(
                                                comp_id,
                                                track_id,
                                                selected_entity_id,
                                                "scale",
                                                PropertyValue::Number(scale as f64),
                                            )
                                            .ok();
                                        needs_refresh = true;
                                    }
                                    ui.end_row();

                                    // opacity
                                    let mut opacity = cached_properties.get_f32("opacity").unwrap_or(100.0);
                                    ui.label("Opacity");
                                    if ui
                                        .add(egui::Slider::new(&mut opacity, 0.0..=100.0).suffix("%"))
                                        .changed()
                                    {
                                        cached_properties.set("opacity".to_string(), Property::constant(PropertyValue::Number(opacity as f64)));
                                        project_service
                                            .update_entity_property(
                                                comp_id,
                                                track_id,
                                                selected_entity_id,
                                                "opacity",
                                                PropertyValue::Number(opacity as f64),
                                            )
                                            .ok();
                                        needs_refresh = true;
                                    }
                                    ui.end_row();

                                    // rotation
                                    let mut rotation = cached_properties.get_f32("rotation").unwrap_or(0.0);
                                    ui.label("Rotation");
                                    if ui
                                        .add(egui::DragValue::new(&mut rotation).speed(1.0).suffix("°"))
                                        .changed()
                                    {
                                        cached_properties.set("rotation".to_string(), Property::constant(PropertyValue::Number(rotation as f64)));
                                        project_service
                                            .update_entity_property(
                                                comp_id,
                                                track_id,
                                                selected_entity_id,
                                                "rotation",
                                                PropertyValue::Number(rotation as f64),
                                            )
                                            .ok();
                                        needs_refresh = true;
                                    }
                                    ui.end_row();

                                    // Start Time
                                    let mut current_start_time = *cached_start_time as f32;
                                    ui.label("Start Time");
                                    if ui
                                        .add(egui::DragValue::new(&mut current_start_time).speed(0.1))
                                        .changed()
                                    {
                                        *cached_start_time = current_start_time as f64;
                                        project_service
                                            .with_track_mut(comp_id, track_id, |track_mut| {
                                                if let Some(entity_mut) = track_mut
                                                    .entities
                                                    .iter_mut()
                                                    .find(|e| e.id == selected_entity_id)
                                                {
                                                    let duration =
                                                        entity_mut.end_time - entity_mut.start_time;
                                                    entity_mut.start_time = *cached_start_time;
                                                    entity_mut.end_time =
                                                        entity_mut.start_time + duration;
                                                }
                                            })
                                            .ok();
                                        needs_refresh = true;
                                    }
                                    ui.end_row();

                                    // End Time
                                    let mut current_end_time = *cached_end_time as f32;
                                    ui.label("End Time");
                                    if ui
                                        .add(egui::DragValue::new(&mut current_end_time).speed(0.1))
                                        .changed()
                                    {
                                        *cached_end_time = current_end_time as f64;
                                        project_service
                                            .with_track_mut(comp_id, track_id, |track_mut| {
                                                if let Some(entity_mut) = track_mut
                                                    .entities
                                                    .iter_mut()
                                                    .find(|e| e.id == selected_entity_id)
                                                {
                                                    entity_mut.end_time = *cached_end_time;
                                                }
                                            })
                                            .ok();
                                        needs_refresh = true;
                                    }
                                    ui.end_row();
                                });

                            if ui.button("🗑 Delete Entity").clicked() {
                                if let Err(e) = project_service.remove_entity_from_track(
                                    comp_id,
                                    track_id,
                                    selected_entity_id,
                                ) {
                                    eprintln!("Failed to remove entity: {:?}", e);
                                } else {
                                    self.selected_entity_id = None;
                                    self.inspector_entity_cache = None; // Clear cache on deletion
                                    needs_refresh = true;
                                }
                            }
                        } else {
                            // This case should ideally not be hit if cache management in MyApp::update is correct
                            ui.label("Inspector cache is stale or mismatched. Please re-select entity.");
                            self.inspector_entity_cache = None; // Invalidate cache
                        }
                    } else {
                        ui.label("Inspector cache not populated for selected entity.");
                    }
                } else {
                    ui.label("No track selected for entity properties.");
                }
            } else {
                ui.label("No composition selected for entity properties.");
            }
        } else {
            ui.label("Select an entity to edit");
        }

        if needs_refresh {
            ui.ctx().request_repaint();
        }
    }
}

// --- 3. メイン構造体 ---

struct EditorTabViewer<'a> {
    context: &'a mut EditorContext,
    project_service: &'a ProjectService,
    project: &'a Arc<RwLock<Project>>,
}
impl<'a> egui_dock::TabViewer for EditorTabViewer<'a> {
    type Tab = Tab;
    fn title(&mut self, tab: &mut Self::Tab) -> egui::WidgetText {
        match tab {
            Tab::Preview => "📺 Preview".into(),
            Tab::Timeline => "⏱ Timeline".into(),
            Tab::Inspector => "🔧 Inspector".into(),
            Tab::Assets => "📁 Assets".into(),
        }
    }
    fn ui(&mut self, ui: &mut egui::Ui, tab: &mut Self::Tab) {
        match tab {
            Tab::Preview => self
                .context
                .show_preview(ui, self.project_service, self.project),
            Tab::Timeline => self
                .context
                .show_timeline(ui, self.project_service, self.project),
            Tab::Inspector => self
                .context
                .show_inspector(ui, self.project_service, self.project),
            Tab::Assets => self.context.show_assets(ui),
        }
    }
}

struct MyApp {
    context: EditorContext,
    dock_state: DockState<Tab>,
    project_service: ProjectService,
    project: Arc<RwLock<Project>>,
}
impl MyApp {
    fn new(cc: &eframe::CreationContext<'_>) -> Self {
        setup_custom_fonts(&cc.egui_ctx); // フォント設定

        let mut dock_state = DockState::new(vec![Tab::Preview]);
        let surface = dock_state.main_surface_mut();

        // 1. Split off the timeline at the bottom (30% of height)
        let [main_area, _] = surface.split_below(NodeIndex::root(), 0.7, vec![Tab::Timeline]);

        // 2. Split off the inspector on the right (20% of width)
        // The remaining area is 80% wide, so we split at 0.8
        let [main_area, _] = surface.split_right(main_area, 0.8, vec![Tab::Inspector]);

        // 3. Split off the assets on the left (20% of original width)
        // The remaining area is 80% wide. 0.2 / 0.8 = 0.25
        surface.split_left(main_area, 0.25, vec![Tab::Assets]);

        let project = Arc::new(RwLock::new(Project::new("Default Project")));
        // Add a default composition when the app starts
        let default_comp = Composition::new("Main Composition", 1920, 1080, 30.0, 60.0);
        let default_comp_id = default_comp.id;
        project.write().unwrap().add_composition(default_comp);

        let project_service = ProjectService::new(Arc::clone(&project));

        let mut context = EditorContext::new(default_comp_id); // Pass default_comp_id
        context.selected_composition_id = Some(default_comp_id); // Select the default composition

        Self {
            context,
            dock_state,
            project_service,
            project,
        }
    }

    fn reset_layout(&mut self) {
        let mut dock_state = DockState::new(vec![Tab::Preview]);
        let surface = dock_state.main_surface_mut();

        // 1. Split off the timeline at the bottom (30% of height)
        let [main_area, _] = surface.split_below(NodeIndex::root(), 0.7, vec![Tab::Timeline]);

        // 2. Split off the inspector on the right (20% of width)
        // The remaining area is 80% wide, so we split at 0.8
        let [main_area, _] = surface.split_right(main_area, 0.8, vec![Tab::Inspector]);

        // 3. Split off the assets on the left (20% of original width)
        // The remaining area is 80% wide. 0.2 / 0.8 = 0.25
        surface.split_left(main_area, 0.25, vec![Tab::Assets]);

        self.dock_state = dock_state;
    }
}

impl eframe::App for MyApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        egui::TopBottomPanel::top("menu_bar").show(ctx, |ui| {
            egui::menu::bar(ui, |ui| {
                ui.menu_button("ファイル", |ui| {
                    if ui.button("新規プロジェクト").clicked() {
                        let new_comp_id = self
                            .project_service
                            .add_composition("Main Composition", 1920, 1080, 30.0, 60.0)
                            .expect("Failed to add composition");
                        self.context.selected_composition_id = Some(new_comp_id);
                        ui.close_menu();
                    }
                    if ui.button("プロジェクトを開く").clicked() {
                        if let Some(path) = rfd::FileDialog::new()
                            .add_filter("Project File", &["json"])
                            .pick_file()
                        {
                            match fs::read_to_string(&path) {
                                Ok(json_str) => {
                                    if let Err(e) = self.project_service.load_project(&json_str) {
                                        eprintln!("Failed to load project: {}", e);
                                    } else {
                                        println!("Project loaded from {}", path.display());
                                    }
                                }
                                Err(e) => eprintln!("Failed to read project file: {}", e),
                            }
                        }
                        ui.close_menu();
                    }
                    if ui.button("プロジェクトを保存").clicked() {
                        if let Some(path) = rfd::FileDialog::new()
                            .add_filter("Project File", &["json"])
                            .set_file_name("project.json")
                            .save_file()
                        {
                            match self.project_service.save_project() {
                                Ok(json_str) => match fs::File::create(&path) {
                                    Ok(mut file) => {
                                        if let Err(e) = file.write_all(json_str.as_bytes()) {
                                            eprintln!("Failed to write project to file: {}", e);
                                        } else {
                                            println!("Project saved to {}", path.display());
                                        }
                                    }
                                    Err(e) => eprintln!("Failed to create file: {}", e),
                                },
                                Err(e) => eprintln!("Failed to save project: {}", e),
                            }
                        }
                        ui.close_menu();
                    }
                    ui.separator();
                    if ui.button("終了").clicked() {
                        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                    }
                });

                ui.menu_button("編集", |ui| {
                    if ui.button("コピー").clicked() {
                        println!("コピー (Mock)");
                        ui.close_menu();
                    }
                    if ui.button("ペースト").clicked() {
                        println!("ペースト (Mock)");
                        ui.close_menu();
                    }
                    ui.separator();
                    if ui.button("削除").clicked() {
                        if let Some(comp_id) = self.context.selected_composition_id {
                            if let Some(track_id) = self.context.selected_track_id {
                                if let Some(entity_id) = self.context.selected_entity_id {
                                    if let Err(e) = self
                                        .project_service
                                        .remove_entity_from_track(comp_id, track_id, entity_id)
                                    {
                                        eprintln!("Failed to remove entity: {:?}", e);
                                    } else {
                                        self.context.selected_entity_id = None;
                                    }
                                }
                            }
                        }
                        ui.close_menu();
                    }
                });

                ui.menu_button("表示", |ui| {
                    if ui.button("レイアウトをリセット").clicked() {
                        self.reset_layout();
                        ui.close_menu();
                    }
                });
            });
        });

        if !ctx.wants_keyboard_input() {
            if ctx.input(|i| i.key_pressed(egui::Key::Space)) {
                self.context.is_playing = !self.context.is_playing;
            }
            if ctx.input(|i| i.key_pressed(egui::Key::Delete)) {
                if let Some(comp_id) = self.context.selected_composition_id {
                    if let Some(track_id) = self.context.selected_track_id {
                        if let Some(entity_id) = self.context.selected_entity_id {
                            if let Err(e) = self
                                .project_service
                                .remove_entity_from_track(comp_id, track_id, entity_id)
                            {
                                eprintln!("Failed to remove entity: {:?}", e);
                            } else {
                                self.context.selected_entity_id = None;
                            }
                            // Request repaint if entity was removed
                            ctx.request_repaint();
                        }
                    }
                }
            }
        }

        // Manage inspector_entity_cache
        let current_selected_entity_id = self.context.selected_entity_id;
        let current_selected_composition_id = self.context.selected_composition_id;
        let current_selected_track_id = self.context.selected_track_id;

        let mut should_update_cache = false;

        // Check if selected_entity_id changed or if cache is empty
        if let Some(selected_id) = current_selected_entity_id {
            if self.context.inspector_entity_cache.is_none() || self.context.inspector_entity_cache.as_ref().unwrap().0 != selected_id {
                should_update_cache = true;
            }
        } else {
            // No entity selected, clear cache
            if self.context.inspector_entity_cache.is_some() {
                self.context.inspector_entity_cache = None;
            }
        }

        if should_update_cache {
            // Populate cache with new selected entity's data
            if let (Some(entity_id), Some(comp_id), Some(track_id)) = (current_selected_entity_id, current_selected_composition_id, current_selected_track_id) {
                if let Ok(proj_read) = self.project.read() {
                    if let Some(comp) = proj_read.compositions.iter().find(|c| c.id == comp_id) {
                        if let Some(track) = comp.tracks.iter().find(|t| t.id == track_id) {
                            if let Some(entity) = track.entities.iter().find(|e| e.id == entity_id) {
                                self.context.inspector_entity_cache = Some((
                                    entity.id,
                                    entity.entity_type.clone(),
                                    entity.properties.clone(),
                                    entity.start_time,
                                    entity.end_time,
                                ));
                            }
                        }
                    }
                }
            }
        }

        egui::TopBottomPanel::bottom("status_bar").show(ctx, |ui| {
            ui.horizontal(|ui| {
                // Add status bar content here
                ui.label("Ready");
                ui.separator();
                ui.label(format!("Time: {:.2}", self.context.current_time));
            });
        });

        egui::CentralPanel::default().show(ctx, |ui| {
            let mut tab_viewer = EditorTabViewer {
                context: &mut self.context,
                project_service: &self.project_service, // Pass reference to project_service
                project: &self.project,                 // Pass reference to project
            };
            DockArea::new(&mut self.dock_state)
                .style(Style::from_egui(ui.style().as_ref()))
                .show_inside(ui, &mut tab_viewer);

            if self.context.is_playing {
                ui.ctx().request_repaint(); // Request repaint to update time
            }
        });

        if ctx.input(|i| i.pointer.any_released()) {
            self.context.dragged_asset = None;
        }

        if self.context.is_playing {
            self.context.current_time += 0.016; // Assuming 60fps
        }
    }
}

fn main() -> eframe::Result<()> {
    env_logger::init();
    eframe::run_native(
        "Video Editor with Canvas",
        eframe::NativeOptions {
            viewport: egui::ViewportBuilder::default().with_inner_size([1280.0, 720.0]),
            ..Default::default()
        },
        Box::new(|cc| Ok(Box::new(MyApp::new(cc)))),
    )
}
