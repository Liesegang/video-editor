#!/usr/bin/env python3
"""Create, place, customize, animate, and reload a reusable Plexus Network."""

import os
import pathlib

from qa_automation_support import history_shortcut
from qa_node_module_support import (
    active_definition,
    connect_nodes,
    create_node_from_menu,
    ensure_node_editor_authoring_scale,
    enter_exact_numeric,
    node_content_type,
    open_timeline_item_definition,
    reload_node_clip_project,
    sample_rendered_preview,
    unpublish_node_input_parameter,
)
from qa_particle_force_support import constant_property, native_catalog_id
from qa_support import (
    QaFailure,
    activate_dock_tab,
    capture_viewport,
    component_center,
    component_in_inspector,
    media_seconds,
    run_suite_main,
    save_project_to_disk,
    seek_timeline_seconds,
)


ASSETS_TAB = "dock.tab:assets"
INSPECTOR_TAB = "dock.tab:inspector"
TIMELINE_TAB = "dock.tab:timeline"
CREATE_ACTION = "assets.create.plexus_network_template"


def _invocation(project, item_id):
    source = project["items"][item_id]["source"]
    if source.get("kind") != "module":
        raise QaFailure("Plexus placement is not a Node Clip invocation")
    return source["value"]


def _catalog_nodes(definition):
    result = {}
    for node_id, node in definition["graph"]["nodes"].items():
        catalog_id = native_catalog_id(node)
        if catalog_id is None and node_content_type(node) == "color":
            value = str((node.get("content") or {}).get("data") or "")
            if value.replace("_", "").lower() == "colorramp":
                catalog_id = "native.color.ramp"
        if catalog_id:
            if catalog_id in result:
                raise QaFailure("Plexus graph duplicated " + catalog_id)
            result[catalog_id] = node_id
    return result


def _parameter(definition, name):
    matches = [
        parameter
        for parameter in definition["interface"]["parameters"]
        if parameter.get("name") == name
    ]
    if len(matches) != 1:
        raise QaFailure("Plexus has no unique published " + name)
    return matches[0]


def _route_set(definition):
    return {
        (
            route["from"]["node_id"],
            route["from"]["port"],
            route["to"]["node_id"],
            route["to"]["port"],
        )
        for route in definition["graph"]["connections"]
    }


def _assert_factory(definition):
    if definition.get("name") != "Plexus Network" or definition.get("sharing") != {
        "kind": "reusable_template",
        "origin": {"kind": "project"},
    }:
        raise QaFailure("Plexus factory did not create one project-owned reusable template")
    if len(definition["graph"]["nodes"]) != 4 or len(
        definition["graph"]["connections"]
    ) != 3:
        raise QaFailure("Plexus factory did not create the finite 4-Node chain")
    nodes = _catalog_nodes(definition)
    required = {
        "native.point.grid",
        "native.point.connect-points",
        "native.point.line-renderer",
    }
    if set(nodes) != required:
        raise QaFailure("Plexus factory catalog Nodes drifted: {!r}".format(nodes))
    grid = definition["graph"]["nodes"][nodes["native.point.grid"]]
    if {
        "count_x": constant_property(grid, "count_x"),
        "count_y": constant_property(grid, "count_y"),
        "count_z": constant_property(grid, "count_z"),
        "spacing": constant_property(grid, "spacing"),
        "center": constant_property(grid, "center"),
    } != {
        "count_x": 10,
        "count_y": 7,
        "count_z": 1,
        "spacing": {"x": 40.0, "y": 40.0, "z": 40.0},
        "center": {"x": 0.0, "y": 0.0, "z": 0.0},
    }:
        raise QaFailure("Plexus factory Grid defaults drifted")
    if [parameter["name"] for parameter in definition["interface"]["parameters"]] != [
        "Count X",
        "Count Y",
        "Count Z",
        "Spacing",
        "Center",
        "Min Distance",
        "Max Distance",
        "Max Neighbors",
        "Color",
        "Width",
        "Fade",
    ]:
        raise QaFailure("Plexus factory published interface drifted")
    expected_routes = {
        (
            nodes["native.point.grid"],
            "points",
            nodes["native.point.connect-points"],
            "points",
        ),
        (
            nodes["native.point.connect-points"],
            "connections",
            nodes["native.point.line-renderer"],
            "connections",
        ),
    }
    if not expected_routes.issubset(_route_set(definition)):
        raise QaFailure("Plexus factory omitted its typed Point connection routes")
    return nodes


