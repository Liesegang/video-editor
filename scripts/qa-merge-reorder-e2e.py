#!/usr/bin/env python3
"""Focused coordinate E2E for physical Merge-layer wire reordering."""

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
SPEC = importlib.util.spec_from_file_location("ruvie_qa_merge_reorder_base", BASE_PATH)
if SPEC is None or SPEC.loader is None:
    raise RuntimeError("cannot load scripts/qa-e2e.py")
BASE = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(BASE)

QaClient = BASE.QaClient
QaFailure = BASE.QaFailure


def merge_layers(project):
    return sorted(
        [
            connection
            for connection in project["connections"]
            if connection["to"]["owner"].get("owner_type") == "Node"
            and connection["to"]["owner"].get("owner_id") == BASE.MERGE
            and connection["to"]["port"] == "images"
        ],
        key=lambda connection: (connection["order"], connection["id"]),
    )


def merge_component_id(kind, connection_id):
    return "node_editor.merge_layer.{}:{}:{}".format(
        kind, BASE.MERGE, connection_id
    )


def canvas_viewport(client):
    _, canvas = client.wait_component_settled("node_editor.canvas")
    metadata = canvas.get("metadata") or {}
    translation = metadata.get("translation") or {}
    viewport = {
        "scale": metadata.get("scale"),
        "translation": {
            "x": translation.get("x"),
            "y": translation.get("y"),
        },
    }
    values = [
        viewport["scale"],
        viewport["translation"]["x"],
        viewport["translation"]["y"],
    ]
    if not all(
        isinstance(value, (int, float)) and math.isfinite(float(value))
        for value in values
    ):
        raise QaFailure("Node Editor canvas omitted a finite viewport transform")
    return viewport


def wait_merge_gesture(client, description, predicate):
    def matches():
        state = client.state()
        gesture = state["editor"]["node_editor"]["merge_layer_reorder"]
        return state if predicate(gesture) else None

    return client.wait_until(description, matches)


def assert_wire_identity(state, authored_by_id, expected_ids, operation):
    current_layers = merge_layers(state["project"])
    if [item["id"] for item in current_layers] != expected_ids:
        raise QaFailure("{} produced the wrong physical order".format(operation))
    for expected_order, current in enumerate(current_layers):
        authored = authored_by_id[current["id"]]
        for field in ("id", "from", "to", "blend_mode"):
            if current[field] != authored[field]:
                raise QaFailure(
                    "{} changed per-wire {} on {}".format(
                        operation, field, current["id"]
                    )
                )
        if current["order"] != expected_order:
            raise QaFailure(
                "{} left non-canonical order {} on {}".format(
                    operation, current["order"], current["id"]
                )
            )


def assert_layout_and_viewport(client, state, positions, viewport, operation):
    if BASE.node_editor_layout_positions(state["project"]) != positions:
        raise QaFailure("{} changed Node Editor geometry".format(operation))
    if canvas_viewport(client) != viewport:
        raise QaFailure("{} changed the Node Editor viewport".format(operation))


