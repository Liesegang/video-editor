#!/usr/bin/env python3
"""Exercise Boolean Point fields through Compare and typed Select Nodes."""

import os
import pathlib

from qa_automation_support import history_shortcut
from qa_node_module_support import (
    connect_nodes,
    create_node_from_menu,
    enter_exact_numeric,
    node_content_type,
    open_timeline_item_definition,
    place_created_node,
    place_private_node_clip_source,
    reload_node_clip_project,
    sample_rendered_preview,
)
from qa_support import (
    QaClient,
    QaFailure,
    capture_viewport,
    media_seconds,
    run_suite_main,
    save_project_to_disk,
)


def _catalog_id(node):
    if node_content_type(node) != "nativeoperation":
        return None
    data = (node.get("content") or {}).get("data")
    return data.get("catalog_id") if isinstance(data, dict) else None


def _constant(node, key):
    prop = (node.get("properties") or {}).get(key) or {}
    if prop.get("type") != "constant":
        return None
    return (prop.get("properties") or {}).get("value")


def _edit_float(client, definition_id, node_id, key, value, description):
    control_id = "node_editor.property.node:{}:{}".format(node_id, key)
    _, control = client.wait_component_settled(control_id)
    metadata = control.get("metadata") or {}
    if metadata.get("editor_kind") != "float" or metadata.get("connected") is not False:
        raise QaFailure(description + " bypassed the shared Number editor")
    before = client.state()
    enter_exact_numeric(client, control_id, value)

    def edited():
        state = client.state()
        node = state["project"]["module_definitions"][definition_id]["graph"][
            "nodes"
        ][node_id]
        return (
            state
            if state["history"]["revision"] == before["history"]["revision"] + 1
            and _constant(node, key) == value
            else None
        )

    return client.wait_until(description + " exact edit", edited)


def _assert_routes(definition, expected):
    actual = {
        (
            route["from"]["node_id"],
            route["from"]["port"],
            route["to"]["node_id"],
            route["to"]["port"],
        )
        for route in definition["graph"]["connections"]
    }
    if actual != expected:
        raise QaFailure(
            "Point conditional routes differ: missing={!r}, extra={!r}".format(
                sorted(expected - actual), sorted(actual - expected)
            )
        )


def _assert_definition(definition, context, threshold):
    nodes = definition["graph"]["nodes"]
    expected_native = {
        context["grid_id"]: "native.point.grid",
        context["info_id"]: "native.point.info",
        context["number_id"]: "native.point.store-number-attribute",
        context["greater_id"]: "native.logic.greater",
        context["boolean_id"]: "native.point.store-boolean-attribute",
        context["select_id"]: "native.logic.select-number",
        context["sprite_id"]: "native.particle.sprite-renderer",
    }
    for node_id, catalog_id in expected_native.items():
        if node_id not in nodes or _catalog_id(nodes[node_id]) != catalog_id:
            raise QaFailure("Point conditional graph lost " + catalog_id)
    ramp = nodes.get(context["ramp_id"])
    ramp_data = str((ramp or {}).get("content", {}).get("data", ""))
    if (
        node_content_type(ramp or {}) != "color"
        or ramp_data.replace("_", "").lower() != "colorramp"
    ):
        raise QaFailure("Point conditional graph lost Color Ramp")
    if node_content_type(nodes.get(context["output_id"], {})) != "moduleoutput":
        raise QaFailure("Point conditional graph lost its Output boundary")
    if nodes[context["number_id"]].get("name") != "Store Number Attribute":
        raise QaFailure("Number Point attribute lost its authoritative Node name")
    if nodes[context["boolean_id"]].get("name") != "Store Boolean Attribute":
        raise QaFailure("Boolean Point attribute lost its authoritative Node name")
    if _constant(nodes[context["greater_id"]], "b") != threshold:
        raise QaFailure("Greater Than threshold changed")
    if _constant(nodes[context["select_id"]], "if_true") != 1.0:
        raise QaFailure("Select Number true branch changed")
    if _constant(nodes[context["select_id"]], "if_false") != 0.0:
        raise QaFailure("Select Number false branch changed")
    _assert_routes(definition, context["routes"])


