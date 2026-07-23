#!/usr/bin/env python3
"""Fast coordinate E2E for the Node Editor's 100x overview zoom.

The suite uses only the public loopback QA bridge. Zoom is a real command-
modified mouse-wheel event at the latest canvas center, and pan is a real
primary-button drag from a freshly queried unobstructed screen coordinate.
No test-only transform command or mirrored Project model is involved.
"""

import argparse
import importlib.util
import json
import math
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
free_port = BASE.free_port

CANVAS_ID = "node_editor.canvas"
NODE_EDITOR_TAB_ID = "dock.tab:node_editor"
NODE_EDITOR_TAB_LABEL = "Node Editor"
TRANSFORM_STATIC_FIELDS = (
    "scale",
    "min_scale",
    "max_scale",
    "detail_enabled",
    "port_interaction_enabled",
    "resize_interaction_enabled",
)


def finite_number(value, field):
    if isinstance(value, bool) or not isinstance(value, (int, float)):
        raise QaFailure("{} is not numeric: {!r}".format(field, value))
    value = float(value)
    if not math.isfinite(value):
        raise QaFailure("{} is not finite: {!r}".format(field, value))
    return value


def canvas_transform(component):
    """Read and validate the final Snarl transform published for one frame."""
    metadata = component.get("metadata")
    if not isinstance(metadata, dict):
        raise QaFailure("{} has no transform metadata".format(CANVAS_ID))
    translation = metadata.get("translation")
    if not isinstance(translation, dict):
        raise QaFailure("{} metadata has no translation".format(CANVAS_ID))
    transform = {
        "scale": finite_number(metadata.get("scale"), "scale"),
        "translation": {
            "x": finite_number(translation.get("x"), "translation.x"),
            "y": finite_number(translation.get("y"), "translation.y"),
        },
        "min_scale": finite_number(metadata.get("min_scale"), "min_scale"),
        "max_scale": finite_number(metadata.get("max_scale"), "max_scale"),
    }
    for field in (
        "detail_enabled",
        "port_interaction_enabled",
        "resize_interaction_enabled",
    ):
        value = metadata.get(field)
        if not isinstance(value, bool):
            raise QaFailure("{} is not boolean: {!r}".format(field, value))
        transform[field] = value
    if not 0.0 < transform["min_scale"] <= transform["max_scale"]:
        raise QaFailure("invalid Node Editor scale bounds: {!r}".format(transform))
    if not transform["min_scale"] <= transform["scale"] <= transform["max_scale"]:
        raise QaFailure("Node Editor scale escaped its bounds: {!r}".format(transform))
    return transform


def point_in_rect(point, rect, padding=0.0):
    return (
        rect["min_x"] - padding <= point["x"] <= rect["max_x"] + padding
        and rect["min_y"] - padding <= point["y"] <= rect["max_y"] + padding
    )


def find_primary_pan_gesture(snapshot, dx=112.0, dy=64.0):
    """Choose a real canvas coordinate that is not owned by Node chrome."""
    components = {component["id"]: component for component in snapshot["components"]}
    canvas = components.get(CANVAS_ID)
    if canvas is None:
        raise QaFailure("{} is absent in frame {}".format(CANVAS_ID, snapshot["frame"]))
    rect = canvas["rect_points"]
    margin = 12.0
    obstacles = []
    obstacle_prefixes = (
        "node_editor.node:",
        "node_editor.node_header:",
        "node_editor.port.",
        "node_editor.container_header.",
        "node_editor.container_port.",
        "node_editor.resize_edge.",
    )
    for component in snapshot["components"]:
        item_rect = component.get("rect_points", {})
        if (
            component.get("visible", False)
            and item_rect.get("width", 0.0) > 0.0
            and item_rect.get("height", 0.0) > 0.0
            and component["id"].startswith(obstacle_prefixes)
        ):
            obstacles.append(item_rect)

    for x_fraction, y_fraction in (
        (0.22, 0.24),
        (0.22, 0.72),
        (0.48, 0.24),
        (0.70, 0.72),
        (0.50, 0.50),
    ):
        start = {
            "x": rect["min_x"] + rect["width"] * x_fraction,
            "y": rect["min_y"] + rect["height"] * y_fraction,
        }
        end = {"x": start["x"] + dx, "y": start["y"] + dy}
        if not (
            rect["min_x"] + margin <= start["x"] <= rect["max_x"] - margin
            and rect["min_y"] + margin <= start["y"] <= rect["max_y"] - margin
            and rect["min_x"] + margin <= end["x"] <= rect["max_x"] - margin
            and rect["min_y"] + margin <= end["y"] <= rect["max_y"] - margin
        ):
            continue
        if all(not point_in_rect(start, obstacle, 6.0) for obstacle in obstacles):
            return start, end
    raise QaFailure("no unobstructed primary-pan origin in Node Editor canvas")