def _create_template(client):
    activate_dock_tab(client, ASSETS_TAB, "Assets", "Plexus template creation")
    before = client.state()
    before_definitions = set(before["project"]["module_definitions"])
    client.click_component("assets.node_clip_source", button="secondary")
    _, action = client.wait_component_settled(CREATE_ACTION)
    if (action.get("metadata") or {}) != {
        "action": "create_plexus_network_template",
        "definition_sharing": "reusable_template",
    }:
        raise QaFailure("Plexus creation action lost its reusable-template identity")
    client.click_component(CREATE_ACTION)

    def created():
        state = client.state()
        added = set(state["project"]["module_definitions"]) - before_definitions
        return (
            (state, next(iter(added)))
            if state["history"]["revision"] == before["history"]["revision"] + 1
            and len(added) == 1
            and state["project"]["items"] == before["project"]["items"]
            and state["project"]["module_instances"]
            == before["project"]["module_instances"]
            else None
        )

    state, definition_id = client.wait_until("reusable Plexus template", created)
    definition = state["project"]["module_definitions"][definition_id]
    nodes = _assert_factory(definition)
    _, asset = client.wait_component_settled("assets.module:" + definition_id)
    metadata = asset.get("metadata") or {}
    if (
        metadata.get("module_definition_id") != definition_id
        or metadata.get("draggable_to_timeline") is not True
        or metadata.get("kind") != "node_clip"
    ):
        raise QaFailure("Plexus template is not an ordinary draggable Node Clip Asset")
    return state, definition_id, nodes


def _place_template(client, definition_id, seconds, description):
    activate_dock_tab(client, ASSETS_TAB, "Assets", description)
    source_snapshot, source = client.wait_component_settled(
        "assets.module:" + definition_id
    )
    before = client.state()
    _, canvas = client.wait_component_settled("timeline.canvas")
    track = next(
        (
            component
            for component in source_snapshot["components"]
            if component.get("type") == "timeline_track"
            and component.get("visible") is True
            and component.get("enabled") is True
        ),
        None,
    )
    if track is None:
        raise QaFailure("Timeline has no visible Plexus drop track")
    rect = canvas["rect_points"]
    timeline = before["editor"]["timeline"]
    target = {
        "x": float(rect["min_x"])
        + seconds * float(timeline["pixels_per_second"])
        - float(timeline["horizontal_scroll"]),
        "y": component_center(track)["y"],
    }
    if not float(rect["min_x"]) < target["x"] < float(rect["max_x"]):
        raise QaFailure("Plexus Timeline drop target is outside the viewport")
    before_items = set(before["project"]["items"])
    before_instances = set(before["project"]["module_instances"])
    client.drag(component_center(source), target, steps=18)

    def placed():
        state = client.state()
        items = set(state["project"]["items"]) - before_items
        instances = set(state["project"]["module_instances"]) - before_instances
        return (
            (state, next(iter(items)), next(iter(instances)))
            if state["history"]["revision"] == before["history"]["revision"] + 1
            and len(items) == 1
            and len(instances) == 1
            else None
        )

    state, item_id, instance_id = client.wait_until(description, placed)
    invocation = _invocation(state["project"], item_id)
    instance = state["project"]["module_instances"][instance_id]
    if (
        invocation.get("instance_id") != instance_id
        or instance.get("definition_id") != definition_id
        or state["project"]["module_definitions"]
        != before["project"]["module_definitions"]
        or media_seconds(state["project"]["items"][item_id]["interval"]["start"])
        != seconds
    ):
        raise QaFailure("Plexus Asset D&D copied or changed its reusable definition")
    return state, item_id, instance_id


