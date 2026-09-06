#!/usr/bin/env python3
"""Route the production Gradient and Color Ramp Nodes into a real Solid."""

import os
import pathlib

from qa_node_module_support import (
    active_definition,
    connection,
    create_node_from_menu,
    node_content_type,
    place_node_for_inline_edit,
    port,
)
from qa_support import (
    QaFailure,
    activate_dock_tab,
    bring_timeline_component,
    capture_viewport,
    component_center,
    item_by_name,
    media_seconds,
    run_suite_main,
    seek_timeline_seconds,
    settled_preview_state,
)


def _catalog_id(node):
    content = node.get("content") or {}
    if node_content_type(node) not in {"nativeoperation", "data", "color"}:
        return None
    data = content.get("data")
    if isinstance(data, dict):
        return data.get("catalog_id")
    value = str(data or "").replace("_", "").lower()
    return {
        "gradient": "native.data.gradient",
        "colorramp": "native.color.ramp",
    }.get(value)


def _constant(node, key):
    prop = (node.get("properties") or {}).get(key) or {}
    if prop.get("type") != "constant":
        return None
    return (prop.get("properties") or {}).get("value")


def _connect(client, source_id, source_port, target_id, target_port, description):
    def visible_ports():
        snapshot = client.component_snapshot()
        source = port(snapshot, "output", source_id, port_key=source_port)
        target = port(snapshot, "input", target_id, port_key=target_port)
        return (source, target) if source and target else None

    source, target = client.wait_until(description + " ports", visible_ports)
    client.drag(component_center(source), component_center(target), steps=10)

    def connected():
        definition = active_definition(client.state(), "node_clip")[1]
        candidate = connection(definition, source_id, target_id)
        return (
            candidate
            if candidate is not None
            and candidate["from"]["port"] == source_port
            and candidate["to"]["port"] == target_port
            else None
        )

    return client.wait_until(description + " connection", connected)


def _enter_exact_numeric(client, component_id, value):
    client.click_component(component_id)
    client.key("a", True, command=True)
    client.key("a", False, command=True)
    client.inject("text", {"text": format(float(value), "g")})
    client.key("enter", True)
    client.key("enter", False)


def _place_created_node(client, node_id, horizontal_fraction):
    _, canvas = client.wait_component_settled("node_editor.canvas")
    _, header = client.wait_component_settled("node_editor.node_header:" + node_id)
    bounds = canvas["rect_points"]
    target = {
        "x": float(bounds["min_x"])
        + float(bounds["width"]) * horizontal_fraction,
        "y": float(bounds["min_y"]) + 26.0,
    }
    client.drag(component_center(header), target, steps=10)
    _, moved = client.wait_component_settled("node_editor.node_header:" + node_id)
    if abs(float(moved["rect_points"]["center_x"]) - target["x"]) > 3.0:
        raise QaFailure("created Node did not follow its production header drag")
    return moved


def _rendered(client, revision, frame):
    return client.wait_until(
        "Color Ramp Preview",
        lambda: state
        if (
            (state := settled_preview_state(client, revision, frame))
            and state["editor"]["preview"].get("pixel_hash")
            and int(state["editor"]["preview"].get("nontransparent_pixels") or 0) > 0
        )
        else None,
        30.0,
    )


