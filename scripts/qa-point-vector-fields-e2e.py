#!/usr/bin/env python3
"""Exercise vector Point fields through shared Math Nodes and real rendering."""

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


def _value_kind(node):
    if node_content_type(node) != "value":
        return None
    return str((node.get("content") or {}).get("data", "")).replace("_", "").lower()


def _constant(node, key):
    prop = (node.get("properties") or {}).get(key) or {}
    if prop.get("type") != "constant":
        return None
    return (prop.get("properties") or {}).get("value")


def _edit_uniform(client, definition_id, node_id, value, description):
    control_id = "node_editor.property.node:{}:b".format(node_id)
    _, control = client.wait_component_settled(control_id)
    metadata = control.get("metadata") or {}
    if metadata.get("editor_kind") != "float" or metadata.get("connected") is not False:
        raise QaFailure(description + " bypassed the shared numeric uniform editor")
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
            and _constant(node, "b") == value
            else None
        )

    return client.wait_until(description + " exact uniform edit", edited)


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
            "Point vector graph routes differ: missing={!r}, extra={!r}".format(
                sorted(expected - actual), sorted(actual - expected)
            )
        )


def _assert_definition(definition, context):
    nodes = definition["graph"]["nodes"]
    native = {
        context["grid_id"]: "native.point.grid",
        context["info_id"]: "native.point.info",
        context["store_id"]: "native.point.store-vec3-attribute",
        context["length_id"]: "native.numeric.length",
        context["sprite_id"]: "native.particle.sprite-renderer",
    }
    for node_id, catalog_id in native.items():
        if node_id not in nodes or _catalog_id(nodes[node_id]) != catalog_id:
            raise QaFailure("Point vector graph lost " + catalog_id)
    values = {
        context["multiply_id"]: "multiply",
        context["add_id"]: "add",
        context["vector_divide_id"]: "divide",
        context["scalar_divide_id"]: "divide",
    }
    for node_id, kind in values.items():
        if node_id not in nodes or _value_kind(nodes[node_id]) != kind:
            raise QaFailure("Point vector graph lost Math Node " + kind)
    ramp = nodes.get(context["ramp_id"])
    ramp_data = str((ramp or {}).get("content", {}).get("data", ""))
    if (
        node_content_type(ramp or {}) != "color"
        or ramp_data.replace("_", "").lower() != "colorramp"
    ):
        raise QaFailure("Point vector graph lost Color Ramp")
    if node_content_type(nodes.get(context["output_id"], {})) != "moduleoutput":
        raise QaFailure("Point vector graph lost its Output boundary")
    if _constant(nodes[context["multiply_id"]], "b") != 0.5:
        raise QaFailure("Point vector graph changed Multiply uniform")
    if _constant(nodes[context["add_id"]], "b") != 0.0:
        raise QaFailure("Point vector graph changed Add uniform")
    if _constant(nodes[context["vector_divide_id"]], "b") != 1.0:
        raise QaFailure("Point vector graph changed its nonzero vector divisor")
    if _constant(nodes[context["scalar_divide_id"]], "b") != 60.0:
        raise QaFailure("Point vector graph changed radial normalization")
    _assert_routes(definition, context["routes"])


