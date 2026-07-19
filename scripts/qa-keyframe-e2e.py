#!/usr/bin/env python3
"""Coordinate E2E for Inspector, Graph Editor, and keyframe dialog.

Every mutation is driven through a latest-frame component rectangle and normal
egui pointer events. The authoritative Project, history, Inspector metadata,
and rendered Preview are queried independently after each gesture.
"""

import argparse
import importlib.util
import json
import os
import signal
import subprocess
import sys


SCRIPT_DIR = os.path.dirname(os.path.abspath(__file__))
BASE_PATH = os.path.join(SCRIPT_DIR, "qa-e2e.py")
SPEC = importlib.util.spec_from_file_location("ruvie_qa_e2e_base", BASE_PATH)
if SPEC is None or SPEC.loader is None:
    raise RuntimeError("cannot load scripts/qa-e2e.py")
BASE = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(BASE)

QaClient = BASE.QaClient
QaFailure = BASE.QaFailure
assert_history_delta = BASE.assert_history_delta
free_port = BASE.free_port

CLIP_A2 = BASE.CLIP_A2
TEXT_FILL = BASE.TEXT_FILL
TRANSFORM_EFFECTOR = BASE.TRANSFORM_EFFECTOR
BACKPLATE_DECORATOR = BASE.BACKPLATE_DECORATOR
BLUR_EFFECT = BASE.BLUR_EFFECT


def target_property(project, node_id, property_name):
    """Read a property authored directly on one explicit operation Node."""
    return project["nodes"][node_id]["properties"][property_name]


def only_keyframe(project, node_id, property_name):
    keyframes = property_keyframes(project, node_id, property_name)
    if len(keyframes) != 1:
        raise QaFailure(
            "{}.{} has {} keyframes, expected one".format(
                node_id, property_name, len(keyframes)
            )
        )
    return keyframes[0]


def property_keyframes(project, node_id, property_name):
    prop = target_property(project, node_id, property_name)
    return prop["properties"]["keyframes"]


def keyframe_by_id(project, node_id, property_name, keyframe_id):
    return next(
        (
            keyframe
            for keyframe in property_keyframes(project, node_id, property_name)
            if keyframe["id"] == keyframe_id
        ),
        None,
    )


def numeric_keyframe_value(project, node_id, property_name):
    value = only_keyframe(project, node_id, property_name)["value"]
    if not isinstance(value, (int, float)):
        raise QaFailure("{}.{} is not numeric: {!r}".format(node_id, property_name, value))
    return float(value)


def modified_click(client, component_id, command=False, shift=False):
    wait_component_settled(client, component_id)
    snapshot, component = client.component(component_id)
    point = client.point(component["rect_points"])
    client.inject(
        "click",
        {
            "x": point["x"],
            "y": point["y"],
            "coordinate_space": "points",
            "button": "primary",
            "modifiers": {"command": command, "shift": shift},
        },
        {
            "component_id": component_id,
            "component_frame": snapshot["frame"],
            "component_rect_points": component["rect_points"],
        },
    )


def coordinate_drag(client, component_id, dx, dy, steps=12):
    wait_component_settled(client, component_id)
    snapshot, component = client.component(component_id)
    start = client.point(component["rect_points"])
    end = {"x": start["x"] + dx, "y": start["y"] + dy}
    client.inject(
        "drag",
        {
            "from": start,
            "to": end,
            "coordinate_space": "points",
            "steps": steps,
            "button": "primary",
        },
        {
            "component_id": component_id,
            "component_frame": snapshot["frame"],
            "component_rect_points": component["rect_points"],
        },
    )


def coordinate_double_click(client, component_id):
    # Each click resolves the component again immediately before injection.
    # Keeping the two clicks adjacent lets egui's normal double-click timing
    # and hit testing decide whether the curve should receive the gesture.
    client.click_component(component_id)
    client.click_component(component_id)


def shortcut_undo_redo(client, redo=False):
    client.key("z", True, command=True, shift=redo)
    client.key("z", False, command=True, shift=redo)


def graph_keyframe_component(property_name, keyframe_id):
    return "graph.keyframe.{}:{}".format(property_name, keyframe_id)


