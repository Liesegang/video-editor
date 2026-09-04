#!/usr/bin/env python3
"""Exercise one explicit Module in the production Node Editor surface."""

import math

from qa_support import (
    QaFailure,
    component_center,
    find_clear_canvas_point,
    item_by_name,
    run_suite_main,
)


CANVAS_ID = "node_editor.canvas"


def _active_definition(state):
    document = state["editor"]["node_editor"]["document"]
    if not document or document.get("kind") != "module_definition":
        raise QaFailure("Node Editor did not open an explicit Module document")
    definition_id = document["definition_id"]
    return definition_id, state["project"]["module_definitions"][definition_id]


def _ports(snapshot, direction, node_id=None):
    result = []
    for component in snapshot["components"]:
        if component.get("type") != "node_editor_port" or not component.get("visible"):
            continue
        metadata = component.get("metadata") or {}
        if metadata.get("direction") != direction:
            continue
        if node_id is not None and metadata.get("node_id") != node_id:
            continue
        result.append(component)
    return result


def _node_content_type(node):
    content = node.get("content") or {}
    return str(content.get("type", "")).replace("_", "").lower()


def _image_port(snapshot, direction, node_id):
    return next(
        (
            port
            for port in _ports(snapshot, direction, node_id)
            if str((port.get("metadata") or {}).get("data_type", "")).lower()
            == "image"
        ),
        None,
    )


def _connection(definition, from_node, to_node):
    return next(
        (
            connection
            for connection in definition["graph"]["connections"]
            if connection["from"]["node_id"] == from_node
            and connection["to"]["node_id"] == to_node
        ),
        None,
    )


def _connection_component_id(connection_id):
    return "node_editor.connection:" + connection_id


def _connection_handle_component_id(connection_id, endpoint):
    return "node_editor.connection_handle:{}:{}".format(connection_id, endpoint)


def _select_connection(client, connection_id):
    component_id = _connection_component_id(connection_id)
    _, component = client.wait_component_settled(component_id)
    metadata = component.get("metadata") or {}
    if metadata.get("connection_id") != connection_id:
        raise QaFailure("Node Editor wire QA target has the wrong connection identity")
    if metadata.get("interaction_geometry") != "node-editor-ui":
        raise QaFailure("Node Editor wire QA target did not use shared geometry")
    client.click_component(component_id)

    def selected():
        state = client.state()
        return (
            state
            if state["editor"]["node_editor"]["selected_connection"]
            == connection_id
            else None
        )

    client.wait_until("Module connection selection", selected)


