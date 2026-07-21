#!/usr/bin/env python3
"""Focused coordinate E2E for container-output pointer priority."""

import argparse
import copy
import importlib.util
import json
import os
import signal
import subprocess
import sys


SCRIPT_DIR = os.path.dirname(os.path.abspath(__file__))
BASE_PATH = os.path.join(SCRIPT_DIR, "qa-e2e.py")
SPEC = importlib.util.spec_from_file_location(
    "ruvie_qa_container_output_hit_base", BASE_PATH
)
if SPEC is None or SPEC.loader is None:
    raise RuntimeError("cannot load scripts/qa-e2e.py")
BASE = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(BASE)

QaClient = BASE.QaClient
QaFailure = BASE.QaFailure

SOURCE_COMPONENT = (
    "node_editor.container_port.track:{}.external_output:image".format(BASE.TRACK_B)
)
CONTAINER_COMPONENT = "node_editor.container.track:" + BASE.TRACK_B
RESIZE_COMPONENT = "node_editor.resize_edge.track:{}:tr".format(BASE.TRACK_B)
DROP_PADDING_POINTS = 5.0


def point_in_rect(point, rect):
    return (
        rect["min_x"] <= point["x"] <= rect["max_x"]
        and rect["min_y"] <= point["y"] <= rect["max_y"]
    )


def inset_rect(rect, amount):
    return {
        "min_x": rect["min_x"] + amount,
        "min_y": rect["min_y"] + amount,
        "max_x": rect["max_x"] - amount,
        "max_y": rect["max_y"] - amount,
    }


def padded_output_container_point(
    source_rect, source_unclipped_rect, container_rect, resize_rect
):
    """Pick padding owned by output despite overlapping container and resize."""
    normal_pin_rect = inset_rect(source_unclipped_rect, DROP_PADDING_POINTS)
    center_x = (source_unclipped_rect["min_x"] + source_unclipped_rect["max_x"]) * 0.5
    center_y = (source_unclipped_rect["min_y"] + source_unclipped_rect["max_y"]) * 0.5
    inset = DROP_PADDING_POINTS * 0.4
    candidates = [
        {"x": source_unclipped_rect["min_x"] + inset, "y": center_y},
        {"x": source_unclipped_rect["max_x"] - inset, "y": center_y},
        {"x": center_x, "y": source_unclipped_rect["min_y"] + inset},
        {"x": center_x, "y": source_unclipped_rect["max_y"] - inset},
    ]
    for point in candidates:
        if (
            point_in_rect(point, source_rect)
            and point_in_rect(point, container_rect)
            and point_in_rect(point, resize_rect)
            and not point_in_rect(point, normal_pin_rect)
        ):
            return point, normal_pin_rect
    raise QaFailure(
        "container output padding has no container/resize overlap outside its normal pin: "
        "source={} container={} resize={} normal={}".format(
            source_rect, container_rect, resize_rect, normal_pin_rect
        )
    )


def output_to_merge_connections(project, merge_id):
    return [
        connection
        for connection in project.get("connections", ())
        if connection.get("from", {}).get("owner", {}).get("owner_type") == "Track"
        and connection.get("from", {}).get("owner", {}).get("owner_id") == BASE.TRACK_B
        and connection.get("from", {}).get("port") == "image"
        and connection.get("to", {}).get("owner", {}).get("owner_type") == "Node"
        and connection.get("to", {}).get("owner", {}).get("owner_id") == merge_id
        and connection.get("to", {}).get("port") == "images"
    ]


def project_without_connection(project, connection_id):
    result = copy.deepcopy(project)
    result["connections"] = [
        connection
        for connection in result.get("connections", ())
        if connection.get("id") != connection_id
    ]
    return result


def first_differences(left, right, path="project", limit=8):
    differences = []
    if type(left) is not type(right):
        return ["{}: {!r} != {!r}".format(path, left, right)]
    if isinstance(left, dict):
        for key in sorted(set(left) | set(right)):
            if key not in left or key not in right:
                differences.append("{}.{}: key presence differs".format(path, key))
            elif left[key] != right[key]:
                differences.extend(
                    first_differences(
                        left[key], right[key], "{}.{}".format(path, key), limit
                    )
                )
            if len(differences) >= limit:
                break
        return differences[:limit]
    if isinstance(left, list):
        if len(left) != len(right):
            differences.append(
                "{}.length: {} != {}".format(path, len(left), len(right))
            )
        for index, (left_item, right_item) in enumerate(zip(left, right)):
            if left_item != right_item:
                differences.extend(
                    first_differences(
                        left_item,
                        right_item,
                        "{}[{}]".format(path, index),
                        limit,
                    )
                )
            if len(differences) >= limit:
                break
        return differences[:limit]
    return ["{}: {!r} != {!r}".format(path, left, right)]


