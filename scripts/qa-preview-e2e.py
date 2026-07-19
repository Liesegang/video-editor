#!/usr/bin/env python3
"""Fast coordinate E2E for Preview fit and Space-hand pan isolation.

The suite resolves the current Preview rectangles from the loopback QA bridge,
then emits a real egui Space + primary press/move/release lifecycle.  It never
sets the camera or Project through a test-only command.
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
repository_git_commit = BASE.repository_git_commit

CANVAS_ID = "preview.canvas"
CONTENT_ID = "preview.content"
PAN_DELTA = {"x": 72.0, "y": 44.0}


def finite_number(value, field):
    if isinstance(value, bool) or not isinstance(value, (int, float)):
        raise QaFailure("{} is not numeric: {!r}".format(field, value))
    value = float(value)
    if not math.isfinite(value):
        raise QaFailure("{} is not finite: {!r}".format(field, value))
    return value


def component_map(snapshot):
    return {component["id"]: component for component in snapshot["components"]}


def require_component(snapshot, component_id):
    component = component_map(snapshot).get(component_id)
    if component is None:
        raise QaFailure(
            "component {!r} is absent in frame {}".format(
                component_id, snapshot["frame"]
            )
        )
    rect = component.get("rect_points", {})
    if (
        not component.get("enabled", False)
        or not component.get("visible", False)
        or finite_number(rect.get("width"), component_id + ".width") <= 0.0
        or finite_number(rect.get("height"), component_id + ".height") <= 0.0
    ):
        raise QaFailure("component {!r} is not interactable".format(component_id))
    return component


def preview_geometry(snapshot, tolerance=0.1):
    """Validate one completed frame's Preview camera and content rectangle."""
    canvas = require_component(snapshot, CANVAS_ID)
    content = require_component(snapshot, CONTENT_ID)
    canvas_rect = canvas["rect_points"]
    content_rect = content["rect_points"]
    metadata = content.get("metadata")
    canvas_metadata = canvas.get("metadata")
    if not isinstance(metadata, dict) or not isinstance(canvas_metadata, dict):
        raise QaFailure("Preview components omitted camera metadata")

    composition_id = metadata.get("composition_id")
    if not isinstance(composition_id, str) or not composition_id:
        raise QaFailure("Preview content omitted composition_id")
    width = finite_number(metadata.get("canvas_width"), "canvas_width")
    height = finite_number(metadata.get("canvas_height"), "canvas_height")
    zoom = finite_number(metadata.get("zoom"), "zoom")
    pan_value = metadata.get("pan")
    if not isinstance(pan_value, dict):
        raise QaFailure("Preview content omitted pan metadata")
    pan = {
        "x": finite_number(pan_value.get("x"), "pan.x"),
        "y": finite_number(pan_value.get("y"), "pan.y"),
    }
    auto_fit = metadata.get("auto_fit")
    if not isinstance(auto_fit, bool):
        raise QaFailure("Preview content omitted boolean auto_fit")
    if width <= 0.0 or height <= 0.0 or zoom <= 0.0:
        raise QaFailure("Preview content published invalid dimensions or zoom")

    expected = {
        "min_x": finite_number(canvas_rect.get("min_x"), "canvas.min_x")
        + pan["x"],
        "min_y": finite_number(canvas_rect.get("min_y"), "canvas.min_y")
        + pan["y"],
        "width": width * zoom,
        "height": height * zoom,
    }
    for field in ("min_x", "min_y", "width", "height"):
        actual = finite_number(content_rect.get(field), "content." + field)
        if abs(actual - expected[field]) > tolerance:
            raise QaFailure(
                "Preview content {} disagrees with camera: {} != {}".format(
                    field, actual, expected[field]
                )
            )

    if canvas_metadata.get("composition_id") != composition_id:
        raise QaFailure("Preview canvas/content composition IDs disagree")
    canvas_zoom = finite_number(canvas_metadata.get("zoom"), "canvas.zoom")
    if abs(canvas_zoom - zoom) > tolerance:
        raise QaFailure("Preview canvas/content zoom metadata disagrees")

    return {
        "frame": snapshot["frame"],
        "composition_id": composition_id,
        "canvas_size": {"width": width, "height": height},
        "canvas_rect": canvas_rect,
        "content_rect": content_rect,
        "pan": pan,
        "zoom": zoom,
        "auto_fit": auto_fit,
    }


