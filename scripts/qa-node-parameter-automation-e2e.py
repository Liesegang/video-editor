#!/usr/bin/env python3
"""Author a Node input's Timeline keys through the real native surfaces."""

import os
import pathlib

from qa_automation_support import wait_item_automation_surfaces, require_exact_item_times
from qa_curve_support import exercise_curve_key_live_preview
from qa_node_module_support import active_definition, port, connection, place_node_for_inline_edit
from qa_property_gesture_support import begin_reserved_keyframe_scrub, release_property_scrub
from qa_support import (
    QaClient, QaFailure, activate_dock_tab, bring_timeline_component,
    capture_viewport, close_clean_native_app, component_center,
    find_clear_canvas_point, free_port, item_by_name, media_seconds,
    request_clean_native_close, run_suite_main, save_project_to_disk,
    seek_timeline_seconds, settled_preview_state, spawned_authoring_app,
)


def _seek(client, seconds):
    sought = seek_timeline_seconds(client, seconds)
    return client.wait_until(
        "rendered Node automation at {}s".format(seconds),
        lambda: settled_preview_state(
            client, sought["history"]["revision"],
            sought["editor"]["timeline"]["current_frame"],
        ),
        30.0,
    )


def _samples(client, times):
    return [{"seconds": time, "pixel_hash": _seek(client, time)["editor"]["preview"]["pixel_hash"]}
            for time in times]


def _invocation(project, item_id):
    return project["items"][item_id]["source"]["value"]


def _keys(project, item_id, parameter_id):
    track = _invocation(project, item_id).get("automation_tracks", {}).get(parameter_id)
    return [] if track is None else track["keyframes"]


def _node_panel(client):
    activate_dock_tab(client, "dock.tab:node_editor", "Node Editor", "Node input animation")


def _history(client, redo=False):
    client.key("z", True, command=True, shift=redo)
    client.key("z", False, command=True, shift=redo)


def _unchanged_sibling(project, before, item_id):
    invocation = _invocation(before, item_id)
    instance_id = invocation["instance_id"]
    definition_id = before["module_instances"][instance_id]["definition_id"]
    for collection, identity in (("items", item_id), ("module_instances", instance_id)):
        if project[collection][identity] != before[collection][identity]:
            raise QaFailure("Node input edit changed sibling " + collection)
    expected_definition = dict(before["module_definitions"][definition_id])
    # The existing COW owner normalizes the sole remaining SharedLocal instance.
    # Graph, interface, default values and all versions must remain identical.
    if expected_definition["sharing"] == {"kind": "shared_local"}:
        remaining = sum(instance["definition_id"] == definition_id
                        for instance in project["module_instances"].values())
        if remaining != 1:
            raise QaFailure("sibling definition did not become exclusively owned")
        expected_definition["sharing"] = {"kind": "private"}
    if project["module_definitions"][definition_id] != expected_definition:
        raise QaFailure("Node input edit changed sibling definition beyond COW ownership")


def _connect(client, from_node, to_node):
    def ports():
        snapshot = client.component_snapshot()
        source = port(snapshot, "output", from_node, "image")
        target = port(snapshot, "input", to_node, "image")
        return (source, target) if source and target else None
    source, target = client.wait_until("Image ports for Node automation", ports)
    client.drag(component_center(source), component_center(target), steps=10)
    client.wait_until(
        "Image connection for Node automation",
        lambda: connection(active_definition(client.state())[1], from_node, to_node),
    )


def _create_opacity(client, item_id):
    bring_timeline_component(client, "timeline.item:" + item_id, -120.0)
    client.double_click_component("timeline.item:" + item_id)
    client.wait_component_settled("node_editor.canvas")
    _, before = active_definition(client.state(), "node_clip")
    nodes = before["graph"]["nodes"]
    output = next(key for key, node in nodes.items()
                  if node["content"]["type"].replace("_", "").lower() == "moduleoutput")
    source = next(key for key in nodes if key != output)
    point = find_clear_canvas_point(client.component_snapshot(), "node_editor.canvas",
                                   ("node_editor.node:", "node_editor.node_header:"))
    client.inject("click", {**point, "button": "secondary", "coordinate_space": "points"})
    client.wait_component_settled("node_editor.menu.search")
    client.click_component("node_editor.menu.search")
    client.inject("text", {"text": "Image Opacity"})
    client.wait_component_settled("node_editor.menu.create.image_opacity")
    client.click_component("node_editor.menu.create.image_opacity")
    created = client.wait_until(
        "Image Opacity Node creation",
        lambda: current if len((current := active_definition(client.state())[1])["graph"]["nodes"])
        == len(nodes) + 1 else None,
    )
    opacity = next(iter(set(created["graph"]["nodes"]) - set(nodes)))
    original_edge = before["graph"]["connections"][0]["id"]
    client.click_component("node_editor.connection:" + original_edge, button="secondary")
    client.wait_component_settled("node_editor.wire_menu.disconnect")
    client.click_component("node_editor.wire_menu.disconnect")
    client.wait_until("disconnect original Image route", lambda: not connection(
        active_definition(client.state())[1], source, output))
    _connect(client, source, opacity)
    _connect(client, opacity, output)
    place_node_for_inline_edit(client, opacity, "node_editor.property.node:{}:opacity".format(opacity))
    return opacity


