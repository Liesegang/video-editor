#!/usr/bin/env python3
"""Coordinate E2E for Preview trackpad pan and cursor-centered pinch zoom."""

import argparse
import importlib.util
import json
import math
import os
import signal
import subprocess
import sys


SCRIPT_DIR = os.path.dirname(os.path.abspath(__file__))
PREVIEW_PATH = os.path.join(SCRIPT_DIR, "qa-preview-e2e.py")
SPEC = importlib.util.spec_from_file_location("ruvie_qa_preview_base", PREVIEW_PATH)
if SPEC is None or SPEC.loader is None:
    raise RuntimeError("cannot load scripts/qa-preview-e2e.py")
PREVIEW = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(PREVIEW)

QaClient = PREVIEW.QaClient
QaFailure = PREVIEW.QaFailure
free_port = PREVIEW.free_port
repository_git_commit = PREVIEW.repository_git_commit

CANVAS_ID = PREVIEW.CANVAS_ID
SCROLL_DELTA = {"x": 5.0, "y": -4.0}
PINCH_FACTOR = 1.25
PINCH_FRACTION = {"x": 0.63, "y": 0.42}


def assert_near(actual, expected, description, tolerance=1.0e-3):
    if not math.isfinite(float(actual)) or abs(float(actual) - float(expected)) > tolerance:
        raise QaFailure("{}: {} != {}".format(description, actual, expected))


def geometry_pan_matches(origin, value, delta, tolerance=0.25):
    return all(
        abs(value["pan"][axis] - origin["pan"][axis] - delta[axis]) <= tolerance
        for axis in ("x", "y")
    )


def world_at_screen(geometry, point):
    local = {
        "x": point["x"] - geometry["canvas_rect"]["min_x"],
        "y": point["y"] - geometry["canvas_rect"]["min_y"],
    }
    return {
        axis: (local[axis] - geometry["pan"][axis]) / geometry["zoom"]
        for axis in ("x", "y")
    }


def expected_zoomed_pan(geometry, point, factor):
    local = {
        "x": point["x"] - geometry["canvas_rect"]["min_x"],
        "y": point["y"] - geometry["canvas_rect"]["min_y"],
    }
    return {
        axis: local[axis] + (geometry["pan"][axis] - local[axis]) * factor
        for axis in ("x", "y")
    }


def assert_idle_navigation(state, description):
    preview = state["editor"]["preview"]
    if preview["primary_gesture"] != "Idle":
        raise QaFailure("{} left the primary gesture owned".format(description))
    for field in (
        "is_moving_selected_entity",
        "selection_drag_active",
        "body_drag_active",
        "gizmo_active",
        "vector_handle_active",
    ):
        if preview.get(field) is not False:
            raise QaFailure("{} activated Preview {}".format(description, field))


def assert_camera_state(state, geometry, description):
    preview = state["editor"]["preview"]
    assert_near(
        preview["pan"]["x"], geometry["pan"]["x"], description + " pan.x", 0.5
    )
    assert_near(
        preview["pan"]["y"], geometry["pan"]["y"], description + " pan.y", 0.5
    )
    assert_near(preview["zoom"], geometry["zoom"], description + " zoom", 1.0e-5)
    if preview["auto_fit"]:
        raise QaFailure("{} unexpectedly remained in automatic fit mode".format(description))


