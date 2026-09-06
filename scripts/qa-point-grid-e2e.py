#!/usr/bin/env python3
"""Exercise the production Point Grid through the real Node Editor and renderer."""

import os
import pathlib

from qa_automation_support import history_shortcut
from qa_node_module_support import (
    active_definition,
    connect_nodes,
    create_node_from_menu,
    disconnect_node_connection,
    node_content_type,
    place_created_node,
    unpublish_node_input_parameter,
)
from qa_particle_force_support import create_particle_node_clip
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
    return ((node.get("content") or {}).get("data") or {}).get("catalog_id")


def _unique_catalog_node(definition, catalog_id):
    matches = [
        node_id
        for node_id, node in definition["graph"]["nodes"].items()
        if _catalog_id(node) == catalog_id
    ]
    if len(matches) != 1:
        raise QaFailure("Point Grid QA expected one " + catalog_id)
    return matches[0]


def _constant(node, key):
    property_value = ((node.get("properties") or {}).get(key) or {}).get("properties")
    if not isinstance(property_value, dict) or "value" not in property_value:
        raise QaFailure("Point Grid omitted constant Property " + key)
    return property_value["value"]


def _enter_exact_numeric(client, component_id, value):
    client.click_component(component_id)
    client.key("a", True, command=True)
    client.key("a", False, command=True)
    client.inject("text", {"text": str(value)})
    client.key("enter", True)
    client.key("enter", False)


def _edit_typed_store_values(client, definition_id, integer_id, vec3_id, color_id):
    color_control = "node_editor.property.node:{}:value".format(color_id)
    _, color = client.wait_component_settled(color_control)
    if (color.get("metadata") or {}).get("editor_kind") != "managed_color":
        raise QaFailure("Store Color bypassed the shared managed Color editor")

    integer_control = "node_editor.property.node:{}:value".format(integer_id)
    _, integer = client.wait_component_settled(integer_control)
    integer_metadata = integer.get("metadata") or {}
    if integer_metadata.get("editor_kind") != "integer":
        raise QaFailure("Store Integer bypassed the shared typed Integer editor")
    before_integer = client.state()
    _enter_exact_numeric(client, integer_control, 16_777_217)

    def integer_edited():
        state = client.state()
        node = state["project"]["module_definitions"][definition_id]["graph"][
            "nodes"
        ][integer_id]
        return (
            state
            if state["history"]["revision"]
            == before_integer["history"]["revision"] + 1
            and _constant(node, "value") == 16_777_217
            else None
        )

    client.wait_until("Store Integer exact Value edit", integer_edited)

    vec3_control = "node_editor.property.node:{}:value".format(vec3_id)
    vec3_x_control = vec3_control + ":x"
    _, vec3 = client.wait_component_settled(vec3_x_control)
    vec3_metadata = vec3.get("metadata") or {}
    if vec3_metadata.get("axis") != "X" or vec3_metadata.get("value") != 0.0:
        raise QaFailure("Store Vec3 bypassed the shared typed vector editor")
    before_vec3 = client.state()
    _enter_exact_numeric(client, vec3_x_control, 12.5)

    def vec3_edited():
        state = client.state()
        node = state["project"]["module_definitions"][definition_id]["graph"][
            "nodes"
        ][vec3_id]
        return (
            state
            if state["history"]["revision"] == before_vec3["history"]["revision"] + 1
            and _constant(node, "value") == {"x": 12.5, "y": 0.0, "z": 0.0}
            else None
        )

    return client.wait_until("Store Vec3 exact X edit", vec3_edited)


def _assert_typed_stores(definition, context):
    expected = {
        context["store_integer_id"]: (
            "native.point.store-integer-attribute",
            16_777_217,
        ),
        context["store_vec3_id"]: (
            "native.point.store-vec3-attribute",
            {"x": 12.5, "y": 0.0, "z": 0.0},
        ),
        context["store_color_id"]: (
            "native.point.store-color-attribute",
            None,
        ),
    }
    for node_id, (catalog_id, value) in expected.items():
        node = definition["graph"]["nodes"].get(node_id)
        if node is None or _catalog_id(node) != catalog_id:
            raise QaFailure("Point Grid lost typed Store " + catalog_id)
        if value is not None and _constant(node, "value") != value:
            raise QaFailure("Point Grid changed typed Store Value " + catalog_id)