def run_suite(client):
    client.wait_health()
    initial = client.state()
    item = item_by_name(initial["project"], "QA Node Clip")
    item_id = item["id"]
    start = media_seconds(item["interval"]["start"])
    duration = media_seconds(item["interval"]["duration"])
    node_id = _create_opacity(client, item_id)
    before_duplicate = client.state()
    activate_dock_tab(client, "dock.tab:timeline", "Timeline", "Duplicate Node Clip")
    bring_timeline_component(client, "timeline.item:" + item_id, -120.0)
    client.click_component("timeline.item:" + item_id, button="secondary")
    duplicate = "timeline.item.duplicate:" + item_id
    client.wait_component_settled(duplicate)
    client.click_component(duplicate)
    duplicated = client.wait_until(
        "sibling Node Clip placement",
        lambda: state if len((state := client.state())["project"]["items"])
        == len(before_duplicate["project"]["items"]) + 1 else None,
    )
    sibling_id = next(iter(set(duplicated["project"]["items"]) - set(before_duplicate["project"]["items"])))
    sibling_times = [start + duration + 0.5, start + duration + 2.0]
    sibling_before = _samples(client, sibling_times)
    original_times = [start + 0.5, start + 2.0]
    first = _seek(client, original_times[0])
    bring_timeline_component(client, "timeline.item:" + item_id, -120.0)
    client.double_click_component("timeline.item:" + item_id)
    mode_id = "node_editor.property_mode.node:{}:opacity".format(node_id)
    control_id = "node_editor.property.node:{}:opacity".format(node_id)
    place_node_for_inline_edit(client, node_id, control_id)
    before_publish = client.state()
    instance_id = _invocation(before_publish["project"], item_id)["instance_id"]
    old_definition_id, _ = active_definition(before_publish)
    _, mode = client.wait_component_settled(mode_id)
    if not mode.get("enabled") or not mode["metadata"].get("allow_keyframe"):
        raise QaFailure("Node input clock cannot start Timeline animation")
    client.click_component(mode_id)
    client.wait_component_settled(mode_id + ".option:keyframe")
    client.click_component(mode_id + ".option:keyframe")
    published = client.wait_until(
        "one atomic input publication and first Timeline key",
        lambda: state if (state := client.state())["history"]["revision"]
        == before_publish["history"]["revision"] + 1 else None,
    )
    definition_id, definition = active_definition(published)
    parameter = next(p for p in definition["interface"]["parameters"]
                     if p["target"]["node_id"] == node_id)
    parameter_id = parameter["id"]
    keys = _keys(published["project"], item_id, parameter_id)
    if definition_id == old_definition_id or len(keys) != 1:
        raise QaFailure("first key did not privately publish exactly one input")
    if abs(media_seconds(keys[0]["time"]) - 0.5) > 1e-9:
        raise QaFailure("first Node key used absolute Timeline time")
    _unchanged_sibling(published["project"], before_publish["project"], sibling_id)
    _history(client)
    client.wait_until("atomic publication Undo", lambda: client.state()["project"] == before_publish["project"])
    _history(client, redo=True)
    client.wait_until("stable publication Redo", lambda: client.state()["project"] == published["project"])

    baseline = _seek(client, original_times[1])
    _node_panel(client)
    reservation = begin_reserved_keyframe_scrub(
        client, control_id, control_id, -36.0, "Node opacity key insertion",
        control_locator=lambda client, identity: place_node_for_inline_edit(client, node_id, identity),
    )
    frame = baseline["editor"]["timeline"]["current_frame"]
    held = client.wait_until(
        "Node scrub transient pixels without persisted changes",
        lambda: state if (state := settled_preview_state(client, baseline["history"]["revision"], frame))
        and state["project"] == baseline["project"] and state["history"] == baseline["history"]
        and state["editor"]["preview"]["pixel_hash"] != baseline["editor"]["preview"]["pixel_hash"] else None,
        30.0,
    )
    artifact_dir = pathlib.Path(os.environ["RUVIE_QA_ARTIFACT_DIR"])
    held_capture = capture_viewport(client, artifact_dir / "node-input-held.png")
    release_property_scrub(client, reservation)
    committed = client.wait_until(
        "one Node value release command",
        lambda: state if (state := client.state())["history"]["revision"]
        == baseline["history"]["revision"] + 1
        and len(_keys(state["project"], item_id, parameter_id)) == 2 else None,
    )
    keys = sorted(_keys(committed["project"], item_id, parameter_id), key=lambda key: media_seconds(key["time"]))
    if keys[1]["id"] != reservation["id"] or abs(media_seconds(keys[1]["time"]) - 2.0) > 1e-9:
        raise QaFailure("Node release lost the reserved key identity/local time")
    if committed["project"]["module_definitions"] != baseline["project"]["module_definitions"]:
        raise QaFailure("instance animation edited Node topology")
    _unchanged_sibling(committed["project"], before_publish["project"], sibling_id)
    final_render = client.wait_until(
        "Node committed pixels equal held pixels",
        lambda: state if (state := settled_preview_state(client, committed["history"]["revision"], frame))
        and state["editor"]["preview"]["pixel_hash"] == held["editor"]["preview"]["pixel_hash"] else None,
    )
    _history(client)
    client.wait_until("Node value Undo", lambda: client.state()["project"] == baseline["project"])
    _history(client, redo=True)
    client.wait_until("Node value Redo", lambda: client.state()["project"] == committed["project"])

    inspector_mode = "inspector.property_mode:module_instance:{}:{}".format(instance_id, parameter_id)
    _, inspector = client.wait_component_settled(inspector_mode)
    if inspector["metadata"].get("keyframe_count") != 2:
        raise QaFailure("Inspector did not expose the same two Node input keys")
    client.click_component(mode_id)
    client.wait_component_settled(mode_id + ".toggle_keyframe")
    client.click_component(mode_id + ".toggle_keyframe")
    client.wait_until("remove current Node input key", lambda: len(
        _keys(client.state()["project"], item_id, parameter_id)) == 1)
    _history(client)
    client.wait_until("Node key removal Undo", lambda: client.state()["project"] == committed["project"])

    surfaces = wait_item_automation_surfaces(client, item_id,
        {"kind": "module_parameter", "id": parameter_id}, [key["id"] for key in keys], "Node input")
    require_exact_item_times(surfaces, [0.5, 2.0], original_times)
    curve_baseline = _seek(client, start + 1.5)
    activate_dock_tab(client, "dock.tab:curve_editor", "Curve Editor", "Node input curve")
    curve = exercise_curve_key_live_preview(client, surfaces["curve_keys"][1]["id"], curve_baseline,
                                           "Node input curve", delta_x=16.0, delta_y=18.0)
    curve_capture = capture_viewport(client, artifact_dir / "node-input-curve.png")
    if _samples(client, sibling_times) != sibling_before:
        raise QaFailure("Node input automation changed sibling Preview pixels")
    previews = _samples(client, original_times + sibling_times)
    project_file = pathlib.Path(os.environ["RUVIE_QA_PROJECT_PATH"])
    saved, file_evidence = save_project_to_disk(client, project_file, "Node input animation")
    initial_close = request_clean_native_close(client, "Node input animation", client.timeout)
    reload_port = free_port()
    with spawned_authoring_app(reload_port, {
        "RUVIE_QA_PROJECT_PATH": str(project_file), "RUVIE_QA_OPEN_EXISTING_PROJECT": "1",
        "RUVIE_QA_PORT_FILE": None,
        "RUVIE_QA_RUN_ID": os.environ.get("RUVIE_QA_RUN_ID", "node-input") + ":reload",
    }) as process:
        fresh = QaClient("http://127.0.0.1:{}".format(reload_port), client.timeout)
        fresh.wait_health()
        if fresh.state()["project"] != saved["project"]:
            raise QaFailure("fresh process changed Node input keys/IDs/values/easing")
        if _samples(fresh, original_times + sibling_times) != previews:
            raise QaFailure("fresh process changed Node input multi-time pixels")
        bring_timeline_component(fresh, "timeline.item:" + item_id, -120.0)
        fresh.double_click_component("timeline.item:" + item_id)
        fresh.wait_component_settled("node_editor.canvas")
        reload_surfaces = wait_item_automation_surfaces(fresh, item_id,
            {"kind": "module_parameter", "id": parameter_id}, [key["id"] for key in keys], "Reloaded Node input")
        capture = capture_viewport(fresh, artifact_dir / "capture.png")
        close = close_clean_native_app(fresh, process, "reloaded Node input", client.timeout)
    return {
        "suite": "node-parameter-automation", "item_id": item_id, "instance_id": instance_id,
        "sibling_id": sibling_id, "node_id": node_id, "parameter_id": parameter_id,
        "keys": keys, "surfaces": surfaces, "reload_surfaces": reload_surfaces,
        "held_capture": held_capture, "curve_capture": curve_capture, "curve": curve,
        "held_pixel_hash": held["editor"]["preview"]["pixel_hash"],
        "committed_pixel_hash": final_render["editor"]["preview"]["pixel_hash"],
        "previews": previews, "saved": file_evidence, "initial_close": initial_close,
        "reload_close": close, "capture": capture, "actions": client.evidence,
        "reload_actions": fresh.evidence,
    }


if __name__ == "__main__":
    raise SystemExit(run_suite_main("qa-node-parameter-automation-e2e", run_suite,
                                   "target/qa-node-parameter-automation-evidence.json"))
