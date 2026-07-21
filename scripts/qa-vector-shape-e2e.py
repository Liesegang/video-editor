#!/usr/bin/env python3
"""Coordinate E2E for direct vector Path editing in Preview.

The deterministic QA Project is entered through visible tabs/tools/visuals.
Every point, handle, and mode interaction uses the latest registered screen
rectangle and the normal egui pointer path; Project state is read-only QA
evidence.
"""

from __future__ import annotations

import argparse
import importlib.util
import json
import math
import os
import re
import signal
import subprocess
import sys


SCRIPT_DIR = os.path.dirname(os.path.abspath(__file__))
REPOSITORY = os.path.dirname(SCRIPT_DIR)
BASE_PATH = os.path.join(SCRIPT_DIR, "qa-e2e.py")
SPEC = importlib.util.spec_from_file_location("ruvie_qa_vector_base", BASE_PATH)
if SPEC is None or SPEC.loader is None:
    raise RuntimeError("cannot load scripts/qa-e2e.py")
BASE = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(BASE)

QaClient = BASE.QaClient
QaFailure = BASE.QaFailure
SHAPE = BASE.SHAPE
SHAPE_TRANSFORM = BASE.SHAPE_TRANSFORM
CLIP = BASE.CLIP_B1
INITIAL_PATH = "M 0 0 H 160 V 90 H 0 Z"
NUMBER = r"[-+]?(?:\d+(?:\.\d*)?|\.\d+)(?:[eE][-+]?\d+)?"
TOKEN = re.compile(r"[MLHVCZmlhvcz]|" + NUMBER)


def path_value(state):
    return BASE.property_value(state["project"]["nodes"][SHAPE], "path")


def parse_path(path):
    """Parse the M/L/H/V/C/Z subset emitted by the in-app writer."""
    tokens = TOKEN.findall(path)
    points = []
    command = None
    index = 0
    current = [0.0, 0.0]
    while index < len(tokens):
        token = tokens[index]
        if token.isalpha():
            command = token.upper()
            index += 1
            if command == "Z":
                continue
        if command == "M" or command == "L":
            position = [float(tokens[index]), float(tokens[index + 1])]
            points.append(
                {
                    "position": position,
                    "handle_in": [0.0, 0.0],
                    "handle_out": [0.0, 0.0],
                }
            )
            current = position
            index += 2
            command = "L"
        elif command == "H":
            current = [float(tokens[index]), current[1]]
            points.append(
                {
                    "position": list(current),
                    "handle_in": [0.0, 0.0],
                    "handle_out": [0.0, 0.0],
                }
            )
            index += 1
        elif command == "V":
            current = [current[0], float(tokens[index])]
            points.append(
                {
                    "position": list(current),
                    "handle_in": [0.0, 0.0],
                    "handle_out": [0.0, 0.0],
                }
            )
            index += 1
        elif command == "C":
            c1 = [float(tokens[index]), float(tokens[index + 1])]
            c2 = [float(tokens[index + 2]), float(tokens[index + 3])]
            position = [float(tokens[index + 4]), float(tokens[index + 5])]
            if not points:
                raise QaFailure("Cubic path has no preceding point")
            points[-1]["handle_out"] = [c1[0] - current[0], c1[1] - current[1]]
            if points and near_point(position, points[0]["position"]):
                points[0]["handle_in"] = [
                    c2[0] - position[0],
                    c2[1] - position[1],
                ]
            else:
                points.append(
                    {
                        "position": position,
                        "handle_in": [c2[0] - position[0], c2[1] - position[1]],
                        "handle_out": [0.0, 0.0],
                    }
                )
            current = position
            index += 6
        else:
            raise QaFailure("unsupported Path command in {!r}".format(path))
    return points


def near(left, right, tolerance=0.02):
    return abs(float(left) - float(right)) <= tolerance


def near_point(left, right, tolerance=0.02):
    return near(left[0], right[0], tolerance) and near(left[1], right[1], tolerance)


def length(vector):
    return math.hypot(vector[0], vector[1])


def assert_unchanged_points(before, after, excluded):
    if len(before) != len(after):
        raise QaFailure("vector edit changed point count: {} -> {}".format(len(before), len(after)))
    for index, (old, new) in enumerate(zip(before, after)):
        if index == excluded:
            continue
        if not near_point(old["position"], new["position"]):
            raise QaFailure("editing point {} moved unrelated point {}".format(excluded, index))
        if not near_point(old["handle_in"], new["handle_in"]):
            raise QaFailure("editing point {} changed unrelated incoming handle {}".format(excluded, index))
        if not near_point(old["handle_out"], new["handle_out"]):
            raise QaFailure("editing point {} changed unrelated outgoing handle {}".format(excluded, index))


def assert_one_history(before, after, operation):
    delta = BASE.history_depth(after) - BASE.history_depth(before)
    if delta != 1:
        raise QaFailure("{} created {} history entries, expected 1".format(operation, delta))


