#!/usr/bin/env python3
"""Coordinate E2E for transient inherited Time presentation."""

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


def time_connection(project):
    matches = [
        connection
        for connection in project.get("connections", ())
        if connection.get("from", {}).get("owner", {}).get("owner_type") == "Clip"
        and connection.get("from", {}).get("owner", {}).get("owner_id")
        == BASE.CLIP_A1
        and connection.get("from", {}).get("port") == "time"
        and connection.get("to", {}).get("owner", {}).get("owner_type") == "Node"
        and connection.get("to", {}).get("owner", {}).get("owner_id") == BASE.MERGE
        and connection.get("to", {}).get("port") == "time"
    ]
    if len(matches) > 1:
        raise QaFailure("Merge has duplicate explicit Time connections")
    return matches[0] if matches else None


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
                    first_differences(left[key], right[key], "{}.{}".format(path, key), limit)
                )
            if len(differences) >= limit:
                break
        return differences[:limit]
    if isinstance(left, list):
        if len(left) != len(right):
            differences.append("{}.length: {} != {}".format(path, len(left), len(right)))
        for index, (left_item, right_item) in enumerate(zip(left, right)):
            if left_item != right_item:
                differences.extend(
                    first_differences(
                        left_item, right_item, "{}[{}]".format(path, index), limit
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


def wait_badge(client, component_id, expected_state):
    def ready():
        try:
            snapshot, component = client.component(component_id)
        except QaFailure:
            return None
        metadata = component.get("metadata") or {}
        if metadata.get("state") != expected_state:
            return None
        expected_prefix = "Explicit ·" if expected_state == "explicit" else "Inherited ·"
        if not str(metadata.get("label", "")).startswith(expected_prefix):
            raise QaFailure(
                "{} omitted its {} label".format(component_id, expected_prefix)
            )
        if metadata.get("editable") is not False:
            raise QaFailure("{} must be read-only".format(component_id))
        if metadata.get("presentation_persisted") is not False:
            raise QaFailure("{} must remain presentation-only".format(component_id))
        return snapshot, component

    return client.wait_until(
        "{} {} presentation".format(component_id, expected_state), ready
    )


def wait_virtual_wire(client, selected=None, hovered=None):
    component_id = "node_editor.time_context_wire.node:" + BASE.MERGE

    def ready():
        client.state()
        snapshot = client.component_snapshot()
        component = next(
            (item for item in snapshot["components"] if item["id"] == component_id),
            None,
        )
        if component is None:
            return None
        rect = component["rect_points"]
        metadata = component.get("metadata") or {}
        if not (
            component.get("visible")
            and rect["width"] > 0.0
            and rect["height"] > 0.0
            and metadata.get("kind") == "implicit_time"
            and metadata.get("state") == "inherited"
            and metadata.get("editable") is False
            and metadata.get("presentation_persisted") is False
            and metadata.get("derived_state_persisted") is False
            and metadata.get("hit_testable") is False
            and metadata.get("wire_collection") == "context_only"
            and metadata.get("dashed") is True
        ):
            raise QaFailure("implicit Time wire metadata is not transient/read-only")
        if selected is not None and metadata.get("selected") is not selected:
            return None
        if hovered is not None and metadata.get("hovered") is not hovered:
            return None
        return snapshot, component

    return client.wait_until("implicit Time context wire", ready)


def wait_virtual_absent(client):
    component_id = "node_editor.time_context_wire.node:" + BASE.MERGE

    def absent():
        client.state()
        snapshot = client.component_snapshot()
        return (
            snapshot
            if all(item["id"] != component_id for item in snapshot["components"])
            else None
        )

    return client.wait_until("implicit Time context wire hidden", absent)


def cubic_wire_point(metadata, fraction):
    start = metadata["from"]
    end = metadata["to"]
    frame = min(max(abs(end["x"] - start["x"]) * 0.45, 36.0), 110.0)
    control_a = {"x": start["x"] + frame, "y": start["y"]}
    control_b = {"x": end["x"] - frame, "y": end["y"]}
    inverse = 1.0 - fraction
    weights = (
        inverse**3,
        3.0 * inverse**2 * fraction,
        3.0 * inverse * fraction**2,
        fraction**3,
    )
    return {
        "x": sum(
            point["x"] * weight
            for point, weight in zip(
                (start, control_a, control_b, end), weights
            )
        ),
        "y": sum(
            point["y"] * weight
            for point, weight in zip(
                (start, control_a, control_b, end), weights
            )
        ),
    }


def click_wire_at_unobstructed_curve_point(client, component_id, button):
    """Click the fresh physical curve where no Snarl graph item owns input."""
    snapshot, component = client.component(component_id)
    metadata = component.get("metadata") or {}
    canvas = next(
        item for item in snapshot["components"] if item["id"] == "node_editor.canvas"
    )
    graph_item_prefixes = (
        "node_editor.node:",
        "node_editor.node_header:",
        "node_editor.container_header.",
        "node_editor.port.",
        "node_editor.container_port.",
        "node_editor.resize_edge.",
    )
    blockers = [
        item
        for item in snapshot["components"]
        if item.get("visible", False)
        and item["id"].startswith(graph_item_prefixes)
    ]
    fractions = (0.5, 0.4, 0.6, 0.3, 0.7, 0.2, 0.8, 0.15, 0.85)
    for fraction in fractions:
        point = cubic_wire_point(metadata, fraction)
        if not BASE.point_in_component_rect(point, canvas["rect_points"], padding=-4.0):
            continue
        if any(
            BASE.point_in_component_rect(point, item["rect_points"], padding=1.0)
            for item in blockers
        ):
            continue
        client.inject(
            "click",
            {
                "x": point["x"],
                "y": point["y"],
                "coordinate_space": "points",
                "button": button,
            },
            {
                "component_id": component_id,
                "component_frame": snapshot["frame"],
                "component_rect_points": component["rect_points"],
                "component_curve_point": point,
                "curve_fraction": fraction,
                "coordinate_reason": "fresh unobstructed physical wire curve",
            },
        )
        return point
    raise QaFailure("physical Time wire has no unobstructed curve point")


def activate_node_editor(client):
    return BASE.activate_dock_tab(
        client, "dock.tab:node_editor", "Node Editor", "implicit Time"
    )


def activate_inspector(client):
    return BASE.activate_dock_tab(
        client, "dock.tab:inspector", "Inspector", "implicit Time Inspector"
    )


def run_suite(client):
    health = client.wait_health()
    BASE.wait_fresh_fixture(client)
    activate_node_editor(client)

    header_id = "node_editor.node_header:" + BASE.MERGE
    source_id = "node_editor.container_port.clip:{}.internal_output:time".format(
        BASE.CLIP_A1
    )
    target_id = "node_editor.port.node:{}.input:time".format(BASE.MERGE)
    badge_id = "node_editor.time_source.node:" + BASE.MERGE
    inspector_badge_id = "inspector.time_source.node:" + BASE.MERGE

    BASE.ensure_node_editor_ports_interactive(
        client, [header_id, source_id, target_id]
    )
    baseline = client.state()
    if time_connection(baseline["project"]) is not None:
        raise QaFailure("fixture Merge must begin with inherited Time")

    # Hover through the normal pointer path. Context must remain UI-only.
    hover_snapshot, header = client.component(header_id)
    hover_point = client.point(header["rect_points"])
    client.inject(
        "move",
        {
            "x": hover_point["x"],
            "y": hover_point["y"],
            "coordinate_space": "points",
        },
        {
            "component_id": header_id,
            "component_frame": hover_snapshot["frame"],
            "component_rect_points": header["rect_points"],
            "coordinate_reason": "hover the inherited-Time Node",
        },
    )
    hover_wire_snapshot, _ = wait_virtual_wire(client, hovered=True)
    hovered = client.state()
    if hovered["project"] != baseline["project"]:
        raise QaFailure("hovering inherited Time changed Project")
    if hovered["history"] != baseline["history"]:
        raise QaFailure("hovering inherited Time changed history")

    # Selection is a real header click; the same wire records selected context.
    client.click_component(header_id)
    selected = client.wait_until(
        "exact Merge selection",
        lambda: state
        if BASE.selection_matches(state := client.state(), "node", BASE.MERGE)
        else None,
    )
    wait_virtual_wire(client, selected=True)
    wait_badge(client, badge_id, "inherited")
    if selected["project"] != baseline["project"]:
        raise QaFailure("selecting inherited Time changed Project")
    if selected["history"] != baseline["history"]:
        raise QaFailure("selecting inherited Time changed history")

    activate_inspector(client)
    wait_badge(client, inspector_badge_id, "inherited")
    activate_node_editor(client)

    # Re-query after the tab/layout transition, then use a real primary drag.
    BASE.ensure_node_editor_ports_interactive(client, [source_id, target_id])
    connect_before = client.state()
    client.drag_components(source_id, target_id, steps=16)
    connected = client.wait_project(
        "explicit Clip Time to Merge Time connection",
        lambda project: time_connection(project) is not None,
    )
    connection = time_connection(connected["project"])
    BASE.assert_history_delta(connect_before, connected, 1, "explicit Time connection")
    wait_virtual_absent(client)
    wait_badge(client, badge_id, "explicit")

    activate_inspector(client)
    wait_badge(client, inspector_badge_id, "explicit")
    undone = BASE.undo_project_edit(
        client,
        "explicit Time connection",
        lambda project: time_connection(project) is None,
    )
    wait_badge(client, inspector_badge_id, "inherited")
    if undone["project"] != connect_before["project"]:
        raise QaFailure("Undo did not restore the byte-identical implicit Project")

    activate_node_editor(client)
    BASE.reveal_node_editor_components(client, [header_id, source_id, target_id])
    undo_wire_snapshot, _ = wait_virtual_wire(client, selected=True)
    settled_implicit = wait_project_settled(client)

    # Redo, then disconnect through the real physical-wire context menu.
    redone = BASE.redo_project_edit(
        client,
        "explicit Time connection",
        lambda project: time_connection(project) is not None,
    )
    connection = time_connection(redone["project"])
    wait_virtual_absent(client)
    edge_id = "node_editor.edge:" + connection["id"]
    BASE.reveal_node_editor_components(client, [edge_id, header_id])
    client.wait_component_settled(edge_id)
    delete_before = client.state()
    click_wire_at_unobstructed_curve_point(client, edge_id, button="secondary")
    delete_id = "node_editor.wire_menu.delete:" + connection["id"]
    client.wait_component(delete_id)
    client.click_component(delete_id)
    disconnected = client.wait_project(
        "explicit Time wire disconnect",
        lambda project: time_connection(project) is None,
    )
    BASE.assert_history_delta(delete_before, disconnected, 1, "Time disconnect")
    disconnect_wire_snapshot, _ = wait_virtual_wire(client, selected=True)
    wait_badge(client, badge_id, "inherited")
    disconnected = wait_project_settled(client)
    # Baseline main canonicalizes the containing Clip geometry after the same
    # real Time-wire topology edit, even though the Node rect is unchanged.
    # Keep that existing layout boundary explicit while requiring every
    # serialized semantic field to return byte-for-byte.
    expected_semantics = project_without_layout(settled_implicit["project"])
    disconnected_semantics = project_without_layout(disconnected["project"])
    if disconnected_semantics != expected_semantics:
        raise QaFailure(
            "disconnect changed the settled implicit Project semantics: {}".format(
                "; ".join(
                    first_differences(expected_semantics, disconnected_semantics)
                )
            )
        )

    return {
        "ok": True,
        "suite": "implicit-time",
        "health": health,
        "connection_id": connection["id"],
        "hover_wire_frame": hover_wire_snapshot["frame"],
        "undo_wire_frame": undo_wire_snapshot["frame"],
        "disconnect_wire_frame": disconnect_wire_snapshot["frame"],
        "layout_delta_after_authored_connection": first_differences(
            settled_implicit["project"], disconnected["project"]
        ),
        "final_history": disconnected["history"],
        "actions": client.evidence,
    }


def parse_args():
    parser = argparse.ArgumentParser()
    parser.add_argument("--base-url", default=None)
    parser.add_argument("--spawn", action="store_true")
    parser.add_argument("--evidence", default="target/qa-implicit-time-evidence.json")
    parser.add_argument("--timeout", type=float, default=45.0)
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
                cwd=os.path.dirname(SCRIPT_DIR),
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
        print("[qa-implicit-time-e2e] PASS; evidence: {}".format(evidence_path))
        return 0
    except (QaFailure, AssertionError, KeyError, IndexError, TypeError) as error:
        print("[qa-implicit-time-e2e] FAIL: {}".format(error), file=sys.stderr)
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
