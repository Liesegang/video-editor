use egui::{Pos2, Rect, Vec2};
use library::model::project::Project;
use library::model::Clip;
use uuid::Uuid;

#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
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

    pub fn to_screen(self, time: f64, value: f64) -> Pos2 {
        let x = self.graph_rect.min.x + self.pan.x + (time as f32 * self.zoom_x);
        let zero_y = self.graph_rect.center().y + self.pan.y;
        let y = zero_y - (value as f32 * self.zoom_y);
        Pos2::new(x, y)
    }

    pub fn screen_to_graph(self, pos: Pos2) -> (f64, f64) {
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
    pub clip_start_time: f64,
    pub trim_in: f64,
    pub time_stretch: f64,
}

impl TimeMapper {
    pub const fn identity() -> Self {
        Self {
            clip_start_time: 0.0,
            trim_in: 0.0,
            time_stretch: 1.0,
        }
    }

    pub fn from_clip(clip: &Clip) -> Self {
        Self {
            clip_start_time: clip.start_time.into_inner(),
            trim_in: clip.trim_in.into_inner(),
            time_stretch: clip.time_stretch.into_inner(),
        }
    }

    pub fn to_source_time(self, global_time: f64) -> f64 {
        self.trim_in + (global_time - self.clip_start_time) * self.time_stretch
    }

    pub fn to_global_time(self, source_time: f64) -> f64 {
        if self.time_stretch.abs() <= f64::EPSILON {
            self.clip_start_time
        } else {
            self.clip_start_time + (source_time - self.trim_in) / self.time_stretch
        }
    }
}

pub fn time_mapper_for_entity(project: &Project, entity_id: Uuid) -> TimeMapper {
    project
        .get_clip(entity_id)
        .or_else(|| {
            project
                .find_parent_clip(entity_id)
                .and_then(|clip_id| project.get_clip(clip_id))
        })
        .map_or_else(TimeMapper::identity, TimeMapper::from_clip)
}

#[cfg(test)]
mod tests {
    use super::*;
    use ordered_float::OrderedFloat;

    #[test]
    fn clip_time_mapping_is_exact_with_fractional_start_trim_and_stretch() {
        let mut clip = Clip::new("mapped", 1.125, 5.0);
        clip.trim_in = OrderedFloat(0.375);
        clip.time_stretch = OrderedFloat(1.5);
        let mapper = TimeMapper::from_clip(&clip);

        let global = 2.625;
        let source = mapper.to_source_time(global);
        assert!((source - 2.625).abs() < f64::EPSILON);
        assert!((mapper.to_global_time(source) - global).abs() < f64::EPSILON);
    }

    #[test]
    fn zero_stretch_maps_every_source_time_to_the_clip_start() {
        let mut clip = Clip::new("frozen", 3.25, 5.0);
        clip.trim_in = OrderedFloat(1.75);
        clip.time_stretch = OrderedFloat(0.0);
        let mapper = TimeMapper::from_clip(&clip);

        assert_eq!(mapper.to_source_time(99.0), 1.75);
        assert_eq!(mapper.to_global_time(123.0), 3.25);
    }
}
