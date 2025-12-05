use eframe::egui;
use egui_dock::{DockArea, DockState, NodeIndex, Style};
use serde::{Deserialize, Serialize};
use std::fs::File;
use std::io::{BufReader, BufWriter};

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
}

#[derive(Clone, PartialEq, Serialize, Deserialize)]
enum AssetKind {
    Video,
    Audio,
}

#[derive(Clone, Serialize, Deserialize)]
struct Clip {
    id: usize,
    asset_index: usize,
    name: String,
    track: usize,
    start_time: f32,
    duration: f32,
    #[serde(with = "ColorDef")]
    color: egui::Color32,
    // プロパティ
    position: [f32; 2],
    scale: f32,
    opacity: f32,
    rotation: f32,
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
        egui::Color32::from_rgba_premultiplied(def.0.0, def.0.1, def.0.2, def.0.3)
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

    if let Ok(font_data) = std::fs::read(font_path) {
        fonts.font_data.insert(
            "my_font".to_owned(),
            egui::FontData::from_owned(font_data).tweak(
                egui::FontTweak {
                    scale: 1.2,
                    ..Default::default()
                }
            ),
        );

        // 優先順位の先頭に追加
        fonts.families.entry(egui::FontFamily::Proportional).or_default().insert(0, "my_font".to_owned());
        fonts.families.entry(egui::FontFamily::Monospace).or_default().insert(0, "my_font".to_owned());

        ctx.set_fonts(fonts);
    } else {
        eprintln!("Warning: Failed to load font from {}", font_path);
    }
}

#[derive(Serialize, Deserialize)]
struct EditorContext {
    assets: Vec<Asset>,
    clips: Vec<Clip>,
    next_clip_id: usize,

    selected_clip_id: Option<usize>,
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
}

impl EditorContext {
    fn new() -> Self {
        let assets = vec![
            Asset { name: "Intro_Seq.mp4".into(), duration: 5.0, color: egui::Color32::from_rgb(100, 150, 255), kind: AssetKind::Video },
            Asset { name: "Main_Cam.mov".into(), duration: 15.0, color: egui::Color32::from_rgb(80, 120, 200), kind: AssetKind::Video },
            Asset { name: "BGM_Happy.mp3".into(), duration: 30.0, color: egui::Color32::from_rgb(100, 255, 150), kind: AssetKind::Audio },
            Asset { name: "Text_Overlay.png".into(), duration: 5.0, color: egui::Color32::from_rgb(255, 100, 150), kind: AssetKind::Video },
            Asset { name: "Logo.png".into(), duration: 5.0, color: egui::Color32::from_rgb(255, 200, 100), kind: AssetKind::Video },
        ];

        Self {
            assets,
            clips: Vec::new(),
            next_clip_id: 0,
            selected_clip_id: None,
            current_time: 0.0,
            is_playing: false,
            timeline_pixels_per_second: 50.0,

            view_pan: egui::vec2(20.0, 20.0),
            view_zoom: 0.3,
            dragged_asset: None,

            timeline_v_zoom: 1.0,
            timeline_h_zoom: 1.0,
            timeline_scroll_offset: egui::Vec2::ZERO,
        }
    }

    fn add_clip(&mut self, asset_index: usize, track: usize, start_time: f32) {
        if let Some(asset) = self.assets.get(asset_index) {
            let clip = Clip {
                id: self.next_clip_id,
                asset_index,
                name: asset.name.clone(),
                track,
                start_time,
                duration: asset.duration,
                color: asset.color,
                position: [960.0, 540.0], // FHD中心
                scale: 100.0,
                opacity: 100.0,
                rotation: 0.0,
            };
            self.clips.push(clip);
            self.selected_clip_id = Some(self.next_clip_id);
            self.next_clip_id += 1;
        }
    }

    fn get_selected_clip_mut(&mut self) -> Option<&mut Clip> {
        if let Some(id) = self.selected_clip_id {
            self.clips.iter_mut().find(|c| c.id == id)
        } else {
            None
        }
    }