def graph_easing_component(keyframe_id, option):
    return "graph.keyframe_menu.easing.{}:{}".format(option, keyframe_id)


def set_graph_easing(client, property_name, keyframe_id, option, serialized_easing):
    point_id = graph_keyframe_component(property_name, keyframe_id)
    wait_component_settled(client, point_id)
    client.click_component(point_id, button="secondary")
    if option == "ease_in_out_cubic":
        family_id = graph_easing_component(keyframe_id, "family.cubic")
        client.wait_component(family_id)
        client.click_component(family_id)
    option_id = graph_easing_component(keyframe_id, option)
    client.wait_component(option_id)
    before = client.state()
    client.click_component(option_id)
    after = client.wait_project(
        "Graph easing {}".format(option),
        lambda project: keyframe_by_id(
            project, TRANSFORM_EFFECTOR, "tx", keyframe_id
        )["easing"]
        == serialized_easing,
    )
    assert_history_delta(before, after, 1, "Graph easing {}".format(option))
    return before, after


def click_graph_time(client, global_time):
    snapshot = client.component_snapshot()
    components = {item["id"]: item for item in snapshot["components"]}
    canvas = components["graph.canvas"]
    ruler = components["graph.ruler"]
    canvas_rect = canvas["rect_points"]
    ruler_rect = ruler["rect_points"]
    x = (
        canvas_rect["min_x"]
        + float(canvas["metadata"]["pan"]["x"])
        + global_time * float(canvas["metadata"]["zoom_x"])
    )
    if not ruler_rect["min_x"] < x < ruler_rect["max_x"]:
        raise QaFailure("Graph time {} is outside the visible ruler".format(global_time))
    client.inject(
        "click",
        {
            "x": x,
            "y": ruler_rect["center_y"],
            "coordinate_space": "points",
            "button": "primary",
        },
        {
            "component_id": "graph.ruler",
            "component_frame": snapshot["frame"],
            "component_rect_points": ruler_rect,
            "target_global_time": global_time,
        },
    )

    def reached_time():
        state = client.state()
        return (
            state
            if abs(float(state["editor"]["timeline"]["current_time"]) - global_time)
            < 1.0e-3
            else None
        )

    return client.wait_until("Graph ruler time {}".format(global_time), reached_time)


def numeric_easing_value(start_value, end_value, fraction, easing):
    if easing == "Linear":
        eased = fraction
    elif easing == "EaseInOutCubic":
        if fraction < 0.5:
            eased = 4.0 * fraction * fraction * fraction
        else:
            eased = 1.0 - ((-2.0 * fraction + 2.0) ** 3) / 2.0
    else:
        raise QaFailure("unsupported E2E easing {}".format(easing))
    return start_value + (end_value - start_value) * eased


def wait_inspector_numeric_value(client, component_id, expected, label):
    def matches():
        try:
            _, component = client.component(component_id)
            value = float(component["metadata"]["value"])
        except (QaFailure, KeyError, TypeError, ValueError):
            return None
        return component if abs(value - expected) < 1.0e-4 else None

    component = client.wait_until(label, matches)
    approximately(float(component["metadata"]["value"]), expected, label)
    return component


def assert_undo_transition(before, after, operation):
    if after["history"]["undo_depth"] != before["history"]["undo_depth"] - 1:
        raise QaFailure("{} did not remove exactly one Undo state".format(operation))
    if after["history"]["redo_depth"] != before["history"]["redo_depth"] + 1:
        raise QaFailure("{} did not create exactly one Redo state".format(operation))


def assert_redo_transition(before, after, operation):
    if after["history"]["undo_depth"] != before["history"]["undo_depth"] + 1:
        raise QaFailure("{} did not restore exactly one Undo state".format(operation))
    if after["history"]["redo_depth"] != before["history"]["redo_depth"] - 1:
        raise QaFailure("{} did not consume exactly one Redo state".format(operation))


