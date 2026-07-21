use egui::Ui;
use library::LibraryError;
use uuid::Uuid;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum StackQaAction {
    MoveUp,
    MoveDown,
    Remove,
}

impl StackQaAction {
    fn id_suffix(self) -> &'static str {
        match self {
            Self::MoveUp => "move_up",
            Self::MoveDown => "move_down",
            Self::Remove => "remove",
        }
    }

    fn semantic_action(self) -> &'static str {
        match self {
            Self::MoveUp | Self::MoveDown => "reorder",
            Self::Remove => "remove",
        }
    }
}

pub(super) fn stack_item_qa_id(
    stack: &str,
    clip_id: Uuid,
    node_id: Uuid,
    style_anchor_id: Option<Uuid>,
) -> String {
    style_anchor_id.map_or_else(
        || format!("inspector.semantic.{stack}:{clip_id}:{node_id}"),
        |anchor| format!("inspector.semantic.{stack}:{clip_id}:anchor:{anchor}:node:{node_id}"),
    )
}

pub(super) fn stack_action_qa_id(
    stack: &str,
    clip_id: Uuid,
    node_id: Uuid,
    style_anchor_id: Option<Uuid>,
    action: StackQaAction,
) -> String {
    format!(
        "{}.{}",
        stack_item_qa_id(stack, clip_id, node_id, style_anchor_id),
        action.id_suffix()
    )
}

pub(super) fn stack_action_qa_metadata(
    stack: &str,
    node_id: Uuid,
    style_anchor_id: Option<Uuid>,
    action: StackQaAction,
    mutation_semantics: &str,
) -> serde_json::Value {
    let (preserves, changes) = match action {
        StackQaAction::MoveUp | StackQaAction::MoveDown => (
            serde_json::json!([
                "node_uuid",
                "properties",
                "external_property_wires",
                "non_main_flow_wires",
                "connection_uuid",
                "blend_mode"
            ]),
            serde_json::json!([if stack == "style" {
                "merge_input_order"
            } else {
                "main_flow_endpoints"
            }]),
        ),
        StackQaAction::Remove => (
            serde_json::json!([
                "unrelated_nodes",
                "unrelated_properties",
                "unrelated_wires",
                "surviving_connection_uuid",
                "surviving_connection_order",
                "surviving_connection_blend_mode"
            ]),
            serde_json::json!(["target_node", "incident_wires", "semantic_stack_topology"]),
        ),
    };
    serde_json::json!({
        "stack": stack,
        "action": action.semantic_action(),
        "command": action.id_suffix(),
        "node_id": node_id,
        "style_anchor_id": style_anchor_id,
        "mutation_semantics": mutation_semantics,
        "preserves": preserves,
        "changes": changes,
        "selection_identity": "clip",
    })
}

#[allow(
    clippy::too_many_arguments,
    reason = "QA identity mirrors one exact semantic stack action"
)]
pub(super) fn register_action_button(
    response: &egui::Response,
    clip_id: Uuid,
    stack: &str,
    node_id: Uuid,
    style_anchor_id: Option<Uuid>,
    action: StackQaAction,
    mutation_semantics: &str,
) {
    crate::qa::register_component_with_metadata(
        stack_action_qa_id(stack, clip_id, node_id, style_anchor_id, action),
        "inspector_semantic_stack_action",
        response.rect,
        response.enabled(),
        Some(stack_action_qa_metadata(
            stack,
            node_id,
            style_anchor_id,
            action,
            mutation_semantics,
        )),
    );
}

#[allow(
    clippy::too_many_arguments,
    reason = "QA identity mirrors one exact semantic stack row"
)]
pub(super) fn register_stack_row(
    rect: egui::Rect,
    clip_id: Uuid,
    stack: &str,
    node_id: Uuid,
    index: usize,
    order_semantics: &str,
    style_anchor_id: Option<Uuid>,
) {
    crate::qa::register_component_with_metadata(
        stack_item_qa_id(stack, clip_id, node_id, style_anchor_id),
        "inspector_semantic_stack_item",
        rect,
        true,
        Some(serde_json::json!({
            "clip_id": clip_id,
            "stack": stack,
            "node_id": node_id,
            "style_anchor_id": style_anchor_id,
            "index": index,
            "order_semantics": order_semantics,
            "selection_identity": "clip",
        })),
    );
}

pub(super) fn render_query_error(ui: &mut Ui, clip_id: Uuid, stack: &str, error: &LibraryError) {
    let response = ui.colored_label(
        ui.visuals().error_fg_color,
        format!("{stack} stack unavailable: {error}"),
    );
    crate::qa::register_component_with_metadata(
        format!("inspector.semantic.{stack}:{clip_id}.query_error"),
        "inspector_semantic_diagnostic",
        response.rect,
        true,
        Some(serde_json::json!({
            "clip_id": clip_id,
            "stack": stack,
            "message": error.to_string(),
            "fail_closed": true,
        })),
    );
}
