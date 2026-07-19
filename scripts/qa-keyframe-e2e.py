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
    prop = target_property(project, node_id, property_name)
    keyframes = prop["properties"]["keyframes"]
    if len(keyframes) != 1:
        raise QaFailure(
            "{}.{} has {} keyframes, expected one".format(
                node_id, property_name, len(keyframes)
            )
        )
    return keyframes[0]


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
    preview_before = before_update["editor"]["preview"]
    coordinate_drag(client, tx_control, 22.0, 0.0)
    updated = client.wait_project(
        "Transform direct keyframe value update",
        lambda project: numeric_keyframe_value(project, TRANSFORM_EFFECTOR, "tx")
        != old_tx,
    )
    edited_tx = numeric_keyframe_value(updated["project"], TRANSFORM_EFFECTOR, "tx")
    assert_history_delta(before_update, updated, 1, "Transform keyframe value update")
    client.wait_preview_change(
        preview_before["pixel_hash"], preview_before["render_revision"]
    )

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
    tx_property = "direct:tx"
    tx_point = "graph.keyframe.{}:{}".format(tx_property, tx_frame["id"])
    client.wait_component(tx_point)
    modified_click(client, tx_point)

    client.wait_until(
        "direct Transform Graph keyframe selection",
        lambda: client.state()
        if client.state()["editor"]["graph"]["selected_keyframes"]
        == [{"property": tx_property, "keyframe_id": tx_frame["id"]}]
        else None,
    )

    graph_before = client.state()
    preview_before = graph_before["editor"]["preview"]
    tx_before_key = only_keyframe(graph_before["project"], TRANSFORM_EFFECTOR, "tx")
    _, canvas = client.component("graph.canvas")
    zoom_x = float(canvas["metadata"]["zoom_x"])
    zoom_y = float(canvas["metadata"]["zoom_y"])
    dx = 40.0
    dy = -12.0
    coordinate_drag(client, tx_point, dx, dy, steps=12)
    graph_after = client.wait_project(
        "direct Transform Graph coordinate drag",
        lambda project: only_keyframe(project, TRANSFORM_EFFECTOR, "tx")["time"]
        != tx_before_key["time"],
    )
    tx_after_key = only_keyframe(graph_after["project"], TRANSFORM_EFFECTOR, "tx")
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
    inspector_component = client.wait_until(
        "Inspector value reflecting Graph",
        lambda: client.component(direct_tx_control)[1]
        if abs(
            float(client.component(direct_tx_control)[1]["metadata"]["value"])
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