def assert_centered(geometry, tolerance=0.75):
    canvas = geometry["canvas_rect"]
    content = geometry["content_rect"]
    for axis in ("x", "y"):
        delta = content["center_" + axis] - canvas["center_" + axis]
        if abs(delta) > tolerance:
            raise QaFailure(
                "initial Preview content is not centered on {}: delta={}".format(
                    axis, delta
                )
            )
    for content_edge, canvas_edge, relation in (
        ("min_x", "min_x", "min"),
        ("min_y", "min_y", "min"),
        ("max_x", "max_x", "max"),
        ("max_y", "max_y", "max"),
    ):
        value = content[content_edge] - canvas[canvas_edge]
        if (relation == "min" and value < -tolerance) or (
            relation == "max" and value > tolerance
        ):
            raise QaFailure("initial Preview content does not fit its canvas")


def plan_space_pan(geometry, delta=PAN_DELTA, margin=12.0):
    content = geometry["content_rect"]
    canvas = geometry["canvas_rect"]
    start = {"x": content["center_x"], "y": content["center_y"]}
    end = {"x": start["x"] + delta["x"], "y": start["y"] + delta["y"]}
    if not (
        canvas["min_x"] + margin <= start["x"] <= canvas["max_x"] - margin
        and canvas["min_y"] + margin <= start["y"] <= canvas["max_y"] - margin
        and canvas["min_x"] + margin <= end["x"] <= canvas["max_x"] - margin
        and canvas["min_y"] + margin <= end["y"] <= canvas["max_y"] - margin
    ):
        raise QaFailure("Preview viewport is too small for the bounded pan gesture")
    return start, end


def assert_space_pan(before, after, expected_delta=PAN_DELTA, tolerance=1.0):
    if before["composition_id"] != after["composition_id"]:
        raise QaFailure("Preview pan changed the active composition")
    if before["canvas_size"] != after["canvas_size"]:
        raise QaFailure("Preview pan changed composition dimensions")
    for field in ("min_x", "min_y", "max_x", "max_y"):
        if abs(after["canvas_rect"][field] - before["canvas_rect"][field]) > tolerance:
            raise QaFailure("Preview pan changed canvas layout field {}".format(field))
    if abs(after["zoom"] - before["zoom"]) > 1.0e-6:
        raise QaFailure("Space-pan changed Preview zoom")

    actual_delta = {
        "x": after["pan"]["x"] - before["pan"]["x"],
        "y": after["pan"]["y"] - before["pan"]["y"],
    }
    for axis in ("x", "y"):
        if abs(actual_delta[axis] - expected_delta[axis]) > tolerance:
            raise QaFailure(
                "Space-pan {} delta was {}, expected {}".format(
                    axis, actual_delta[axis], expected_delta[axis]
                )
            )
        content_delta = (
            after["content_rect"]["center_" + axis]
            - before["content_rect"]["center_" + axis]
        )
        if abs(content_delta - expected_delta[axis]) > tolerance:
            raise QaFailure(
                "Preview content {} moved {}, expected {}".format(
                    axis, content_delta, expected_delta[axis]
                )
            )
    if after["auto_fit"]:
        raise QaFailure("Space-pan did not disable automatic fitting")
    return actual_delta


def assert_navigation_state_unchanged(initial, final):
    if final["project"] != initial["project"]:
        raise QaFailure("Preview navigation mutated the authoritative Project")
    if final["history"] != initial["history"]:
        raise QaFailure("Preview navigation created an undo/redo history entry")
    if final["editor"]["selection"] != initial["editor"]["selection"]:
        raise QaFailure("Space-pan selected, deselected, or moved content")
    if final["editor"]["timeline"] != initial["editor"]["timeline"]:
        raise QaFailure("Preview navigation changed Timeline state")


def wait_geometry(client, description, after_frame=-1, predicate=None):
    def ready():
        client.state()
        snapshot = client.component_snapshot()
        if snapshot["frame"] <= after_frame:
            return None
        try:
            geometry = preview_geometry(snapshot)
        except QaFailure:
            return None
        if predicate is not None and not predicate(geometry):
            return None
        return snapshot, geometry

    return client.wait_until(description, ready)


