"""Shared native Appearance property assertions and Paint gestures."""

from qa_support import QaFailure


def constant_property(operation, key):
    property_value = (operation.get("properties") or {}).get(key) or {}
    if property_value.get("type") != "constant":
        return None
    return (property_value.get("properties") or {}).get("value")


def canonical_paint_value(value, description="Paint"):
    if not isinstance(value, dict) or value.get("$type") != "paint_value":
        raise QaFailure(description + " omitted its canonical Paint value")
    if set(value) != {"$type", "kind", "value"}:
        raise QaFailure(description + " envelope is not canonical")
    if value.get("kind") not in {"solid", "gradient", "pattern"}:
        raise QaFailure(description + " has an invalid Paint kind")
    if "value" not in value:
        raise QaFailure(description + " omitted its typed value")
    return value


def paint_property(operation):
    return canonical_paint_value(
        constant_property(operation, "paint"), "Appearance operation Paint"
    )


def select_paint_kind(client, control_id, expected_before, selected_kind):
    _, control = client.wait_component_settled(control_id)
    metadata = control.get("metadata") or {}
    if metadata.get("editor_kind") != "paint":
        raise QaFailure("Appearance Paint bypassed the shared Paint editor")
    if (metadata.get("value") or {}).get("kind") != expected_before:
        raise QaFailure("Appearance Paint control exposed the wrong current kind")
    selector_id = control_id + ".paint.kind"
    _, selector = client.wait_component_settled(selector_id)
    if (selector.get("metadata") or {}).get("paint_kind") != expected_before:
        raise QaFailure("Appearance Paint kind selector disagrees with its value")
    client.click_component(selector_id)
    option_id = selector_id + "." + selected_kind
    _, option = client.wait_component_settled(option_id)
    if (option.get("metadata") or {}).get("paint_kind") != selected_kind:
        raise QaFailure("Appearance Paint kind option omitted its typed identity")
    client.click_component(option_id)
    return {
        "control": metadata,
        "selector": selector.get("metadata") or {},
        "option": option.get("metadata") or {},
    }


def edit_first_gradient_stop(client, control_id, delta=12.0):
    client.click_component(control_id + ".paint.value")
    stop_id = control_id + ".gradient.stop.0.offset"
    _, stop = client.wait_component_settled(stop_id)
    if (stop.get("metadata") or {}).get("offset") is None:
        raise QaFailure("Appearance Paint Gradient omitted its first stop offset")
    client.drag_component_by(stop_id, delta, 0.0, steps=8)
    return {"id": stop_id, "metadata": stop.get("metadata") or {}}