    fn delete_selected_clip(&mut self) {
        if let Some(id) = self.selected_clip_id {
            self.clips.retain(|c| c.id != id);
            self.selected_clip_id = None;
        }
    }

    // --- ファイル保存・読み込み ---

    fn save_to_file(&self) {
        if let Some(path) = rfd::FileDialog::new()
          .add_filter("Project File", &["bin"])
          .set_file_name("project.bin")
          .save_file()
        {
            match File::create(path) {
                Ok(file) => {
                    let writer = BufWriter::new(file);
                    if let Err(e) = bincode::serialize_into(writer, self) {
                        eprintln!("Failed to serialize: {}", e);
                    } else {
                        println!("Project saved.");
                    }
                }
                Err(e) => eprintln!("Failed to create file: {}", e),
            }
        }
    }

    fn load_from_file(&mut self) {
        if let Some(path) = rfd::FileDialog::new()
          .add_filter("Project File", &["bin"])
          .pick_file()
        {
            match File::open(path) {
                Ok(file) => {
                    let reader = BufReader::new(file);
                    match bincode::deserialize_from(reader) {
                        Ok(loaded) => {
                            let mut loaded: EditorContext = loaded;
                            loaded.dragged_asset = None; // Ensure transient state is reset
                            *self = loaded;
                            println!("Project loaded.");
                        }
                        Err(e) => eprintln!("Failed to deserialize: {}", e),
                    }
                }
                Err(e) => eprintln!("Failed to open file: {}", e),
            }
        }
    }

    // --- UI実装 ---