def run_suite(client):
    client.wait_health()
    source_snapshot, source = client.wait_component_settled("assets.node_clip_source")
    metadata = source.get("metadata") or {}
    if metadata.get("kind") != "node_clip" or metadata.get("draggable_to_timeline") is not True:
        raise QaFailure("New Node Clip Assets source lost its production drag contract")
    created = place_private_node_clip_source(
        client, source_snapshot, source, 6.5, "Point conditional Node Clip"
    )
    item_id = created["item_id"]
    definition_id = created["definition_id"]
    start = media_seconds(created["item"]["interval"]["start"])
    opened_id, before = open_timeline_item_definition(
        client, item_id, "node_clip", "Point conditional fields"
    )
    if opened_id != definition_id:
        raise QaFailure("Node Editor opened a different Point conditional Definition")
    output_ids = [
        node_id
        for node_id, node in before["graph"]["nodes"].items()
        if node_content_type(node) == "moduleoutput"
    ]
    if len(output_ids) != 1 or before["graph"]["connections"]:
        raise QaFailure("New Node Clip is not the clean Output-only production fixture")
    output_id = output_ids[0]

    _, canvas_before = client.wait_component_settled("node_editor.canvas")
    old_scale = float(canvas_before["metadata"]["scale"])
    client.scroll_component(
        "node_editor.canvas", 0.0, -100.0, modifiers={"command": True}
    )
    _, canvas = client.wait_component_settled("node_editor.canvas")
    scale = float(canvas["metadata"]["scale"])
    if scale >= old_scale or not 0.4 <= scale <= 0.75:
        raise QaFailure("Node Editor did not reach Point conditional authoring overview")
    place_created_node(client, output_id, 0.95, vertical_offset=140.0)

    specs = [
        (
            "sprite",
            "Sprite Renderer",
            "node_editor.menu.create.particle_sprite_renderer",
            0.92,
            26.0,
        ),
        ("grid", "Point Grid", "node_editor.menu.create.point_grid", 0.06, 26.0),
        ("info", "Point Info", "node_editor.menu.create.point_info", 0.20, 26.0),
        (
            "number",
            "Store Number Attribute",
            "node_editor.menu.create.point_store_number_attribute",
            0.35,
            26.0,
        ),
        (
            "greater",
            "Greater Than",
            "node_editor.menu.create.logic:greater",
            0.50,
            26.0,
        ),
        (
            "boolean",
            "Store Boolean Attribute",
            "node_editor.menu.create.point_store_boolean_attribute",
            0.65,
            26.0,
        ),
        (
            "select",
            "Select Number",
            "node_editor.menu.create.logic:select-number",
            0.46,
            140.0,
        ),
        ("ramp", "Color Ramp", "node_editor.menu.create.color:ramp", 0.70, 140.0),
    ]
    nodes = {}
    for name, query, choice_id, horizontal, vertical in specs:
        _, node_id = create_node_from_menu(client, "node_clip", query, choice_id)
        place_created_node(client, node_id, horizontal, vertical_offset=vertical)
        nodes[name] = node_id

    _edit_float(client, definition_id, nodes["greater"], "b", 0.5, "threshold")
    _edit_float(client, definition_id, nodes["select"], "if_true", 1.0, "true branch")
    route_specs = [
        (nodes["grid"], "points", nodes["info"], "points", "Grid to Point Info"),
        (nodes["grid"], "points", nodes["number"], "points", "Grid to Number Store"),
        (nodes["info"], "random", nodes["number"], "value", "Random to Number Store"),
        (
            nodes["number"],
            "points",
            nodes["boolean"],
            "points",
            "Number Store to Boolean Store",
        ),
        (
            nodes["number"],
            "attribute",
            nodes["greater"],
            "a",
            "stored Random to Greater Than",
        ),
        (
            nodes["greater"],
            "result",
            nodes["boolean"],
            "value",
            "comparison to Boolean Store",
        ),
        (
            nodes["boolean"],
            "attribute",
            nodes["select"],
            "condition",
            "Boolean attribute to Select",
        ),
        (
            nodes["boolean"],
            "points",
            nodes["sprite"],
            "particles",
            "Boolean Store to Sprite",
        ),
        (
            nodes["select"],
            "result",
            nodes["ramp"],
            "factor",
            "selected mask to Color Ramp",
        ),
        (
            nodes["ramp"],
            "color",
            nodes["sprite"],
            "color",
            "Color Ramp to Sprite",
        ),
        (nodes["sprite"], "image", output_id, "image_in", "Sprite to Output"),
    ]
    connections = [connect_nodes(client, "node_clip", *route) for route in route_specs]
    routed = client.state()
    context = {
        "item_id": item_id,
        "definition_id": definition_id,
        "grid_id": nodes["grid"],
        "info_id": nodes["info"],
        "number_id": nodes["number"],
        "greater_id": nodes["greater"],
        "boolean_id": nodes["boolean"],
        "select_id": nodes["select"],
        "ramp_id": nodes["ramp"],
        "sprite_id": nodes["sprite"],
        "output_id": output_id,
        "routes": {route[:4] for route in route_specs},
    }
    _assert_definition(
        routed["project"]["module_definitions"][definition_id], context, 0.5
    )
    midpoint = sample_rendered_preview(
        client, start + 0.5, routed["history"]["revision"], "midpoint mask"
    )
    open_timeline_item_definition(
        client, item_id, "node_clip", "Point conditional threshold"
    )
    changed = _edit_float(
        client, definition_id, nodes["greater"], "b", 0.85, "threshold"
    )
    high = sample_rendered_preview(
        client, start + 0.5, changed["history"]["revision"], "high threshold mask"
    )
    if high["pixel_hash"] == midpoint["pixel_hash"]:
        raise QaFailure("Greater Than threshold did not change real masked Grid pixels")

    history_shortcut(client)

    def threshold_undone():
        state = client.state()
        node = state["project"]["module_definitions"][definition_id]["graph"][
            "nodes"
        ][nodes["greater"]]
        return (
            state
            if state["history"]["revision"] == changed["history"]["revision"] + 1
            and _constant(node, "b") == 0.5
            else None
        )

    undone = client.wait_until("Undo conditional threshold", threshold_undone)
    undone_pixels = sample_rendered_preview(
        client, start + 0.5, undone["history"]["revision"], "undone threshold"
    )
    if undone_pixels["pixel_hash"] != midpoint["pixel_hash"]:
        raise QaFailure("Undo did not restore exact midpoint mask pixels")
    history_shortcut(client, redo=True)

    def threshold_redone():
        state = client.state()
        node = state["project"]["module_definitions"][definition_id]["graph"][
            "nodes"
        ][nodes["greater"]]
        return (
            state
            if state["history"]["revision"] == undone["history"]["revision"] + 1
            and _constant(node, "b") == 0.85
            else None
        )

    redone = client.wait_until("Redo conditional threshold", threshold_redone)
    restored = sample_rendered_preview(
        client, start + 0.5, redone["history"]["revision"], "redone threshold"
    )
    if restored["pixel_hash"] != high["pixel_hash"]:
        raise QaFailure("Redo did not restore exact high-threshold mask pixels")

    samples = [
        restored,
        sample_rendered_preview(
            client, start + 1.0, redone["history"]["revision"], "Point conditional"
        ),
    ]
    open_timeline_item_definition(
        client, item_id, "node_clip", "Point conditional capture"
    )
    artifact_dir = pathlib.Path(os.environ["RUVIE_QA_ARTIFACT_DIR"])
    capture = capture_viewport(client, artifact_dir / "capture.png")
    project_path = os.environ.get("RUVIE_QA_PROJECT_PATH")
    if not project_path:
        raise QaFailure("Point conditional persistence QA requires a Project file")
    project_file = pathlib.Path(project_path)
    saved, saved_file = save_project_to_disk(client, project_file, "Point conditional")
    reloaded = reload_node_clip_project(
        client,
        project_file,
        saved,
        item_id,
        definition_id,
        samples,
        artifact_dir,
        "Point conditional",
        "point-conditional",
        "point-conditional-reloaded.png",
        lambda definition: _assert_definition(definition, context, 0.85),
    )
    return {
        "suite": "point-conditional-fields",
        "item_id": item_id,
        "definition_id": definition_id,
        "nodes": nodes,
        "connections": connections,
        "samples": samples,
        "saved": saved_file,
        "capture": capture,
        "reloaded": reloaded,
        "actions": client.evidence,
    }


if __name__ == "__main__":
    raise SystemExit(
        run_suite_main(
            "qa-point-conditional-fields-e2e",
            run_suite,
            "target/qa-point-conditional-fields-e2e-evidence.json",
        )
    )
