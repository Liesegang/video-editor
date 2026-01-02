use library::model::frame::transform::Transform;
use library::model::Layer;
use uuid::Uuid;

pub struct PreviewClip<'a> {
    pub clip: &'a Layer,
    pub track_id: Uuid,
    pub transform: Transform,
    // Calculated bounds in content space (e.g. text/shape bounding box)
    pub content_bounds: Option<(f32, f32, f32, f32)>,
}

impl<'a> PreviewClip<'a> {
    pub fn id(&self) -> Uuid {
        self.clip.id
    }
}
