#!/usr/bin/env python3
"""Exercise the production Point field graph on a real Particle Node Clip."""

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


def _native_catalog_id(node):
    content = node.get("content") or {}
    if node_content_type(node) != "nativeoperation":
        return None
    return (content.get("data") or {}).get("catalog_id")


def _rendered(client, revision, frame, description):
    return client.wait_until(
        description + " rendered Preview",
        lambda: state
        if (
            (state := settled_preview_state(client, revision, frame))
            and state["editor"].get("error") is None
            and state["editor"]["preview"].get("pixel_hash")
            and int(state["editor"]["preview"].get("nontransparent_pixels") or 0) > 0
        )
        else None,
        30.0,
    )


def _sample(client, seconds, revision, description):
    activate_dock_tab(client, "dock.tab:timeline", "Timeline", description)
    sought = seek_timeline_seconds(client, seconds)
    frame = sought["editor"]["timeline"]["current_frame"]
    state = _rendered(client, revision, frame, description)
    preview = state["editor"]["preview"]
    return {
        "seconds": seconds,
        "frame": frame,
        "pixel_hash": preview["pixel_hash"],
        "nontransparent_pixels": preview["nontransparent_pixels"],
    }


def _catalog_nodes(definition):
    result = {}
    for node_id, node in definition["graph"]["nodes"].items():
        if catalog_id := _native_catalog_id(node):
            result.setdefault(catalog_id, []).append(node_id)
    return result


def _find_particle_route(definition):
    catalog = _catalog_nodes(definition)
    drag = catalog.get("native.particle.drag-force") or []
    sprite = catalog.get("native.particle.sprite-renderer") or []
    if len(drag) != 1 or len(sprite) != 1:
        raise QaFailure("Particle factory has no unique Drag and Sprite Nodes")
    route = next(
        (
            candidate
            for candidate in definition["graph"]["connections"]
            if candidate["from"] == {"node_id": drag[0], "port": "particles"}
            and candidate["to"] == {"node_id": sprite[0], "port": "particles"}
        ),
        None,
    )
    if route is None:
        raise QaFailure("Particle factory has no direct Drag-to-Sprite route")
    return drag[0], sprite[0], route


def _published_parameter(definition, node_id, port_key):
    matches = [
        parameter
        for parameter in definition["interface"]["parameters"]
        if parameter["target"] == {"node_id": node_id, "port": port_key}
    ]
    if len(matches) != 1:
        raise QaFailure("Particle factory has no unique published Sprite Color")
    return matches[0]


def _assert_point_nodes(definition, expected):
    for node_id, catalog_id in expected.items():
        node = definition["graph"]["nodes"].get(node_id)
        if node is None or _native_catalog_id(node) != catalog_id:
            raise QaFailure("Point menu created the wrong " + catalog_id + " Node")
    store = definition["graph"]["nodes"][next(
        node_id
        for node_id, catalog_id in expected.items()
        if catalog_id == "native.point.store-number-attribute"
    )]
    value = ((store.get("properties") or {}).get("value") or {}).get("properties") or {}
    if value.get("value") != 0.0:
        raise QaFailure("Store Number Attribute omitted its numeric zero default")


def _assert_exact_routes(definition, expected_routes):
    routes = {
        (
            candidate["from"]["node_id"],
            candidate["from"]["port"],
            candidate["to"]["node_id"],
            candidate["to"]["port"],
        )
        for candidate in definition["graph"]["connections"]
    }
    missing = expected_routes - routes
    if missing:
        raise QaFailure("Point graph omitted exact routes {!r}".format(sorted(missing)))