def project_without_layout(value):
    if isinstance(value, dict):
        return {
            key: project_without_layout(item)
            for key, item in value.items()
            if key not in ("ui_position", "ui_size")
        }
    if isinstance(value, list):
        return [project_without_layout(item) for item in value]
    return value


def wait_project_settled(client, consecutive_frames=3):
    observed = {"frame": None, "signature": None, "stable": 0}

    def settled():
        state = client.state()
        if state["frame"] == observed["frame"]:
            return None
        signature = json.dumps(state["project"], sort_keys=True, separators=(",", ":"))
        if signature == observed["signature"]:
            observed["stable"] += 1
        else:
            observed["stable"] = 0
        observed["frame"] = state["frame"]
        observed["signature"] = signature
        return state if observed["stable"] >= consecutive_frames else None

    return client.wait_until("settled authoritative Project", settled)


def assert_only_connection_added(before_project, after_project, connection_id):
    restored = project_without_connection(after_project, connection_id)
    if restored != before_project:
        raise QaFailure(
            "wire drag changed Project outside the new connection: {}".format(
                "; ".join(first_differences(before_project, restored))
            )
        )


def component_map(snapshot):
    return {item["id"]: item for item in snapshot["components"]}


def validate_port(component, direction, owner, port):
    metadata = component.get("metadata") or {}
    address = metadata.get("address") or {}
    if not (
        metadata.get("action") == "connect_or_reconnect"
        and metadata.get("direction") == direction
        and metadata.get("normal_interaction_enabled") is True
        and address.get("owner") == owner
        and address.get("port") == port
    ):
        raise QaFailure(
            "{} omitted canonical port metadata: {}".format(
                component["id"], json.dumps(metadata, sort_keys=True)
            )
        )


def component_blocks_creation(component):
    component_id = component["id"]
    metadata = component.get("metadata") or {}
    return (
        component_id.startswith(
            (
                "node_editor.node:",
                "node_editor.node_header:",
                "node_editor.container_header.",
                "node_editor.port.",
                "node_editor.container_port.",
            )
        )
        or (
            component_id.startswith(("node_editor.edge:", "node_editor.edge."))
            and metadata.get("editable") is True
        )
    )


def nearby_composition_point(snapshot, source):
    components = component_map(snapshot)
    canvas = components["node_editor.canvas"]["rect_points"]
    composition = components[
        "node_editor.container.composition:" + BASE.COMPOSITION
    ]["rect_points"]
    nested_containers = [
        item
        for item in snapshot["components"]
        if item["id"].startswith(
            ("node_editor.container.track:", "node_editor.container.clip:")
        )
        and item.get("visible", False)
    ]
    blockers = [
        item
        for item in snapshot["components"]
        if component_blocks_creation(item) and item.get("visible", False)
    ]
    source_center = {
        "x": source["rect_points"]["center_x"],
        "y": source["rect_points"]["center_y"],
    }
    candidates = [
        {"x": source_center["x"] + dx, "y": source_center["y"] + dy}
        for dx in (90.0, 130.0, 180.0, 230.0)
        for dy in (80.0, 120.0, -80.0, -120.0, 0.0)
    ]
    for point in candidates:
        if not point_in_rect(point, canvas) or not point_in_rect(point, composition):
            continue
        if any(
            BASE.point_in_component_rect(point, item["rect_points"], padding=18.0)
            for item in nested_containers
        ):
            continue
        if any(
            BASE.point_in_component_rect(point, item["rect_points"], padding=8.0)
            for item in blockers
        ):
            continue
        if any(
            BASE.point_near_node_wire(point, item, radius=12.0)
            for item in snapshot["components"]
            if (item.get("metadata") or {}).get("kind")
            in ("explicit", "output_binding", "derived_output")
        ):
            continue
        return point
    raise QaFailure("no composition-level creation point is visible near Track B output")