def _select_inspector(client, item_id, description):
    activate_dock_tab(client, TIMELINE_TAB, "Timeline", description)
    client.click_component("timeline.item:" + item_id)
    activate_dock_tab(client, INSPECTOR_TAB, "Inspector", description)


def _edit_instance_number(client, item_id, instance_id, parameter, value, description):
    _select_inspector(client, item_id, description)
    control_id = "inspector.property:module_instance:{}:{}".format(
        instance_id, parameter["id"]
    )
    component_in_inspector(client, control_id)
    before = client.state()
    definitions = before["project"]["module_definitions"]
    enter_exact_numeric(client, control_id, value)

    def edited():
        state = client.state()
        overrides = state["project"]["module_instances"][instance_id].get(
            "parameter_overrides", {}
        )
        return (
            state
            if state["history"]["revision"] == before["history"]["revision"] + 1
            and overrides.get(parameter["id"]) == value
            and state["project"]["module_definitions"] == definitions
            else None
        )

    return client.wait_until(description, edited), control_id


def _keys(project, item_id, parameter_id):
    return (
        _invocation(project, item_id)
        .get("automation_tracks", {})
        .get(parameter_id, {})
        .get("keyframes", [])
    )


def _animate_width(client, item_id, instance_id, parameter, start, value):
    seek_timeline_seconds(client, start + 0.5)
    _select_inspector(client, item_id, "Plexus Width animation")
    mode_id = "inspector.property_mode:module_instance:{}:{}".format(
        instance_id, parameter["id"]
    )
    _, mode = component_in_inspector(client, mode_id)
    if not mode.get("enabled") or not (mode.get("metadata") or {}).get(
        "allow_keyframe"
    ):
        raise QaFailure("Plexus Width is not frame-keyframeable")
    before = client.state()
    client.click_component(mode_id)
    client.wait_component_settled(mode_id + ".option:keyframe")
    client.click_component(mode_id + ".option:keyframe")
    first = client.wait_until(
        "Plexus first Width key",
        lambda: state
        if (state := client.state())["history"]["revision"]
        == before["history"]["revision"] + 1
        and len(_keys(state["project"], item_id, parameter["id"])) == 1
        else None,
    )
    if _keys(first["project"], item_id, parameter["id"])[0]["value"] != value:
        raise QaFailure("Plexus first Width key lost the effective Inspector value")

    seek_timeline_seconds(client, start + 2.0)
    _select_inspector(client, item_id, "Plexus second Width key")
    control_id = "inspector.property:module_instance:{}:{}".format(
        instance_id, parameter["id"]
    )
    component_in_inspector(client, control_id)
    before_second = client.state()
    enter_exact_numeric(client, control_id, 1.0)
    second = client.wait_until(
        "Plexus second Width key",
        lambda: state
        if (state := client.state())["history"]["revision"]
        == before_second["history"]["revision"] + 1
        and len(_keys(state["project"], item_id, parameter["id"])) == 2
        else None,
    )
    times = sorted(
        media_seconds(key["time"])
        for key in _keys(second["project"], item_id, parameter["id"])
    )
    if times != [0.5, 2.0]:
        raise QaFailure("Plexus Width keys did not use exact item-local time")
    return second


