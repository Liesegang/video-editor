use egui::Color32;

/// Stroke width and color; the color alpha is the grid intensity.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GridStroke {
    pub color: Color32,
    pub width: f32,
}

impl GridStroke {
    pub const fn new(width: f32, color: Color32) -> Self {
        Self { color, width }
    }
}

/// Shared visual token for canvas adapters.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CanvasTheme {
    pub background: Color32,
    pub minor_grid: GridStroke,
    pub major_grid: GridStroke,
    pub origin_grid: GridStroke,
}

impl Default for CanvasTheme {
    fn default() -> Self {
        Self {
            background: Color32::from_gray(30),
            minor_grid: GridStroke::new(1.0, Color32::from_gray(43)),
            major_grid: GridStroke::new(1.0, Color32::from_gray(55)),
            origin_grid: GridStroke::new(1.0, Color32::from_gray(78)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_theme_exposes_one_canvas_background_token() {
        assert_eq!(CanvasTheme::default().background, Color32::from_gray(30));
    }
}
