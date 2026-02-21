use library::project::clip::TrackClip;
use library::runtime::transform::Transform;
use uuid::Uuid;

pub(super) struct PreviewClip<'a> {
    pub(super) clip: &'a TrackClip,
    pub(super) track_id: Uuid,
    pub(super) transform: Transform,
    /// The compositing.transform graph node ID (if any).
    pub(super) transform_node_id: Option<Uuid>,
    // Calculated bounds in content space (e.g. text/shape bounding box)
    pub(super) content_bounds: Option<(f32, f32, f32, f32)>,
}

impl<'a> PreviewClip<'a> {
    pub(super) fn id(&self) -> Uuid {
        self.clip.id
    }
}