def create_nearby_merge(client):
    BASE.ensure_node_editor_ports_interactive(
        client, [SOURCE_COMPONENT]
    )
    snapshot = client.component_snapshot()
    components = component_map(snapshot)
    source = components[SOURCE_COMPONENT]
    canvas = components["node_editor.canvas"]["rect_points"]
    desired_source_x = canvas["max_x"] - 280.0
    horizontal_delta = desired_source_x - source["rect_points"]["center_x"]
    if abs(horizontal_delta) > 12.0:
        pan_start = {"x": canvas["center_x"], "y": canvas["center_y"]}
        client.inject(
            "drag",
            {
                "from": pan_start,
                "to": {
                    "x": pan_start["x"] + horizontal_delta,
                    "y": pan_start["y"],
                },
                "coordinate_space": "points",
                "button": "middle",
                "steps": 8,
            },
            {
                "component_id": "node_editor.canvas",
                "component_frame": snapshot["frame"],
                "coordinate_reason": "leave visible composition room beside Track B output",
            },
        )
    client.wait_component_settled(SOURCE_COMPONENT)
    before = wait_project_settled(client)
    snapshot = client.component_snapshot()
    source = component_map(snapshot)[SOURCE_COMPONENT]
    point = nearby_composition_point(snapshot, source)
    nodes_before = set(before["project"]["nodes"])
    client.inject(
        "click",
        {
            "x": point["x"],
            "y": point["y"],
            "coordinate_space": "points",
            "button": "secondary",
        },
        {
            "component_id": "node_editor.canvas",
            "component_frame": snapshot["frame"],
            "coordinate_reason": "fresh composition-only point near Track B output",
        },
    )
    client.wait_component("node_editor.menu.search")
    client.replace_component_text("node_editor.menu.search", "merge")
    client.wait_component_settled("node_editor.menu.create.merge")
    client.click_component("node_editor.menu.create.merge")
    created = client.wait_project(
        "nearby temporary Merge creation",
        lambda project: len(project["nodes"]) == len(nodes_before) + 1,
    )
    BASE.assert_history_delta(before, created, 1, "temporary Merge creation")
    merge_id = (set(created["project"]["nodes"]) - nodes_before).pop()
    owner = BASE.validate_canonical_ownership(created["project"])["node_owners"].get(
        merge_id
    )
    if owner != "composition:" + BASE.COMPOSITION:
        raise QaFailure("temporary Merge was not created at Composition scope")
    if created["project"]["nodes"][merge_id]["content"].get("type") != "Merge":
        raise QaFailure("searchable Add menu did not create a Merge Node")
    return merge_id, created, point, before


def fresh_drag_geometry(client, merge_id):
    target_component = "node_editor.port.node:{}.input:images".format(merge_id)
    component_ids = [SOURCE_COMPONENT, target_component]
    snapshot, _ = BASE.ensure_node_editor_ports_interactive(client, component_ids)
    components = component_map(snapshot)
    source = components[SOURCE_COMPONENT]
    container = components[CONTAINER_COMPONENT]
    resize = components[RESIZE_COMPONENT]
    target = components[target_component]
    validate_port(source, "output", "track:" + BASE.TRACK_B, "image")
    validate_port(target, "input", "node:" + merge_id, "images")
    source_unclipped = (source.get("metadata") or {}).get("unclipped_rect")
    if source_unclipped is None:
        raise QaFailure("container output omitted its unclipped drop rectangle")
    start, normal_pin_rect = padded_output_container_point(
        source["rect_points"],
        source_unclipped,
        container["rect_points"],
        resize["rect_points"],
    )
    end = client.point(target["rect_points"])
    return {
        "snapshot": snapshot,
        "source": source,
        "container": container,
        "resize": resize,
        "target": target,
        "target_component": target_component,
        "start": start,
        "end": end,
        "normal_pin_rect": normal_pin_rect,
        "overlaps_resize": True,
    }


