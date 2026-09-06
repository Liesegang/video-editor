#!/usr/bin/env python3
"""Exercise vector Point fields through shared Math Nodes and real rendering."""

import os
import pathlib

from qa_automation_support import history_shortcut
from qa_node_module_support import (
    active_definition,
    connect_nodes,
    create_node_from_menu,
    enter_exact_numeric,
    node_content_type,
    place_created_node,
    place_private_node_clip_source,
)
from qa_support import (
    QA_APP_BINARY_ENV,
    QaClient,
    QaFailure,
    activate_dock_tab,
    bring_timeline_component,
    capture_viewport,
    close_clean_native_app,
    free_port,
    media_seconds,
    request_clean_native_close,
    run_suite_main,
    save_project_to_disk,
    seek_timeline_seconds,
    settled_preview_state,
    spawned_authoring_app,
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


def _sample(client, seconds, revision, description):
    activate_dock_tab(client, "dock.tab:timeline", "Timeline", description)
    sought = seek_timeline_seconds(client, seconds)
    frame = sought["editor"]["timeline"]["current_frame"]

    def rendered():
        state = settled_preview_state(client, revision, frame)
        preview = (state or {}).get("editor", {}).get("preview", {})
        return (
            state
            if state
            and state["editor"].get("error") is None
            and preview.get("pixel_hash")
            and int(preview.get("nontransparent_pixels") or 0) > 0
            else None
        )

    state = client.wait_until(description + " rendered Preview", rendered, 30.0)
    preview = state["editor"]["preview"]
    return {
        "seconds": seconds,
        "frame": frame,
        "pixel_hash": preview["pixel_hash"],
        "nontransparent_pixels": preview["nontransparent_pixels"],
    }


def _open_definition(client, item_id, description):
    bring_timeline_component(client, "timeline.item:" + item_id, -120.0)
    client.double_click_component("timeline.item:" + item_id)
    activate_dock_tab(client, "dock.tab:node_editor", "Node Editor", description)
    client.wait_component_settled("node_editor.canvas")
    return active_definition(client.state(), "node_clip")


def _reload(client, project_file, saved, context, samples, artifact_dir):
    initial_close = request_clean_native_close(
        client, "Point vector authoring", client.timeout
    )
    port_number = free_port()
    environment = {
        "RUVIE_QA_PROJECT_PATH": str(project_file),
        "RUVIE_QA_OPEN_EXISTING_PROJECT": "1",
        "RUVIE_QA_PORT_FILE": None,
        "RUVIE_QA_RUN_ID": os.environ.get("RUVIE_QA_RUN_ID", "point-vector")
        + ":reload",
        QA_APP_BINARY_ENV: os.environ.get(QA_APP_BINARY_ENV),
    }
    with spawned_authoring_app(port_number, environment) as process:
        fresh = QaClient("http://127.0.0.1:{}".format(port_number), client.timeout)
        fresh.wait_health()
        state = fresh.state()
        if state["project"] != saved["project"]:
            raise QaFailure("fresh process changed Point vector Project state")
        revision = state["history"]["revision"]
        reloaded_samples = [
            _sample(fresh, sample["seconds"], revision, "reloaded Point vector")
            for sample in samples
        ]
        if [sample["pixel_hash"] for sample in reloaded_samples] != [
            sample["pixel_hash"] for sample in samples
        ]:
            raise QaFailure("fresh process changed Point vector pixels")
        definition_id, definition = _open_definition(
            fresh, context["item_id"], "Point vector reload"
        )
        if definition_id != context["definition_id"]:
            raise QaFailure("fresh process changed Point vector Definition identity")
        _assert_definition(definition, context)
        capture = capture_viewport(fresh, artifact_dir / "point-vector-reloaded.png")
        reload_close = close_clean_native_app(
            fresh, process, "reloaded Point vector app", client.timeout
        )
    return {
        "initial_close": initial_close,
        "reload_close": reload_close,
        "samples": reloaded_samples,
        "capture": capture,
    }


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
    opened_id, before = _open_definition(client, item_id, "Point vector fields")
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
    plain = _sample(
        client, start + 0.5, unrouted["history"]["revision"], "plain vector Grid"
    )
    _open_definition(client, item_id, "Point vector radial color")
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
    colored = _sample(
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
    undone_pixels = _sample(
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
    restored = _sample(
        client, start + 0.5, redone["history"]["revision"], "redone vector color"
    )
    if restored["pixel_hash"] != colored["pixel_hash"]:
        raise QaFailure("Redo did not restore exact radial vector Grid pixels")

    samples = [
        restored,
        _sample(client, start + 1.0, redone["history"]["revision"], "Point vector"),
    ]
    _open_definition(client, item_id, "Point vector capture")
    artifact_dir = pathlib.Path(os.environ["RUVIE_QA_ARTIFACT_DIR"])
    capture = capture_viewport(client, artifact_dir / "capture.png")
    project_path = os.environ.get("RUVIE_QA_PROJECT_PATH")
    if not project_path:
        raise QaFailure("Point vector persistence QA requires a Project file")
    project_file = pathlib.Path(project_path)
    saved, saved_file = save_project_to_disk(client, project_file, "Point vector")
    reloaded = _reload(client, project_file, saved, context, samples, artifact_dir)
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