def assert_header_interaction_metadata(snapshot, expected_move_enabled):
    """Selection stays available when header movement is gated by LOD."""
    header_prefixes = (
        "node_editor.node_header:",
        "node_editor.container_header.",
    )
    headers = [
        component
        for component in snapshot["components"]
        if component["id"].startswith(header_prefixes)
        and component.get("visible", False)
    ]
    if not headers:
        raise QaFailure(
            "frame {} has no visible Node/Group header metadata".format(snapshot["frame"])
        )
    for component in headers:
        metadata = component.get("metadata") or {}
        if metadata.get("selection_enabled") is not True:
            raise QaFailure("{} disabled semantic selection".format(component["id"]))
        if metadata.get("move_enabled") is not expected_move_enabled:
            raise QaFailure(
                "{} move_enabled={!r}, expected {!r}".format(
                    component["id"],
                    metadata.get("move_enabled"),
                    expected_move_enabled,
                )
            )

    move_components = [
        component
        for component in snapshot["components"]
        if component["id"].startswith("node_editor.container_move_header.")
        and component.get("visible", False)
    ]
    if not move_components:
        raise QaFailure(
            "frame {} has no visible Group move component".format(snapshot["frame"])
        )
    for component in move_components:
        metadata = component.get("metadata") or {}
        if component.get("enabled") is not expected_move_enabled:
            raise QaFailure(
                "{} component enabled={!r}, expected {!r}".format(
                    component["id"], component.get("enabled"), expected_move_enabled
                )
            )
        if metadata.get("selection_enabled") is not True:
            raise QaFailure("{} omitted selection_enabled".format(component["id"]))
        if metadata.get("move_enabled") is not expected_move_enabled:
            raise QaFailure("{} metadata disagrees with component gate".format(component["id"]))
    return {
        "header_ids": [component["id"] for component in headers],
        "move_component_ids": [component["id"] for component in move_components],
        "move_enabled": expected_move_enabled,
    }


def assert_minimum_zoom(before, zoomed, tolerance=1.0e-5):
    if not zoomed["scale"] < before["scale"]:
        raise QaFailure(
            "command-wheel did not reduce scale: {} -> {}".format(
                before["scale"], zoomed["scale"]
            )
        )
    if abs(zoomed["scale"] - zoomed["min_scale"]) > tolerance:
        raise QaFailure(
            "100x zoom did not clamp at min_scale: {!r}".format(zoomed)
        )
    for field in (
        "detail_enabled",
        "port_interaction_enabled",
        "resize_interaction_enabled",
    ):
        if zoomed[field]:
            raise QaFailure("{} remained enabled at overview scale".format(field))


def assert_only_translation_changed(before, after, expected_delta, tolerance=2.5):
    for field in TRANSFORM_STATIC_FIELDS:
        if before[field] != after[field]:
            raise QaFailure(
                "primary pan unexpectedly changed {}: {!r} -> {!r}".format(
                    field, before[field], after[field]
                )
            )
    actual = {
        "x": after["translation"]["x"] - before["translation"]["x"],
        "y": after["translation"]["y"] - before["translation"]["y"],
    }
    if abs(actual["x"] - expected_delta["x"]) > tolerance or abs(
        actual["y"] - expected_delta["y"]
    ) > tolerance:
        raise QaFailure(
            "primary pan translation mismatch: actual={!r}, expected={!r}".format(
                actual, expected_delta
            )
        )
    return actual