def run_suite(client):
    health = client.wait_health()
    fixture_initial = BASE.wait_fresh_fixture(client)
    BASE.activate_dock_tab(
        client, "dock.tab:node_editor", "Node Editor", "container output priority"
    )
    merge_id, created, creation_point, creation_before = create_nearby_merge(client)
    geometry = fresh_drag_geometry(client, merge_id)
    before = client.state()
    before_project = before["project"]
    if before_project != created["project"]:
        raise QaFailure("viewport setup changed Project after temporary Merge creation")
    if output_to_merge_connections(before_project, merge_id):
        raise QaFailure("temporary Merge input did not begin vacant")

    # This physical start point is deliberately outside the smaller Snarl pin
    # rectangle while still inside both the real output drop target and the
    # broad container frame. The output must own every injected pointer frame.
    client.inject(
        "drag",
        {
            "from": geometry["start"],
            "to": geometry["end"],
            "coordinate_space": "points",
            "button": "primary",
            "steps": 16,
        },
        {
            "source_component_id": SOURCE_COMPONENT,
            "target_component_id": geometry["target_component"],
            "competing_component_ids": [CONTAINER_COMPONENT, RESIZE_COMPONENT],
            "component_frame": geometry["snapshot"]["frame"],
            "source_rect_points": geometry["source"]["rect_points"],
            "source_normal_pin_rect_points": geometry["normal_pin_rect"],
            "container_rect_points": geometry["container"]["rect_points"],
            "resize_rect_points": geometry["resize"]["rect_points"],
            "target_rect_points": geometry["target"]["rect_points"],
            "start_overlaps_container": True,
            "start_overlaps_resize": geometry["overlaps_resize"],
            "coordinate_reason": (
                "fresh output padding point competing with broad container drag"
            ),
        },
    )

    connected = client.wait_project(
        "container output padding drag connection",
        lambda project: len(output_to_merge_connections(project, merge_id)) == 1,
    )
    BASE.assert_history_delta(before, connected, 1, "container output wire drag")
    connection = output_to_merge_connections(connected["project"], merge_id)[0]
    assert_only_connection_added(before_project, connected["project"], connection["id"])

    undone = BASE.undo_project_edit(
        client,
        "container output wire drag",
        lambda project: project == before_project,
    )
    redone = BASE.redo_project_edit(
        client,
        "container output wire drag",
        lambda project: project == connected["project"],
    )
    assert_only_connection_added(before_project, redone["project"], connection["id"])
    restored = BASE.undo_project_edit(
        client,
        "container output wire drag after Redo",
        lambda project: project == before_project,
    )
    fixture_restored = BASE.undo_project_edit(
        client,
        "temporary Merge creation",
        lambda project: project == creation_before["project"],
    )
    if (
        fixture_restored["history"]["undo_depth"]
        != creation_before["history"]["undo_depth"]
    ):
        raise QaFailure("temporary Merge cleanup did not restore the Undo depth")
    if merge_id in fixture_restored["project"]["nodes"] or any(
        item.get("id") == connection["id"]
        for item in fixture_restored["project"]["connections"]
    ):
        raise QaFailure("temporary Merge or output wire remained after final restore")
    if project_without_layout(fixture_initial["project"]) != project_without_layout(
        creation_before["project"]
    ):
        raise QaFailure("Node Editor activation changed non-layout fixture data")
    if restored["editor"]["preview"]["modal_error"] is not None:
        raise QaFailure("container output wire drag surfaced a Preview error")

    return {
        "ok": True,
        "suite": "container-output-hit-priority",
        "health": health,
        "initial_frame": before["frame"],
        "fixture_initial_frame": fixture_initial["frame"],
        "create_baseline_frame": creation_before["frame"],
        "connected_frame": connected["frame"],
        "undo_frame": undone["frame"],
        "redo_frame": redone["frame"],
        "final_frame": fixture_restored["frame"],
        "connection_id": connection["id"],
        "temporary_merge_id": merge_id,
        "temporary_merge_creation_point": creation_point,
        "source_component_id": SOURCE_COMPONENT,
        "target_component_id": geometry["target_component"],
        "start_point": geometry["start"],
        "start_overlaps_resize": geometry["overlaps_resize"],
        "history_after_restore": fixture_restored["history"],
        "fixture_to_create_baseline_layout_differences": first_differences(
            fixture_initial["project"], creation_before["project"]
        ),
        "actions": client.evidence,
    }


def parse_args():
    parser = argparse.ArgumentParser()
    parser.add_argument("--base-url", default=None)
    parser.add_argument("--spawn", action="store_true")
    parser.add_argument(
        "--evidence", default="target/qa-container-output-hit-evidence.json"
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
        print("[qa-container-output-hit-e2e] PASS; evidence: {}".format(evidence_path))
        return 0
    except (QaFailure, AssertionError, KeyError, IndexError) as error:
        print("[qa-container-output-hit-e2e] FAIL: {}".format(error), file=sys.stderr)
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