def wait_path(client, description, predicate):
    return client.wait_until(
        description,
        lambda: state if predicate(path_value(state := client.state())) else None,
    )


def activate_shape_editor(client):
    BASE.activate_dock_tab(client, "dock.tab:preview", "Preview", "vector Path E2E")
    _, select_tool = client.wait_component_settled("preview.tool.select")
    if not (select_tool.get("metadata") or {}).get("selected"):
        client.click_component("preview.tool.select")
    client.wait_until(
        "Preview Select tool",
        lambda: item
        if (item := client.component("preview.tool.select"))[1].get("metadata", {}).get("selected")
        else None,
    )
    visual_id = "preview.visual.spatial:" + SHAPE_TRANSFORM
    client.wait_component_settled(visual_id)
    client.click_component(visual_id)
    client.wait_until(
        "Shape Clip selection and exact Preview edit target",
        lambda: state
        if BASE.selection_matches(state := client.state(), "clip", CLIP)
        and (state["editor"]["preview"].get("edit_target") or {}).get("content_node_id") == SHAPE
        and (state["editor"]["preview"].get("edit_target") or {}).get("spatial_node_id")
        == SHAPE_TRANSFORM
        else None,
    )
    _, shape_tool = client.wait_component_settled("preview.tool.shape")
    if not (shape_tool.get("metadata") or {}).get("selected"):
        client.click_component("preview.tool.shape")
    client.wait_until(
        "Preview Shape tool",
        lambda: item
        if (item := client.component("preview.tool.shape"))[1].get("metadata", {}).get("selected")
        else None,
    )
    client.wait_component("preview.vector.point:0")


def run_suite(client):
    initial = BASE.wait_fresh_fixture(client)
    if path_value(initial) != INITIAL_PATH:
        raise QaFailure("unexpected initial Shape path {!r}".format(path_value(initial)))
    activate_shape_editor(client)
    initial = client.state()
    original_points = parse_path(path_value(initial))
    if len(original_points) != 4:
        raise QaFailure("fixture Shape must expose four logical points")

    # A real point drag must retain every unrelated point and produce exactly
    # one history snapshot even though the bridge emits many move frames.
    _, drag_end = client.drag_component_by(
        "preview.vector.point:0", 24.0, 14.0, steps=18
    )
    moved = wait_path(client, "vector point drag", lambda value: value != INITIAL_PATH)
    moved_points = parse_path(path_value(moved))
    assert_unchanged_points(original_points, moved_points, 0)
    if near_point(moved_points[0]["position"], original_points[0]["position"]):
        raise QaFailure("point drag left its target vertex unchanged")
    _, moved_component = client.wait_component_settled("preview.vector.point:0")
    moved_rect = moved_component["rect_points"]
    if not (
        near(moved_rect["center_x"], drag_end["x"], 1.5)
        and near(moved_rect["center_y"], drag_end["y"], 1.5)
    ):
        raise QaFailure(
            "point drag screen target drifted: ({}, {}) != ({}, {})".format(
                moved_rect["center_x"],
                moved_rect["center_y"],
                drag_end["x"],
                drag_end["y"],
            )
        )
    assert_one_history(initial, moved, "vector point drag")
    client.wait_preview_change(
        initial["editor"]["preview"]["pixel_hash"],
        initial["editor"]["preview"]["render_revision"],
    )

    undone = BASE.undo_project_edit(
        client,
        "vector point drag",
        lambda project: BASE.property_value(project["nodes"][SHAPE], "path") == INITIAL_PATH,
    )
    BASE.redo_project_edit(
        client,
        "vector point drag",
        lambda project: BASE.property_value(project["nodes"][SHAPE], "path") == path_value(moved),
    )
    BASE.undo_project_edit(
        client,
        "vector point drag restore",
        lambda project: BASE.property_value(project["nodes"][SHAPE], "path") == INITIAL_PATH,
    )
    if path_value(undone) != INITIAL_PATH:
        raise QaFailure("undo did not restore authored Path data")

    # Select one anchor by coordinate, then drive the visible point-mode
    # controls. Smooth creates tangible handles without moving any vertex.
    activate_shape_editor(client)
    client.click_component("preview.vector.point:1")
    client.wait_component("preview.vector.mode.smooth")
    smooth_before = client.state()
    client.click_component("preview.vector.mode.smooth")
    smooth = wait_path(client, "Smooth point mode", lambda value: value != INITIAL_PATH)
    smooth_points = parse_path(path_value(smooth))
    assert_unchanged_points(original_points, smooth_points, 1)
    if length(smooth_points[1]["handle_in"]) <= 0.1 or length(
        smooth_points[1]["handle_out"]
    ) <= 0.1:
        raise QaFailure("Smooth mode did not create editable handles")
    assert_one_history(smooth_before, smooth, "Smooth mode")

    # Drag a rendered handle. The opposite length is retained while its angle
    # remains collinear, and every vertex stays fixed.
    client.wait_component("preview.vector.handle_out:1")
    handle_before = client.state()
    before_handle_points = parse_path(path_value(handle_before))
    client.drag_component_by("preview.vector.handle_out:1", 18.0, 22.0, steps=16)
    handle_after = wait_path(
        client,
        "Smooth handle drag",
        lambda value: value != path_value(handle_before),
    )
    after_handle_points = parse_path(path_value(handle_after))
    for old, new in zip(before_handle_points, after_handle_points):
        if not near_point(old["position"], new["position"]):
            raise QaFailure("handle drag moved a Path vertex")
    incoming = after_handle_points[1]["handle_in"]
    outgoing = after_handle_points[1]["handle_out"]
    cross = incoming[0] * outgoing[1] - incoming[1] * outgoing[0]
    dot = incoming[0] * outgoing[0] + incoming[1] * outgoing[1]
    if abs(cross) > 0.05 * max(1.0, length(incoming) * length(outgoing)) or dot >= 0.0:
        raise QaFailure("Smooth handle drag broke the linked tangent")
    assert_one_history(handle_before, handle_after, "Smooth handle drag")
    client.wait_preview_change(
        handle_before["editor"]["preview"]["pixel_hash"],
        handle_before["editor"]["preview"]["render_revision"],
    )
    BASE.undo_project_edit(
        client,
        "Smooth handle drag",
        lambda project: BASE.property_value(project["nodes"][SHAPE], "path")
        == path_value(handle_before),
    )
    BASE.redo_project_edit(
        client,
        "Smooth handle drag",
        lambda project: BASE.property_value(project["nodes"][SHAPE], "path")
        == path_value(handle_after),
    )

    activate_shape_editor(client)
    client.click_component("preview.vector.point:1")
    client.wait_component("preview.vector.mode.symmetric")
    symmetric_before = client.state()
    client.click_component("preview.vector.mode.symmetric")
    symmetric = wait_path(
        client,
        "Symmetric point mode",
        lambda value: value != path_value(symmetric_before),
    )
    symmetric_point = parse_path(path_value(symmetric))[1]
    if not near_point(
        symmetric_point["handle_in"],
        [-symmetric_point["handle_out"][0], -symmetric_point["handle_out"][1]],
    ):
        raise QaFailure("Symmetric mode did not mirror both handle angle and length")
    assert_one_history(symmetric_before, symmetric, "Symmetric mode")

    corner_before = client.state()
    client.click_component("preview.vector.mode.corner")
    corner = wait_path(
        client,
        "Corner point mode",
        lambda value: value != path_value(corner_before),
    )
    corner_points = parse_path(path_value(corner))
    if length(corner_points[1]["handle_in"]) > 0.01 or length(
        corner_points[1]["handle_out"]
    ) > 0.01:
        raise QaFailure("Corner mode did not collapse the linked handles")
    if len(corner_points) != 4:
        raise QaFailure("closed Path round trip grew duplicate points")
    assert_one_history(corner_before, corner, "Corner mode")

    return {
        "ok": True,
        "suite": "vector-shape",
        "initial_path": INITIAL_PATH,
        "final_path": path_value(corner),
        "point_count": len(corner_points),
        "final_history": corner["history"],
        "actions": client.evidence,
    }