def point_payload(point):
    return {
        "x": point["x"],
        "y": point["y"],
        "coordinate_space": "points",
        "button": "primary",
    }


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
    initial_snapshot, initial_geometry = wait_geometry(
        client, "fitted initial Preview geometry"
    )
    assert_centered(initial_geometry)
    if not initial_geometry["auto_fit"]:
        raise QaFailure("fresh Preview did not start in automatic fit mode")
    if initial_state["editor"]["preview"]["primary_gesture"] != "Idle":
        raise QaFailure("fresh Preview did not start with an Idle gesture owner")

    client.key("space", True)
    press_snapshot, press_geometry = wait_geometry(
        client,
        "fresh Preview geometry after Space press",
        after_frame=initial_snapshot["frame"],
    )
    start, end = plan_space_pan(press_geometry)
    client.inject(
        "press",
        point_payload(start),
        {
            "component_id": CONTENT_ID,
            "component_frame": press_snapshot["frame"],
            "component_rect_points": press_geometry["content_rect"],
            "coordinate_reason": "real primary press over fitted Preview content while Space is held",
        },
    )

    def pan_owned():
        state = client.state()
        return (
            state
            if state["editor"]["preview"]["primary_gesture"] == "Pan"
            else None
        )

    client.wait_until("Preview pan ownership after real press", pan_owned)

    midpoint = {
        "x": start["x"] + PAN_DELTA["x"] * 0.5,
        "y": start["y"] + PAN_DELTA["y"] * 0.5,
    }
    move_snapshot, move_geometry = wait_geometry(
        client,
        "fresh Preview geometry before pointer movement",
        after_frame=press_snapshot["frame"],
    )
    client.inject(
        "move",
        point_payload(midpoint),
        {
            "component_id": CONTENT_ID,
            "component_frame": move_snapshot["frame"],
            "component_rect_points": move_geometry["content_rect"],
            "coordinate_reason": "real pointer movement establishes the Space-hand drag",
        },
    )
    moved_snapshot, moved_geometry = wait_geometry(
        client,
        "Preview content translated to the drag midpoint",
        after_frame=move_snapshot["frame"],
        predicate=lambda value: value["pan"] != press_geometry["pan"],
    )
    partial_delta = {"x": PAN_DELTA["x"] * 0.5, "y": PAN_DELTA["y"] * 0.5}
    assert_space_pan(initial_geometry, moved_geometry, partial_delta)

    # Release Space after movement but before the pointer. The shared shortcut
    # suppression has now observed a real hand drag, while the Preview owner
    # must remain latched so the final release cannot click through.
    client.key("space", False)
    latched_state = client.wait_until("latched Preview pan owner", pan_owned)
    release_snapshot, release_geometry = wait_geometry(
        client,
        "fresh Preview geometry before pointer release",
        after_frame=moved_snapshot["frame"],
    )
    if release_geometry["pan"] != moved_geometry["pan"]:
        raise QaFailure("Preview moved while only the Space key was released")

    client.inject(
        "release",
        point_payload(end),
        {
            "component_id": CONTENT_ID,
            "component_frame": release_snapshot["frame"],
            "component_rect_points": release_geometry["content_rect"],
            "coordinate_reason": "final real pointer move/release with latched Space-pan ownership",
        },
    )

    final_snapshot, final_geometry = wait_geometry(
        client,
        "Preview content translated by the coordinate delta",
        after_frame=release_snapshot["frame"],
        predicate=lambda value: value["pan"] != initial_geometry["pan"],
    )
    actual_delta = assert_space_pan(initial_geometry, final_geometry)
    final_state = client.state()
    final_preview = final_state["editor"]["preview"]
    if final_preview["primary_gesture"] != "Idle":
        raise QaFailure("Preview gesture owner did not return to Idle")
    if final_preview["auto_fit"]:
        raise QaFailure("Preview state re-enabled auto-fit after pan")
    for axis in ("x", "y"):
        if abs(final_preview["pan"][axis] - final_geometry["pan"][axis]) > 1.0:
            raise QaFailure("Preview state/component pan metadata disagrees")
    if abs(final_preview["zoom"] - initial_geometry["zoom"]) > 1.0e-6:
        raise QaFailure("Preview state zoom changed during Space-pan")
    assert_navigation_state_unchanged(initial_state, final_state)

    return {
        "ok": True,
        "suite": "preview-fit-space-pan",
        "health": health,
        "initial_frame": initial_snapshot["frame"],
        "press_frame": press_snapshot["frame"],
        "move_frame": move_snapshot["frame"],
        "moved_frame": moved_snapshot["frame"],
        "latched_frame": latched_state["frame"],
        "release_component_frame": release_snapshot["frame"],
        "final_frame": final_snapshot["frame"],
        "initial_geometry": initial_geometry,
        "final_geometry": final_geometry,
        "pan": {"from": start, "to": end, "actual_delta": actual_delta},
        "state_guard": {
            "project_unchanged": True,
            "history": final_state["history"],
            "selection": final_state["editor"]["selection"],
            "timeline": final_state["editor"]["timeline"],
            "primary_gesture": final_preview["primary_gesture"],
            "auto_fit": final_preview["auto_fit"],
            "zoom": final_preview["zoom"],
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
        default="target/qa-preview-e2e-evidence.json",
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
        print("[qa-preview-e2e] PASS; evidence: {}".format(evidence_path))
        return 0
    except (QaFailure, AssertionError, KeyError, IndexError, TypeError) as error:
        print("[qa-preview-e2e] FAIL: {}".format(error), file=sys.stderr)
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
