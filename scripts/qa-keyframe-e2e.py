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

TEXT = "00000000-0000-0000-0000-000000000403"
TEXT_FILL = "00000000-0000-0000-0000-000000000601"
TRANSFORM_EFFECTOR = "00000000-0000-0000-0000-000000000501"
BACKPLATE_DECORATOR = "00000000-0000-0000-0000-000000000503"
BLUR_EFFECT = "00000000-0000-0000-0000-000000000504"


def node(project):
    return project["nodes"][TEXT]


def instance(project, collection, instance_id):
    return next(item for item in node(project)[collection] if item["id"] == instance_id)


def target_property(project, target, property_name):
    kind, instance_id = target
    if kind == "direct":
        owner = node(project)
    else:
        collection = {
            "effect": "effects",
            "style": "styles",
            "effector": "effectors",
            "decorator": "decorators",
        }[kind]
        owner = instance(project, collection, instance_id)
    return owner["properties"][property_name]


def only_keyframe(project, target, property_name):
    prop = target_property(project, target, property_name)
    keyframes = prop["properties"]["keyframes"]
    if len(keyframes) != 1:
        raise QaFailure(
            "{}.{} has {} keyframes, expected one".format(
                target, property_name, len(keyframes)
            )
        )
    return keyframes[0]


def numeric_keyframe_value(project, target, property_name):
    value = only_keyframe(project, target, property_name)["value"]
    if not isinstance(value, (int, float)):
        raise QaFailure("{}.{} is not numeric: {!r}".format(target, property_name, value))
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
            "steps": steps,
            "button": "primary",
        },
        {
            "component_id": component_id,
            "component_frame": snapshot["frame"],
            "component_rect_points": component["rect_points"],
        },
    )


def wait_component_settled(client, component_id, consecutive_frames=2):
    observed = {"frame": None, "rect": None, "stable": 0}

    def settled():
        try:
            snapshot, component = client.component(component_id)
        except QaFailure:
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


def expand_item(client, header_id, child_id):
    ensure_in_inspector(client, header_id)
    try:
        client.component(child_id)
        return
    except QaFailure:
        pass
    wait_component_settled(client, header_id)
    client.click_component(header_id)
    client.wait_component(child_id)


def add_keyframe(client, control_id, target, property_name):
    ensure_in_inspector(client, control_id)
    wait_component_settled(client, control_id)
    before = client.state()
    client.click_component(control_id)
    after = client.wait_project(
        "{} {} keyframe add".format(target[0], property_name),
        lambda project: target_property(project, target, property_name)["type"]
        == "keyframe",
    )
    assert_history_delta(before, after, 1, "{} keyframe add".format(target[0]))
    return after


def approximately(actual, expected, label, tolerance=1.0e-4):
    if abs(actual - expected) > tolerance:
        raise QaFailure(
            "{} was {}, expected {} ± {}".format(label, actual, expected, tolerance)
        )