def parse_args():
    parser = argparse.ArgumentParser()
    parser.add_argument("--base-url", default=None)
    parser.add_argument("--spawn", action="store_true")
    parser.add_argument("--timeout", type=float, default=45.0)
    parser.add_argument(
        "--evidence",
        default="target/qa-vector-shape-e2e-evidence.json",
    )
    return parser.parse_args()


def main():
    args = parse_args()
    process = None
    port = BASE.free_port() if args.spawn else 39091
    base_url = args.base_url or "http://127.0.0.1:{}".format(port)
    try:
        if args.spawn:
            environment = os.environ.copy()
            environment["RUVIE_QA_PORT"] = str(port)
            environment["RUVIE_QA_FIXTURE"] = "node_editor_e2e"
            process = subprocess.Popen(
                [
                    os.path.join(SCRIPT_DIR, "with-managed-python.sh"),
                    "cargo",
                    "run",
                    "-p",
                    "app",
                    "--locked",
                ],
                cwd=REPOSITORY,
                env=environment,
                start_new_session=True,
            )
        client = QaClient(base_url, args.timeout)
        client.wait_health()
        result = run_suite(client)
        result["git_commit"] = BASE.repository_git_commit()
        result["component_frame"] = client.component_snapshot()["frame"]
        result["action_count"] = len(result["actions"])
        evidence = os.path.abspath(args.evidence)
        os.makedirs(os.path.dirname(evidence), exist_ok=True)
        with open(evidence, "w", encoding="utf-8") as output:
            json.dump(result, output, ensure_ascii=False, indent=2)
            output.write("\n")
        print("[qa-vector-shape-e2e] PASS; evidence: {}".format(evidence))
        return 0
    except (QaFailure, AssertionError, KeyError, IndexError, ValueError) as error:
        print("[qa-vector-shape-e2e] FAIL: {}".format(error), file=sys.stderr)
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