def wait_component_settled(client, component_id, consecutive_frames=2):
    observed = {"frame": None, "rect": None, "stable": 0}

    def settled():
        try:
            # Force another completed UI pass; repeated reads of one registry
            # snapshot are not evidence that geometry settled.
            client.state()
            snapshot, component = client.component(component_id)
        except QaFailure:
            return None
        if snapshot["frame"] == observed["frame"]:
            return None
        rect = component["rect_points"]
        current = (
            float(rect["min_x"]),
            float(rect["min_y"]),
            float(rect["max_x"]),
            float(rect["max_y"]),
        )
        previous = observed["rect"]
        observed["frame"] = snapshot["frame"]
        observed["rect"] = current
        if previous is not None and all(
            abs(before - after) <= 0.05 for before, after in zip(previous, current)
        ):
            observed["stable"] += 1
        else:
            observed["stable"] = 0
        return (snapshot, component) if observed["stable"] >= consecutive_frames else None

    return client.wait_until("settled component {}".format(component_id), settled)


def ensure_in_inspector(client, component_id, max_attempts=48):
    last = None
    for _ in range(max_attempts):
        snapshot = client.component_snapshot()
        components = {item["id"]: item for item in snapshot["components"]}
        target = components.get(component_id)
        area = components.get("inspector.scroll_area")
        last = target
        if target is None or area is None:
            continue
        target_rect = target["rect_points"]
        area_rect = area["rect_points"]
        # At the bottom scroll limit the final header can sit only two points
        # inside the clip rect; that is still a valid interior hit target.
        margin = 1.0
        if (
            target.get("visible", False)
            and target_rect["min_y"] >= area_rect["min_y"] + margin
            and target_rect["max_y"] <= area_rect["max_y"] - margin
        ):
            wait_component_settled(client, component_id)
            _, settled = client.component(component_id)
            settled_rect = settled["rect_points"]
            if (
                settled.get("visible", False)
                and settled_rect["min_y"] >= area_rect["min_y"] + margin
                and settled_rect["max_y"] <= area_rect["max_y"] - margin
            ):
                return settled
            continue

        desired_y = area_rect["center_y"]
        delta_y = max(-120.0, min(120.0, desired_y - target_rect["center_y"]))
        if abs(delta_y) < 4.0:
            delta_y = -24.0 if target_rect["center_y"] > desired_y else 24.0
        point = client.point(area_rect)
        client.inject(
            "scroll",
            {
                "x": point["x"],
                "y": point["y"],
                "delta_x": 0.0,
                "delta_y": delta_y,
            },
            {
                "component_id": "inspector.scroll_area",
                "target_component_id": component_id,
                "component_frame": snapshot["frame"],
                "component_rect_points": area_rect,
            },
        )
    raise QaFailure(
        "could not settle {} in the Inspector viewport; last={!r}".format(
            component_id, last
        )
    )


def expose_operation_property(client, operation_id, property_name):
    header_id = "inspector.operation:" + operation_id
    control_id = "inspector.property.node:{}:{}".format(operation_id, property_name)
    ensure_in_inspector(client, header_id)
    snapshot = client.component_snapshot()
    if control_id not in {item["id"] for item in snapshot["components"]}:
        client.click_component(header_id)
        def registered():
            current = client.component_snapshot()
            return (
                current
                if control_id in {item["id"] for item in current["components"]}
                else None
            )

        client.wait_until(
            "direct operation property {}".format(control_id),
            registered,
        )
    return ensure_in_inspector(client, control_id)


def add_keyframe(client, control_id, node_id, property_name):
    ensure_in_inspector(client, control_id)
    wait_component_settled(client, control_id)
    before = client.state()
    client.click_component(control_id)
    after = client.wait_project(
        "{} {} keyframe add".format(node_id, property_name),
        lambda project: target_property(project, node_id, property_name)["type"]
        == "keyframe",
    )
    assert_history_delta(before, after, 1, "{} keyframe add".format(node_id))
    return after


def approximately(actual, expected, label, tolerance=1.0e-4):
    if abs(actual - expected) > tolerance:
        raise QaFailure(
            "{} was {}, expected {} ± {}".format(label, actual, expected, tolerance)
        )