def _assert_grid_defaults(definition, grid_id):
    grid = definition["graph"]["nodes"].get(grid_id)
    if grid is None or _catalog_id(grid) != "native.point.grid":
        raise QaFailure("Point Grid menu created the wrong Node")
    expected = {
        "count_x": 8,
        "count_y": 8,
        "count_z": 1,
        "spacing": {"x": 24.0, "y": 24.0, "z": 24.0},
        "center": {"x": 0.0, "y": 0.0, "z": 0.0},
        "size": 8.0,
        "seed": 1,
    }
    actual = {key: _constant(grid, key) for key in expected}
    if actual != expected:
        raise QaFailure("Point Grid defaults drifted: {!r}".format(actual))


def _find_factory_route(definition):
    drag_id = _unique_catalog_node(definition, "native.particle.drag-force")
    sprite_id = _unique_catalog_node(definition, "native.particle.sprite-renderer")
    route = next(
        (
            candidate
            for candidate in definition["graph"]["connections"]
            if candidate["from"] == {"node_id": drag_id, "port": "particles"}
            and candidate["to"] == {"node_id": sprite_id, "port": "particles"}
        ),
        None,
    )
    if route is None:
        raise QaFailure("Particle factory has no exact Drag-to-Sprite route")
    return drag_id, sprite_id, route


def _published_parameter(definition, node_id, port_key):
    matches = [
        parameter
        for parameter in definition["interface"]["parameters"]
        if parameter["target"] == {"node_id": node_id, "port": port_key}
    ]
    if len(matches) != 1:
        raise QaFailure("Particle factory has no unique published Sprite Color")
    return matches[0]


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
    missing = expected - actual
    if missing:
        raise QaFailure("Point Grid graph omitted routes {!r}".format(sorted(missing)))


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
    initial_close = request_clean_native_close(client, "Point Grid authoring", client.timeout)
    port_number = free_port()
    environment = {
        "RUVIE_QA_PROJECT_PATH": str(project_file),
        "RUVIE_QA_OPEN_EXISTING_PROJECT": "1",
        "RUVIE_QA_PORT_FILE": None,
        "RUVIE_QA_RUN_ID": os.environ.get("RUVIE_QA_RUN_ID", "point-grid") + ":reload",
        QA_APP_BINARY_ENV: os.environ.get(QA_APP_BINARY_ENV),
    }
    with spawned_authoring_app(port_number, environment) as process:
        fresh = QaClient("http://127.0.0.1:{}".format(port_number), client.timeout)
        fresh.wait_health()
        state = fresh.state()
        if state["project"] != saved["project"]:
            raise QaFailure("fresh process changed Point Grid Project state")
        revision = state["history"]["revision"]
        reloaded_samples = [
            _sample(fresh, sample["seconds"], revision, "reloaded Point Grid")
            for sample in samples
        ]
        if [sample["pixel_hash"] for sample in reloaded_samples] != [
            sample["pixel_hash"] for sample in samples
        ]:
            raise QaFailure("fresh process changed Point Grid pixels")
        definition_id, definition = _open_definition(
            fresh, context["item_id"], "Point Grid reload"
        )
        if definition_id != context["definition_id"]:
            raise QaFailure("fresh process changed Point Grid Definition identity")
        _assert_grid_defaults(definition, context["grid_id"])
        _assert_typed_stores(definition, context)
        _assert_routes(definition, context["routes"])
        capture = capture_viewport(fresh, artifact_dir / "point-grid-reloaded.png")
        reload_close = close_clean_native_app(
            fresh, process, "reloaded Point Grid app", client.timeout
        )
    return {
        "initial_close": initial_close,
        "reload_close": reload_close,
        "samples": reloaded_samples,
        "capture": capture,
    }