    // 1. Canvasプレビュー
    fn show_preview(&mut self, ui: &mut egui::Ui) {
        let (rect, response) = ui.allocate_exact_size(ui.available_size(), egui::Sense::click_and_drag());

        let pointer_pos = ui.input(|i| i.pointer.hover_pos());
        let space_down = ui.input(|i| i.key_down(egui::Key::Space));
        let middle_down = ui.input(|i| i.pointer.button_down(egui::PointerButton::Middle));
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
                    self.view_pan = mouse_in_canvas - (mouse_in_canvas - self.view_pan) * (self.view_zoom / old_zoom);
                }
            }
        }

        let view_offset = rect.min + self.view_pan;
        let view_zoom = self.view_zoom;

        let to_screen = |pos: egui::Pos2| -> egui::Pos2 {
            view_offset + (pos.to_vec2() * view_zoom)
        };
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
                painter.line_segment([egui::pos2(x, rect.min.y), egui::pos2(x, rect.max.y)], egui::Stroke::new(1.0, grid_color));
            }
            for i in 0..rows {
                let y = rect.min.y + start_y + (i as f32) * grid_size;
                painter.line_segment([egui::pos2(rect.min.x, y), egui::pos2(rect.max.x, y)], egui::Stroke::new(1.0, grid_color));
            }
        }

        // フルHDフレーム
        let frame_rect = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1920.0, 1080.0));
        let screen_frame_min = to_screen(frame_rect.min);
        let screen_frame_max = to_screen(frame_rect.max);
        painter.rect_stroke(
            egui::Rect::from_min_max(screen_frame_min, screen_frame_max),
            0.0,
            egui::Stroke::new(2.0 * self.view_zoom.max(1.0), egui::Color32::WHITE)
        );

        // クリップヒットテスト
        let mut hovered_clip_id = None;
        if let Some(mouse_screen_pos) = pointer_pos {
            if rect.contains(mouse_screen_pos) {
                let mut sorted_indices: Vec<usize> = self.clips.iter().enumerate()
                  .filter(|(_, c)| self.current_time >= c.start_time && self.current_time < c.start_time + c.duration)
                  .map(|(i, _)| i)
                  .collect();
                sorted_indices.sort_by_key(|&i| self.clips[i].track);

                for &idx in sorted_indices.iter().rev() {
                    let clip = &self.clips[idx];
                    let is_audio = self.assets.get(clip.asset_index).map(|a| a.kind == AssetKind::Audio).unwrap_or(false);
                    if is_audio { continue; }

                    let mouse_world_pos = to_world(mouse_screen_pos);
                    let center = egui::pos2(clip.position[0], clip.position[1]);

                    let vec = mouse_world_pos - center;
                    let angle_rad = -clip.rotation.to_radians();
                    let cos = angle_rad.cos();
                    let sin = angle_rad.sin();
                    let local_x = vec.x * cos - vec.y * sin;
                    let local_y = vec.x * sin + vec.y * cos;

                    let base_w = 640.0;
                    let base_h = 360.0;
                    let half_w = (base_w * clip.scale / 100.0) / 2.0;
                    let half_h = (base_h * clip.scale / 100.0) / 2.0;

                    if local_x.abs() <= half_w && local_y.abs() <= half_h {
                        hovered_clip_id = Some(clip.id);
                        break;
                    }
                }
            }
        }

        // クリップ描画
        let mut visible_indices: Vec<usize> = self.clips.iter().enumerate()
          .filter(|(_, c)| self.current_time >= c.start_time && self.current_time < c.start_time + c.duration)
          .map(|(i, _)| i)
          .collect();
        visible_indices.sort_by_key(|&i| self.clips[i].track);

        for &idx in &visible_indices {
            let clip = &self.clips[idx];
            let is_audio = self.assets.get(clip.asset_index).map(|a| a.kind == AssetKind::Audio).unwrap_or(false);
            if is_audio { continue; }

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
                egui::vec2(-w/2.0, -h/2.0),
                egui::vec2( w/2.0, -h/2.0),
                egui::vec2( w/2.0,  h/2.0),
                egui::vec2(-w/2.0,  h/2.0),
            ];

            let screen_points: Vec<egui::Pos2> = local_corners.iter().map(|corner| {
                let rot_x = corner.x * cos_r - corner.y * sin_r;
                let rot_y = corner.x * sin_r + corner.y * cos_r;
                to_screen(center + egui::vec2(rot_x, rot_y))
            }).collect();

            painter.add(egui::Shape::convex_polygon(
                screen_points.clone(),
                color,
                egui::Stroke::NONE,
            ));
        }

        let interacted_with_gizmo = false;

        // 選択・移動
        if !is_panning_input && !interacted_with_gizmo {
            if response.clicked() {
                self.selected_clip_id = hovered_clip_id;
            } else if response.dragged() {
                if let Some(_sid) = self.selected_clip_id {
                    let current_zoom = self.view_zoom;
                    if let Some(clip) = self.get_selected_clip_mut() {
                        let world_delta = response.drag_delta() / current_zoom;
                        clip.position[0] += world_delta.x;
                        clip.position[1] += world_delta.y;
                    }
                }
            }
        }

        // 情報
        let info_text = format!("Time: {:.2}\nZoom: {:.0}%", self.current_time, self.view_zoom * 100.0);
        painter.text(rect.left_top() + egui::vec2(10.0, 10.0), egui::Align2::LEFT_TOP, info_text, egui::FontId::monospace(14.0), egui::Color32::WHITE);
    }

    // 2. アセット
    fn show_assets(&mut self, ui: &mut egui::Ui) {
        ui.heading("Assets");
        ui.separator();
        egui::ScrollArea::vertical().show(ui, |ui| {
            for (index, asset) in self.assets.iter().enumerate() {
                let label_text = format!("{} ({:.1}s)", asset.name, asset.duration);

                let item_response = egui::Frame::none().show(ui, |ui| {
                     ui.horizontal(|ui| {
                        ui.label(egui::RichText::new(if asset.kind == AssetKind::Video { "📹" } else { "🎵" }).strong());
                        ui.label(egui::RichText::new(&label_text).background_color(asset.color).color(egui::Color32::BLACK));
                    });
                }).response.interact(egui::Sense::drag());

                if item_response.drag_started() {
                    self.dragged_asset = Some(index);
                }
                 ui.add_space(5.0);
            }
        });
    }

    fn show_timeline_ruler(&mut self, ui: &mut egui::Ui) {
        let (outer_rect, _) = ui.allocate_exact_size(egui::vec2(ui.available_width(), ui.available_height()), egui::Sense::hover());
        
        ui.horizontal(|ui| {
            // Spacer for the track list
            let (spacer_rect, _) = ui.allocate_exact_size(egui::vec2(100.0, outer_rect.height()), egui::Sense::hover());
            ui.painter_at(spacer_rect).rect_filled(spacer_rect, 0.0, ui.style().visuals.widgets.noninteractive.bg_fill);

            // The actual ruler
            let (rect, _) = ui.allocate_at_least(egui::vec2(ui.available_width(), outer_rect.height()), egui::Sense::hover());
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
            let last_second = ((-self.timeline_scroll_offset.x + rect.width()) / time_scale).ceil() as i32;

            for sec in first_second..=last_second {
                let s = sec as f32;
                if s % minor_interval == 0.0 {
                    let x = start_x + s * time_scale;
                    if x >= rect.min.x && x <= rect.max.x {
                        let is_major = s % major_interval == 0.0;
                        let height = if is_major { rect.height() } else { rect.height() * 0.5 };
                        painter.line_segment(
                            [egui::pos2(x, rect.max.y - height), egui::pos2(x, rect.max.y)],
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
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui|{
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
    fn show_timeline(&mut self, ui: &mut egui::Ui) {
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
                let (track_list_rect, _) = ui.allocate_exact_size(egui::vec2(100.0, ui.available_height()), egui::Sense::hover());
                let track_list_painter = ui.painter_at(track_list_rect);
                track_list_painter.rect_filled(track_list_rect, 0.0, ui.style().visuals.window_fill()); // Fill entire sidebar background

                let row_height = 30.0;
                let track_spacing = 2.0;
                let num_tracks = 5;

                for i in 0..num_tracks {
                    let y = track_list_rect.min.y + (i as f32 * (row_height + track_spacing)) + self.timeline_scroll_offset.y;
                    let track_label_rect = egui::Rect::from_min_size(egui::pos2(track_list_rect.min.x, y), egui::vec2(track_list_rect.width(), row_height));
                    
                    if track_list_rect.intersects(track_label_rect) {
                        // Draw alternating background for this row
                        track_list_painter.rect_filled(
                            track_label_rect,
                            0.0,
                            if i % 2 == 0 { egui::Color32::from_gray(50) } else { egui::Color32::from_gray(60) }
                        );
                        // Draw text label
                        track_list_painter.text(
                            track_label_rect.left_center() + egui::vec2(5.0, 0.0),
                            egui::Align2::LEFT_CENTER,
                            format!("Track {}", i + 1),
                            egui::FontId::monospace(10.0),
                            egui::Color32::GRAY
                        );
                    }
                }

                ui.separator();

                // --- Clip area ---
                let (content_rect, response) = ui.allocate_at_least(ui.available_size(), egui::Sense::click_and_drag());
                
                // --- Interaction ---
                if response.hovered() {
                    let scroll_delta = ui.input(|i| i.raw_scroll_delta);
                    if ui.input(|i| i.modifiers.ctrl) && scroll_delta.y != 0.0 {
                        let zoom_factor = if scroll_delta.y > 0.0 { 1.1 } else { 0.9 };
                        self.timeline_h_zoom = (self.timeline_h_zoom * zoom_factor).clamp(0.1, 10.0);
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
                let max_scroll_y = (num_tracks as f32 * (row_height + track_spacing)) - content_rect.height();
                self.timeline_scroll_offset.y = self.timeline_scroll_offset.y.clamp(-max_scroll_y.max(0.0), 0.0);
                self.timeline_scroll_offset.x = self.timeline_scroll_offset.x.min(0.0);

                for i in 0..num_tracks {
                    let y = content_rect.min.y + (i as f32 * (row_height + track_spacing)) + self.timeline_scroll_offset.y;
                    let track_rect = egui::Rect::from_min_size(
                        egui::pos2(content_rect.min.x, y),
                        egui::vec2(content_rect.width(), row_height)
                    );
                     painter.rect_filled(
                        track_rect,
                        0.0,
                        if i % 2 == 0 { egui::Color32::from_gray(50) } else { egui::Color32::from_gray(60) }
                    );
                }

                if let Some(asset_index) = self.dragged_asset {
                     if let Some(mouse_pos) = response.hover_pos() {
                        let drop_time = ((mouse_pos.x - content_rect.min.x - self.timeline_scroll_offset.x) / time_scale).max(0.0);
                        let drop_track = ((mouse_pos.y - content_rect.min.y - self.timeline_scroll_offset.y) / (row_height + track_spacing)).floor() as usize;

                        if drop_track < num_tracks {
                            if let Some(asset) = self.assets.get(asset_index) {
                                let gx = content_rect.min.x + self.timeline_scroll_offset.x + drop_time * time_scale;
                                let gy = content_rect.min.y + self.timeline_scroll_offset.y + (drop_track as f32) * (row_height + track_spacing);
                                let gr = egui::Rect::from_min_size(egui::pos2(gx, gy), egui::vec2(asset.duration * time_scale, row_height));
                                painter.rect_filled(gr, 4.0, asset.color.linear_multiply(0.5));
                            }
                        }
                    }
                }

                if ui.input(|i| i.pointer.any_released()) {
                     if let Some(asset_index) = self.dragged_asset {
                        if let Some(mouse_pos) = response.hover_pos() {
                            let drop_time = ((mouse_pos.x - content_rect.min.x - self.timeline_scroll_offset.x) / time_scale).max(0.0);
                            let drop_track = ((mouse_pos.y - content_rect.min.y - self.timeline_scroll_offset.y) / (row_height + track_spacing)).floor() as usize;
                            if drop_track < num_tracks {
                                self.add_clip(asset_index, drop_track, drop_time);
                            }
                        }
                    }
                }

                let is_dragging_asset = self.dragged_asset.is_some();
                let mut clicked_on_clip = false;

                if !is_dragging_asset && response.is_pointer_button_down_on() {
                    if let Some(pos) = response.interact_pointer_pos() {
                        self.current_time = ((pos.x - content_rect.min.x - self.timeline_scroll_offset.x) / time_scale).max(0.0);
                    }
                }

                let mut next_selected = self.selected_clip_id;

                for (_idx, clip) in self.clips.iter_mut().enumerate() {
                    let x = content_rect.min.x + self.timeline_scroll_offset.x + clip.start_time * time_scale;
                    let y = content_rect.min.y + self.timeline_scroll_offset.y + (clip.track as f32) * (row_height + track_spacing);
                    let clip_rect = egui::Rect::from_min_size(egui::pos2(x, y), egui::vec2(clip.duration * time_scale, row_height));

                    let clip_resp = ui.interact(clip_rect, ui.id().with(clip.id), egui::Sense::click_and_drag());
                    if clip_resp.clicked() {
                        next_selected = Some(clip.id);
                        clicked_on_clip = true;
                    }

                    if clip_resp.drag_started() {
                        next_selected = Some(clip.id);
                    }
                    if clip_resp.dragged() && Some(clip.id) == self.selected_clip_id {
                        let dt = clip_resp.drag_delta().x / time_scale;
                        clip.start_time = (clip.start_time + dt).max(0.0);
                    }


                    let is_sel = Some(clip.id) == self.selected_clip_id;
                    let color = clip.color;
                    let transparent_color = egui::Color32::from_rgba_premultiplied(color.r(), color.g(), color.b(), 150);

                    painter.rect_filled(clip_rect, 4.0, transparent_color);
                    if is_sel { painter.rect_stroke(clip_rect, 4.0, egui::Stroke::new(2.0, egui::Color32::WHITE)); }
                    painter.text(clip_rect.center(), egui::Align2::CENTER_CENTER, &clip.name, egui::FontId::default(), egui::Color32::BLACK);
                }

                if clicked_on_clip {
                    self.selected_clip_id = next_selected;
                } else if response.clicked() && !is_dragging_asset {
                    self.selected_clip_id = None;
                }

                let cx = content_rect.min.x + self.timeline_scroll_offset.x + self.current_time * time_scale;
                if cx > content_rect.min.x && cx < content_rect.max.x {
                    painter.line_segment([egui::pos2(cx, content_rect.min.y), egui::pos2(cx, content_rect.max.y)], egui::Stroke::new(2.0, egui::Color32::RED));
                }
            });
        });
    }

    // 4. インスペクタ
    fn show_inspector(&mut self, ui: &mut egui::Ui) {
        if let Some(clip) = self.get_selected_clip_mut() {
            ui.heading("Clip Properties");
            ui.separator();
            ui.text_edit_singleline(&mut clip.name);
            egui::Grid::new("p").striped(true).show(ui, |ui| {
                ui.label("Position");
                ui.horizontal(|ui| {
                    ui.add(egui::DragValue::new(&mut clip.position[0]).speed(1.0).suffix("px"));
                    ui.add(egui::DragValue::new(&mut clip.position[1]).speed(1.0).suffix("px"));
                });
                ui.end_row();

                ui.label("Rotation");
                ui.add(egui::DragValue::new(&mut clip.rotation).speed(1.0).suffix("°"));
                ui.end_row();

                ui.label("Scale");
                ui.add(egui::Slider::new(&mut clip.scale, 0.0..=200.0).suffix("%"));
                ui.end_row();

                ui.label("Opacity");
                ui.add(egui::Slider::new(&mut clip.opacity, 0.0..=100.0).suffix("%"));
                ui.end_row();

                ui.label("Start Time");
                ui.add(egui::DragValue::new(&mut clip.start_time).speed(0.1));
                ui.end_row();
            });
            if ui.button("🗑 Delete Clip").clicked() {
                self.delete_selected_clip();
            }
        } else {
            ui.label("Select a clip to edit");
        }
    }
}

// --- 3. メイン構造体 ---

struct EditorTabViewer<'a> { context: &'a mut EditorContext }
impl<'a> egui_dock::TabViewer for EditorTabViewer<'a> {
    type Tab = Tab;
    fn title(&mut self, tab: &mut Self::Tab) -> egui::WidgetText { match tab { Tab::Preview => "📺 Preview".into(), Tab::Timeline => "⏱ Timeline".into(), Tab::Inspector => "🔧 Inspector".into(), Tab::Assets => "📁 Assets".into() } }
    fn ui(&mut self, ui: &mut egui::Ui, tab: &mut Self::Tab) {
        match tab {
            Tab::Preview => self.context.show_preview(ui),
            Tab::Timeline => self.context.show_timeline(ui),
            Tab::Inspector => self.context.show_inspector(ui),
            Tab::Assets => self.context.show_assets(ui),
        }
    }
}

struct MyApp { context: EditorContext, dock_state: DockState<Tab> }
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

        Self { context: EditorContext::new(), dock_state }
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
                    if ui.button("保存").clicked() {
                        self.context.save_to_file();
                        ui.close_menu();
                    }
                    if ui.button("開く").clicked() {
                        self.context.load_from_file();
                        ui.close_menu();
                    }
                    ui.separator();
                    if ui.button("終了").clicked() {
                        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                    }
                });

                ui.menu_button("編集", |ui| {
                    if ui.button("コピー").clicked() { println!("コピー (Mock)"); ui.close_menu(); }
                    if ui.button("ペースト").clicked() { println!("ペースト (Mock)"); ui.close_menu(); }
                    ui.separator();
                    if ui.button("削除").clicked() {
                        self.context.delete_selected_clip();
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
            if ctx.input(|i| i.key_pressed(egui::Key::Space)) { self.context.is_playing = !self.context.is_playing; }
            if ctx.input(|i| i.key_pressed(egui::Key::Delete)) { self.context.delete_selected_clip(); }
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
            let mut tab_viewer = EditorTabViewer { context: &mut self.context };
            DockArea::new(&mut self.dock_state).style(Style::from_egui(ui.style().as_ref())).show_inside(ui, &mut tab_viewer);
        });
        
        if ctx.input(|i| i.pointer.any_released()) {
            self.context.dragged_asset = None;
        }

        if self.context.is_playing {
            self.context.current_time += 0.016; // Assuming 60fps
            ctx.request_repaint();
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