def run_suite(client):
    health = client.wait_health()

    def rendered():
        state = client.state()
        preview = state["editor"]["preview"]
        if (
            preview["render_revision"] > 0
            and preview["nontransparent_pixels"] is not None
            and preview["nontransparent_pixels"] > 0
            and preview["pixel_hash"] is not None
            and preview["modal_error"] is None
        ):
            return state
        return None

    initial_state = client.wait_until("a rendered initial Preview", rendered)
    initial_snapshot, initial_geometry = PREVIEW.wait_geometry(
        client, "initial Preview trackpad geometry"
    )
    assert_idle_navigation(initial_state, "initial Preview")

    scroll_snapshot, scroll_component, scroll_point = client.scroll_component(
        CANVAS_ID,
        SCROLL_DELTA["x"],
        SCROLL_DELTA["y"],
    )
    if scroll_snapshot["frame"] < initial_snapshot["frame"]:
        raise QaFailure("scroll used a stale Preview component frame")
    scrolled_snapshot, scrolled_geometry = PREVIEW.wait_geometry(
        client,
        "two-axis Preview trackpad pan",
        after_frame=scroll_snapshot["frame"],
        predicate=lambda value: geometry_pan_matches(
            initial_geometry, value, SCROLL_DELTA
        ),
    )
    assert_near(
        scrolled_geometry["zoom"],
        initial_geometry["zoom"],
        "trackpad pan changed zoom",
        1.0e-6,
    )
    for axis in ("x", "y"):
        assert_near(
            scrolled_geometry["pan"][axis] - initial_geometry["pan"][axis],
            SCROLL_DELTA[axis],
            "trackpad pan delta " + axis,
            0.25,
        )
    scrolled_state = client.state()
    PREVIEW.assert_navigation_state_unchanged(initial_state, scrolled_state)
    assert_idle_navigation(scrolled_state, "trackpad pan")
    assert_camera_state(scrolled_state, scrolled_geometry, "trackpad pan")

    # Resolve the canvas again after panning. The pinch coordinate must come
    # from this completed frame, never from the pre-scroll rectangle.
    pinch_snapshot, pinch_component, pinch_point = client.pinch_component(
        CANVAS_ID,
        PINCH_FACTOR,
        PINCH_FRACTION["x"],
        PINCH_FRACTION["y"],
    )
    if pinch_snapshot["frame"] <= scroll_snapshot["frame"]:
        raise QaFailure("pinch did not re-query Preview geometry after scrolling")
    world_before = world_at_screen(scrolled_geometry, pinch_point)
    expected_pan = expected_zoomed_pan(scrolled_geometry, pinch_point, PINCH_FACTOR)
    expected_zoom = scrolled_geometry["zoom"] * PINCH_FACTOR
    pinched_snapshot, pinched_geometry = PREVIEW.wait_geometry(
        client,
        "cursor-centered Preview pinch zoom",
        after_frame=pinch_snapshot["frame"],
        predicate=lambda value: abs(value["zoom"] - expected_zoom) <= 1.0e-4,
    )
    assert_near(pinched_geometry["zoom"], expected_zoom, "pinch zoom factor", 1.0e-4)
    for axis in ("x", "y"):
        assert_near(
            pinched_geometry["pan"][axis],
            expected_pan[axis],
            "pinch-adjusted pan " + axis,
            0.25,
        )
    world_after = world_at_screen(pinched_geometry, pinch_point)
    for axis in ("x", "y"):
        assert_near(
            world_after[axis],
            world_before[axis],
            "pinch cursor world point " + axis,
            1.0e-2,
        )

    final_state = client.state()
    PREVIEW.assert_navigation_state_unchanged(initial_state, final_state)
    assert_idle_navigation(final_state, "pinch zoom")
    assert_camera_state(final_state, pinched_geometry, "pinch zoom")

    return {
        "ok": True,
        "suite": "preview-trackpad-pan-pinch",
        "health": health,
        "frames": {
            "initial": initial_snapshot["frame"],
            "scroll_component": scroll_snapshot["frame"],
            "scrolled": scrolled_snapshot["frame"],
            "pinch_component": pinch_snapshot["frame"],
            "pinched": pinched_snapshot["frame"],
        },
        "scroll": {
            "point": scroll_point,
            "delta": SCROLL_DELTA,
            "component_rect": scroll_component["rect_points"],
            "from": initial_geometry,
            "to": scrolled_geometry,
        },
        "pinch": {
            "point": pinch_point,
            "factor": PINCH_FACTOR,
            "component_rect": pinch_component["rect_points"],
            "world_before": world_before,
            "world_after": world_after,
            "from": scrolled_geometry,
            "to": pinched_geometry,
        },
        "state_guard": {
            "project_unchanged": True,
            "history": final_state["history"],
            "selection": final_state["editor"]["selection"],
            "timeline": final_state["editor"]["timeline"],
            "primary_gesture": final_state["editor"]["preview"]["primary_gesture"],
        },
        "actions": client.evidence,
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
        default="target/qa-preview-trackpad-e2e-evidence.json",
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
                cwd=os.path.dirname(SCRIPT_DIR),
                env=environment,
                start_new_session=True,
            )
        client = QaClient(base_url, args.timeout)
        result = run_suite(client)
        result["run_id"] = os.environ.get("RUVIE_QA_RUN_ID")
        result["git_commit"] = repository_git_commit()
        result["component_frame"] = client.component_snapshot()["frame"]
        result["action_count"] = len(result.get("actions", []))
        evidence_path = os.path.abspath(args.evidence)
        os.makedirs(os.path.dirname(evidence_path), exist_ok=True)
        with open(evidence_path, "w", encoding="utf-8") as output:
            json.dump(result, output, ensure_ascii=False, indent=2)
            output.write("\n")
        print("[qa-preview-trackpad-e2e] PASS; evidence: {}".format(evidence_path))
        return 0
    except (QaFailure, AssertionError, KeyError, IndexError, TypeError) as error:
        print("[qa-preview-trackpad-e2e] FAIL: {}".format(error), file=sys.stderr)
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