def run_suite(client):
    health = client.wait_health()
    initial = BASE.wait_fresh_fixture(client)

    wait_component_settled(client, "dock.tab:node_editor")
    client.click_component("dock.tab:node_editor")
    client.wait_until(
        "Node Editor activation",
        lambda: client.state()
        if "Node Editor" in client.state()["dock"]["active_tabs"]
        else None,
    )

    # Select Clip A2 through its rendered container. Its Inspector facade
    # exposes each explicit operation Node while retaining one Project model.
    clip_header = "node_editor.container_header.clip:" + CLIP_A2
    BASE.reveal_node_editor_component(client, clip_header)
    client.click_component(clip_header)
    client.wait_until(
        "Clip A2 coordinate selection",
        lambda: client.state()
        if client.state()["editor"]["selection"]["last_selected_entity_id"] == CLIP_A2
        else None,
    )
    client.wait_component("inspector.owner.clip:" + CLIP_A2)

    tx_control = "inspector.property.node:{}:tx".format(TRANSFORM_EFFECTOR)
    tx_key = "inspector.keyframe.node:{}:tx".format(TRANSFORM_EFFECTOR)
    expose_operation_property(client, TRANSFORM_EFFECTOR, "tx")

    # Adding, editing, and removing the final keyframe all target Node 501
    # directly. Removing the last key preserves its typed value as a constant.
    add_keyframe(client, tx_key, TRANSFORM_EFFECTOR, "tx")
    before_update = client.state()
    old_tx = numeric_keyframe_value(
        before_update["project"], TRANSFORM_EFFECTOR, "tx"
    )
    coordinate_drag(client, tx_control, 22.0, 0.0)
    updated = client.wait_project(
        "Transform direct keyframe value update",
        lambda project: numeric_keyframe_value(project, TRANSFORM_EFFECTOR, "tx")
        != old_tx,
    )
    edited_tx = numeric_keyframe_value(updated["project"], TRANSFORM_EFFECTOR, "tx")
    assert_history_delta(before_update, updated, 1, "Transform keyframe value update")
    client.wait_preview_render_after(before_update, "Inspector keyframe value update")

    before_remove = client.state()
    client.click_component(tx_key)
    removed = client.wait_project(
        "last Transform keyframe removal",
        lambda project: target_property(project, TRANSFORM_EFFECTOR, "tx")["type"]
        == "constant",
    )
    restored = target_property(removed["project"], TRANSFORM_EFFECTOR, "tx")[
        "properties"
    ]["value"]
    if not isinstance(restored, (int, float)):
        raise QaFailure("last-key removal did not restore a numeric constant")
    approximately(float(restored), edited_tx, "restored Transform constant")
    assert_history_delta(before_remove, removed, 1, "last Transform keyframe removal")
    add_keyframe(client, tx_key, TRANSFORM_EFFECTOR, "tx")

    # Other plugin categories use the same direct Node property/keyframe IDs;
    # no embedded Style/Effect/Effector/Decorator collection is consulted.
    for operation_id, property_name in (
        (TEXT_FILL, "color"),
        (BACKPLATE_DECORATOR, "padding"),
        (BLUR_EFFECT, "sigma_x"),
    ):
        expose_operation_property(client, operation_id, property_name)
        add_keyframe(
            client,
            "inspector.keyframe.node:{}:{}".format(operation_id, property_name),
            operation_id,
            property_name,
        )

    # Graph Editor edits a direct operation Node. Selecting Clip A2 would map
    # the graph to its Blur output, so select Transform by its real Node header.
    transform_header = "node_editor.node_header:" + TRANSFORM_EFFECTOR
    BASE.reveal_node_editor_component(client, transform_header)
    client.click_component(transform_header)
    client.wait_until(
        "Transform operation Node selection",
        lambda: client.state()
        if client.state()["editor"]["selection"]["last_selected_entity_id"]
        == TRANSFORM_EFFECTOR
        else None,
    )
    client.wait_component("inspector.owner.node:" + TRANSFORM_EFFECTOR)

    client.click_component("dock.tab:graph_editor")
    client.wait_component("graph.canvas")
    current = client.state()
    tx_frame = only_keyframe(current["project"], TRANSFORM_EFFECTOR, "tx")
    tx_property = "node:tx"
    tx_point = graph_keyframe_component(tx_property, tx_frame["id"])
    client.wait_component(tx_point)
    modified_click(client, tx_point)

    client.wait_until(
        "direct Transform Graph keyframe selection",
        lambda: client.state()
        if client.state()["editor"]["graph"]["selected_keyframes"]
        == [{"property": tx_property, "keyframe_id": tx_frame["id"]}]
        else None,
    )

    # Add a second keyframe with the Graph's actual curve double-click path.
    # The component center is one sampled point on the rendered tx curve and
    # deliberately avoids every existing keyframe hit rectangle.
    curve_id = "graph.curve_hit." + tx_property
    _, curve_component = wait_component_settled(client, curve_id)
    curve_metadata = curve_component["metadata"]
    graph_add_before = client.state()
    existing_ids = {
        keyframe["id"]
        for keyframe in property_keyframes(
            graph_add_before["project"], TRANSFORM_EFFECTOR, "tx"
        )
    }
    coordinate_double_click(client, curve_id)
    graph_added = client.wait_project(
        "Graph curve double-click keyframe add",
        lambda project: len(property_keyframes(project, TRANSFORM_EFFECTOR, "tx"))
        == 2,
    )
    assert_history_delta(graph_add_before, graph_added, 1, "Graph double-click add")
    added_keys = [
        keyframe
        for keyframe in property_keyframes(
            graph_added["project"], TRANSFORM_EFFECTOR, "tx"
        )
        if keyframe["id"] not in existing_ids
    ]
    if len(added_keys) != 1:
        raise QaFailure("Graph double-click did not add exactly one new keyframe")
    added_key = added_keys[0]
    approximately(
        float(added_key["time"]),
        float(curve_metadata["source_time"]),
        "Graph added source time",
        tolerance=2.0e-3,
    )
    approximately(
        float(added_key["value"]),
        float(curve_metadata["value"]),
        "Graph added curve value",
        tolerance=2.0e-3,
    )
    client.wait_preview_render_after(graph_add_before, "Graph keyframe add")

    # Context-menu easing is also a real coordinate path. Easing belongs to
    # the segment's start keyframe, so select the earlier of the two keys.
    segment_start = min(
        property_keyframes(graph_added["project"], TRANSFORM_EFFECTOR, "tx"),
        key=lambda keyframe: float(keyframe["time"]),
    )
    cubic_before, _ = set_graph_easing(
        client,
        tx_property,
        segment_start["id"],
        "ease_in_out_cubic",
        "EaseInOutCubic",
    )
    client.wait_preview_render_after(cubic_before, "Graph cubic easing")

    # Preserve the existing coordinate drag regression, now with two keys in
    # the same authoritative property.
    graph_before = client.state()
    preview_before = graph_before["editor"]["preview"]
    tx_before_key = keyframe_by_id(
        graph_before["project"], TRANSFORM_EFFECTOR, "tx", tx_frame["id"]
    )
    _, canvas = client.component("graph.canvas")
    zoom_x = float(canvas["metadata"]["zoom_x"])
    zoom_y = float(canvas["metadata"]["zoom_y"])
    dx = 40.0
    dy = -12.0
    coordinate_drag(client, tx_point, dx, dy, steps=12)
    graph_after = client.wait_project(
        "direct Transform Graph coordinate drag",
        lambda project: keyframe_by_id(
            project, TRANSFORM_EFFECTOR, "tx", tx_frame["id"]
        )["time"]
        != tx_before_key["time"],
    )
    tx_after_key = keyframe_by_id(
        graph_after["project"], TRANSFORM_EFFECTOR, "tx", tx_frame["id"]
    )
    approximately(
        float(tx_after_key["time"]) - float(tx_before_key["time"]),
        dx / zoom_x,
        "Transform Graph time delta",
    )
    approximately(
        float(tx_after_key["value"]) - float(tx_before_key["value"]),
        -dy / zoom_y,
        "Transform Graph value delta",
    )
    assert_history_delta(graph_before, graph_after, 1, "Transform Graph drag")
    if graph_after["editor"]["graph"]["drag"] is not None:
        raise QaFailure("Graph drag transaction remained active after pointer release")
    client.wait_preview_change(
        preview_before["pixel_hash"], preview_before["render_revision"]
    )

    # Inspector metadata must independently reflect the Project value produced
    # by Graph Editor for the same direct operation Node.
    direct_tx_control = "inspector.property.node:{}:tx".format(TRANSFORM_EFFECTOR)
    ensure_in_inspector(client, direct_tx_control)
    wait_inspector_numeric_value(
        client,
        direct_tx_control,
        float(tx_after_key["value"]),
        "Inspector authoritative value",
    )

    # Move the playhead to a non-symmetric point within the two-key segment.
    # Linear and EaseInOutCubic differ at 30%, so both Inspector metadata and
    # the rendered Preview hash independently expose the easing mutation.
    keys = sorted(
        property_keyframes(graph_after["project"], TRANSFORM_EFFECTOR, "tx"),
        key=lambda keyframe: float(keyframe["time"]),
    )
    start_key, end_key = keys
    if abs(float(end_key["value"]) - float(start_key["value"])) < 1.0e-4:
        raise QaFailure("Graph move did not create a visible easing value range")
    if start_key["easing"] != "EaseInOutCubic":
        cubic_before, _ = set_graph_easing(
            client,
            tx_property,
            start_key["id"],
            "ease_in_out_cubic",
            "EaseInOutCubic",
        )
        client.wait_preview_render_after(cubic_before, "segment cubic easing")

    fraction = 0.3
    source_time = float(start_key["time"]) + (
        float(end_key["time"]) - float(start_key["time"])
    ) * fraction
    easing_global_time = source_time + 1.0
    seek_before = client.state()
    click_graph_time(client, easing_global_time)
    cubic_render = client.wait_preview_render_after(seek_before, "Graph ruler seek")
    cubic_expected = numeric_easing_value(
        float(start_key["value"]),
        float(end_key["value"]),
        fraction,
        "EaseInOutCubic",
    )
    wait_inspector_numeric_value(
        client,
        direct_tx_control,
        cubic_expected,
        "Inspector cubic value",
    )

    set_graph_easing(
        client,
        tx_property,
        start_key["id"],
        "linear",
        "Linear",
    )
    linear_render = client.wait_preview_change(
        cubic_render["editor"]["preview"]["pixel_hash"],
        cubic_render["editor"]["preview"]["render_revision"],
    )
    linear_expected = numeric_easing_value(
        float(start_key["value"]),
        float(end_key["value"]),
        fraction,
        "Linear",
    )
    wait_inspector_numeric_value(
        client,
        direct_tx_control,
        linear_expected,
        "Inspector linear value",
    )

    undo_before = client.state()
    shortcut_undo_redo(client)
    cubic_undo = client.wait_project(
        "Undo Graph linear easing",
        lambda project: keyframe_by_id(
            project, TRANSFORM_EFFECTOR, "tx", start_key["id"]
        )["easing"]
        == "EaseInOutCubic",
    )
    assert_undo_transition(undo_before, cubic_undo, "Graph easing Undo")
    cubic_undo_render = client.wait_preview_change(
        linear_render["editor"]["preview"]["pixel_hash"],
        linear_render["editor"]["preview"]["render_revision"],
    )
    wait_inspector_numeric_value(
        client,
        direct_tx_control,
        cubic_expected,
        "Inspector easing Undo value",
    )

    redo_before = client.state()
    shortcut_undo_redo(client, redo=True)
    linear_redo = client.wait_project(
        "Redo Graph linear easing",
        lambda project: keyframe_by_id(
            project, TRANSFORM_EFFECTOR, "tx", start_key["id"]
        )["easing"]
        == "Linear",
    )
    assert_redo_transition(redo_before, linear_redo, "Graph easing Redo")
    linear_redo_render = client.wait_preview_change(
        cubic_undo_render["editor"]["preview"]["pixel_hash"],
        cubic_undo_render["editor"]["preview"]["render_revision"],
    )

    set_graph_easing(
        client,
        tx_property,
        start_key["id"],
        "ease_in_out_cubic",
        "EaseInOutCubic",
    )
    cubic_again_render = client.wait_preview_change(
        linear_redo_render["editor"]["preview"]["pixel_hash"],
        linear_redo_render["editor"]["preview"]["render_revision"],
    )

    # Delete the key added by double-click through its rendered context item.
    # Undo/Redo must restore/remove that exact stable KeyframeId.
    added_point = graph_keyframe_component(tx_property, added_key["id"])
    wait_component_settled(client, added_point)
    client.click_component(added_point, button="secondary")
    delete_item = "graph.keyframe_menu.delete:" + added_key["id"]
    client.wait_component(delete_item)
    delete_before = client.state()
    client.click_component(delete_item)
    deleted = client.wait_project(
        "Graph context keyframe delete",
        lambda project: keyframe_by_id(
            project, TRANSFORM_EFFECTOR, "tx", added_key["id"]
        )
        is None,
    )
    assert_history_delta(delete_before, deleted, 1, "Graph keyframe delete")
    delete_render = client.wait_preview_change(
        cubic_again_render["editor"]["preview"]["pixel_hash"],
        cubic_again_render["editor"]["preview"]["render_revision"],
    )

    delete_undo_before = client.state()
    shortcut_undo_redo(client)
    delete_undone = client.wait_project(
        "Undo Graph keyframe delete",
        lambda project: keyframe_by_id(
            project, TRANSFORM_EFFECTOR, "tx", added_key["id"]
        )
        is not None,
    )
    assert_undo_transition(delete_undo_before, delete_undone, "Graph delete Undo")
    delete_undo_render = client.wait_preview_change(
        delete_render["editor"]["preview"]["pixel_hash"],
        delete_render["editor"]["preview"]["render_revision"],
    )

    delete_redo_before = client.state()
    shortcut_undo_redo(client, redo=True)
    delete_redone = client.wait_project(
        "Redo Graph keyframe delete",
        lambda project: keyframe_by_id(
            project, TRANSFORM_EFFECTOR, "tx", added_key["id"]
        )
        is None,
    )
    assert_redo_transition(delete_redo_before, delete_redone, "Graph delete Redo")
    client.wait_preview_change(
        delete_undo_render["editor"]["preview"]["pixel_hash"],
        delete_undo_render["editor"]["preview"]["render_revision"],
    )

    final_tx_key = only_keyframe(delete_redone["project"], TRANSFORM_EFFECTOR, "tx")
    if final_tx_key["id"] != tx_after_key["id"]:
        raise QaFailure("Graph delete removed the pre-existing Transform keyframe")

    # The fixture Clip starts at global 1s with trim=0/stretch=1, so the dialog
    # maps source time to global time by exactly +1.
    client.click_component(tx_point, button="secondary")
    edit_menu = "graph.keyframe_menu.edit:" + tx_after_key["id"]
    client.wait_component(edit_menu)
    client.click_component(edit_menu)
    dialog_state = client.wait_until(
        "keyframe dialog",
        lambda: client.state()
        if client.state()["editor"]["keyframe_dialog"]["is_open"]
        else None,
    )
    approximately(
        float(dialog_state["editor"]["keyframe_dialog"]["global_time"]),
        float(tx_after_key["time"]) + 1.0,
        "dialog global time",
    )
    dialog_before = client.state()
    coordinate_drag(client, "keyframe_dialog.time", 20.0, 0.0)
    dialog_after = client.wait_project(
        "dialog time edit",
        lambda project: only_keyframe(project, TRANSFORM_EFFECTOR, "tx")["time"]
        != tx_after_key["time"],
    )
    source_time = float(
        only_keyframe(dialog_after["project"], TRANSFORM_EFFECTOR, "tx")["time"]
    )
    global_time = float(dialog_after["editor"]["keyframe_dialog"]["global_time"])
    approximately(source_time, global_time - 1.0, "dialog global/local mapping")
    assert_history_delta(dialog_before, dialog_after, 1, "keyframe dialog drag")
    client.click_component("keyframe_dialog.close")
    client.wait_until(
        "keyframe dialog close",
        lambda: client.state()
        if not client.state()["editor"]["keyframe_dialog"]["is_open"]
        else None,
    )

    # Finish through the real Preview hit-testing path.
    preview_before = client.state()
    selection_before = preview_before["editor"]["selection"]
    client.wait_component("preview.canvas")
    client.key("space", True)
    client.drag_component_by("preview.canvas", 54.0, 32.0, steps=12)
    client.key("space", False)
    final = client.wait_until(
        "final Preview hand-tool pan",
        lambda: client.state()
        if client.state()["editor"]["preview"]["pan"]
        != preview_before["editor"]["preview"]["pan"]
        else None,
    )
    if final["editor"]["preview"]["primary_gesture"] != "Idle":
        raise QaFailure("Preview gesture owner did not return to Idle")
    if final["editor"]["selection"] != selection_before:
        raise QaFailure("final Preview pan changed the editor selection")

    print("[qa-keyframe-e2e] direct operation Inspector/Graph/dialog E2E passed")
    return {
        "ok": True,
        "initial_frame": initial["frame"],
        "final_frame": final["frame"],
        "final_history": final["history"],
        "final_preview": final["editor"]["preview"],
        "final_selection": final["editor"]["selection"],
        "graph_crud": {
            "property": tx_property,
            "curve_component_id": curve_id,
            "existing_keyframe_id": tx_frame["id"],
            "added_keyframe_id": added_key["id"],
            "added_source_time": added_key["time"],
            "moved_source_time": tx_after_key["time"],
            "sample_global_time": easing_global_time,
            "cubic_inspector_value": cubic_expected,
            "linear_inspector_value": linear_expected,
            "cubic_preview_hash": cubic_again_render["editor"]["preview"][
                "pixel_hash"
            ],
            "linear_preview_hash": linear_redo_render["editor"]["preview"][
                "pixel_hash"
            ],
            "delete_redo_removed_added_keyframe": keyframe_by_id(
                delete_redone["project"],
                TRANSFORM_EFFECTOR,
                "tx",
                added_key["id"],
            )
            is None,
        },
        "actions": client.evidence,
        "health_frame": health["frame"],
    }


