//! Presentation state for the Module document shown in the production Node Editor.
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
    ModulePortAddress, TimelineItemId, TransitionId,
};
use library::model::project::PortDirection;
use pan_zoom_ui::CanvasState;
use uuid::Uuid;

use crate::command::CommandId;

#[derive(Debug, Clone)]
pub struct NodeEditorState {
    pub panel_rect: Option<egui::Rect>,
    pub focus_requested: bool,
    pub pending_layout_command: Option<CommandId>,
    pub active_document: Option<NodeEditorDocument>,
    pub surface_interaction:
        node_editor_ui::InteractionState<Uuid, ModuleEditorPortId, ModuleConnectionId, Uuid>,
    pub selected_nodes: HashSet<Uuid>,
    pub primary_node: Option<Uuid>,
    pub selected_connection: Option<ModuleConnectionId>,
    pub create_menu: Option<ModuleCreateMenuState>,
    pub node_drag_offsets: HashMap<Uuid, egui::Vec2>,
    /// Authoritative Node Editor camera. The production Snarl surface consumes
    /// this value, but never owns or feeds back a second navigation state.
    pub canvas: CanvasState,
    /// One-shot request to frame a newly opened Module through the shared
    /// canvas transform. User navigation is preserved after the first frame.
    pub fit_requested: bool,
    /// Absolute press-time transform held while a direct-manipulation gesture
    /// owns the primary pointer anywhere on the Node canvas.
    pub direct_gesture_transform: Option<egui::emath::TSTransform>,
}

impl Default for NodeEditorState {
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
            canvas: CanvasState::uniform(egui::Vec2::ZERO, 1.0),
            fit_requested: false,
            direct_gesture_transform: None,
        }
    }
}

impl NodeEditorState {
    pub fn request_document(&mut self, document: NodeEditorDocument) {
        if self.active_document.as_ref() != Some(&document) {
            self.surface_interaction.cancel();
            self.selected_nodes.clear();
            self.primary_node = None;
            self.selected_connection = None;
            self.create_menu = None;
            self.node_drag_offsets.clear();
            self.canvas = CanvasState::uniform(egui::Vec2::ZERO, 1.0);
            self.fit_requested = true;
            self.direct_gesture_transform = None;
            self.active_document = Some(document);
        }
        self.focus_requested = true;
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NodeEditorDocument {
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
    /// A finite processor graph explicitly promoted from one Timeline-owned
    /// Transition. The host owns A/B placement and normalized progress; the
    /// Module owns only processing topology.
    Transition {
        transition_id: TransitionId,
        instance_path: Option<InstancePath>,
        module_instance_id: ModuleInstanceId,
    },
}

impl ModuleEditorHost {
    pub(crate) const fn kind_name(&self) -> &'static str {
        match self {
            Self::NodeClip { .. } => "node_clip",
            Self::Attachment { .. } => "attachment",
            Self::Transition { .. } => "transition",
        }
    }

    pub(crate) const fn module_instance_id(&self) -> ModuleInstanceId {
        match self {
            Self::NodeClip {
                module_instance_id, ..
            }
            | Self::Attachment {
                module_instance_id, ..
            }
            | Self::Transition {
                module_instance_id, ..
            } => *module_instance_id,
        }
    }

    pub(crate) fn captured_instance_path(&self) -> Option<&InstancePath> {
        match self {
            Self::NodeClip { instance_path, .. }
            | Self::Attachment { instance_path, .. }
            | Self::Transition { instance_path, .. } => instance_path.as_ref(),
        }
    }

    pub(crate) const fn transition_id(&self) -> Option<TransitionId> {
        match self {
            Self::Transition { transition_id, .. } => Some(*transition_id),
            Self::NodeClip { .. } | Self::Attachment { .. } => None,
        }
    }
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
        let mut state = NodeEditorState::default();
        state.selected_nodes.insert(Uuid::new_v4());
        state.canvas = CanvasState::uniform(egui::vec2(10.0, 20.0), 0.75);
        state.direct_gesture_transform = Some(egui::emath::TSTransform::IDENTITY);
        let document = NodeEditorDocument::ModuleDefinition {
            definition_id: ModuleDefinitionId::new(),
            host: ModuleEditorHost::NodeClip {
                timeline_item_id: TimelineItemId::new(),
                instance_path: Some(InstancePath::root(TimelineId::new())),
                module_instance_id: ModuleInstanceId::new(),
            },
        };
        state.request_document(document.clone());
        assert!(state.selected_nodes.is_empty());
        assert_eq!(state.canvas, CanvasState::uniform(egui::Vec2::ZERO, 1.0));
        assert!(state.fit_requested);
        assert_eq!(state.direct_gesture_transform, None);
        assert_eq!(state.active_document, Some(document));
        assert!(state.focus_requested);
    }
}
