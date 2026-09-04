//! Presentation state for the Timeline-first Module Node Editor.
//!
//! This state contains no authoritative graph data. A document is always an
//! explicit Module instance host, and every mutation goes through
//! `TimelineEditorService`.

use std::collections::{HashMap, HashSet};

use eframe::egui;
#[cfg(test)]
use library::model::authoring::TimelineId;
use library::model::authoring::{
    AttachmentId, InstancePath, ModuleConnectionId, ModuleDefinitionId, ModuleInstanceId,
    ModulePortAddress, TimelineItemId,
};
use library::model::project::PortDirection;
use uuid::Uuid;

use crate::command::CommandId;

#[derive(Debug, Clone)]
pub struct ModuleNodeEditorState {
    pub panel_rect: Option<egui::Rect>,
    pub focus_requested: bool,
    pub pending_layout_command: Option<CommandId>,
    pub active_document: Option<ModuleNodeEditorDocument>,
    pub surface_interaction:
        node_editor_ui::InteractionState<Uuid, ModuleEditorPortId, ModuleConnectionId, Uuid>,
    pub selected_nodes: HashSet<Uuid>,
    pub primary_node: Option<Uuid>,
    pub selected_connection: Option<ModuleConnectionId>,
    pub create_menu: Option<ModuleCreateMenuState>,
    pub node_drag_offsets: HashMap<Uuid, egui::Vec2>,
    pub canvas_pan: egui::Vec2,
    pub canvas_zoom: f32,
}

impl Default for ModuleNodeEditorState {
    fn default() -> Self {
        Self {
            panel_rect: None,
            focus_requested: false,
            pending_layout_command: None,
            active_document: None,
            surface_interaction: node_editor_ui::InteractionState::default(),
            selected_nodes: HashSet::new(),
            primary_node: None,
            selected_connection: None,
            create_menu: None,
            node_drag_offsets: HashMap::new(),
            canvas_pan: egui::Vec2::ZERO,
            canvas_zoom: 1.0,
        }
    }
}

impl ModuleNodeEditorState {
    pub fn request_document(&mut self, document: ModuleNodeEditorDocument) {
        if self.active_document.as_ref() != Some(&document) {
            self.surface_interaction.cancel();
            self.selected_nodes.clear();
            self.primary_node = None;
            self.selected_connection = None;
            self.create_menu = None;
            self.node_drag_offsets.clear();
            self.canvas_pan = egui::Vec2::ZERO;
            self.canvas_zoom = 1.0;
            self.active_document = Some(document);
        }
        self.focus_requested = true;
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ModuleNodeEditorDocument {
    ModuleDefinition {
        definition_id: ModuleDefinitionId,
        host: ModuleEditorHost,
    },
}

/// Invocation context remains separate from the definition identity so the
/// same editor can host a Node Clip today and a Module Attachment later.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ModuleEditorHost {
    NodeClip {
        timeline_item_id: TimelineItemId,
        /// Concrete runtime placement when the user entered through a nested
        /// Composition item. `None` means the Timeline definition was opened
        /// directly from Assets, so instance-scoped binding actions stay
        /// unavailable until a placement is chosen.
        instance_path: Option<InstancePath>,
        module_instance_id: ModuleInstanceId,
    },
    #[allow(
        dead_code,
        reason = "reserved for the Attachment host already modeled by AuthoringProject"
    )]
    Attachment {
        attachment_id: AttachmentId,
        instance_path: Option<InstancePath>,
        module_instance_id: ModuleInstanceId,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ModuleEditorPortId {
    pub address: ModulePortAddress,
    pub direction: PortDirection,
}

#[derive(Debug, Clone)]
pub struct ModuleCreateMenuState {
    pub position: egui::Pos2,
    pub open_time: f64,
}

impl ModuleCreateMenuState {
    pub const fn new(position: egui::Pos2, open_time: f64) -> Self {
        Self {
            position,
            open_time,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn switching_documents_clears_only_transient_module_interaction() {
        let mut state = ModuleNodeEditorState::default();
        state.selected_nodes.insert(Uuid::new_v4());
        state.canvas_pan = egui::vec2(10.0, 20.0);
        let document = ModuleNodeEditorDocument::ModuleDefinition {
            definition_id: ModuleDefinitionId::new(),
            host: ModuleEditorHost::NodeClip {
                timeline_item_id: TimelineItemId::new(),
                instance_path: Some(InstancePath::root(TimelineId::new())),
                module_instance_id: ModuleInstanceId::new(),
            },
        };
        state.request_document(document.clone());
        assert!(state.selected_nodes.is_empty());
        assert_eq!(state.canvas_pan, egui::Vec2::ZERO);
        assert_eq!(state.active_document, Some(document));
        assert!(state.focus_requested);
    }
}
