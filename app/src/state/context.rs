use library::model::project::project::{Composition, Project};

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::model::ui_types::{DraggedItem, TimelineDisplayMode, Vec2Def};

#[derive(Serialize, Deserialize)]
pub struct EditorContext {
    pub current_time: f32,
    pub is_playing: bool,
    pub timeline_pixels_per_second: f32,
    pub timeline_display_mode: TimelineDisplayMode, // New field for timeline display mode,

    #[serde(with = "Vec2Def")]
    pub view_pan: egui::Vec2,
    pub view_zoom: f32,

    #[serde(skip)]
    pub dragged_item: Option<DraggedItem>,
    #[serde(skip)]
    pub asset_delete_candidate: Option<Uuid>,
    #[serde(skip)]
    pub comp_delete_candidate: Option<Uuid>,
    #[serde(skip)]
    pub active_modal_error: Option<String>,

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
    pub drag_start_property_name: Option<String>,
    #[serde(skip)]
    pub drag_start_property_value: Option<library::model::project::property::PropertyValue>,

    #[serde(skip)]
    pub dragged_entity_original_track_id: Option<Uuid>,
    #[serde(skip)]
    pub dragged_entity_hovered_track_id: Option<Uuid>,
    #[serde(skip)]
    pub dragged_entity_has_moved: bool, // Track if entity was actually moved during drag
    #[serde(skip)]
    pub is_resizing_entity: bool,

    #[serde(skip)]
    pub current_time_text_input: String,
    #[serde(skip)]
    pub is_editing_current_time: bool,
}

impl EditorContext {
    pub fn new(default_comp_id: Uuid) -> Self {
        Self {
            current_time: 0.0,
            is_playing: false,
            timeline_pixels_per_second: 50.0,
            timeline_display_mode: TimelineDisplayMode::Seconds, // Default display mode,

            view_pan: egui::vec2(20.0, 20.0),
            view_zoom: 0.3,
            dragged_item: None,
            asset_delete_candidate: None,
            comp_delete_candidate: None,
            active_modal_error: None,

            timeline_v_zoom: 1.0,
            timeline_h_zoom: 1.0,
            timeline_scroll_offset: egui::Vec2::ZERO,

            selected_composition_id: Some(default_comp_id),
            selected_track_id: None,
            selected_entity_id: None,

            drag_start_property_name: None,
            drag_start_property_value: None,
            dragged_entity_original_track_id: None,
            dragged_entity_hovered_track_id: None,
            dragged_entity_has_moved: false,
            is_resizing_entity: false,

            current_time_text_input: "".to_string(), // Initialize new field
            is_editing_current_time: false,          // Initialize new field
        }
    }

    pub fn get_current_composition<'a>(&self, project: &'a Project) -> Option<&'a Composition> {
        self.selected_composition_id
            .and_then(|id| project.compositions.iter().find(|&c| c.id == id))
    }
}