def run_suite(client):
    client.wait_health()
    created = create_particle_node_clip(client, 6.5)
    item_id = created["item_id"]
    definition_id = created["definition_id"]
    instance_id = created["instance_id"]
    start = media_seconds(created["item"]["interval"]["start"])
    particle_pixels = _sample(
        client,
        start + 0.5,
        created["state"]["history"]["revision"],
        "factory Particle",
    )

    opened_id, before = _open_definition(client, item_id, "Point Grid")
    if opened_id != definition_id:
        raise QaFailure("Node Editor opened a different Particle Definition")
    drag_id, sprite_id, direct_route = _find_factory_route(before)
    color_parameter = _published_parameter(before, sprite_id, "color")
    output_ids = [
        node_id
        for node_id, node in before["graph"]["nodes"].items()
        if node_content_type(node) == "moduleoutput"
    ]
    if len(output_ids) != 1:
        raise QaFailure("Particle factory has no unique Module Output")
    output_id = output_ids[0]

    place_created_node(client, drag_id, 0.58)
    disconnect_node_connection(
        client, "node_clip", direct_route["id"], "direct Drag-to-Sprite route"
    )
    _, canvas_before = client.wait_component_settled("node_editor.canvas")
    old_scale = float(canvas_before["metadata"]["scale"])
    client.scroll_component(
        "node_editor.canvas", 0.0, -100.0, modifiers={"command": True}
    )
    _, canvas = client.wait_component_settled("node_editor.canvas")
    scale = float(canvas["metadata"]["scale"])
    if scale >= old_scale or not 0.4 <= scale <= 0.75:
        raise QaFailure("Node Editor did not reach the measured Point authoring overview")
    place_created_node(client, output_id, 0.95, vertical_offset=140.0)
    place_created_node(client, drag_id, 0.06, vertical_offset=140.0)
    place_created_node(client, sprite_id, 0.95)

    _, grid_id = create_node_from_menu(
        client, "node_clip", "Point Grid", "node_editor.menu.create.point_grid"
    )
    place_created_node(client, grid_id, 0.06)
    _, info_id = create_node_from_menu(
        client, "node_clip", "Point Info", "node_editor.menu.create.point_info"
    )
    place_created_node(client, info_id, 0.17)
    _, store_vec3_id = create_node_from_menu(
        client,
        "node_clip",
        "Store Vec3 Attribute",
        "node_editor.menu.create.point_store_vec3_attribute",
    )
    place_created_node(client, store_vec3_id, 0.30)
    _, store_id = create_node_from_menu(
        client,
        "node_clip",
        "Store Number Attribute",
        "node_editor.menu.create.point_store_number_attribute",
    )
    place_created_node(client, store_id, 0.40)
    _, gradient_id = create_node_from_menu(
        client, "node_clip", "Gradient", "node_editor.menu.create.data:gradient"
    )
    place_created_node(client, gradient_id, 0.53)
    _, store_integer_id = create_node_from_menu(
        client,
        "node_clip",
        "Store Integer Attribute",
        "node_editor.menu.create.point_store_integer_attribute",
    )
    place_created_node(client, store_integer_id, 0.74, vertical_offset=140.0)
    _, ramp_id = create_node_from_menu(
        client, "node_clip", "Color Ramp", "node_editor.menu.create.color:ramp"
    )
    place_created_node(client, ramp_id, 0.65)
    _, store_color_id = create_node_from_menu(
        client,
        "node_clip",
        "Store Color Attribute",
        "node_editor.menu.create.point_store_color_attribute",
    )
    place_created_node(client, store_color_id, 0.82)
    _edit_typed_store_values(
        client,
        definition_id,
        store_integer_id,
        store_vec3_id,
        store_color_id,
    )

    authored_definition = active_definition(client.state(), "node_clip")[1]
    _assert_grid_defaults(authored_definition, grid_id)
    _assert_typed_stores(
        authored_definition,
        {
            "store_integer_id": store_integer_id,
            "store_vec3_id": store_vec3_id,
            "store_color_id": store_color_id,
        },
    )
    route_specs = [
        (grid_id, "points", info_id, "points", "Grid to Point Info"),
        (grid_id, "points", store_vec3_id, "points", "Grid to Vec3 Store"),
        (info_id, "position", store_vec3_id, "value", "Position to stored Vec3"),
        (store_vec3_id, "points", store_id, "points", "Vec3 Store to Number Store"),
        (info_id, "random", store_id, "value", "Random to stored field"),
        (store_id, "points", store_integer_id, "points", "Number Store to Integer Store"),
        (
            store_integer_id,
            "points",
            store_color_id,
            "points",
            "Integer Store to Color Store",
        ),
        (gradient_id, "value", ramp_id, "gradient", "Gradient to Color Ramp"),
        (store_id, "attribute", ramp_id, "factor", "field to Color Ramp Factor"),
        (ramp_id, "color", store_color_id, "value", "Ramp to stored Color"),
        (store_color_id, "points", sprite_id, "particles", "Stored Grid to Sprite"),
        (store_color_id, "attribute", sprite_id, "color", "Color field to Sprite"),
    ]
    connections = [
        connect_nodes(client, "node_clip", *route) for route in route_specs[:-1]
    ]
    unpublish_node_input_parameter(
        client,
        definition_id,
        instance_id,
        sprite_id,
        "color",
        color_parameter,
        before,
    )
    uncolored = client.state()
    plain_grid = _sample(
        client, start + 0.5, uncolored["history"]["revision"], "plain Point Grid"
    )
    opened_id, _ = _open_definition(client, item_id, "Point Grid color field")
    if opened_id != definition_id:
        raise QaFailure("Point Grid color routing reopened a different Definition")
    connections.append(connect_nodes(client, "node_clip", *route_specs[-1]))
    routed = client.state()
    definition = routed["project"]["module_definitions"][definition_id]
    expected_routes = {route[:4] for route in route_specs}
    _assert_routes(definition, expected_routes)
    if len(definition["graph"]["connections"]) != len(before["graph"]["connections"]) + 11:
        raise QaFailure("Point Grid routing changed an unexpected connection")

    colored = _sample(
        client, start + 0.5, routed["history"]["revision"], "colored Point Grid"
    )
    if plain_grid["pixel_hash"] == colored["pixel_hash"]:
        raise QaFailure("Random field Color Ramp did not change Point Grid pixels")
    if particle_pixels["pixel_hash"] == colored["pixel_hash"]:
        raise QaFailure("Point Grid rendered the unchanged Particle factory pixels")

    color_connection = connections[-1]
    history_shortcut(client)
    undone = client.wait_until(
        "Undo Point Grid color route",
        lambda: state
        if (
            (state := client.state())["history"]["revision"]
            == routed["history"]["revision"] + 1
            and all(
                route["id"] != color_connection["id"]
                for route in state["project"]["module_definitions"][definition_id]["graph"][
                    "connections"
                ]
            )
        )
        else None,
    )
    undone_pixels = _sample(
        client, start + 0.5, undone["history"]["revision"], "undone Grid color"
    )
    if undone_pixels["pixel_hash"] != plain_grid["pixel_hash"]:
        raise QaFailure("Undo did not restore exact uncolored Point Grid pixels")
    history_shortcut(client, redo=True)
    redone = client.wait_until(
        "Redo Point Grid color route",
        lambda: state
        if (
            (state := client.state())["history"]["revision"]
            == undone["history"]["revision"] + 1
            and any(
                route == color_connection
                for route in state["project"]["module_definitions"][definition_id]["graph"][
                    "connections"
                ]
            )
        )
        else None,
    )
    restored = _sample(
        client, start + 0.5, redone["history"]["revision"], "redone Grid color"
    )
    if restored["pixel_hash"] != colored["pixel_hash"]:
        raise QaFailure("Redo did not restore exact colored Point Grid pixels")

    samples = [
        restored,
        _sample(client, start + 1.0, redone["history"]["revision"], "Point Grid"),
    ]
    _open_definition(client, item_id, "Point Grid capture")
    artifact_dir = pathlib.Path(os.environ["RUVIE_QA_ARTIFACT_DIR"])
    capture = capture_viewport(client, artifact_dir / "capture.png")
    project_path = os.environ.get("RUVIE_QA_PROJECT_PATH")
    if not project_path:
        raise QaFailure("Point Grid persistence QA requires a Project file")
    project_file = pathlib.Path(project_path)
    saved, saved_file = save_project_to_disk(client, project_file, "Point Grid")
    reloaded = _reload(
        client,
        project_file,
        saved,
        {
            "item_id": item_id,
            "definition_id": definition_id,
            "grid_id": grid_id,
            "store_integer_id": store_integer_id,
            "store_vec3_id": store_vec3_id,
            "store_color_id": store_color_id,
            "routes": expected_routes,
        },
        samples,
        artifact_dir,
    )
    return {
        "suite": "point-grid",
        "item_id": item_id,
        "definition_id": definition_id,
        "nodes": {
            "grid": grid_id,
            "point_info": info_id,
            "store_integer_attribute": store_integer_id,
            "store_vec3_attribute": store_vec3_id,
            "store_number_attribute": store_id,
            "store_color_attribute": store_color_id,
            "gradient": gradient_id,
            "color_ramp": ramp_id,
            "sprite": sprite_id,
        },
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
            "qa-point-grid-e2e",
            run_suite,
            "target/qa-point-grid-e2e-evidence.json",
        )
    )
