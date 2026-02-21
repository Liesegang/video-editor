/// Commonly-used timeline layout parameters, bundled to avoid passing
/// many individual arguments through the clip-area function chain.
#[derive(Clone, Copy, Debug)]
pub(super) struct TimelineGeometry {
    pub(super) pixels_per_unit: f32,
    pub(super) row_height: f32,
    pub(super) track_spacing: f32,
    pub(super) composition_fps: f64,
}
