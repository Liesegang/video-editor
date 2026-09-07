#!/usr/bin/env python3
"""Exercise per-Point Size fields through the production Node Editor."""

import os
import pathlib

from qa_automation_support import history_shortcut
from qa_node_module_support import (
    active_definition,
    connect_nodes,
    create_node_from_menu,
    disconnect_node_connection,
    ensure_node_editor_authoring_scale,
    enter_exact_numeric,
    node_content_type,
    open_timeline_item_definition,
    place_created_node,
    reload_node_clip_project,
    sample_rendered_preview,
)
from qa_particle_force_support import (
    create_particle_node_clip,
    particle_factory_terminal_route,
)
from qa_support import (
    QaFailure,
    activate_dock_tab,
    bring_timeline_component,
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


def _definition(state, definition_id):
    return state["project"]["module_definitions"][definition_id]


def _routes(definition):
    return {
        (
            route["from"]["node_id"],
            route["from"]["port"],
            route["to"]["node_id"],
            route["to"]["port"],
        )
        for route in definition["graph"]["connections"]
    }


def _assert_definition(definition, context, scale):
    expected_catalogs = {
        context["info_id"]: "native.point.info",
        context["store_id"]: "native.point.store-number-attribute",
        context["set_size_id"]: "native.point.set-size",
    }
    nodes = definition["graph"]["nodes"]
    for node_id, catalog_id in expected_catalogs.items():
        if node_id not in nodes or _catalog_id(nodes[node_id]) != catalog_id:
            raise QaFailure("Point Size graph lost " + catalog_id)
    if _value_kind(nodes.get(context["multiply_id"], {})) != "multiply":
        raise QaFailure("Point Size graph lost its Value Multiply Node")
    set_size = nodes[context["set_size_id"]]
    if "size" in (set_size.get("properties") or {}):
        raise QaFailure("Set Point Size exposed its field-only Size as authored Property")
    if _constant(set_size, "scale") != scale:
        raise QaFailure("Set Point Size changed its exact Scale")
    if _constant(set_size, "selection") is not True:
        raise QaFailure("Set Point Size changed its default Selection")
    if _constant(nodes[context["store_id"]], "value") != 0.0:
        raise QaFailure("Store Number Attribute changed its disconnected default")
    actual_routes = _routes(definition)
    if actual_routes != context["routes"]:
        raise QaFailure(
            "Point Size routes differ: missing={!r}, extra={!r}".format(
                sorted(context["routes"] - actual_routes),
                sorted(actual_routes - context["routes"]),
            )
        )


def _edit_scale(client, definition_id, node_id, value, description):
    control_id = "node_editor.property.node:{}:scale".format(node_id)
    _, control = client.wait_component_settled(control_id)
    metadata = control.get("metadata") or {}
    if metadata.get("editor_kind") != "float" or metadata.get("connected") is not False:
        raise QaFailure("Set Point Size bypassed the shared Number editor")
    before = client.state()
    enter_exact_numeric(client, control_id, value)

    def edited():
        state = client.state()
        node = _definition(state, definition_id)["graph"]["nodes"][node_id]
        return (
            state
            if state["history"]["revision"] == before["history"]["revision"] + 1
            and _constant(node, "scale") == value
            else None
        )

    return before, client.wait_until(description + " exact edit", edited)


def run_suite(client):
    client.wait_health()
    created = create_particle_node_clip(client, 6.5)
    item_id = created["item_id"]
    definition_id = created["definition_id"]
    start = media_seconds(created["item"]["interval"]["start"])
    factory_pixels = sample_rendered_preview(
        client,
        start + 0.5,
        created["state"]["history"]["revision"],
        "factory Particle Size",
    )

    opened_id, initial = open_timeline_item_definition(
        client, item_id, "node_clip", "Point Size fields"
    )
    if opened_id != definition_id:
        raise QaFailure("Node Editor opened a different Particle Definition")
    source_id, sprite_id, direct_route = particle_factory_terminal_route(initial)
    output_ids = [
        node_id
        for node_id, node in initial["graph"]["nodes"].items()
        if node_content_type(node) == "moduleoutput"
    ]
    if len(output_ids) != 1:
        raise QaFailure("Particle factory has no unique Module Output")
    output_id = output_ids[0]
    disconnected = disconnect_node_connection(
        client, "node_clip", direct_route["id"], "Collision-to-Sprite route"
    )
    no_particle = sample_rendered_preview(
        client,
        start + 0.5,
        disconnected["history"]["revision"],
        "disconnected Particle Size output",
        require_visible=False,
    )
    if no_particle["pixel_hash"] == factory_pixels["pixel_hash"]:
        raise QaFailure("Disconnecting Particle output did not remove rendered sprites")

    open_timeline_item_definition(client, item_id, "node_clip", "Point Size authoring")
    ensure_node_editor_authoring_scale(client)
    place_created_node(client, output_id, 0.95, vertical_offset=140.0)
    place_created_node(client, source_id, 0.05)
    place_created_node(client, sprite_id, 0.90)

    specs = [
        ("info", "Point Info", "node_editor.menu.create.point_info", 0.22, 26.0),
        (
            "multiply",
            "Multiply",
            "node_editor.menu.create.value:multiply",
            0.40,
            140.0,
        ),
        (
            "store",
            "Store Number Attribute",
            "node_editor.menu.create.point_store_number_attribute",
            0.58,
            26.0,
        ),
        (
            "set_size",
            "Set Point Size",
            "node_editor.menu.create.point_set_size",
            0.76,
            140.0,
        ),
    ]
    nodes = {}
    for name, query, choice_id, horizontal, vertical in specs:
        _, node_id = create_node_from_menu(client, "node_clip", query, choice_id)
        place_created_node(client, node_id, horizontal, vertical_offset=vertical)
        nodes[name] = node_id

    route_specs = [
        (source_id, "particles", nodes["info"], "points", "Collision to Point Info"),
        (source_id, "particles", nodes["store"], "points", "Collision to Size Store"),
        (nodes["info"], "size", nodes["multiply"], "a", "Size to Multiply"),
        (
            nodes["info"],
            "normalized_age",
            nodes["multiply"],
            "b",
            "Age to Multiply",
        ),
        (
            nodes["multiply"],
            "result",
            nodes["store"],
            "value",
            "Size field to Store",
        ),
        (
            nodes["store"],
            "points",
            nodes["set_size"],
            "points",
            "Stored points to Set Size",
        ),
        (
            nodes["store"],
            "attribute",
            nodes["set_size"],
            "size",
            "Stored value to Point Size",
        ),
        (
            nodes["set_size"],
            "points",
            sprite_id,
            "particles",
            "Sized points to Sprite",
        ),
    ]
    connections = [connect_nodes(client, "node_clip", *route) for route in route_specs]
    routed = client.state()
    expected_routes = _routes(initial)
    expected_routes.remove((source_id, "particles", sprite_id, "particles"))
    expected_routes.update(route[:4] for route in route_specs)
    context = {
        "info_id": nodes["info"],
        "multiply_id": nodes["multiply"],
        "store_id": nodes["store"],
        "set_size_id": nodes["set_size"],
        "routes": expected_routes,
    }
    _assert_definition(_definition(routed, definition_id), context, 1.0)
    sized = sample_rendered_preview(
        client,
        start + 0.5,
        routed["history"]["revision"],
        "age-scaled Particle Size",
    )
    if sized["pixel_hash"] in {factory_pixels["pixel_hash"], no_particle["pixel_hash"]}:
        raise QaFailure("Point Size field did not produce distinct visible Particle pixels")

    open_timeline_item_definition(client, item_id, "node_clip", "zero Point Scale")
    scale_before, zero_state = _edit_scale(
        client, definition_id, nodes["set_size"], 0.0, "zero Point Scale"
    )
    _assert_definition(_definition(zero_state, definition_id), context, 0.0)
    zero_pixels = sample_rendered_preview(
        client,
        start + 0.5,
        zero_state["history"]["revision"],
        "zero Point Scale",
        require_visible=False,
    )
    if zero_pixels["pixel_hash"] != no_particle["pixel_hash"]:
        raise QaFailure("Scale zero did not restore the exact no-Particle output")

    history_shortcut(client)
    undone = client.wait_until(
        "Undo exact Point Scale",
        lambda: state
        if (state := client.state())["history"]["revision"]
        == zero_state["history"]["revision"] + 1
        and state["project"] == scale_before["project"]
        else None,
    )
    undone_pixels = sample_rendered_preview(
        client, start + 0.5, undone["history"]["revision"], "undone Point Scale"
    )
    if undone_pixels["pixel_hash"] != sized["pixel_hash"]:
        raise QaFailure("Undo did not restore exact age-scaled Particle pixels")

    history_shortcut(client, redo=True)
    redone = client.wait_until(
        "Redo exact zero Point Scale",
        lambda: state
        if (state := client.state())["history"]["revision"]
        == undone["history"]["revision"] + 1
        and state["project"] == zero_state["project"]
        else None,
    )
    redone_pixels = sample_rendered_preview(
        client,
        start + 0.5,
        redone["history"]["revision"],
        "redone zero Point Scale",
        require_visible=False,
    )
    if redone_pixels["pixel_hash"] != no_particle["pixel_hash"]:
        raise QaFailure("Redo did not restore exact zero-size Particle output")

    open_timeline_item_definition(client, item_id, "node_clip", "saved Point Scale")
    _, final_state = _edit_scale(
        client, definition_id, nodes["set_size"], 0.5, "saved Point Scale"
    )
    _assert_definition(_definition(final_state, definition_id), context, 0.5)
    samples = [
        sample_rendered_preview(
            client,
            start + 0.5,
            final_state["history"]["revision"],
            "saved Point Size",
        ),
        sample_rendered_preview(
            client,
            start + 1.0,
            final_state["history"]["revision"],
            "saved Point Size",
        ),
    ]
    if samples[0]["pixel_hash"] == samples[1]["pixel_hash"]:
        raise QaFailure("Particle Size-over-life did not vary across sampled times")

    open_timeline_item_definition(client, item_id, "node_clip", "Point Size capture")
    artifact_dir = pathlib.Path(os.environ["RUVIE_QA_ARTIFACT_DIR"])
    capture = capture_viewport(client, artifact_dir / "capture.png")
    project_path = os.environ.get("RUVIE_QA_PROJECT_PATH")
    if not project_path:
        raise QaFailure("Point Size persistence QA requires a Project file")
    project_file = pathlib.Path(project_path)
    saved, saved_file = save_project_to_disk(client, project_file, "Point Size")
    reloaded = reload_node_clip_project(
        client,
        project_file,
        saved,
        item_id,
        definition_id,
        samples,
        artifact_dir,
        "Point Size",
        "point-size",
        "point-size-reloaded.png",
        lambda definition: _assert_definition(definition, context, 0.5),
    )
    return {
        "suite": "point-size",
        "item_id": item_id,
        "definition_id": definition_id,
        "nodes": nodes,
        "connections": connections,
        "factory": factory_pixels,
        "no_particle": no_particle,
        "sized": sized,
        "zero": zero_pixels,
        "samples": samples,
        "capture": capture,
        "saved": saved_file,
        "reloaded": reloaded,
        "actions": client.evidence,
    }


if __name__ == "__main__":
    raise SystemExit(
        run_suite_main(
            "qa-point-size-e2e",
            run_suite,
            "target/qa-point-size-e2e-evidence.json",
        )
    )