def run_suite(client):
    health = client.wait_health()
    initial = BASE.wait_fresh_fixture(client)
    BASE.activate_dock_tab(
        client, "dock.tab:node_editor", "Node Editor", "Merge reorder"
    )

    original_layers = merge_layers(initial["project"])
    original = BASE.find_project_connection(
        initial["project"],
        "Node",
        BASE.SOLID,
        "image",
        "Node",
        BASE.MERGE,
        "images",
    )
    connection_id = original["id"]
    if len(original_layers) != 3 or original_layers[0]["id"] != connection_id:
        raise QaFailure("Merge fixture does not start with the expected three layers")
    authored_by_id = {item["id"]: item for item in original_layers}
    original_ids = [item["id"] for item in original_layers]
    original_connections = list(initial["project"]["connections"])
    original_preview = dict(initial["editor"]["preview"])

    BASE.ensure_node_editor_ports_interactive(
        client, ["node_editor.node_header:" + BASE.MERGE]
    )
    handle_id = merge_component_id("drag_handle", connection_id)
    target_row_id = "node_editor.merge_layer:{}:{}".format(
        BASE.MERGE, original_layers[2]["id"]
    )
    BASE.reveal_node_editor_components(client, [handle_id, target_row_id])
    _, handle = client.wait_component_settled(handle_id)
    metadata = handle.get("metadata") or {}
    if not (
        metadata.get("action") == "physical_reorder"
        and metadata.get("gesture") == "primary_vertical_drag"
        and metadata.get("merge_id") == BASE.MERGE
        and metadata.get("connection_id") == connection_id
        and metadata.get("back_to_front_index") == 0
        and metadata.get("layer_count") == 3
        and metadata.get("invalid_drop_cancels") is True
    ):
        raise QaFailure("Merge drag handle omitted canonical gesture metadata")

    before = client.state()
    positions = BASE.node_editor_layout_positions(before["project"])
    viewport = canvas_viewport(client)
    expected_ids = [original_layers[1]["id"], original_layers[2]["id"], connection_id]
    client.drag_components(handle_id, target_row_id, steps=16)
    reordered = client.wait_project(
        "physical Merge handle drag",
        lambda project: [item["id"] for item in merge_layers(project)]
        == expected_ids,
    )
    BASE.assert_history_delta(before, reordered, 1, "physical Merge handle drag")
    assert_wire_identity(reordered, authored_by_id, expected_ids, "physical drag")
    assert_layout_and_viewport(client, reordered, positions, viewport, "physical drag")
    reordered_connections = list(reordered["project"]["connections"])
    reordered_rendered = client.wait_preview_render_after(reordered, "physical drag")

    undone = BASE.undo_project_edit(
        client,
        "physical Merge handle drag",
        lambda project: project["connections"] == original_connections,
    )
    undone_rendered = BASE.wait_preview_hash_after(
        client,
        original_preview["pixel_hash"],
        reordered_rendered["editor"]["preview"]["render_revision"],
        "physical Merge handle drag Undo",
    )
    assert_wire_identity(undone, authored_by_id, original_ids, "physical drag Undo")

    redone = BASE.redo_project_edit(
        client,
        "physical Merge handle drag",
        lambda project: project["connections"] == reordered_connections,
    )
    redone_rendered = client.wait_preview_render_after(redone, "physical drag Redo")
    assert_wire_identity(redone, authored_by_id, expected_ids, "physical drag Redo")

    restored = BASE.undo_project_edit(
        client,
        "physical Merge handle drag after Redo",
        lambda project: project["connections"] == original_connections,
    )
    BASE.wait_preview_hash_after(
        client,
        original_preview["pixel_hash"],
        redone_rendered["editor"]["preview"]["render_revision"],
        "physical Merge handle drag final Undo",
    )
    assert_wire_identity(restored, authored_by_id, original_ids, "physical drag restore")
    assert_layout_and_viewport(client, restored, positions, viewport, "Undo/Redo")

    # Invalid drop: both points come from one fresh complete component frame.
    BASE.reveal_node_editor_components(client, [handle_id])
    invalid_before = client.state()
    invalid_viewport = canvas_viewport(client)
    invalid_snapshot, invalid_point = BASE.find_free_canvas_point(client)
    invalid_components = {
        item["id"]: item for item in invalid_snapshot["components"]
    }
    invalid_handle = invalid_components.get(handle_id)
    if invalid_handle is None or not invalid_handle.get("visible", False):
        raise QaFailure("Merge drag handle was absent from invalid-drop frame")
    invalid_start = client.point(invalid_handle["rect_points"])
    client.inject(
        "drag",
        {
            "from": invalid_start,
            "to": invalid_point,
            "coordinate_space": "points",
            "steps": 16,
            "button": "primary",
        },
        {
            "source_component_id": handle_id,
            "target_component_id": "node_editor.canvas",
            "component_frame": invalid_snapshot["frame"],
            "source_rect_points": invalid_handle["rect_points"],
            "coordinate_reason": "fresh unobstructed canvas point outside Merge rows",
        },
    )
    invalid_after = wait_merge_gesture(
        client, "invalid Merge drop cleanup", lambda gesture: gesture is None
    )
    if (
        invalid_after["project"] != invalid_before["project"]
        or invalid_after["history"] != invalid_before["history"]
    ):
        raise QaFailure("invalid physical Merge drop changed Project/history")
    assert_layout_and_viewport(
        client, invalid_after, positions, invalid_viewport, "invalid drop"
    )

    # Escape: press, held-pointer move, key, and release are distinct real
    # input actions so cancellation occurs while primary remains down.
    BASE.reveal_node_editor_components(client, [handle_id, target_row_id])
    escape_before = client.state()
    escape_viewport = canvas_viewport(client)
    press_snapshot = client.component_snapshot()
    press_components = {item["id"]: item for item in press_snapshot["components"]}
    press_handle = press_components.get(handle_id)
    if press_handle is None or not press_handle.get("visible", False):
        raise QaFailure("Merge drag handle was absent from Escape press frame")
    press_point = client.point(press_handle["rect_points"])
    client.inject(
        "press",
        {
            "x": press_point["x"],
            "y": press_point["y"],
            "coordinate_space": "points",
            "button": "primary",
        },
        {
            "component_id": handle_id,
            "component_frame": press_snapshot["frame"],
            "component_rect_points": press_handle["rect_points"],
        },
    )
    move_snapshot, target_row = client.wait_component_settled(target_row_id)
    move_point = client.point(target_row["rect_points"])
    client.inject(
        "move",
        {
            "x": move_point["x"],
            "y": move_point["y"],
            "coordinate_space": "points",
        },
        {
            "target_component_id": target_row_id,
            "component_frame": move_snapshot["frame"],
            "target_rect_points": target_row["rect_points"],
            "pointer_lifecycle": "held_primary",
        },
    )
    wait_merge_gesture(
        client,
        "active physical Merge drag before Escape",
        lambda gesture: gesture is not None and gesture.get("target_index") == 2,
    )
    client.key("escape", True)
    client.key("escape", False)
    release_snapshot, release_row = client.wait_component_settled(target_row_id)
    release_point = client.point(release_row["rect_points"])
    client.inject(
        "release",
        {
            "x": release_point["x"],
            "y": release_point["y"],
            "coordinate_space": "points",
            "button": "primary",
        },
        {
            "target_component_id": target_row_id,
            "component_frame": release_snapshot["frame"],
            "target_rect_points": release_row["rect_points"],
            "cancelled_by": "escape",
        },
    )
    escape_after = wait_merge_gesture(
        client, "escaped Merge drag cleanup", lambda gesture: gesture is None
    )
    if (
        escape_after["project"] != escape_before["project"]
        or escape_after["history"] != escape_before["history"]
    ):
        raise QaFailure("Escape-cancelled Merge drag changed Project/history")
    assert_layout_and_viewport(
        client, escape_after, positions, escape_viewport, "Escape cancellation"
    )
    if escape_after["editor"]["preview"]["modal_error"] is not None:
        raise QaFailure("physical Merge drag surfaced a Preview error")

    return {
        "ok": True,
        "suite": "merge-reorder",
        "health": health,
        "initial_frame": initial["frame"],
        "final_frame": escape_after["frame"],
        "connection_id": connection_id,
        "original_order": original_ids,
        "dragged_order": expected_ids,
        "viewport": viewport,
        "node_positions": positions["nodes"],
        "history_after_restore": escape_after["history"],
        "undo_preview_revision": undone_rendered["editor"]["preview"][
            "render_revision"
        ],
        "actions": client.evidence,
    }


