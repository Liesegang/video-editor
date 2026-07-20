use egui::{Pos2, Rect, Vec2};
use library::model::project::Project;
use library::model::Clip;
use library::PropertyOwner;

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

pub fn time_mapper_for_owner(project: &Project, owner: PropertyOwner) -> TimeMapper {
    let clip = match owner {
        PropertyOwner::Clip(clip_id) => project.get_clip(clip_id),
        PropertyOwner::Node(node_id) => project
            .find_parent_clip(node_id)
            .and_then(|clip_id| project.get_clip(clip_id)),
    };
    clip.map_or_else(TimeMapper::identity, TimeMapper::from_clip)
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

    #[test]
    fn same_uuid_clip_does_not_hijack_node_time_scope() {
        let shared_id = uuid::Uuid::new_v4();
        let parent_clip_id = uuid::Uuid::new_v4();
        let mut project = Project::new("typed graph time scope");

        let mut colliding_clip = Clip::new("same UUID Clip", 100.0, 5.0);
        colliding_clip.id = shared_id;
        colliding_clip.trim_in = OrderedFloat(20.0);
        let mut parent_clip = Clip::new("actual Node parent", 2.0, 5.0);
        parent_clip.id = parent_clip_id;
        parent_clip.trim_in = OrderedFloat(0.5);
        let mut node = library::model::Node::new_merge("same UUID Node");
        node.id = shared_id;
        parent_clip.node_ids.push(shared_id);

        project.add_clip(colliding_clip);
        project.add_clip(parent_clip);
        project.add_node(node);

        let node_mapper = time_mapper_for_owner(&project, PropertyOwner::Node(shared_id));
        let clip_mapper = time_mapper_for_owner(&project, PropertyOwner::Clip(shared_id));

        assert_eq!(node_mapper.clip_start_time, 2.0);
        assert_eq!(node_mapper.trim_in, 0.5);
        assert_eq!(clip_mapper.clip_start_time, 100.0);
        assert_eq!(clip_mapper.trim_in, 20.0);
    }
}
