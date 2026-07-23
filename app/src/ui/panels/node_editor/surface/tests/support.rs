use std::cell::RefCell;

pub(super) use std::collections::HashMap;
pub(super) use std::sync::{Arc, RwLock};

pub(super) use eframe::egui;
pub(super) use library::model::project::{
    PortAddress, PortDirection as ProjectPortDirection, PortOwner as ProjectPortOwner,
};
pub(super) use library::model::{Composition, Node, NodeContainer, Project};
pub(super) use node_editor_ui::{ItemId, MoveEndOutcome};
pub(super) use uuid::Uuid;

pub(super) use crate::action::{commit_live_project_edits, HistoryManager};
pub(super) use crate::state::context::EditorContext;
pub(super) use crate::state::context_types::{NodeEditorEditableWire, SelectionTarget};
pub(super) use crate::ui::panels::node_editor::{
    apply_layout_edit, build_snarl, test_fixture::fixture, ContainerKind, ContainerVisual,
    LayoutEdit, RenderedPortKey, CONTAINER_HEADER_HEIGHT,
};

use super::super::SurfacePortId;
pub(super) use super::super::{
    deselects_wire, move_change, move_end, selection_change, SurfaceCapture, SurfaceOutput,
    SurfaceProjection, SurfaceSelectionChange,
};

pub(super) type SurfaceState =
    node_editor_ui::InteractionState<Uuid, SurfacePortId, NodeEditorEditableWire, ProjectPortOwner>;

pub(super) fn pointer_button(position: egui::Pos2, pressed: bool) -> egui::Event {
    egui::Event::PointerButton {
        pos: position,
        button: egui::PointerButton::Primary,
        pressed,
        modifiers: egui::Modifiers::NONE,
    }
}

pub(super) fn key_a(pressed: bool) -> egui::Event {
    egui::Event::Key {
        key: egui::Key::A,
        physical_key: Some(egui::Key::A),
        pressed,
        repeat: false,
        modifiers: egui::Modifiers::NONE,
    }
}

pub(super) fn run_pointer_frame(
    context: &egui::Context,
    projection: &SurfaceProjection<'_>,
    state: &mut SurfaceState,
    options: node_editor_ui::InteractionOptions,
    events: Vec<egui::Event>,
) -> Vec<SurfaceOutput> {
    let outputs = RefCell::new(Vec::new());
    drop(context.run(
        egui::RawInput {
            screen_rect: Some(projection.viewport),
            events,
            ..Default::default()
        },
        |context| {
            egui::CentralPanel::default()
                .frame(egui::Frame::NONE)
                .show(context, |ui| {
                    outputs
                        .borrow_mut()
                        .extend(node_editor_ui::Editor::interact(
                            ui,
                            &projection.frame(),
                            state,
                            options,
                            false,
                        ));
                });
        },
    ));
    outputs.into_inner()
}
