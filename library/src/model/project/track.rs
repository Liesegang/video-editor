use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::model::frame::draw_type::BlendMode;

#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
pub struct TrackData {
    pub id: Uuid,
    pub name: String,
    #[serde(default)]
    pub child_ids: Vec<Uuid>,
    #[serde(default)]
    pub blend_mode: BlendMode,
    #[serde(default = "default_opacity")]
    pub opacity: f64,
    #[serde(default = "default_visible")]
    pub visible: bool,
}

fn default_opacity() -> f64 {
    1.0
}

fn default_visible() -> bool {
    true
}

impl TrackData {
    pub fn new(name: &str) -> Self {
        Self {
            id: Uuid::new_v4(),
            name: name.to_string(),
            child_ids: Vec::new(),
            blend_mode: BlendMode::default(),
            opacity: 1.0,
            visible: true,
        }
    }

    /// Add a child node ID
    pub fn add_child(&mut self, child_id: Uuid) {
        self.child_ids.push(child_id);
    }

    /// Insert a child node ID at a specific index
    pub fn insert_child(&mut self, index: usize, child_id: Uuid) {
        if index <= self.child_ids.len() {
            self.child_ids.insert(index, child_id);
        } else {
            self.child_ids.push(child_id);
        }
    }

    /// Remove a child node ID
    pub fn remove_child(&mut self, child_id: Uuid) -> bool {
        if let Some(pos) = self.child_ids.iter().position(|id| *id == child_id) {
            self.child_ids.remove(pos);
            true
        } else {
            false
        }
    }
}
