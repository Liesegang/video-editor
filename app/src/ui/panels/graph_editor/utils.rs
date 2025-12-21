use egui::{Pos2, Rect, Vec2};

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum PropertyComponent {
    Scalar,
    X,
    Y,
}


#[derive(Clone, Copy)]
pub struct GraphTransform {
    pub graph_rect: Rect,
    pub pan: Vec2,
    pub zoom_x: f32, // pixels per second
    pub zoom_y: f32, // pixels per unit
}

impl GraphTransform {
    pub fn new(graph_rect: Rect, pan: Vec2, zoom_x: f32, zoom_y: f32) -> Self {
        Self {
            graph_rect,
            pan,
            zoom_x,
            zoom_y,
        }
    }

    pub fn to_screen(&self, time: f64, value: f64) -> Pos2 {
        let x = self.graph_rect.min.x + self.pan.x + (time as f32 * self.zoom_x);
        let zero_y = self.graph_rect.center().y + self.pan.y;
        let y = zero_y - (value as f32 * self.zoom_y);
        Pos2::new(x, y)
    }

    pub fn from_screen(&self, pos: Pos2) -> (f64, f64) {
        let x = pos.x;
        let time = (x - self.graph_rect.min.x - self.pan.x) / self.zoom_x;
        let zero_y = self.graph_rect.center().y + self.pan.y;
        let y = pos.y;
        let value = (zero_y - y) / self.zoom_y;
        (time as f64, value as f64)
    }
}

#[derive(Clone, Copy)]
pub struct TimeMapper {
    pub clip_start_frame: i64,
    pub clip_source_begin_frame: i64,
    pub clip_fps: f64,
    pub clip_inherent_fps: f64,
}

impl TimeMapper {
    pub fn to_source_time(&self, global_time: f64) -> f64 {
        let in_time = self.clip_start_frame as f64 / self.clip_fps;
        let source_start_time = self.clip_source_begin_frame as f64 / self.clip_inherent_fps;
        source_start_time + (global_time - in_time)
    }

    pub fn to_global_time(&self, source_time: f64) -> f64 {
        let in_time = self.clip_start_frame as f64 / self.clip_fps;
        let source_start_time = self.clip_source_begin_frame as f64 / self.clip_inherent_fps;
        in_time + (source_time - source_start_time)
    }
}