def assert_navigation_state_unchanged(initial_state, final_state):
    if final_state["project"] != initial_state["project"]:
        raise QaFailure("Node Editor navigation mutated the authoritative Project")
    if final_state["history"] != initial_state["history"]:
        raise QaFailure("Node Editor navigation created an undo history entry")
    if final_state["editor"]["selection"] != initial_state["editor"]["selection"]:
        raise QaFailure("overview pan selected or deselected a Node or Clip")
    if (
        final_state["editor"]["node_editor"]
        != initial_state["editor"]["node_editor"]
    ):
        raise QaFailure("overview navigation changed Node Editor modal/navigation state")


def wait_canvas(client, description, predicate=None, after_frame=-1):
    def ready():
        try:
            snapshot, component = client.component(CANVAS_ID)
            transform = canvas_transform(component)
        except QaFailure:
            return None
        if snapshot["frame"] <= after_frame:
            return None
        if predicate is not None and not predicate(transform):
            return None
        return snapshot, component, transform

    return client.wait_until(description, ready)


def activate_node_editor_tab(client):
    """Activate the Node Editor through the latest rendered dock-tab rect."""
    client.wait_component_settled(NODE_EDITOR_TAB_ID)
    point = client.click_component(NODE_EDITOR_TAB_ID)

    def active():
        state = client.state()
        return state if NODE_EDITOR_TAB_LABEL in state["dock"]["active_tabs"] else None

    state = client.wait_until("Node Editor dock activation", active)
    if not client.evidence:
        raise QaFailure("Node Editor tab click produced no coordinate evidence")
    action = client.evidence[-1]
    if action.get("endpoint") != "click" or action.get("component_id") != NODE_EDITOR_TAB_ID:
        raise QaFailure("Node Editor tab activation did not use its rendered rectangle")
    return (
        {
            "component_id": NODE_EDITOR_TAB_ID,
            "point": point,
            "action_id": action["action_id"],
            "component_frame": action["component_frame"],
            "component_rect_points": action["component_rect_points"],
            "active_frame": state["frame"],
        },
        state,
    )


def command_scroll_at_canvas_center(client, delta_y, purpose):
    snapshot, component = client.component(CANVAS_ID)
    point = client.point(component["rect_points"])
    client.inject(
        "scroll",
        {
            "x": point["x"],
            "y": point["y"],
            "delta_x": 0.0,
            "delta_y": delta_y,
            "coordinate_space": "points",
            "modifiers": {"command": True},
        },
        {
            "component_id": CANVAS_ID,
            "component_frame": snapshot["frame"],
            "component_rect_points": component["rect_points"],
            "coordinate_reason": purpose,
        },
    )
    return snapshot["frame"], point