def run_suite(client):
    client.wait_health()
    initial = client.state()
    node_clip = item_by_name(initial["project"], "QA Node Clip")
    client.double_click_component("timeline.item:" + node_clip["id"])
    client.wait_component("dock.tab:node_editor")
    client.click_component("dock.tab:node_editor")
    _, canvas = client.wait_component_settled(CANVAS_ID)
    canvas_metadata = canvas.get("metadata") or {}
    if canvas_metadata.get("production_surface") != "egui_snarl":
        raise QaFailure("Module document did not use the production Node Editor")
    if canvas_metadata.get("timeline_graph_expansion") is not False:
        raise QaFailure("Node Editor expanded Timeline structure into Nodes")

    opened = client.state()
    definition_id, definition = _active_definition(opened)
    original_nodes = set(definition["graph"]["nodes"])
    output_nodes = {
        node_id
        for node_id, node in definition["graph"]["nodes"].items()
        if _node_content_type(node) == "moduleoutput"
    }
    if len(original_nodes) != 2 or len(output_nodes) != 1:
        raise QaFailure("fixture Module should start with one source and one Output terminal")
    output_node_id = next(iter(output_nodes))
    source_node_id = next(iter(original_nodes - output_nodes))
    if len(definition["graph"]["connections"]) != 1 or not _connection(
        definition, source_node_id, output_node_id
    ):
        raise QaFailure("fixture Module should initially route its source to Output")

    _, output_header = client.wait_component("node_editor.node_header:" + output_node_id)
    if (output_header.get("metadata") or {}).get("module_output") is not True:
        raise QaFailure("dedicated Output node was not identified by the production surface")

    snapshot = client.component_snapshot()
    menu_point = find_clear_canvas_point(
        snapshot,
        CANVAS_ID,
        ("node_editor.node:", "node_editor.node_header:", "node_editor.port."),
    )
    client.inject(
        "click",
        {**menu_point, "button": "secondary", "coordinate_space": "points"},
    )
    client.wait_component("node_editor.menu.search")
    client.click_component("node_editor.menu.search")
    client.inject("text", {"text": "blur"})
    client.wait_component_settled("node_editor.menu.create.effect:blur")
    client.click_component("node_editor.menu.create.effect:blur")

    def created():
        state = client.state()
        _, current = _active_definition(state)
        return state if len(current["graph"]["nodes"]) == 3 else None

    after_create = client.wait_until("a second Module Node", created)
    current_definition_id, current = _active_definition(after_create)
    new_nodes = set(current["graph"]["nodes"]) - original_nodes
    if len(new_nodes) != 1:
        raise QaFailure("Node creation did not add exactly one Module Node")
    effect_node_id = next(iter(new_nodes))
    if current_definition_id == definition_id and current == definition:
        raise QaFailure("Node creation did not update the Module definition")

    def connection_targets():
        snapshot = client.component_snapshot()
        source = _image_port(snapshot, "output", source_node_id)
        target = _image_port(snapshot, "input", effect_node_id)
        return (source, target) if source and target else None

    source, target = client.wait_until("connectable Module ports", connection_targets)
    _, source_header = client.wait_component("node_editor.node_header:" + source_node_id)
    header_point = component_center(source_header)
    _, before_wire_canvas = client.wait_component_settled(CANVAS_ID)
    gesture_scale = float((before_wire_canvas.get("metadata") or {})["scale"])
    client.inject(
        "press",
        {**header_point, "button": "primary", "coordinate_space": "points"},
    )
    # A wheel/pinch arriving while a Node owns the pointer must not change the
    # press-time transform. The same lock is consumed by port/wire gestures.
    client.inject(
        "scroll",
        {
            **header_point,
            "delta_x": 0.0,
            "delta_y": -600.0,
            "coordinate_space": "points",
            "modifiers": {"command": True},
        },
    )
    _, during_wire_canvas = client.wait_component_settled(CANVAS_ID)
    during_wire_scale = float((during_wire_canvas.get("metadata") or {})["scale"])
    if abs(during_wire_scale - gesture_scale) > 1.0e-6:
        raise QaFailure("Node Editor transform changed during a Node gesture")
    client.inject(
        "release",
        {**header_point, "button": "primary", "coordinate_space": "points"},
    )

    def original_wire_ports():
        snapshot = client.component_snapshot()
        source_port = _image_port(snapshot, "output", source_node_id)
        output_port = _image_port(snapshot, "input", output_node_id)
        return (source_port, output_port) if source_port and output_port else None

    client.wait_until(
        "original source-to-Output ports", original_wire_ports
    )
    original_connection_id = definition["graph"]["connections"][0]["id"]
    _select_connection(client, original_connection_id)
    client.key("backspace", True)
    client.key("backspace", False)

    def disconnected():
        state = client.state()
        _, current = _active_definition(state)
        return state if not current["graph"]["connections"] else None

    client.wait_until("source-to-Output disconnect", disconnected)
    source, target = client.wait_until("ports after frozen gesture", connection_targets)
    client.drag(component_center(source), component_center(target), steps=12)

    def source_connected_to_effect():
        state = client.state()
        _, candidate = _active_definition(state)
        return state if _connection(candidate, source_node_id, effect_node_id) else None

    client.wait_until("source connected to effect", source_connected_to_effect)

    def effect_output_targets():
        snapshot = client.component_snapshot()
        effect_output = _image_port(snapshot, "output", effect_node_id)
        module_output = _image_port(snapshot, "input", output_node_id)
        return (effect_output, module_output) if effect_output and module_output else None

    effect_output, module_output = client.wait_until(
        "effect-to-Output ports", effect_output_targets
    )
    client.drag(component_center(effect_output), component_center(module_output), steps=12)

    def completed_module_route():
        state = client.state()
        _, candidate = _active_definition(state)
        complete = (
            len(candidate["graph"]["connections"]) == 2
            and _connection(candidate, source_node_id, effect_node_id)
            and _connection(candidate, effect_node_id, output_node_id)
        )
        return state if complete else None

    after_connect = client.wait_until("source-effect-Output route", completed_module_route)
    _, routed = _active_definition(after_connect)
    source_effect = _connection(routed, source_node_id, effect_node_id)
    effect_output_connection = _connection(routed, effect_node_id, output_node_id)
    source_effect_metadata = {
        key: source_effect[key] for key in ("id", "order", "blend_mode")
    }
    output_metadata = {
        key: effect_output_connection[key] for key in ("id", "order", "blend_mode")
    }

    # Add a second compatible processor so both existing-edge handles can be
    # exercised without introducing an invalid intermediate topology.
    second_snapshot = client.component_snapshot()
    second_menu_point = find_clear_canvas_point(
        second_snapshot,
        CANVAS_ID,
        (
            "node_editor.node:",
            "node_editor.node_header:",
            "node_editor.port.",
            "node_editor.connection:",
            "node_editor.connection_handle:",
        ),
    )
    client.inject(
        "click",
        {**second_menu_point, "button": "secondary", "coordinate_space": "points"},
    )
    client.wait_component("node_editor.menu.search")
    client.click_component("node_editor.menu.search")
    client.inject("text", {"text": "blur"})
    client.wait_component_settled("node_editor.menu.create.effect:blur")
    client.click_component("node_editor.menu.create.effect:blur")

    def second_effect_created():
        state = client.state()
        _, candidate = _active_definition(state)
        return state if len(candidate["graph"]["nodes"]) == 4 else None

    with_second_effect = client.wait_until("a second Blur Node", second_effect_created)
    _, with_second_definition = _active_definition(with_second_effect)
    second_effect_nodes = (
        set(with_second_definition["graph"]["nodes"])
        - original_nodes
        - {effect_node_id}
    )
    if len(second_effect_nodes) != 1:
        raise QaFailure("second Blur creation did not add exactly one Module Node")
    second_effect_node_id = next(iter(second_effect_nodes))

    def selected_target_wire_ports():
        snapshot = client.component_snapshot()
        source = _image_port(snapshot, "output", source_node_id)
        old_target = _image_port(snapshot, "input", effect_node_id)
        return (source, old_target) if source and old_target else None

    client.wait_until(
        "selected target wire ports", selected_target_wire_ports
    )
    _select_connection(client, source_effect_metadata["id"])

    def target_reconnect_ports():
        snapshot = client.component_snapshot()
        old_target = _image_port(snapshot, "input", effect_node_id)
        new_target = _image_port(snapshot, "input", second_effect_node_id)
        return (old_target, new_target) if old_target and new_target else None

    _, new_target = client.wait_until(
        "visible target reconnect ports", target_reconnect_ports
    )
    _, target_handle = client.wait_component_settled(
        _connection_handle_component_id(source_effect_metadata["id"], "target")
    )
    client.drag(component_center(target_handle), component_center(new_target), steps=12)

    def target_reconnected():
        state = client.state()
        _, candidate = _active_definition(state)
        connection = _connection(candidate, source_node_id, second_effect_node_id)
        if not connection:
            return None
        for key, expected in source_effect_metadata.items():
            if connection[key] != expected:
                raise QaFailure(
                    "target reconnect changed connection {}: {!r} != {!r}".format(
                        key, connection[key], expected
                    )
                )
        return state

    client.wait_until("target endpoint reconnect", target_reconnected)

    def source_reconnect_ports():
        snapshot = client.component_snapshot()
        old_source = _image_port(snapshot, "output", effect_node_id)
        new_source = _image_port(snapshot, "output", second_effect_node_id)
        target = _image_port(snapshot, "input", output_node_id)
        return (
            (old_source, new_source, target)
            if old_source and new_source and target
            else None
        )

    _, new_source, _ = client.wait_until(
        "source reconnect ports", source_reconnect_ports
    )
    _select_connection(client, output_metadata["id"])
    _, source_handle = client.wait_component_settled(
        _connection_handle_component_id(output_metadata["id"], "source")
    )
    client.drag(component_center(source_handle), component_center(new_source), steps=12)

    def source_reconnected():
        state = client.state()
        _, candidate = _active_definition(state)
        connection = _connection(candidate, second_effect_node_id, output_node_id)
        if not connection:
            return None
        for key, expected in output_metadata.items():
            if connection[key] != expected:
                raise QaFailure(
                    "source reconnect changed connection {}: {!r} != {!r}".format(
                        key, connection[key], expected
                    )
                )
        return state

    after_reconnect = client.wait_until("source endpoint reconnect", source_reconnected)
    _, reconnected_definition = _active_definition(after_reconnect)
    if len(reconnected_definition["graph"]["connections"]) != 2:
        raise QaFailure("endpoint reconnect duplicated or dropped an authored connection")

    after_connect = after_reconnect
    project_before_navigation = after_connect["project"]
    history_before_navigation = after_connect["history"]

    _, before_pan_canvas = client.wait_component_settled(CANVAS_ID)
    before_pan_metadata = before_pan_canvas.get("metadata") or {}
    if before_pan_metadata.get("viewport_controller") != "shared":
        raise QaFailure("Node Editor did not use the shared ViewportController")
    before_pan = before_pan_metadata.get("pan") or {}
    pan_start = find_clear_canvas_point(
        client.component_snapshot(),
        CANVAS_ID,
        ("node_editor.node:", "node_editor.node_header:", "node_editor.port."),
    )
    pan_delta = {"x": 72.0, "y": 44.0}
    client.drag(
        pan_start,
        {"x": pan_start["x"] + pan_delta["x"], "y": pan_start["y"] + pan_delta["y"]},
        steps=8,
        button="middle",
    )
    _, after_pan_canvas = client.wait_component_settled(CANVAS_ID)
    after_pan = (after_pan_canvas.get("metadata") or {}).get("pan") or {}
    applied_pan = {
        "x": float(after_pan["x"]) - float(before_pan["x"]),
        "y": float(after_pan["y"]) - float(before_pan["y"]),
    }
    if abs(applied_pan["x"] - pan_delta["x"]) > 2.0 or abs(
        applied_pan["y"] - pan_delta["y"]
    ) > 2.0:
        raise QaFailure(
            "Node Editor pan was ignored or applied more than once: {!r}".format(applied_pan)
        )

    _, before_canvas = client.wait_component_settled(CANVAS_ID)
    scale_before = float((before_canvas.get("metadata") or {})["scale"])
    if not math.isfinite(scale_before) or scale_before <= 0:
        raise QaFailure("Node Editor published an invalid initial zoom")

    observed_scales = [scale_before]
    # Exercise the formerly freezing extreme zoom path repeatedly. Every
    # injected step must publish a fresh frame and remain health-responsive.
    for delta in (-1200.0, -1200.0, -1200.0, 1200.0, 1200.0):
        client.scroll_component(CANVAS_ID, 0.0, delta, modifiers={"command": True})
        _, candidate = client.wait_component_settled(CANVAS_ID)
        scale = float((candidate.get("metadata") or {})["scale"])
        if not math.isfinite(scale) or scale <= 0:
            raise QaFailure("Node Editor zoom became non-finite")
        observed_scales.append(scale)
        if client.request("/health").get("ok") is not True:
            raise QaFailure("Node Editor zoom made the app unresponsive")
    if len(set(observed_scales)) < 2:
        raise QaFailure("Node Editor ignored every zoom gesture")

    final = client.state()
    if (
        final["project"] != project_before_navigation
        or final["history"] != history_before_navigation
    ):
        raise QaFailure("Node Editor navigation mutated the Module or undo history")
    return {
        "suite": "node-editor",
        "item_id": node_clip["id"],
        "definition_id": current_definition_id,
        "created_node_id": effect_node_id,
        "reconnect_node_id": second_effect_node_id,
        "connection_count": 2,
        "reconnected_connection_ids": [
            source_effect_metadata["id"],
            output_metadata["id"],
        ],
        "direct_gesture_scale": gesture_scale,
        "pan_delta": applied_pan,
        "zoom_scales": observed_scales,
        "canvas": canvas_metadata,
        "history": final["history"],
        "actions": client.evidence,
    }


if __name__ == "__main__":
    raise SystemExit(
        run_suite_main(
            "qa-node-editor-e2e",
            run_suite,
            "target/qa-node-editor-e2e-evidence.json",
        )
    )