def run_suite(client):
    health = client.wait_health()
    initial = client.state()
    if initial["project"].get("name") != "RuViE QA E2E":
        raise QaFailure("start with RUVIE_QA_FIXTURE=node_editor_e2e")

    wait_component_settled(client, "dock.tab:node_editor")
    client.click_component("dock.tab:node_editor")
    client.wait_until(
        "Node Editor activation",
        lambda: client.state()
        if "Node Editor" in client.state()["dock"]["active_tabs"]
        else None,
    )
    node_header = "node_editor.node_header:" + TEXT
    wait_component_settled(client, node_header)
    client.click_component(node_header)
    client.wait_until(
        "Text Node coordinate selection",
        lambda: client.state()
        if client.state()["editor"]["selection"]["last_selected_entity_id"] == TEXT
        else None,
    )
    client.wait_component("inspector.owner.node:" + TEXT)

    direct = ("direct", None)
    style = ("style", TEXT_FILL)
    effect = ("effect", BLUR_EFFECT)
    effector = ("effector", TRANSFORM_EFFECTOR)
    decorator = ("decorator", BACKPLATE_DECORATOR)

    # Inspector add/update/remove: deleting the final key must preserve the
    # edited numeric value as a typed constant.
    opacity_key = "inspector.keyframe.node:{}:opacity".format(TEXT)
    add_keyframe(client, opacity_key, direct, "opacity")
    opacity_control = "inspector.property.node:{}:opacity".format(TEXT)
    ensure_in_inspector(client, opacity_control)
    before_update = client.state()
    old_opacity = numeric_keyframe_value(before_update["project"], direct, "opacity")
    coordinate_drag(client, opacity_control, -18.0, 0.0)
    updated = client.wait_project(
        "direct keyframe value update",
        lambda project: numeric_keyframe_value(project, direct, "opacity") != old_opacity,
    )
    edited_opacity = numeric_keyframe_value(updated["project"], direct, "opacity")
    assert_history_delta(before_update, updated, 1, "direct keyframe value update")
    before_remove = client.state()
    client.click_component(opacity_key)
    removed = client.wait_project(
        "last direct keyframe removal",
        lambda project: target_property(project, direct, "opacity")["type"]
        == "constant",
    )
    restored = target_property(removed["project"], direct, "opacity")["properties"]["value"]
    if not isinstance(restored, (int, float)):
        raise QaFailure("last-key removal did not restore a numeric constant")
    approximately(float(restored), edited_opacity, "restored constant")
    assert_history_delta(before_remove, removed, 1, "last keyframe removal")

    style_key = "inspector.keyframe.node:{}.style:{}:color".format(TEXT, TEXT_FILL)
    expand_item(
        client,
        "inspector.style.node:{}:{}".format(TEXT, TEXT_FILL),
        style_key,
    )
    add_keyframe(client, style_key, style, "color")

    tx_key = "inspector.keyframe.node:{}.effector:{}:tx".format(
        TEXT, TRANSFORM_EFFECTOR
    )
    add_keyframe(client, tx_key, effector, "tx")
    padding_key = "inspector.keyframe.node:{}.decorator:{}:padding".format(
        TEXT, BACKPLATE_DECORATOR
    )
    add_keyframe(client, padding_key, decorator, "padding")

    sigma_key = "inspector.keyframe.node:{}.effect:{}:sigma_x".format(
        TEXT, BLUR_EFFECT
    )
    expand_item(
        client,
        "inspector.effect.node:{}:{}".format(TEXT, BLUR_EFFECT),
        sigma_key,
    )
    add_keyframe(client, sigma_key, effect, "sigma_x")

    # Update one nested keyframe through Inspector and require the real Preview
    # to observe the same authoritative Project mutation.
    tx_control = "inspector.property.node:{}.effector:{}:tx".format(
        TEXT, TRANSFORM_EFFECTOR
    )
    ensure_in_inspector(client, tx_control)
    tx_before = client.state()
    tx_preview = tx_before["editor"]["preview"]
    old_tx = numeric_keyframe_value(tx_before["project"], effector, "tx")
    coordinate_drag(client, tx_control, 22.0, 0.0)
    tx_updated = client.wait_project(
        "Effector keyframe update",
        lambda project: numeric_keyframe_value(project, effector, "tx") != old_tx,
    )
    assert_history_delta(tx_before, tx_updated, 1, "Effector keyframe update")
    client.wait_preview_change(tx_preview["pixel_hash"], tx_preview["render_revision"])

    # Graph multi-selection and an actual coordinate drag. Both points share
    # one cumulative screen delta, and the complete gesture commits once.
    client.click_component("dock.tab:graph_editor")
    client.wait_component("graph.canvas")
    current = client.state()
    tx_frame = only_keyframe(current["project"], effector, "tx")
    padding_frame = only_keyframe(current["project"], decorator, "padding")
    tx_property = "effector:{}:tx".format(TRANSFORM_EFFECTOR)
    padding_property = "decorator:{}:padding".format(BACKPLATE_DECORATOR)
    tx_point = "graph.keyframe.{}:{}".format(tx_property, tx_frame["id"])
    padding_point = "graph.keyframe.{}:{}".format(
        padding_property, padding_frame["id"]
    )
    client.wait_component(tx_point)
    client.wait_component(padding_point)
    modified_click(client, tx_point)

    def only_tx_selected():
        state = client.state()
        selected = state["editor"]["graph"]["selected_keyframes"]
        if len(selected) == 1 and selected[0] == {
            "property": tx_property,
            "keyframe_id": tx_frame["id"],
        }:
            return state
        return None

    client.wait_until(
        "first Graph keyframe selected",
        only_tx_selected,
    )
    modified_click(client, padding_point, shift=True)
    client.wait_until(
        "two Graph keyframes selected",
        lambda: client.state()
        if len(client.state()["editor"]["graph"]["selected_keyframes"]) == 2
        else None,
    )

    graph_before = client.state()
    preview_before = graph_before["editor"]["preview"]
    tx_before_key = only_keyframe(graph_before["project"], effector, "tx")
    padding_before_key = only_keyframe(graph_before["project"], decorator, "padding")
    _, canvas = client.component("graph.canvas")
    zoom_x = float(canvas["metadata"]["zoom_x"])
    zoom_y = float(canvas["metadata"]["zoom_y"])
    dx = 40.0
    dy = -12.0
    coordinate_drag(client, tx_point, dx, dy, steps=12)
    graph_after = client.wait_project(
        "Graph multi-keyframe coordinate drag",
        lambda project: only_keyframe(project, effector, "tx")["time"]
        != tx_before_key["time"],
    )
    tx_after_key = only_keyframe(graph_after["project"], effector, "tx")
    padding_after_key = only_keyframe(graph_after["project"], decorator, "padding")
    expected_time_delta = dx / zoom_x
    expected_value_delta = -dy / zoom_y
    approximately(
        float(tx_after_key["time"]) - float(tx_before_key["time"]),
        expected_time_delta,
        "Effector Graph time delta",
    )
    approximately(
        float(padding_after_key["time"]) - float(padding_before_key["time"]),
        expected_time_delta,
        "Decorator Graph time delta",
    )
    approximately(
        float(tx_after_key["value"]) - float(tx_before_key["value"]),
        expected_value_delta,
        "Effector Graph value delta",
    )
    approximately(
        float(padding_after_key["value"]) - float(padding_before_key["value"]),
        expected_value_delta,
        "Decorator Graph value delta",
    )
    assert_history_delta(graph_before, graph_after, 1, "Graph multi-keyframe drag")
    if graph_after["editor"]["graph"]["drag"] is not None:
        raise QaFailure("Graph drag transaction remained active after pointer release")
    client.wait_preview_change(preview_before["pixel_hash"], preview_before["render_revision"])

    # Inspector must display the value produced by Graph from the same Project.
    ensure_in_inspector(client, tx_control)
    inspector_component = client.wait_until(
        "Inspector value reflecting Graph",
        lambda: client.component(tx_control)[1]
        if abs(
            float(client.component(tx_control)[1]["metadata"]["value"])
            - float(tx_after_key["value"])
        )
        < 1.0e-4
        else None,
    )
    approximately(
        float(inspector_component["metadata"]["value"]),
        float(tx_after_key["value"]),
        "Inspector authoritative value",
    )

    # Open the real point context menu and edit dialog. The fixture Clip starts
    # at global 1s with trim=0/stretch=1, so source == global-1 exactly.
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
        lambda project: only_keyframe(project, effector, "tx")["time"]
        != tx_after_key["time"],
    )
    source_time = float(only_keyframe(dialog_after["project"], effector, "tx")["time"])
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

    # Keep the final interaction on the real Preview canvas. Space engages
    # the hand tool, while the multi-frame coordinate drag must traverse
    # egui hit testing and return its gesture owner to Idle on release.
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

    print("[qa-keyframe-e2e] Inspector/Graph/dialog/final Preview coordinate E2E passed")
    return {
        "ok": True,
        "initial_frame": initial["frame"],
        "final_frame": final["frame"],
        "final_history": final["history"],
        "final_preview": final["editor"]["preview"],
        "final_selection": final["editor"]["selection"],
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