def run_suite(client):
    health = client.wait_health()
    BASE.wait_fresh_fixture(client)
    tab_click, initial_state = activate_node_editor_tab(client)
    initial_snapshot, _, initial = wait_canvas(client, "final Node Editor canvas metadata")
    detail_header_evidence = None
    if initial["detail_enabled"]:
        detail_header_evidence = assert_header_interaction_metadata(
            client.component_snapshot(), True
        )

    # A manually reused app may already be at the lower clamp. Recover only
    # through the same public wheel path so the subsequent decrease remains
    # observable without any transform-setting shortcut.
    before = initial
    before_frame = initial_snapshot["frame"]
    if before["scale"] <= before["min_scale"] + 1.0e-5:
        frame, _ = command_scroll_at_canvas_center(
            client, 900.0, "real command-wheel setup above the minimum"
        )
        _, _, before = wait_canvas(
            client,
            "Node Editor zoom-in setup",
            lambda value: value["scale"] > value["min_scale"] * 2.0,
            after_frame=frame,
        )
        before_frame = frame

    scroll_frame, zoom_point = command_scroll_at_canvas_center(
        client, -10_000.0, "real command-wheel 100x overview zoom at canvas center"
    )
    zoom_snapshot, _, zoomed = wait_canvas(
        client,
        "Node Editor minimum zoom clamp",
        lambda value: value["scale"] < before["scale"]
        and abs(value["scale"] - value["min_scale"]) <= 1.0e-5,
        after_frame=scroll_frame,
    )
    assert_minimum_zoom(before, zoomed)
    overview_header_evidence = assert_header_interaction_metadata(
        client.component_snapshot(), False
    )

    # Re-query all rectangles after zoom. The drag origin is selected from the
    # completed overview frame and sent as a normal primary-button lifecycle.
    pan_snapshot = client.component_snapshot()
    start, end = find_primary_pan_gesture(pan_snapshot)
    expected_delta = {"x": end["x"] - start["x"], "y": end["y"] - start["y"]}
    client.inject(
        "drag",
        {
            "from": start,
            "to": end,
            "coordinate_space": "points",
            "button": "primary",
            "steps": 8,
        },
        {
            "component_id": CANVAS_ID,
            "component_frame": pan_snapshot["frame"],
            "component_rect_points": next(
                component["rect_points"]
                for component in pan_snapshot["components"]
                if component["id"] == CANVAS_ID
            ),
            "coordinate_reason": "real primary drag from unobstructed overview canvas",
        },
    )
    final_snapshot, _, panned = wait_canvas(
        client,
        "Node Editor primary pan translation",
        lambda value: value["scale"] == zoomed["scale"]
        and value["translation"] != zoomed["translation"],
        after_frame=pan_snapshot["frame"],
    )
    actual_delta = assert_only_translation_changed(zoomed, panned, expected_delta)

    final_state = client.state()
    assert_navigation_state_unchanged(initial_state, final_state)

    return {
        "ok": True,
        "suite": "node-editor-zoom",
        "health": health,
        "tab_click": tab_click,
        "initial_frame": initial_snapshot["frame"],
        "zoom_frame": zoom_snapshot["frame"],
        "final_frame": final_snapshot["frame"],
        "before": before,
        "zoomed": zoomed,
        "panned": panned,
        "zoom_point": zoom_point,
        "pan": {"from": start, "to": end, "actual_delta": actual_delta},
        "header_lod": {
            "detail": detail_header_evidence,
            "overview": overview_header_evidence,
        },
        "state_guard": {
            "history": final_state["history"],
            "selection": final_state["editor"]["selection"],
            "node_editor": final_state["editor"]["node_editor"],
            "project_unchanged": True,
        },
        "actions": client.evidence,
        "setup_frame": before_frame,
    }


def parse_args():
    parser = argparse.ArgumentParser()
    parser.add_argument("--base-url", default=None)
    parser.add_argument(
        "--spawn",
        action="store_true",
        help="launch a fresh headful app with the deterministic fixture",
    )
    parser.add_argument(
        "--evidence",
        default="target/qa-node-editor-e2e-evidence.json",
        help="JSON evidence output path",
    )
    parser.add_argument("--timeout", type=float, default=30.0)
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
        client = QaClient(base_url, args.timeout)
        result = run_suite(client)
        result["run_id"] = os.environ.get("RUVIE_QA_RUN_ID")
        evidence_path = os.path.abspath(args.evidence)
        os.makedirs(os.path.dirname(evidence_path), exist_ok=True)
        with open(evidence_path, "w", encoding="utf-8") as output:
            json.dump(result, output, ensure_ascii=False, indent=2)
            output.write("\n")
        print("[qa-node-editor-e2e] PASS; evidence: {}".format(evidence_path))
        return 0
    except (QaFailure, AssertionError, KeyError, StopIteration, TypeError) as error:
        print("[qa-node-editor-e2e] FAIL: {}".format(error), file=sys.stderr)
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