def _reload_and_compare(client, project_file, saved, samples, context, artifact_dir):
    initial_close = request_clean_native_close(client, "Point attribute authoring", client.timeout)
    port_number = free_port()
    environment = {
        "RUVIE_QA_PROJECT_PATH": str(project_file),
        "RUVIE_QA_OPEN_EXISTING_PROJECT": "1",
        "RUVIE_QA_PORT_FILE": None,
        "RUVIE_QA_RUN_ID": os.environ.get("RUVIE_QA_RUN_ID", "point-attributes")
        + ":reload",
        QA_APP_BINARY_ENV: os.environ.get(QA_APP_BINARY_ENV),
    }
    with spawned_authoring_app(port_number, environment) as process:
        fresh = QaClient("http://127.0.0.1:{}".format(port_number), client.timeout)
        fresh.wait_health()
        reloaded = fresh.state()
        if reloaded["project"] != saved["project"]:
            raise QaFailure("fresh process changed Point graph IDs, names, or routes")
        revision = reloaded["history"]["revision"]
        reloaded_samples = [
            _sample(fresh, sample["seconds"], revision, "reloaded Point field")
            for sample in samples
        ]
        if [sample["pixel_hash"] for sample in reloaded_samples] != [
            sample["pixel_hash"] for sample in samples
        ]:
            raise QaFailure("fresh process changed Point field multi-time pixels")
        item_id = context["item_id"]
        bring_timeline_component(fresh, "timeline.item:" + item_id, -120.0)
        fresh.double_click_component("timeline.item:" + item_id)
        activate_dock_tab(fresh, "dock.tab:node_editor", "Node Editor", "Point reload")
        definition_id, definition = active_definition(fresh.state(), "node_clip")
        if definition_id != context["definition_id"]:
            raise QaFailure("fresh process changed the Point Module Definition ID")
        _assert_point_nodes(definition, context["point_nodes"])
        _assert_exact_routes(definition, context["routes"])
        capture = capture_viewport(fresh, artifact_dir / "point-attributes-reloaded.png")
        reload_close = close_clean_native_app(
            fresh, process, "reloaded Point attribute app", client.timeout
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
    start = media_seconds(created["item"]["interval"]["start"])
    baseline = _sample(
        client,
        start + 0.5,
        created["state"]["history"]["revision"],
        "unmodified Particle",
    )

    bring_timeline_component(client, "timeline.item:" + item_id, -120.0)
    client.double_click_component("timeline.item:" + item_id)
    activate_dock_tab(client, "dock.tab:node_editor", "Node Editor", "Point attributes")
    client.wait_component_settled("node_editor.canvas")
    opened_id, before = active_definition(client.state(), "node_clip")
    if opened_id != definition_id:
        raise QaFailure("Node Editor opened a different Particle Definition")
    drag_id, sprite_id, direct_route = _find_particle_route(before)
    sprite_color_parameter = _published_parameter(before, sprite_id, "color")
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

    _, canvas_before_zoom = client.wait_component_settled("node_editor.canvas")
    scale_before_zoom = float(canvas_before_zoom["metadata"]["scale"])
    client.scroll_component(
        "node_editor.canvas", 0.0, -100.0, modifiers={"command": True}
    )
    _, overview_canvas = client.wait_component_settled("node_editor.canvas")
    overview_scale = float(overview_canvas["metadata"]["scale"])
    if overview_scale >= scale_before_zoom or not 0.4 <= overview_scale <= 0.75:
        raise QaFailure(
            "Node Editor did not reach a usable authoring overview: {:.4f}".format(
                overview_scale
            )
        )
    place_created_node(client, output_id, 0.95, vertical_offset=140.0)
    place_created_node(client, drag_id, 0.06)
    place_created_node(client, sprite_id, 0.88)

    _, info_id = create_node_from_menu(
        client, "node_clip", "Point Info", "node_editor.menu.create.point_info"
    )
    place_created_node(client, info_id, 0.22)
    _, store_id = create_node_from_menu(
        client,
        "node_clip",
        "Store Number Attribute",
        "node_editor.menu.create.point_store_number_attribute",
    )
    place_created_node(client, store_id, 0.38)
    _, gradient_id = create_node_from_menu(
        client, "node_clip", "Gradient", "node_editor.menu.create.data:gradient"
    )
    place_created_node(client, gradient_id, 0.54)
    _, ramp_id = create_node_from_menu(
        client, "node_clip", "Color Ramp", "node_editor.menu.create.color:ramp"
    )
    place_created_node(client, ramp_id, 0.70)

    point_nodes = {
        info_id: "native.point.info",
        store_id: "native.point.store-number-attribute",
    }
    _assert_point_nodes(active_definition(client.state(), "node_clip")[1], point_nodes)
    route_specs = [
        (drag_id, "particles", info_id, "points", "Drag to Point Info"),
        (drag_id, "particles", store_id, "points", "Drag to Store Attribute"),
        (info_id, "normalized_age", store_id, "value", "Age to stored heat"),
        (store_id, "points", sprite_id, "particles", "Stored Points to Sprite"),
        (gradient_id, "value", ramp_id, "gradient", "Gradient to Color Ramp"),
        (store_id, "attribute", ramp_id, "factor", "heat to Color Ramp Factor"),
        (ramp_id, "color", sprite_id, "color", "Color Ramp to Sprite Color"),
    ]
    connections = [
        connect_nodes(client, "node_clip", *route) for route in route_specs[:-1]
    ]
    unpublish_node_input_parameter(
        client,
        definition_id,
        created["instance_id"],
        sprite_id,
        "color",
        sprite_color_parameter,
        before,
    )
    connections.append(connect_nodes(client, "node_clip", *route_specs[-1]))
    expected_routes = {route[:4] for route in route_specs}
    routed = client.state()
    definition = routed["project"]["module_definitions"][definition_id]
    _assert_exact_routes(definition, expected_routes)
    if len(definition["graph"]["connections"]) != len(before["graph"]["connections"]) + 6:
        raise QaFailure("Point routing added or removed an unexpected connection")

    routed_baseline = _sample(
        client, start + 0.5, routed["history"]["revision"], "Point field"
    )
    if routed_baseline["pixel_hash"] == baseline["pixel_hash"]:
        raise QaFailure("Point attribute graph did not change same-frame Particle pixels")

    color_connection = connections[-1]
    history_shortcut(client)
    undone = client.wait_until(
        "Undo Color Ramp-to-Sprite route",
        lambda: state
        if (
            (state := client.state())["history"]["revision"]
            == routed["history"]["revision"] + 1
            and all(
                candidate["id"] != color_connection["id"]
                for candidate in state["project"]["module_definitions"][definition_id]["graph"][
                    "connections"
                ]
            )
        )
        else None,
    )
    _sample(client, start + 0.5, undone["history"]["revision"], "undone Point color")
    history_shortcut(client, redo=True)
    routed = client.wait_until(
        "Redo Color Ramp-to-Sprite route",
        lambda: state
        if (
            (state := client.state())["history"]["revision"]
            == undone["history"]["revision"] + 1
            and any(
                candidate == color_connection
                for candidate in state["project"]["module_definitions"][definition_id]["graph"][
                    "connections"
                ]
            )
        )
        else None,
    )
    restored = _sample(
        client, start + 0.5, routed["history"]["revision"], "redone Point color"
    )
    if restored["pixel_hash"] != routed_baseline["pixel_hash"]:
        raise QaFailure("Redo did not restore exact same-frame Point field pixels")

    samples = [
        restored,
        _sample(client, start + 1.0, routed["history"]["revision"], "Point field"),
    ]
    bring_timeline_component(client, "timeline.item:" + item_id, -120.0)
    client.double_click_component("timeline.item:" + item_id)
    activate_dock_tab(client, "dock.tab:node_editor", "Node Editor", "Point attributes")
    client.wait_component_settled("node_editor.canvas")
    capture = capture_viewport(
        client, pathlib.Path(os.environ["RUVIE_QA_ARTIFACT_DIR"]) / "capture.png"
    )

    project_file_value = os.environ.get("RUVIE_QA_PROJECT_PATH")
    if not project_file_value:
        raise QaFailure("Point attribute persistence QA requires a Project file")
    project_file = pathlib.Path(project_file_value)
    saved, saved_file = save_project_to_disk(client, project_file, "Point attributes")
    context = {
        "item_id": item_id,
        "definition_id": definition_id,
        "point_nodes": point_nodes,
        "routes": expected_routes,
    }
    reloaded = _reload_and_compare(
        client,
        project_file,
        saved,
        samples,
        context,
        pathlib.Path(os.environ["RUVIE_QA_ARTIFACT_DIR"]),
    )
    return {
        "suite": "point-attributes",
        "item_id": item_id,
        "definition_id": definition_id,
        "nodes": {
            "point_info": info_id,
            "store_number_attribute": store_id,
            "gradient": gradient_id,
            "color_ramp": ramp_id,
            "sprite": sprite_id,
        },
        "connections": connections,
        "samples": samples,
        "saved": saved_file,
        "reloaded": reloaded,
        "capture": capture,
        "rename": {
            "exercised": False,
            "reason": "Node name editor has no stable QA component; model/compiler tests own stable ID across rename",
        },
        "actions": client.evidence,
    }


if __name__ == "__main__":
    raise SystemExit(
        run_suite_main(
            "qa-point-attributes-e2e",
            run_suite,
            "target/qa-point-attributes-e2e-evidence.json",
        )
    )
