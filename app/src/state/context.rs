use serde::{Deserialize, Serialize};
use uuid::Uuid;
use library::model::project::project::{Composition, Project};
use library::model::project::property::PropertyMap;

use crate::model::assets::{Asset, AssetKind};
use crate::model::ui_types::Vec2Def;

#[derive(Serialize, Deserialize)]
pub struct EditorContext {
    pub assets: Vec<Asset>,
    pub current_time: f32,
    pub is_playing: bool,
    pub timeline_pixels_per_second: f32,

    #[serde(with = "Vec2Def")]
    pub view_pan: egui::Vec2,
    pub view_zoom: f32,

    #[serde(skip)]
    pub dragged_asset: Option<usize>,

    pub timeline_v_zoom: f32,
    pub timeline_h_zoom: f32,
    #[serde(skip)]
    pub timeline_scroll_offset: egui::Vec2,

    #[serde(skip)]
    pub selected_composition_id: Option<Uuid>,
    #[serde(skip)]
    pub selected_track_id: Option<Uuid>,
    #[serde(skip)]
    pub selected_entity_id: Option<Uuid>,
    #[serde(skip)]
    pub inspector_entity_cache: Option<(Uuid, String, PropertyMap, f64, f64)>,

    #[serde(skip)]
    pub drag_start_property_name: Option<String>,
    #[serde(skip)]
    pub drag_start_property_value: Option<library::model::project::property::PropertyValue>,
    #[serde(skip)]
    pub last_project_state_before_drag: Option<Project>,

}

impl EditorContext {
    pub fn new(default_comp_id: Uuid) -> Self {
        let assets = vec![
            Asset {
                name: "Intro_Seq.mp4".into(),
                duration: 5.0,
                color: egui::Color32::from_rgb(100, 150, 255),
                kind: AssetKind::Video,
                composition_id: None,
            },
            Asset {
                name: "Main_Cam.mov".into(),
                duration: 15.0,
                color: egui::Color32::from_rgb(80, 120, 200),
                kind: AssetKind::Video,
                composition_id: None,
            },
            Asset {
                name: "BGM_Happy.mp3".into(),
                duration: 30.0,
                color: egui::Color32::from_rgb(100, 255, 150),
                kind: AssetKind::Audio,
                composition_id: None,
            },
            Asset {
                name: "Text_Overlay.png".into(),
                duration: 5.0,
                color: egui::Color32::from_rgb(255, 100, 150),
                kind: AssetKind::Image,
                composition_id: None,
            },
            Asset {
                name: "Logo.png".into(),
                duration: 5.0,
                color: egui::Color32::from_rgb(255, 200, 100),
                kind: AssetKind::Image,
                composition_id: None,
            },
            Asset {
                name: "Main Composition".into(),
                duration: 60.0,
                color: egui::Color32::from_rgb(255, 150, 255),
                kind: AssetKind::Composition(default_comp_id),
                composition_id: Some(default_comp_id),
            },
        ];

        Self {
            assets,
            current_time: 0.0,
            is_playing: false,
            timeline_pixels_per_second: 50.0,

            view_pan: egui::vec2(20.0, 20.0),
            view_zoom: 0.3,
            dragged_asset: None,

            timeline_v_zoom: 1.0,
            timeline_h_zoom: 1.0,
            timeline_scroll_offset: egui::Vec2::ZERO,

            selected_composition_id: Some(default_comp_id),
            selected_track_id: None,
            selected_entity_id: None,
            inspector_entity_cache: None,
            drag_start_property_name: None,
                        drag_start_property_value: None,
                        last_project_state_before_drag: None,
                    }
    }

    pub fn get_current_composition<'a>(&self, project: &'a Project) -> Option<&'a Composition> {
        self.selected_composition_id
            .and_then(|id| project.compositions.iter().find(|&c| c.id == id))
    }
}