def parse_args():
    parser = argparse.ArgumentParser()
    parser.add_argument("--base-url", default=None)
    parser.add_argument("--spawn", action="store_true")
    parser.add_argument(
        "--evidence",
        default="target/qa-keyframe-e2e-evidence.json",
    )
    parser.add_argument("--timeout", type=float, default=12.0)
    return parser.parse_args()


def main():
    args = parse_args()
    process = None
    port = free_port() if args.spawn else 39091
    base_url = args.base_url or "http://127.0.0.1:{}".format(port)
    try:
        if args.spawn:
            environment = os.environ.copy()
            environment["RUVIE_QA_PORT"] = str(port)
            environment["RUVIE_QA_FIXTURE"] = "node_editor_e2e"
            process = subprocess.Popen(
                ["cargo", "run", "-p", "app", "--locked"],
                env=environment,
                start_new_session=True,
            )
        result = run_suite(QaClient(base_url, args.timeout))
        result["run_id"] = os.environ.get("RUVIE_QA_RUN_ID")
        evidence_path = os.path.abspath(args.evidence)
        os.makedirs(os.path.dirname(evidence_path), exist_ok=True)
        with open(evidence_path, "w", encoding="utf-8") as output:
            json.dump(result, output, ensure_ascii=False, indent=2)
            output.write("\n")
        print("[qa-keyframe-e2e] PASS; evidence: {}".format(evidence_path))
        return 0
    except (QaFailure, AssertionError, KeyError, IndexError, TypeError) as error:
        print("[qa-keyframe-e2e] FAIL: {}".format(error), file=sys.stderr)
        return 1
    finally:
        if process is not None:
            if process.poll() is None:
                try:
                    os.killpg(process.pid, signal.SIGTERM)
                except ProcessLookupError:
                    pass
            try:
                process.wait(timeout=5.0)
            except subprocess.TimeoutExpired:
                try:
                    os.killpg(process.pid, signal.SIGKILL)
                except ProcessLookupError:
                    pass
                process.wait(timeout=2.0)


if __name__ == "__main__":
    sys.exit(main())