def run_suite(client):
    client.wait_health()
    source_snapshot, source = client.wait_component_settled("assets.node_clip_source")
    metadata = source.get("metadata") or {}
    if metadata.get("kind") != "node_clip" or metadata.get("draggable_to_timeline") is not True:
        raise QaFailure("New Node Clip Assets source lost its production drag contract")
    created = place_private_node_clip_source(
        client, source_snapshot, source, 6.5, "Point vector Node Clip"
    )
    item_id = created["item_id"]
    definition_id = created["definition_id"]
    start = media_seconds(created["item"]["interval"]["start"])
    opened_id, before = open_timeline_item_definition(
        client, item_id, "node_clip", "Point vector fields"
    )
    if opened_id != definition_id:
        raise QaFailure("Node Editor opened a different Point vector Definition")
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
        raise QaFailure("Node Editor did not reach Point vector authoring overview")
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
        ("multiply", "Multiply", "node_editor.menu.create.value:multiply", 0.34, 26.0),
        ("add", "Add", "node_editor.menu.create.value:add", 0.46, 26.0),
        ("vector_divide", "Divide", "node_editor.menu.create.value:divide", 0.58, 26.0),
        (
            "store",
            "Store Vec3 Attribute",
            "node_editor.menu.create.point_store_vec3_attribute",
            0.70,
            26.0,
        ),
        ("length", "Length", "node_editor.menu.create.numeric:length", 0.34, 140.0),
        ("scalar_divide", "Divide", "node_editor.menu.create.value:divide", 0.52, 140.0),
        ("ramp", "Color Ramp", "node_editor.menu.create.color:ramp", 0.70, 140.0),
    ]
    nodes = {}
    for name, query, choice_id, horizontal, vertical in specs:
        _, node_id = create_node_from_menu(client, "node_clip", query, choice_id)
        place_created_node(client, node_id, horizontal, vertical_offset=vertical)
        nodes[name] = node_id

    _edit_uniform(client, definition_id, nodes["multiply"], 0.5, "Multiply B")
    _edit_uniform(client, definition_id, nodes["scalar_divide"], 60.0, "Divide B")
    route_specs = [
        (nodes["grid"], "points", nodes["info"], "points", "Grid to Point Info"),
        (nodes["grid"], "points", nodes["store"], "points", "Grid to Vec3 Store"),
        (nodes["info"], "position", nodes["multiply"], "a", "Position to Multiply"),
        (nodes["multiply"], "result", nodes["add"], "a", "Multiply to Add"),
        (nodes["add"], "result", nodes["vector_divide"], "a", "Add to Divide"),
        (
            nodes["vector_divide"],
            "result",
            nodes["store"],
            "value",
            "Vector math to Store",
        ),
        (nodes["store"], "attribute", nodes["length"], "value", "Attribute to Length"),
        (
            nodes["length"],
            "result",
            nodes["scalar_divide"],
            "a",
            "Length to normalization",
        ),
        (
            nodes["scalar_divide"],
            "result",
            nodes["ramp"],
            "factor",
            "Normalized radius to Color Ramp",
        ),
        (nodes["store"], "points", nodes["sprite"], "particles", "Points to Sprite"),
        (nodes["sprite"], "image", output_id, "image_in", "Sprite to Output"),
        (nodes["ramp"], "color", nodes["sprite"], "color", "Ramp to Sprite Color"),
    ]
    connections = [
        connect_nodes(client, "node_clip", *route) for route in route_specs[:-1]
    ]
    unrouted = client.state()
    plain = sample_rendered_preview(
        client, start + 0.5, unrouted["history"]["revision"], "plain vector Grid"
    )
    open_timeline_item_definition(
        client, item_id, "node_clip", "Point vector radial color"
    )
    connections.append(connect_nodes(client, "node_clip", *route_specs[-1]))
    routed = client.state()
    expected_routes = {route[:4] for route in route_specs}
    context = {
        "item_id": item_id,
        "definition_id": definition_id,
        "grid_id": nodes["grid"],
        "info_id": nodes["info"],
        "multiply_id": nodes["multiply"],
        "add_id": nodes["add"],
        "vector_divide_id": nodes["vector_divide"],
        "store_id": nodes["store"],
        "length_id": nodes["length"],
        "scalar_divide_id": nodes["scalar_divide"],
        "ramp_id": nodes["ramp"],
        "sprite_id": nodes["sprite"],
        "output_id": output_id,
        "routes": expected_routes,
    }
    _assert_definition(
        routed["project"]["module_definitions"][definition_id], context
    )
    colored = sample_rendered_preview(
        client, start + 0.5, routed["history"]["revision"], "radial vector Grid"
    )
    if colored["pixel_hash"] == plain["pixel_hash"]:
        raise QaFailure("vector Length Color Ramp did not change real Grid pixels")

    color_connection = connections[-1]
    history_shortcut(client)

    def color_route_removed():
        state = client.state()
        routes = state["project"]["module_definitions"][definition_id]["graph"][
            "connections"
        ]
        return (
            state
            if all(route["id"] != color_connection["id"] for route in routes)
            else None
        )

    undone = client.wait_until(
        "Undo vector color route",
        color_route_removed,
    )
    undone_pixels = sample_rendered_preview(
        client, start + 0.5, undone["history"]["revision"], "undone vector color"
    )
    if undone_pixels["pixel_hash"] != plain["pixel_hash"]:
        raise QaFailure("Undo did not restore exact plain vector Grid pixels")
    history_shortcut(client, redo=True)

    def color_route_restored():
        state = client.state()
        routes = state["project"]["module_definitions"][definition_id]["graph"][
            "connections"
        ]
        return state if any(route == color_connection for route in routes) else None

    redone = client.wait_until(
        "Redo vector color route",
        color_route_restored,
    )
    restored = sample_rendered_preview(
        client, start + 0.5, redone["history"]["revision"], "redone vector color"
    )
    if restored["pixel_hash"] != colored["pixel_hash"]:
        raise QaFailure("Redo did not restore exact radial vector Grid pixels")

    samples = [
        restored,
        sample_rendered_preview(
            client, start + 1.0, redone["history"]["revision"], "Point vector"
        ),
    ]
    open_timeline_item_definition(
        client, item_id, "node_clip", "Point vector capture"
    )
    artifact_dir = pathlib.Path(os.environ["RUVIE_QA_ARTIFACT_DIR"])
    capture = capture_viewport(client, artifact_dir / "capture.png")
    project_path = os.environ.get("RUVIE_QA_PROJECT_PATH")
    if not project_path:
        raise QaFailure("Point vector persistence QA requires a Project file")
    project_file = pathlib.Path(project_path)
    saved, saved_file = save_project_to_disk(client, project_file, "Point vector")
    reloaded = reload_node_clip_project(
        client,
        project_file,
        saved,
        item_id,
        definition_id,
        samples,
        artifact_dir,
        "Point vector",
        "point-vector",
        "point-vector-reloaded.png",
        lambda definition: _assert_definition(definition, context),
    )
    return {
        "suite": "point-vector-fields",
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
            "qa-point-vector-fields-e2e",
            run_suite,
            "target/qa-point-vector-fields-e2e-evidence.json",
        )
    )