def parse_args():
    parser = argparse.ArgumentParser()
    parser.add_argument("--base-url", default=None)
    parser.add_argument("--spawn", action="store_true")
    parser.add_argument(
        "--evidence", default="target/qa-merge-reorder-evidence.json"
    )
    parser.add_argument("--timeout", type=float, default=30.0)
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
                ["cargo", "run", "-p", "app", "--locked"],
                env=environment,
                start_new_session=True,
            )
        client = QaClient(base_url, args.timeout)
        result = run_suite(client)
        result["run_id"] = os.environ.get("RUVIE_QA_RUN_ID")
        result["git_commit"] = BASE.repository_git_commit()
        result["component_frame"] = client.component_snapshot()["frame"]
        result["action_count"] = len(result["actions"])
        evidence_path = os.path.abspath(args.evidence)
        os.makedirs(os.path.dirname(evidence_path), exist_ok=True)
        with open(evidence_path, "w", encoding="utf-8") as output:
            json.dump(result, output, ensure_ascii=False, indent=2)
            output.write("\n")
        print("[qa-merge-reorder-e2e] PASS; evidence: {}".format(evidence_path))
        return 0
    except (QaFailure, AssertionError, KeyError, IndexError) as error:
        print("[qa-merge-reorder-e2e] FAIL: {}".format(error), file=sys.stderr)
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
