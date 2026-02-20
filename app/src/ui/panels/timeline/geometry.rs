/// Commonly-used timeline layout parameters, bundled to avoid passing
/// many individual arguments through the clip-area function chain.
#[derive(Clone, Copy, Debug)]
pub struct TimelineGeometry {
    pub pixels_per_unit: f32,
    pub row_height: f32,
    pub track_spacing: f32,
    pub composition_fps: f64,
}
