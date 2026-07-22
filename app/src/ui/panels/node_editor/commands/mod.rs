mod create_menu;
mod edit;
mod types;
mod wire;

pub(in crate::ui::panels::node_editor) use create_menu::{
    create_operation_node_for_request, node_create_menu_items, wire_splice_menu_items,
    NodeCreateRequest,
};
pub use edit::flush_pending_continuous_edit;
pub(in crate::ui::panels::node_editor) use edit::{
    apply_edit, apply_queued_node_edits, flush_pending_continuous_edit_with_project,
};
pub(in crate::ui::panels::node_editor) use types::{NodeEdit, QueuedNodeEdit};
pub(in crate::ui::panels::node_editor) use wire::node_can_splice_connection;
#[cfg(test)]
pub(in crate::ui::panels::node_editor) use wire::{
    insert_node_on_connection, splice_existing_node_on_connection,
};
