use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::runtime::draw_type::BlendMode;

#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
pub struct LayerData {
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
    /// When this layer starts on the timeline (frame number).
    #[serde(default)]
    pub in_frame: u64,
    /// When this layer ends on the timeline (frame number).
    #[serde(default)]
    pub out_frame: u64,
}

fn default_opacity() -> f64 {
    1.0
}

fn default_visible() -> bool {
    true
}

impl LayerData {
    pub fn new(name: &str, in_frame: u64, out_frame: u64) -> Self {
        Self {
            id: Uuid::new_v4(),
            name: name.to_string(),
            child_ids: Vec::new(),
            blend_mode: BlendMode::default(),
            opacity: 1.0,
            visible: true,
            in_frame,
            out_frame,
        }
    }

    pub fn add_child(&mut self, child_id: Uuid) {
        self.child_ids.push(child_id);
    }

    pub fn insert_child(&mut self, index: usize, child_id: Uuid) {
        if index <= self.child_ids.len() {
            self.child_ids.insert(index, child_id);
        } else {
            self.child_ids.push(child_id);
        }
    }

    pub fn remove_child(&mut self, child_id: Uuid) -> bool {
        if let Some(pos) = self.child_ids.iter().position(|id| *id == child_id) {
            self.child_ids.remove(pos);
            true
        } else {
            false
        }
    }
}