def run_suite(client):
    client.wait_health()
    initial = client.state()
    item = item_by_name(initial["project"], "QA Node Clip")
    item_id = item["id"]
    activate_dock_tab(client, "dock.tab:timeline", "Timeline", "Color Ramp Node Clip")
    bring_timeline_component(client, "timeline.item:" + item_id, -120.0)
    client.double_click_component("timeline.item:" + item_id)
    activate_dock_tab(client, "dock.tab:node_editor", "Node Editor", "Color Ramp")
    client.wait_component_settled("node_editor.canvas")

    definition_id, before = active_definition(client.state(), "node_clip")
    output_ids = [
        node_id
        for node_id, node in before["graph"]["nodes"].items()
        if node_content_type(node) == "moduleoutput"
    ]
    if len(output_ids) != 1:
        raise QaFailure("Color Ramp fixture has no unique Image Output")
    output_id = output_ids[0]
    output_routes = [
        candidate
        for candidate in before["graph"]["connections"]
        if candidate["to"]["node_id"] == output_id
    ]
    if len(output_routes) != 1:
        raise QaFailure("Color Ramp fixture has no unique starter Image route")

    _, gradient_id = create_node_from_menu(
        client,
        "node_clip",
        "Gradient",
        "node_editor.menu.create.data:gradient",
    )
    _place_created_node(client, gradient_id, 0.64)
    _, ramp_id = create_node_from_menu(
        client,
        "node_clip",
        "Color Ramp",
        "node_editor.menu.create.color:ramp",
    )
    _place_created_node(client, ramp_id, 0.78)
    _, solid_id = create_node_from_menu(
        client,
        "node_clip",
        "Solid",
        "node_editor.menu.create.solid",
    )
    _place_created_node(client, solid_id, 0.92)

    current = active_definition(client.state(), "node_clip")[1]
    expected_catalogs = {
        gradient_id: "native.data.gradient",
        ramp_id: "native.color.ramp",
    }
    for node_id, expected in expected_catalogs.items():
        if _catalog_id(current["graph"]["nodes"][node_id]) != expected:
            raise QaFailure("Node menu created the wrong " + expected + " implementation")

    route = output_routes[0]
    output_input_port = route["to"]["port"]
    client.click_component("node_editor.connection:" + route["id"], button="secondary")
    client.wait_component_settled("node_editor.wire_menu.disconnect")
    client.click_component("node_editor.wire_menu.disconnect")
    client.wait_until(
        "starter Image route disconnected",
        lambda: state
        if connection(
            active_definition((state := client.state()), "node_clip")[1],
            route["from"]["node_id"],
            output_id,
        )
        is None
        else None,
    )
    gradient_link = _connect(
        client, gradient_id, "value", ramp_id, "gradient", "Gradient to Color Ramp"
    )
    ramp_link = _connect(
        client, ramp_id, "color", solid_id, "color", "Color Ramp to Solid"
    )
    image_link = _connect(
        client,
        solid_id,
        "image",
        output_id,
        output_input_port,
        "Solid to Output",
    )

    routed = client.state()
    routed_definition = routed["project"]["module_definitions"][definition_id]
    if len(routed_definition["graph"]["connections"]) != len(before["graph"]["connections"]) + 2:
        raise QaFailure("Color Ramp routing added or dropped graph connections")
    factor_before = _constant(routed_definition["graph"]["nodes"][ramp_id], "factor")
    if not isinstance(factor_before, (int, float)) or float(factor_before) != 0.5:
        raise QaFailure("Color Ramp did not expose its authoritative Factor default")

    sample_time = media_seconds(item["interval"]["start"]) + 0.5
    activate_dock_tab(client, "dock.tab:timeline", "Timeline", "Color Ramp sample")
    sought = seek_timeline_seconds(client, sample_time)
    frame = sought["editor"]["timeline"]["current_frame"]
    _rendered(client, routed["history"]["revision"], frame)

    bring_timeline_component(client, "timeline.item:" + item_id, -120.0)
    client.double_click_component("timeline.item:" + item_id)
    activate_dock_tab(client, "dock.tab:node_editor", "Node Editor", "Color Ramp edit")
    factor_control = "node_editor.property.node:{}:factor".format(ramp_id)
    place_node_for_inline_edit(client, ramp_id, factor_control)
    before_factor_edit = client.state()
    baseline = _rendered(client, before_factor_edit["history"]["revision"], frame)
    _enter_exact_numeric(client, factor_control, 0.9)

    def factor_edited():
        state = client.state()
        definition = state["project"]["module_definitions"][definition_id]
        value = _constant(definition["graph"]["nodes"][ramp_id], "factor")
        return (
            state
            if state["history"]["revision"]
            == before_factor_edit["history"]["revision"] + 1
            and isinstance(value, (int, float))
            and float(value) == 0.9
            else None
        )

    edited = client.wait_until("Color Ramp Factor exact edit", factor_edited)
    edited_render = _rendered(client, edited["history"]["revision"], frame)
    if edited_render["editor"]["preview"]["pixel_hash"] == baseline["editor"]["preview"]["pixel_hash"]:
        raise QaFailure("Color Ramp Factor did not change real Solid pixels")

    gradient_control = "node_editor.property.node:{}:value".format(gradient_id)
    gradient = place_node_for_inline_edit(client, gradient_id, gradient_control)
    if (gradient.get("metadata") or {}).get("editor_kind") != "gradient":
        raise QaFailure("Gradient Node bypassed the shared typed Gradient editor")
    client.click_component(gradient_control)
    stop_id = gradient_control + ".gradient.stop.0.offset"
    _, stop = client.wait_component_settled(stop_id)
    if (stop.get("metadata") or {}).get("offset") is None:
        raise QaFailure("shared Gradient popup omitted its first stop value")
    capture = capture_viewport(
        client,
        pathlib.Path(os.environ["RUVIE_QA_ARTIFACT_DIR"]) / "color-ramp.png",
    )

    return {
        "suite": "color-ramp",
        "definition_id": definition_id,
        "nodes": {
            "gradient": gradient_id,
            "ramp": ramp_id,
            "solid": solid_id,
            "output": output_id,
        },
        "connections": [gradient_link, ramp_link, image_link],
        "factor": {"before": factor_before, "after": 0.9},
        "pixels": {
            "before": baseline["editor"]["preview"]["pixel_hash"],
            "after": edited_render["editor"]["preview"]["pixel_hash"],
        },
        "gradient_control": gradient,
        "gradient_stop": stop,
        "capture": capture,
        "actions": client.evidence,
    }


if __name__ == "__main__":
    raise SystemExit(
        run_suite_main(
            "qa-color-ramp-e2e",
            run_suite,
            "target/qa-color-ramp-e2e-evidence.json",
        )
    )