def _author_color_field(client, item_id, reusable_id, sibling_id, sibling_instance_id):
    opened_id, before = open_timeline_item_definition(
        client, item_id, "node_clip", "Plexus color field"
    )
    if opened_id != reusable_id:
        raise QaFailure("Plexus opened a different reusable definition")
    old_nodes = _catalog_nodes(before)
    grid_id = old_nodes["native.point.grid"]
    line_id = old_nodes["native.point.line-renderer"]
    color = _parameter(before, "Color")
    ensure_node_editor_authoring_scale(client)
    _, info_id = create_node_from_menu(
        client, "node_clip", "Point Info", "node_editor.menu.create.point_info"
    )
    private_id, copied = active_definition(client.state(), "node_clip")
    if private_id == reusable_id or copied.get("sharing") != {"kind": "private"}:
        raise QaFailure("Plexus instance edit did not isolate the reusable template")
    _, ramp_id = create_node_from_menu(
        client, "node_clip", "Color Ramp", "node_editor.menu.create.color:ramp"
    )
    # Use the production Clean Layout All command, after COW has made this
    # instance private. Menu-created headers need not start clear of every
    # existing body, and dragging their assumed centers is not a safe gesture.
    before_layout = client.state()
    client.key("l", True, shift=True)
    client.key("l", False, shift=True)
    client.wait_until(
        "Plexus private node layout",
        lambda: state
        if (state := client.state())["history"]["revision"]
        == before_layout["history"]["revision"] + 1
        else None,
    )
    activate_dock_tab(client, TIMELINE_TAB, "Timeline", "Plexus fitted nodes")
    open_timeline_item_definition(client, sibling_id, "node_clip", "Plexus sibling document")
    activate_dock_tab(client, TIMELINE_TAB, "Timeline", "Plexus private document")
    open_timeline_item_definition(client, item_id, "node_clip", "Plexus fitted nodes")
    connect_nodes(
        client, "node_clip", grid_id, "points", info_id, "points", "Grid to Point Info"
    )
    connect_nodes(
        client,
        "node_clip",
        info_id,
        "random",
        ramp_id,
        "factor",
        "Point random to Color Ramp",
    )
    current = active_definition(client.state(), "node_clip")[1]
    instance_id = _invocation(client.state()["project"], item_id)["instance_id"]
    unpublish_node_input_parameter(
        client, private_id, instance_id, line_id, "color", color, current
    )
    color_route = connect_nodes(
        client,
        "node_clip",
        ramp_id,
        "color",
        line_id,
        "color",
        "Color Ramp to Line Renderer",
    )
    state = client.state()
    sibling = state["project"]["module_instances"][sibling_instance_id]
    if (
        sibling.get("definition_id") != reusable_id
        or state["project"]["module_definitions"][reusable_id]
        != before
        or _invocation(state["project"], sibling_id)["instance_id"]
        != sibling_instance_id
    ):
        raise QaFailure("Plexus instance color field changed the reusable sibling")
    return state, private_id, info_id, ramp_id, color_route


