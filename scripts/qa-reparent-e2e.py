#!/usr/bin/env python3
"""Fast coordinate E2E for Node Editor geometry-based reparenting.

The fixture is never mutated through an internal QA command.  The semantic
edit starts with a fresh rendered Node header rectangle, keeps a real primary
button pressed while querying hover geometry, and finishes with a coordinate
release through egui's normal hit-testing path.
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
SPEC = importlib.util.spec_from_file_location("ruvie_qa_reparent_base", BASE_PATH)
if SPEC is None or SPEC.loader is None:
    raise RuntimeError("cannot load scripts/qa-e2e.py")
BASE = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(BASE)

QaClient = BASE.QaClient
QaFailure = BASE.QaFailure
free_port = BASE.free_port

SOURCE_NODE = BASE.MERGE
SOURCE_CLIP = BASE.CLIP_A1
TARGET_CLIP = BASE.CLIP_A2
TARGET_CLIP_OUTPUT = BASE.BLUR_EFFECT


def component_from_snapshot(snapshot, component_id):
    component = next(
        (item for item in snapshot["components"] if item["id"] == component_id),
        None,
    )
    if component is None:
        raise QaFailure(
            "component {!r} absent in frame {}".format(
                component_id, snapshot["frame"]
            )
        )
    return component


def owner_for_node(project, node_id):
    owners = BASE.validate_canonical_ownership(project)["node_owners"]
    try:
        return owners[node_id]
    except KeyError as error:
        raise QaFailure("Node {} has no canonical owner".format(node_id)) from error


def connection_by_endpoints(project, source_id, target_id):
    matches = [
        connection
        for connection in project["connections"]
        if connection["from"]["owner"]
        == {"owner_type": "Node", "owner_id": source_id}
        and connection["to"]["owner"]
        == {"owner_type": "Node", "owner_id": target_id}
    ]
    if len(matches) != 1:
        raise QaFailure(
            "expected one {} -> {} wire, found {}".format(
                source_id, target_id, len(matches)
            )
        )
    return matches[0]


def point_for_centered_node(source_node_rect, source_pointer, target_rect):
    pointer_offset = {
        "x": source_pointer["x"] - source_node_rect["min_x"],
        "y": source_pointer["y"] - source_node_rect["min_y"],
    }
    desired_min = {
        "x": target_rect["center_x"] - source_node_rect["width"] * 0.5,
        "y": target_rect["center_y"] - source_node_rect["height"] * 0.5,
    }
    return {
        "x": desired_min["x"] + pointer_offset["x"],
        "y": desired_min["y"] + pointer_offset["y"],
    }


def inject_pointer(client, endpoint, point, evidence):
    client.inject(
        endpoint,
        {
            "x": point["x"],
            "y": point["y"],
            "coordinate_space": "points",
            "button": "primary",
        },
        evidence,
    )


def wait_selected_target(client, component_id):
    def selected():
        snapshot = client.component_snapshot()
        try:
            component = component_from_snapshot(snapshot, component_id)
        except QaFailure:
            return None
        metadata = component.get("metadata") or {}
        state = client.state()["editor"]["node_editor"].get("reparent_gesture")
        if (
            metadata.get("selected") is True
            and metadata.get("eligible") is True
            and state is not None
            and state.get("hovered_target") == "clip:" + TARGET_CLIP
            and state.get("primary_node_id") == SOURCE_NODE
        ):
            return snapshot, component, state
        return None

    return client.wait_until("destination Clip geometry reparent hover", selected)


def assert_reparented_state(before, after, explicit_wire):
    if owner_for_node(after["project"], SOURCE_NODE) != "clip:" + TARGET_CLIP:
        raise QaFailure("Merge Node did not adopt the destination Clip as canonical owner")
    if after["project"]["clips"][SOURCE_CLIP]["output_node_id"] is not None:
        raise QaFailure("old Clip retained a cross-owner output binding")
    if (
        after["project"]["clips"][TARGET_CLIP]["output_node_id"]
        != TARGET_CLIP_OUTPUT
    ):
        raise QaFailure("destination Clip output binding was overwritten")
    before_connections = before["project"]["connections"]
    after_connections = after["project"]["connections"]
    if [item["id"] for item in after_connections] != [
        item["id"] for item in before_connections
    ]:
        raise QaFailure("geometry reparent added, removed, or reordered a typed wire")
    remapped_scope_wires = []
    for before_wire, after_wire in zip(before_connections, after_connections):
        if before_wire == after_wire:
            continue
        expected = dict(before_wire)
        expected["from"] = {
            "owner": {"owner_type": "Clip", "owner_id": TARGET_CLIP},
            "port": before_wire["from"]["port"],
        }
        if not (
            before_wire["from"]["owner"]
            == {"owner_type": "Clip", "owner_id": SOURCE_CLIP}
            and before_wire["from"]["port"] in ("time", "duration", "resolution")
            and before_wire["to"]["owner"]
            == {"owner_type": "Node", "owner_id": SOURCE_NODE}
            and after_wire == expected
        ):
            raise QaFailure(
                "geometry reparent changed a non-scope wire: before={!r}, after={!r}".format(
                    before_wire, after_wire
                )
            )
        remapped_scope_wires.append(before_wire["id"])
    if not remapped_scope_wires:
        raise QaFailure("fixture did not prove stable-ID internal Time scope remapping")
    if connection_by_endpoints(after["project"], BASE.SOLID, SOURCE_NODE) != explicit_wire:
        raise QaFailure("cross-container explicit wire lost identity or metadata")
    BASE.assert_history_delta(before, after, 1, "Node geometry reparent")
    if after["history"]["redo_depth"] != 0:
        raise QaFailure("new reparent gesture left stale redo history")
    if after["editor"]["node_editor"].get("reparent_gesture") is not None:
        raise QaFailure("reparent gesture remained latched after pointer release")
    return remapped_scope_wires


def run_suite(client):
    health = client.wait_health()
    BASE.wait_fresh_fixture(client)
    client.click_component("dock.tab:node_editor")
    client.wait_until(
        "Node Editor dock activation",
        lambda: client.state()
        if "Node Editor" in client.state()["dock"]["active_tabs"]
        else None,
    )
    BASE.reveal_node_editor_component(client, "node_editor.node_header:" + SOURCE_NODE)

    snapshot = client.component_snapshot()
    source_header = component_from_snapshot(
        snapshot, "node_editor.node_header:" + SOURCE_NODE
    )
    source_node = component_from_snapshot(snapshot, "node_editor.node:" + SOURCE_NODE)
    target_container = component_from_snapshot(
        snapshot, "node_editor.container.clip:" + TARGET_CLIP
    )
    for component in (source_header, source_node, target_container):
        rect = component["rect_points"]
        if not component.get("visible") or rect["width"] <= 0 or rect["height"] <= 0:
            raise QaFailure(
                "{} is not simultaneously visible for reparent".format(component["id"])
            )

    before = client.state()
    if owner_for_node(before["project"], SOURCE_NODE) != "clip:" + SOURCE_CLIP:
        raise QaFailure("fixture Merge Node does not start in Clip A1")
    if before["project"]["clips"][SOURCE_CLIP]["output_node_id"] != SOURCE_NODE:
        raise QaFailure("fixture Clip A1 output is not the Merge Node")
    explicit_wire = connection_by_endpoints(before["project"], BASE.SOLID, SOURCE_NODE)
    preview_before = before["editor"]["preview"]
    start = client.point(source_header["rect_points"])
    first_target = client.point(target_container["rect_points"], 0.55, 0.72)

    common_evidence = {
        "source_component_id": source_header["id"],
        "source_component_frame": snapshot["frame"],
        "source_header_rect_points": source_header["rect_points"],
        "source_node_rect_points": source_node["rect_points"],
        "coordinate_reason": "fresh rendered Node header and nested Clip geometry",
    }
    inject_pointer(client, "move", start, common_evidence)
    inject_pointer(client, "press", start, common_evidence)
    inject_pointer(
        client,
        "move",
        first_target,
        {
            **common_evidence,
            "target_component_id": target_container["id"],
            "target_component_rect_points": target_container["rect_points"],
        },
    )

    target_id = "node_editor.reparent_target.clip:" + TARGET_CLIP

    def target_geometry():
        current = client.component_snapshot()
        try:
            target = component_from_snapshot(current, target_id)
        except QaFailure:
            return None
        rect = target["rect_points"]
        return (current, target) if rect["width"] > 0 and rect["height"] > 0 else None

    target_snapshot, target = client.wait_until(
        "published destination Clip reparent content rect", target_geometry
    )
    final_point = point_for_centered_node(
        source_node["rect_points"], start, target["rect_points"]
    )
    inject_pointer(
        client,
        "move",
        final_point,
        {
            "target_component_id": target_id,
            "target_component_frame": target_snapshot["frame"],
            "target_content_rect_points": target["rect_points"],
            "target_metadata": target.get("metadata"),
            "coordinate_reason": "center exact final Node rect in published legal content",
        },
    )
    selected_snapshot, selected_target, selected_state = wait_selected_target(
        client, target_id
    )
    inject_pointer(
        client,
        "release",
        final_point,
        {
            "target_component_id": target_id,
            "target_component_frame": selected_snapshot["frame"],
            "target_content_rect_points": selected_target["rect_points"],
            "target_metadata": selected_target.get("metadata"),
            "gesture_state_before_release": selected_state,
            "coordinate_reason": "actual primary release on selected legal Clip target",
        },
    )

    after = client.wait_project(
        "Merge Node geometry reparent",
        lambda project: project
        if owner_for_node(project, SOURCE_NODE) == "clip:" + TARGET_CLIP
        else None,
    )
    remapped_scope_wires = assert_reparented_state(before, after, explicit_wire)
    rendered_after = client.wait_preview_change(
        preview_before["pixel_hash"], preview_before["render_revision"]
    )
    edge_id = "node_editor.edge:" + explicit_wire["id"]
    _, edge = client.wait_component(edge_id)
    if (edge.get("metadata") or {}).get("connection_id") != explicit_wire["id"]:
        raise QaFailure("rendered cross-container wire no longer names its Project UUID")

    undone = BASE.undo_project_edit(
        client,
        "Node geometry reparent",
        lambda project: project == before["project"],
    )
    restored = BASE.wait_preview_hash_after(
        client,
        preview_before["pixel_hash"],
        rendered_after["editor"]["preview"]["render_revision"],
        "Node geometry reparent undo",
    )
    redone = BASE.redo_project_edit(
        client,
        "Node geometry reparent",
        lambda project: project == after["project"],
    )
    BASE.wait_preview_hash_after(
        client,
        rendered_after["editor"]["preview"]["pixel_hash"],
        restored["editor"]["preview"]["render_revision"],
        "Node geometry reparent redo",
    )
    if undone["history"]["redo_depth"] != 1:
        raise QaFailure("one reparent gesture did not undo as one history entry")
    if redone["history"]["redo_depth"] != 0:
        raise QaFailure("reparent redo did not consume its one history entry")

    return {
        "ok": True,
        "suite": "node-reparent",
        "health": health,
        "initial_owner": "clip:" + SOURCE_CLIP,
        "final_owner": "clip:" + TARGET_CLIP,
        "wire_id": explicit_wire["id"],
        "remapped_scope_wire_ids": remapped_scope_wires,
        "initial_history": before["history"],
        "final_history": redone["history"],
        "initial_preview": preview_before,
        "reparented_preview": rendered_after["editor"]["preview"],
        "actions": client.evidence,
    }


def parse_args():
    parser = argparse.ArgumentParser()
    parser.add_argument("--base-url", default=None)
    parser.add_argument("--spawn", action="store_true")
    parser.add_argument(
        "--evidence",
        default="target/qa-reparent-e2e-evidence.json",
    )
    parser.add_argument("--timeout", type=float, default=45.0)
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
        result["git_commit"] = BASE.repository_git_commit()
        result["component_frame"] = client.component_snapshot()["frame"]
        result["action_count"] = len(result["actions"])
        evidence_path = os.path.abspath(args.evidence)
        os.makedirs(os.path.dirname(evidence_path), exist_ok=True)
        with open(evidence_path, "w", encoding="utf-8") as output:
            json.dump(result, output, ensure_ascii=False, indent=2)
            output.write("\n")
        print("[qa-reparent-e2e] PASS; evidence: {}".format(evidence_path))
        return 0
    except (QaFailure, AssertionError, KeyError, TypeError, ValueError) as error:
        print("[qa-reparent-e2e] FAIL: {}".format(error), file=sys.stderr)
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
