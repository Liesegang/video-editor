#!/usr/bin/env python3
"""Exercise selected Point position updates through the production Node Editor."""

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
    write_json,
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


def _definition(state, definition_id):
    return state["project"]["module_definitions"][definition_id]


def _edit_float(
    client,
    definition_id,
    node_id,
    key,
    value,
    description,
    diagnostic_stem=None,
):
    control_id = "node_editor.property.node:{}:{}".format(node_id, key)
    _, control = client.wait_component_settled(control_id)
    metadata = control.get("metadata") or {}
    if metadata.get("editor_kind") != "float" or metadata.get("connected") is not False:
        raise QaFailure(description + " bypassed the shared Number editor")
    before = client.state()
    evidence_start = len(client.evidence)
    artifact_dir = pathlib.Path(os.environ["RUVIE_QA_ARTIFACT_DIR"])
    after_action = None
    if diagnostic_stem:
        capture_viewport(client, artifact_dir / (diagnostic_stem + "-before.png"))

        def capture_action(phase):
            capture = capture_viewport(
                client, artifact_dir / (diagnostic_stem + "-after-" + phase + ".png")
            )
            write_json(
                artifact_dir / (diagnostic_stem + "-after-" + phase + ".json"),
                {
                    "component_id": control_id,
                    "phase": phase,
                    "state": client.state(),
                    "components": client.component_snapshot(),
                    "capture": capture,
                    "actions": client.evidence[evidence_start:],
                },
            )

        after_action = capture_action
    enter_exact_numeric(client, control_id, value, after_action=after_action)
    if diagnostic_stem:
        after_input = client.state()
        capture_viewport(client, artifact_dir / (diagnostic_stem + "-after-input.png"))
        write_json(
            artifact_dir / (diagnostic_stem + "-operation.json"),
            {
                "component_id": control_id,
                "requested_value": value,
                "before": before,
                "after_input": after_input,
                "components_after_input": client.component_snapshot(),
                "actions": client.evidence[evidence_start:],
            },
        )

    def edited():
        state = client.state()
        node = _definition(state, definition_id)["graph"]["nodes"][node_id]
        return (
            state
            if state["history"]["revision"] == before["history"]["revision"] + 1
            and _constant(node, key) == value
            else None
        )

    return before, client.wait_until(description + " exact edit", edited)


def _edit_offset_x(client, definition_id, node_id, value):
    control_id = "node_editor.property.node:{}:offset:x".format(node_id)
    _, control = client.wait_component_settled(control_id)
    metadata = control.get("metadata") or {}
    if metadata.get("axis") != "X" or metadata.get("value") != 0.0:
        raise QaFailure("Set Point Position bypassed the shared Vec3 Offset editor")
    before = client.state()
    enter_exact_numeric(client, control_id, value)
    expected = {"x": value, "y": 0.0, "z": 0.0}

    def edited():
        state = client.state()
        node = _definition(state, definition_id)["graph"]["nodes"][node_id]
        return (
            state
            if state["history"]["revision"] == before["history"]["revision"] + 1
            and _constant(node, "offset") == expected
            else None
        )

    return before, client.wait_until("Set Point Position exact Offset X edit", edited)


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
            "Point position routes differ: missing={!r}, extra={!r}".format(
                sorted(expected - actual), sorted(actual - expected)
            )
        )


def _assert_definition(definition, context):
    expected_catalogs = {
        context["grid_id"]: "native.point.grid",
        context["info_id"]: "native.point.info",
        context["store_id"]: "native.point.store-vec3-attribute",
        context["greater_id"]: "native.logic.greater",
        context["position_id"]: "native.point.set-position",
        context["sprite_id"]: "native.particle.sprite-renderer",
    }
    nodes = definition["graph"]["nodes"]
    for node_id, catalog_id in expected_catalogs.items():
        if node_id not in nodes or _catalog_id(nodes[node_id]) != catalog_id:
            raise QaFailure("Point position graph lost " + catalog_id)
    if node_content_type(nodes.get(context["output_id"], {})) != "moduleoutput":
        raise QaFailure("Point position graph lost its Output boundary")
    if _constant(nodes[context["greater_id"]], "b") != 0.5:
        raise QaFailure("Point position graph changed its Selection threshold")
    if _constant(nodes[context["position_id"]], "offset") != {
        "x": 72.0,
        "y": 0.0,
        "z": 0.0,
    }:
        raise QaFailure("Point position graph changed its authored Offset")
    if _constant(nodes[context["position_id"]], "selection") is not True:
        raise QaFailure("Point position graph changed its Selection default")
    _assert_routes(definition, context["routes"])


