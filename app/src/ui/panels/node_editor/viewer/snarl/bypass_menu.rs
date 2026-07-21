use super::*;
use library::model::Node;

pub(super) fn is_bypassed(node: Option<&Node>) -> bool {
    node.is_some_and(|node| node.bypassed)
}

pub(super) fn status(bypassed: bool, inactive: bool) -> (&'static str, &'static str) {
    // Disabled and out-of-range Nodes produce NoOutput before bypass is
    // considered, so the header must show the same authoritative precedence
    // as the evaluator and Inspector.
    if inactive {
        (icons::CIRCLE_DASHED, "Node has no output")
    } else if bypassed {
        (icons::ARROW_RIGHT, "Node is bypassed")
    } else {
        (icons::CHECK_CIRCLE, "Node is active")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inactive_no_output_state_has_precedence_over_bypass() {
        assert_eq!(
            status(true, true),
            (icons::CIRCLE_DASHED, "Node has no output")
        );
        assert_eq!(
            status(true, false),
            (icons::ARROW_RIGHT, "Node is bypassed")
        );
    }
}

pub(super) fn show_toggle(
    ui: &mut egui::Ui,
    node: &Node,
    node_id: Uuid,
    edits: &mut Vec<QueuedNodeEdit>,
) -> bool {
    let bypassed = !node.bypassed;
    let label = if bypassed {
        "Bypass Node"
    } else {
        "Disable Bypass"
    };
    let response = ui.add_enabled(node.supports_bypass(), egui::Button::new(label));
    crate::qa::register_component_with_metadata(
        format!("node_editor.menu.toggle_bypass.node:{node_id}"),
        "node_editor_menu_item",
        response.rect,
        response.enabled(),
        Some(serde_json::json!({
            "action": if bypassed { "bypass" } else { "disable_bypass" },
            "owner": qa_container_key(PortOwner::Node(node_id)),
            "bypassed": bypassed,
        })),
    );
    if !response.clicked() {
        return false;
    }
    edits.push(QueuedNodeEdit::Atomic(NodeEdit::SetBypassed {
        node_id,
        bypassed,
    }));
    ui.close();
    true
}
