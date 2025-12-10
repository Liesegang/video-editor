use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Clone, Serialize, Deserialize)]
pub struct Asset {
    pub name: String,
    pub duration: f32,
    #[serde(with = "crate::model::ui_types::ColorDef")]
    pub color: egui::Color32,
    pub kind: AssetKind,
    pub composition_id: Option<Uuid>, // Added for AssetKind::Composition
}

impl Asset {
    // Helper to generate a unique ID for egui, especially useful after composition_id is added
    pub fn id(&self) -> egui::Id {
        match self.kind {
            AssetKind::Composition(id) => egui::Id::new(id),
            // For other asset kinds, generate a stable ID from a combination of name and kind
            _ => egui::Id::new((&self.name, &self.kind)),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Hash)] // Added #[derive(Hash)]
pub enum AssetKind {
    Video,
    Audio,
    Image, // Added Image back
    Composition(Uuid), // Added: a composition asset links to its actual composition
}

// Helper trait to convert AssetKind to String for display, if needed in the UI
impl ToString for AssetKind {
    fn to_string(&self) -> String {
        match self {
            AssetKind::Video => "Video".to_string(),
            AssetKind::Audio => "Audio".to_string(),
            AssetKind::Image => "Image".to_string(),
            AssetKind::Composition(_) => "Composition".to_string(),
        }
    }
}