def run_suite(client):
    client.wait_health()
    source_snapshot, source = client.wait_component_settled("assets.node_clip_source")
    metadata = source.get("metadata") or {}
    if metadata.get("kind") != "node_clip" or metadata.get("draggable_to_timeline") is not True:
        raise QaFailure("New Node Clip Assets source lost its production drag contract")
    created = place_private_node_clip_source(
        client, source_snapshot, source, 6.5, "Point position Node Clip"
    )
    item_id = created["item_id"]
    definition_id = created["definition_id"]
    start = media_seconds(created["item"]["interval"]["start"])
    opened_id, initial = open_timeline_item_definition(
        client, item_id, "node_clip", "Point position fields"
    )
    if opened_id != definition_id:
        raise QaFailure("Node Editor opened a different Point position Definition")
    output_ids = [
        node_id
        for node_id, node in initial["graph"]["nodes"].items()
        if node_content_type(node) == "moduleoutput"
    ]
    if len(output_ids) != 1 or initial["graph"]["connections"]:
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
        raise QaFailure("Node Editor did not reach Point position authoring overview")
    place_created_node(client, output_id, 0.95, vertical_offset=140.0)

    specs = [
        (
            "sprite",
            "Sprite Renderer",
            "node_editor.menu.create.particle_sprite_renderer",
            0.91,
        ),
        ("grid", "Point Grid", "node_editor.menu.create.point_grid", 0.05),
        ("info", "Point Info", "node_editor.menu.create.point_info", 0.20),
        (
            "store",
            "Store Vec3 Attribute",
            "node_editor.menu.create.point_store_vec3_attribute",
            0.37,
        ),
        ("greater", "Greater Than", "node_editor.menu.create.logic:greater", 0.54),
        (
            "position",
            "Set Point Position",
            "node_editor.menu.create.point_set_position",
            0.72,
        ),
    ]
    nodes = {}
    for name, query, choice_id, horizontal in specs:
        _, node_id = create_node_from_menu(client, "node_clip", query, choice_id)
        place_created_node(client, node_id, horizontal)
        nodes[name] = node_id

    _edit_float(client, definition_id, nodes["greater"], "b", 0.5, "Selection threshold")
    route_specs = [
        (nodes["grid"], "points", nodes["info"], "points", "Grid to Point Info"),
        (nodes["grid"], "points", nodes["store"], "points", "Grid to Vec3 Store"),
        (nodes["info"], "position", nodes["store"], "value", "Position to Store"),
        (
            nodes["store"],
            "points",
            nodes["position"],
            "points",
            "Stored points to Set Position",
        ),
        (
            nodes["store"],
            "attribute",
            nodes["position"],
            "position",
            "Stored Position to Set Position",
        ),
        (nodes["info"], "random", nodes["greater"], "a", "Random to Selection"),
        (
            nodes["greater"],
            "result",
            nodes["position"],
            "selection",
            "Comparison to Selection",
        ),
        (
            nodes["position"],
            "points",
            nodes["sprite"],
            "particles",
            "Positioned points to Sprite",
        ),
        (nodes["sprite"], "image", output_id, "image_in", "Sprite to Output"),
    ]
    connections = [connect_nodes(client, "node_clip", *route) for route in route_specs]
    routed = client.state()
    expected_routes = {route[:4] for route in route_specs}
    _assert_routes(_definition(routed, definition_id), expected_routes)
    baseline = sample_rendered_preview(
        client, start + 0.5, routed["history"]["revision"], "zero-offset selected Grid"
    )

    open_timeline_item_definition(
        client, item_id, "node_clip", "Point position Offset edit"
    )
    offset_before, offset_edited = _edit_offset_x(
        client, definition_id, nodes["position"], 72.0
    )
    moved = sample_rendered_preview(
        client,
        start + 0.5,
        offset_edited["history"]["revision"],
        "selected displaced Grid",
    )
    if moved["pixel_hash"] == baseline["pixel_hash"]:
        raise QaFailure("Set Point Position Offset did not change real Grid pixels")

    history_shortcut(client)
    undone = client.wait_until(
        "Undo exact Point Offset edit",
        lambda: state
        if (state := client.state())["history"]["revision"]
        == offset_edited["history"]["revision"] + 1
        and state["project"] == offset_before["project"]
        else None,
    )
    undone_pixels = sample_rendered_preview(
        client, start + 0.5, undone["history"]["revision"], "undone Point Offset"
    )
    if undone_pixels["pixel_hash"] != baseline["pixel_hash"]:
        raise QaFailure("Undo did not restore exact zero-offset Grid pixels")
    history_shortcut(client, redo=True)
    redone = client.wait_until(
        "Redo exact Point Offset edit",
        lambda: state
        if (state := client.state())["history"]["revision"]
        == undone["history"]["revision"] + 1
        and state["project"] == offset_edited["project"]
        else None,
    )
    redone_pixels = sample_rendered_preview(
        client, start + 0.5, redone["history"]["revision"], "redone Point Offset"
    )
    if redone_pixels["pixel_hash"] != moved["pixel_hash"]:
        raise QaFailure("Redo did not restore exact selected displacement pixels")

    open_timeline_item_definition(
        client, item_id, "node_clip", "Point position Selection proof"
    )
    selection_before, selection_none = _edit_float(
        client,
        definition_id,
        nodes["greater"],
        "b",
        1.1,
        "Empty Selection threshold",
        diagnostic_stem="selection-threshold",
    )
    none_selected = sample_rendered_preview(
        client,
        start + 0.5,
        selection_none["history"]["revision"],
        "empty-selection displaced Grid",
    )
    if none_selected["pixel_hash"] != baseline["pixel_hash"]:
        raise QaFailure("Set Point Position moved points outside its Boolean Selection")
    history_shortcut(client)
    selected = client.wait_until(
        "Undo exact Selection threshold",
        lambda: state
        if (state := client.state())["history"]["revision"]
        == selection_none["history"]["revision"] + 1
        and state["project"] == selection_before["project"]
        else None,
    )
    selected_pixels = sample_rendered_preview(
        client,
        start + 0.5,
        selected["history"]["revision"],
        "restored selected Point displacement",
    )
    if selected_pixels["pixel_hash"] != moved["pixel_hash"]:
        raise QaFailure("Undo did not restore exact Boolean-selected displacement pixels")

    open_timeline_item_definition(
        client, item_id, "node_clip", "Point position bypass"
    )
    bypass_before = client.state()
    _, bypass_control = client.wait_component_settled(
        "node_editor.node_state:" + nodes["position"]
    )
    bypass_metadata = bypass_control.get("metadata") or {}
    if (
        bypass_metadata.get("node_id") != nodes["position"]
        or bypass_metadata.get("bypassed") is not False
        or bypass_metadata.get("supports_bypass") is not True
        or bypass_control.get("enabled") is not True
    ):
        raise QaFailure("Set Point Position bypass control lost its Node state")
    client.click_component("node_editor.node_state:" + nodes["position"])

    def position_bypassed():
        state = client.state()
        definition = _definition(state, definition_id)
        node = definition["graph"]["nodes"][nodes["position"]]
        return (
            state
            if state["history"]["revision"] == bypass_before["history"]["revision"] + 1
            and node.get("enabled") is True
            and node.get("bypassed") is True
            else None
        )

    bypassed = client.wait_until("Set Point Position header bypass", position_bypassed)
    _assert_routes(_definition(bypassed, definition_id), expected_routes)
    bypassed_pixels = sample_rendered_preview(
        client,
        start + 0.5,
        bypassed["history"]["revision"],
        "bypassed Point position",
    )
    if bypassed_pixels["pixel_hash"] != baseline["pixel_hash"]:
        raise QaFailure("Set Point Position bypass did not restore incoming Point pixels")
    history_shortcut(client)
    resumed = client.wait_until(
        "Undo exact Set Point Position bypass",
        lambda: state
        if (state := client.state())["history"]["revision"]
        == bypassed["history"]["revision"] + 1
        and state["project"] == bypass_before["project"]
        else None,
    )
    _assert_routes(_definition(resumed, definition_id), expected_routes)
    resumed_pixels = sample_rendered_preview(
        client,
        start + 0.5,
        resumed["history"]["revision"],
        "resumed Point position",
    )
    if resumed_pixels["pixel_hash"] != moved["pixel_hash"]:
        raise QaFailure("Undo bypass did not restore exact displaced Point pixels")

    context = {
        "grid_id": nodes["grid"],
        "info_id": nodes["info"],
        "store_id": nodes["store"],
        "greater_id": nodes["greater"],
        "position_id": nodes["position"],
        "sprite_id": nodes["sprite"],
        "output_id": output_id,
        "routes": expected_routes,
    }
    final_definition = _definition(resumed, definition_id)
    _assert_definition(final_definition, context)
    samples = [
        resumed_pixels,
        sample_rendered_preview(
            client, start + 1.0, resumed["history"]["revision"], "Point position"
        ),
    ]
    open_timeline_item_definition(
        client, item_id, "node_clip", "Point position capture"
    )
    artifact_dir = pathlib.Path(os.environ["RUVIE_QA_ARTIFACT_DIR"])
    capture = capture_viewport(client, artifact_dir / "capture.png")
    project_path = os.environ.get("RUVIE_QA_PROJECT_PATH")
    if not project_path:
        raise QaFailure("Point position persistence QA requires a Project file")
    project_file = pathlib.Path(project_path)
    saved, saved_file = save_project_to_disk(client, project_file, "Point position")
    reloaded = reload_node_clip_project(
        client,
        project_file,
        saved,
        item_id,
        definition_id,
        samples,
        artifact_dir,
        "Point position",
        "point-position",
        "point-position-reloaded.png",
        lambda definition: _assert_definition(definition, context),
    )
    return {
        "suite": "point-position",
        "item_id": item_id,
        "definition_id": definition_id,
        "nodes": nodes,
        "connections": connections,
        "samples": samples,
        "selection_none": none_selected,
        "bypassed": bypassed_pixels,
        "saved": saved_file,
        "capture": capture,
        "reloaded": reloaded,
        "actions": client.evidence,
    }


if __name__ == "__main__":
    raise SystemExit(
        run_suite_main(
            "qa-point-position-e2e",
            run_suite,
            "target/qa-point-position-e2e-evidence.json",
        )
    )