def run_suite(client):
    client.wait_health()
    created, reusable_id, factory_nodes = _create_template(client)
    placed, item_id, instance_id = _place_template(
        client, reusable_id, 6.5, "first Plexus placement"
    )
    sibling_state, sibling_id, sibling_instance_id = _place_template(
        client, reusable_id, 12.0, "second Plexus placement"
    )
    definition = sibling_state["project"]["module_definitions"][reusable_id]
    maximum = _parameter(definition, "Max Distance")
    width = _parameter(definition, "Width")
    start = media_seconds(sibling_state["project"]["items"][item_id]["interval"]["start"])
    baseline = sample_rendered_preview(
        client, start + 0.5, sibling_state["history"]["revision"], "Plexus baseline"
    )

    distance_state, _ = _edit_instance_number(
        client, item_id, instance_id, maximum, 45.0, "Plexus Max Distance"
    )
    distance_pixels = sample_rendered_preview(
        client, start + 0.5, distance_state["history"]["revision"], "Plexus distance"
    )
    if distance_pixels["pixel_hash"] == baseline["pixel_hash"]:
        raise QaFailure("Plexus Max Distance did not change rendered connections")
    width_state, _ = _edit_instance_number(
        client, item_id, instance_id, width, 6.0, "Plexus Width"
    )
    wide_pixels = sample_rendered_preview(
        client, start + 0.5, width_state["history"]["revision"], "wide Plexus"
    )
    if wide_pixels["pixel_hash"] == distance_pixels["pixel_hash"]:
        raise QaFailure("Plexus Width did not change rendered line pixels")
    history_shortcut(client)
    undone = client.wait_until(
        "Undo Plexus Width",
        lambda: state
        if (state := client.state())["history"]["revision"]
        == width_state["history"]["revision"] + 1
        and state["project"] == distance_state["project"]
        else None,
    )
    undone_pixels = sample_rendered_preview(
        client, start + 0.5, undone["history"]["revision"], "undone Plexus Width"
    )
    if undone_pixels["pixel_hash"] != distance_pixels["pixel_hash"]:
        raise QaFailure("Undo Plexus Width did not restore exact pixels")
    history_shortcut(client, redo=True)
    redone = client.wait_until(
        "Redo Plexus Width",
        lambda: state
        if (state := client.state())["history"]["revision"]
        == undone["history"]["revision"] + 1
        and state["project"] == width_state["project"]
        else None,
    )

    _select_inspector(client, item_id, "Plexus Inspector capture")
    component_in_inspector(
        client,
        "inspector.property:module_instance:{}:{}".format(instance_id, width["id"]),
    )
    artifact_dir = pathlib.Path(os.environ["RUVIE_QA_ARTIFACT_DIR"])
    inspector_capture = capture_viewport(client, artifact_dir / "plexus-inspector.png")
    animated = _animate_width(client, item_id, instance_id, width, start, 6.0)
    animated_samples = [
        sample_rendered_preview(
            client, time, animated["history"]["revision"], "animated Plexus"
        )
        for time in (start + 0.5, start + 2.0)
    ]
    if animated_samples[0]["pixel_hash"] == animated_samples[1]["pixel_hash"]:
        raise QaFailure("Plexus Width keyframes did not change multi-time pixels")

    colored, private_id, info_id, ramp_id, color_route = _author_color_field(
        client, item_id, reusable_id, sibling_id, sibling_instance_id
    )
    colored_pixels = sample_rendered_preview(
        client, start + 0.5, colored["history"]["revision"], "colored Plexus"
    )
    if colored_pixels["pixel_hash"] == animated_samples[0]["pixel_hash"]:
        raise QaFailure("Point field Color Ramp did not change Plexus line pixels")
    open_timeline_item_definition(client, item_id, "node_clip", "Plexus graph capture")
    graph_capture = capture_viewport(client, artifact_dir / "capture.png")

    samples = [
        colored_pixels,
        sample_rendered_preview(
            client, start + 2.0, colored["history"]["revision"], "colored Plexus later"
        ),
    ]
    project_path = os.environ.get("RUVIE_QA_PROJECT_PATH")
    if not project_path:
        raise QaFailure("Plexus persistence QA requires a Project file")
    project_file = pathlib.Path(project_path)
    saved, saved_file = save_project_to_disk(client, project_file, "Plexus Network")
    expected_routes = _route_set(saved["project"]["module_definitions"][private_id])

    def validate_reloaded(definition):
        nodes = _catalog_nodes(definition)
        if (
            nodes.get("native.point.info") != info_id
            or nodes.get("native.color.ramp") != ramp_id
            or _route_set(definition) != expected_routes
            or not any(route.get("id") == color_route["id"] for route in definition["graph"]["connections"])
        ):
            raise QaFailure("fresh process changed the authored Plexus color graph")

    reloaded = reload_node_clip_project(
        client,
        project_file,
        saved,
        item_id,
        private_id,
        samples,
        artifact_dir,
        "Plexus Network",
        "plexus-network",
        "plexus-reloaded.png",
        validate_reloaded,
    )
    return {
        "suite": "plexus-network",
        "reusable_definition_id": reusable_id,
        "private_definition_id": private_id,
        "item_id": item_id,
        "instance_id": instance_id,
        "sibling_item_id": sibling_id,
        "sibling_instance_id": sibling_instance_id,
        "factory_nodes": factory_nodes,
        "authored_nodes": {"point_info": info_id, "color_ramp": ramp_id},
        "samples": samples,
        "saved": saved_file,
        "inspector_capture": inspector_capture,
        "capture": graph_capture,
        "reloaded": reloaded,
        "actions": client.evidence,
    }


if __name__ == "__main__":
    raise SystemExit(
        run_suite_main(
            "qa-plexus-network-e2e",
            run_suite,
            "target/qa-plexus-network-e2e-evidence.json",
        )
    )
