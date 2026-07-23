#!/usr/bin/env python3
"""Focused real-coordinate E2E for the native Trim Path operation."""

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

CLIP_ID = BASE.CLIP_B1
SOURCE_ID = BASE.SHAPE_TRANSFORM
TARGET_ID = BASE.SHAPE_STROKE
MENU_ID = "node_editor.menu.create.path_effect:trim"
NORMALIZED = "Normalized (1 = full path)"
LENGTH = "Length (px)"


def activate_node_editor(client):
    return BASE.activate_dock_tab(
        client, "dock.tab:node_editor", "Node Editor", "Trim Path E2E"
    )


def property_value(state, node_id, name):
    return BASE.property_value(state["project"]["nodes"][node_id], name)


def expected_properties(mode, start, end, offset):
    return {"mode": mode, "start": start, "end": end, "offset": offset}


def assert_properties(state, node_id, expected, operation):
    actual = {
        name: property_value(state, node_id, name)
        for name in ("mode", "start", "end", "offset")
    }
    if actual != expected:
        raise QaFailure(
            "{} authoritative properties {!r}, expected {!r}".format(
                operation, actual, expected
            )
        )
    return actual


def preview_evidence(state):
    preview = state["editor"]["preview"]
    return {
        "render_revision": preview["render_revision"],
        "pixel_hash": preview["pixel_hash"],
        "nontransparent_pixels": preview["nontransparent_pixels"],
        "modal_error": preview["modal_error"],
        "scheduler": preview["scheduler"],
    }


def assert_valid_preview(state, operation):
    preview = state["editor"]["preview"]
    scheduler = preview["scheduler"]
    if (
        preview["render_revision"] <= 0
        or preview["pixel_hash"] is None
        or preview["nontransparent_pixels"] is None
        or preview["nontransparent_pixels"] <= 0
        or preview["modal_error"] is not None
        or scheduler["in_flight_request"] is not None
        or scheduler["desired_pending"] is not False
    ):
        raise QaFailure("{} did not leave a completed valid Preview".format(operation))
    return state


def wait_preview_idle(client, operation):
    return client.wait_until(
        "idle Preview after {}".format(operation),
        lambda: assert_valid_preview(state, operation)
        if (
            (state := client.state())["editor"]["preview"]["render_revision"] > 0
            and state["editor"]["preview"]["scheduler"]["in_flight_request"]
            is None
            and state["editor"]["preview"]["scheduler"]["desired_pending"]
            is False
        )
        else None,
    )


def wait_preview_result_after(
    client, before, operation, project_predicate, expected_hash=None
):
    before_preview = before["editor"]["preview"]
    before_scheduler = before_preview["scheduler"]
    observed = {"frame": None, "signature": None, "stable": 0}

    def completed():
        state = client.state()
        if not project_predicate(state):
            return None
        preview = state["editor"]["preview"]
        scheduler = preview["scheduler"]
        if not (
            preview["render_revision"] > before_preview["render_revision"]
            and scheduler["generation"] > before_scheduler["generation"]
            and scheduler["published"] > before_scheduler["published"]
            and scheduler["in_flight_request"] is None
            and scheduler["desired_pending"] is False
            and preview["pixel_hash"] is not None
            and preview["nontransparent_pixels"] is not None
            and preview["nontransparent_pixels"] > 0
            and preview["modal_error"] is None
            and (expected_hash is None or preview["pixel_hash"] == expected_hash)
        ):
            return None
        signature = (
            scheduler["generation"],
            scheduler["published"],
            preview["render_revision"],
            preview["pixel_hash"],
        )
        if state["frame"] == observed["frame"]:
            return None
        if signature == observed["signature"]:
            observed["stable"] += 1
        else:
            observed["signature"] = signature
            observed["stable"] = 0
        observed["frame"] = state["frame"]
        return state if observed["stable"] >= 1 else None

    state = client.wait_until(
        "published Preview result after {}".format(operation), completed
    )
    return assert_valid_preview(state, operation)


def reveal_detailed_node(client, node_id, max_zooms=16):
    header_id = "node_editor.node_header:" + node_id
    for _ in range(max_zooms):
        BASE.reveal_node_editor_component(client, header_id)
        snapshot = client.component_snapshot()
        components = {item["id"]: item for item in snapshot["components"]}
        canvas = components.get("node_editor.canvas")
        header = components.get(header_id)
        if canvas is None or header is None:
            continue
        scale = float((canvas.get("metadata") or {}).get("scale", 0.0))
        if (
            (canvas.get("metadata") or {}).get("detail_enabled") is True
            and scale >= 0.75
        ):
            return BASE.reveal_node_editor_component(client, header_id)
        canvas_rect = canvas["rect_points"]
        focus = client.point(header["rect_points"])
        focus["x"] = max(
            canvas_rect["min_x"] + 8.0,
            min(canvas_rect["max_x"] - 8.0, focus["x"]),
        )
        focus["y"] = max(
            canvas_rect["min_y"] + 8.0,
            min(canvas_rect["max_y"] - 8.0, focus["y"]),
        )
        previous_scale = scale
        client.inject(
            "scroll",
            {
                "x": focus["x"],
                "y": focus["y"],
                "delta_x": 0.0,
                "delta_y": 90.0,
                "coordinate_space": "points",
                "modifiers": {"command": True},
            },
            {
                "component_id": header_id,
                "component_frame": snapshot["frame"],
                "component_rect_points": header["rect_points"],
                "coordinate_reason": "zoom the visible Trim Node to detailed gestures",
            },
        )
        def scale_increased():
            current = client.component_snapshot()
            current_canvas = next(
                (
                    item
                    for item in current["components"]
                    if item["id"] == "node_editor.canvas"
                ),
                None,
            )
            if current_canvas is None:
                return None
            scale = float((current_canvas.get("metadata") or {}).get("scale", 0.0))
            return current if scale > previous_scale + 1.0e-4 else None

        client.wait_until("Trim Node detail zoom step", scale_increased)
    raise QaFailure("Trim Node did not reach detailed interaction scale")


def header_surface_point(header):
    outer = header["rect_points"]
    return {"x": float(outer["center_x"]), "y": float(outer["center_y"])}


def select_trim(client, node_id):
    header_id = "node_editor.node_header:" + node_id
    snapshot, header = reveal_detailed_node(client, node_id)
    point = header_surface_point(header)
    client.inject(
        "drag",
        {
            "from": point,
            "to": {"x": point["x"] + 12.0, "y": point["y"]},
            "coordinate_space": "points",
            "steps": 4,
            "button": "primary",
        },
        {
            "component_id": header_id,
            "component_frame": snapshot["frame"],
            "component_rect_points": header["rect_points"],
            "component_content_rect_points": (header.get("metadata") or {}).get(
                "content_rect"
            ),
            "coordinate_reason": "short Node-header selection gesture",
        },
    )
    try:
        state = client.wait_until(
            "exact Trim Path Node selection",
            lambda: current
            if (current := client.state())["editor"]["selection"]["primary"]
            == {"kind": "node", "id": node_id}
            else None,
            timeout=4.0,
        )
    except QaFailure as error:
        current = client.state()
        current_snapshot, header = client.component(header_id)
        overlaps = [
            item["id"]
            for item in current_snapshot["components"]
            if item.get("visible", False)
            and BASE.point_in_component_rect(point, item["rect_points"])
        ]
        raise QaFailure(
            "{}; selection={!r}; overlaps={!r}; header={!r}; action={!r}".format(
                error,
                current["editor"]["selection"],
                overlaps,
                header.get("metadata"),
                client.evidence[-1] if client.evidence else None,
            )
        ) from error
    client.wait_component("inspector.owner.node:" + node_id)
    return state


def control_id(node_id, name):
    return "inspector.property.node:{}:{}".format(node_id, name)


def inspect_controls(client, node_id):
    controls = {}
    for name, kind in (
        ("mode", "dropdown"),
        ("start", "float"),
        ("end", "float"),
        ("offset", "float"),
    ):
        component_id = control_id(node_id, name)
        client.ensure_in_scroll_area(component_id)
        _, component = client.wait_component_settled(component_id)
        metadata = component.get("metadata") or {}
        if not (
            metadata.get("scope") == "node:" + node_id
            and metadata.get("property") == name
            and metadata.get("control_kind") == kind
        ):
            raise QaFailure(
                "{} omitted canonical Inspector metadata: {!r}".format(
                    component_id, metadata
                )
            )
        controls[name] = metadata
    mode_ui = controls["mode"].get("definition", {}).get("ui", {})
    if mode_ui != {"kind": "dropdown", "options": [NORMALIZED, LENGTH]}:
        raise QaFailure("Trim mode Inspector options are stale: {!r}".format(mode_ui))
    return controls


def author_float(client, node_id, name, text):
    component_id = control_id(node_id, name)
    client.replace_text(component_id, text)
    client.key("enter", True)
    client.key("enter", False)
    expected = float(text)
    return client.wait_until(
        "Trim {}={} authoritative Project value".format(name, text),
        lambda: current
        if property_value((current := client.state()), node_id, name) == expected
        else None,
    )


def select_length_mode(client, node_id):
    mode_id = control_id(node_id, "mode")
    option_id = "inspector.property_option.node:{}:mode:1".format(node_id)
    client.ensure_in_scroll_area(mode_id)
    client.click_component(mode_id)
    _, option = client.wait_component_settled(option_id)
    metadata = option.get("metadata") or {}
    if not (
        metadata.get("scope") == "node:" + node_id
        and metadata.get("property") == "mode"
        and metadata.get("option") == LENGTH
        and metadata.get("option_index") == 1
    ):
        raise QaFailure("Length popup option metadata is stale: {!r}".format(metadata))
    client.click_component(option_id)
    state = client.wait_until(
        "Length (px) authoritative Project value",
        lambda: current
        if property_value((current := client.state()), node_id, "mode") == LENGTH
        else None,
    )
    return state, {"component_id": option_id, "metadata": metadata}


def create_trim(client):
    node_id, state, metadata = BASE.create_node_from_add_search(
        client,
        "trim path",
        MENU_ID,
        "node_editor.container.clip:" + CLIP_ID,
    )
    content = state["project"]["nodes"][node_id].get("content") or {}
    operation = content.get("data") or {}
    if not (
        content.get("type") == "PluginOperation"
        and operation.get("category") == "path_effect"
        and operation.get("component_id") == "trim"
        and operation.get("operation") == "path_effect.apply.v1"
    ):
        raise QaFailure("Add menu created the wrong Trim operation: {!r}".format(content))
    expected_metadata = {
        "action": "create",
        "kind": "path_effect",
        "component_id": "trim",
        "operation_category": "path_effect",
        "operation": "path_effect.apply.v1",
    }
    mismatches = {
        key: (metadata.get(key), value)
        for key, value in expected_metadata.items()
        if metadata.get(key) != value
    }
    if mismatches:
        raise QaFailure("Trim Add item metadata mismatch: {!r}".format(mismatches))
    owner = BASE.validate_canonical_ownership(state["project"])["node_owners"][node_id]
    if owner != "clip:" + CLIP_ID:
        raise QaFailure("Trim Node owner is {}, expected clip:{}".format(owner, CLIP_ID))
    return node_id, state, metadata


def splice_trim_into_stroke(client, node_id):
    before = client.state()
    original = BASE.find_project_connection(
        before["project"], "Node", SOURCE_ID, "shape", "Node", TARGET_ID, "shape_in"
    )
    connection_id = original["id"]
    edge_id = "node_editor.edge:" + connection_id
    header_id = "node_editor.node_header:" + node_id
    node_id_component = "node_editor.node:" + node_id
    snapshot = None
    source = None
    edge = None
    source_component_id = None
    for _ in range(3):
        current = client.component_snapshot()
        canvas = next(
            item for item in current["components"] if item["id"] == "node_editor.canvas"
        )
        candidate = (
            header_id
            if (canvas.get("metadata") or {}).get("detail_enabled") is True
            else node_id_component
        )
        current, targets = BASE.reveal_node_editor_components(
            client, [candidate, edge_id]
        )
        current_canvas = next(
            item for item in current["components"] if item["id"] == "node_editor.canvas"
        )
        expected = (
            header_id
            if (current_canvas.get("metadata") or {}).get("detail_enabled") is True
            else node_id_component
        )
        if expected == candidate:
            snapshot = current
            source, edge = targets
            source_component_id = candidate
            break
    if snapshot is None or source is None or edge is None or source_component_id is None:
        raise QaFailure("Trim splice source changed level of detail while revealing")
    start = (
        header_surface_point(source)
        if source_component_id == header_id
        else client.point(source["rect_points"])
    )
    hit_point = (edge.get("metadata") or {}).get("hit_point")
    if hit_point is None or not BASE.point_in_component_rect(
        hit_point, edge["rect_points"]
    ):
        raise QaFailure("Trim splice wire omitted a usable rendered hit point")
    client.inject(
        "drag",
        {
            "from": start,
            "to": hit_point,
            "coordinate_space": "points",
            "steps": 16,
            "button": "primary",
        },
        {
            "source_component_id": source_component_id,
            "target_component_id": edge_id,
            "component_frame": snapshot["frame"],
            "source_rect_points": source["rect_points"],
            "source_content_rect_points": (source.get("metadata") or {}).get(
                "content_rect"
            ),
            "target_rect_points": edge["rect_points"],
            "target_hit_point": hit_point,
            "coordinate_reason": "outer Node-header surface to rendered Bezier hit point",
        },
    )

    def spliced(project):
        downstream = BASE.project_connection(project, connection_id)
        if downstream is None:
            return False
        upstream = [
            connection
            for connection in project["connections"]
            if connection["from"] == original["from"]
            and connection["to"]["owner"]
            == {"owner_type": "Node", "owner_id": node_id}
            and connection["to"]["port"] == "shape_in"
        ]
        return (
            len(upstream) == 1
            and downstream["from"]["owner"]
            == {"owner_type": "Node", "owner_id": node_id}
            and downstream["from"]["port"] == "shape"
            and downstream["to"] == original["to"]
            and downstream["order"] == original["order"]
        )

    state = client.wait_project("Trim Path wire-drop splice", spliced)
    downstream = BASE.project_connection(state["project"], connection_id)
    upstream = BASE.find_project_connection(
        state["project"], "Node", SOURCE_ID, "shape", "Node", node_id, "shape_in"
    )
    return before, state, {
        "original": original,
        "upstream": upstream,
        "downstream": downstream,
    }


def run_suite(client, capture_path):
    health = client.wait_health()
    initial = BASE.wait_fresh_fixture(client)
    initial = wait_preview_idle(client, "fixture startup")
    baseline_hash = initial["editor"]["preview"]["pixel_hash"]
    baseline_pixels = initial["editor"]["preview"]["nontransparent_pixels"]
    activate_node_editor(client)

    create_before = client.state()
    node_id, created, menu_metadata = create_trim(client)
    defaults = expected_properties(NORMALIZED, 0.0, 1.0, 0.0)
    assert_properties(created, node_id, defaults, "Trim creation defaults")
    created_render = wait_preview_result_after(
        client,
        create_before,
        "unconnected Trim creation",
        lambda state: node_id in state["project"]["nodes"],
        expected_hash=baseline_hash,
    )

    select_trim(client, node_id)
    controls = inspect_controls(client, node_id)
    splice_before, spliced, wires = splice_trim_into_stroke(client, node_id)
    assert_properties(spliced, node_id, defaults, "default connected Trim")
    default_render = wait_preview_result_after(
        client,
        splice_before,
        "default Normalized 0 to 1 splice",
        lambda state: BASE.project_connection(
            state["project"], wires["downstream"]["id"]
        )
        is not None,
        expected_hash=baseline_hash,
    )
    if default_render["editor"]["preview"]["nontransparent_pixels"] != baseline_pixels:
        raise QaFailure("default Trim changed full-identity alpha coverage")

    select_trim(client, node_id)
    start_before = client.state()
    start_state = author_float(client, node_id, "start", "0.3")
    assert_properties(
        start_state,
        node_id,
        expected_properties(NORMALIZED, 0.3, 1.0, 0.0),
        "Normalized start edit",
    )
    start_render = wait_preview_result_after(
        client,
        start_before,
        "Normalized start 0.3",
        lambda state: property_value(state, node_id, "start") == 0.3,
    )

    end_before = start_render
    end_state = author_float(client, node_id, "end", "1.5")
    full_properties = expected_properties(NORMALIZED, 0.3, 1.5, 0.0)
    assert_properties(end_state, node_id, full_properties, "periodic full range")
    full_render = wait_preview_result_after(
        client,
        end_before,
        "Normalized 0.3 to 1.5 full range",
        lambda state: assert_properties(
            state, node_id, full_properties, "periodic full render"
        ),
        expected_hash=baseline_hash,
    )

    offset_before = full_render
    offset_state = author_float(client, node_id, "offset", "0.25")
    offset_properties = expected_properties(NORMALIZED, 0.3, 1.5, 0.25)
    assert_properties(offset_state, node_id, offset_properties, "offset edit")
    offset_render = wait_preview_result_after(
        client,
        offset_before,
        "Normalized full-range offset 0.25",
        lambda state: assert_properties(
            state, node_id, offset_properties, "offset render"
        ),
        expected_hash=baseline_hash,
    )

    length_before = offset_render
    _, length_option = select_length_mode(client, node_id)
    author_float(client, node_id, "start", "0.0")
    author_float(client, node_id, "offset", "0.0")
    length_state = author_float(client, node_id, "end", "10.0")
    length_properties = expected_properties(LENGTH, 0.0, 10.0, 0.0)
    assert_properties(length_state, node_id, length_properties, "Length 10px input")
    length_render = wait_preview_result_after(
        client,
        length_before,
        "Length (px) 0 to 10",
        lambda state: assert_properties(
            state, node_id, length_properties, "Length render"
        ),
    )

    select_trim(client, node_id)
    capture = (
        client.capture(os.path.abspath(capture_path))
        if capture_path is not None
        else None
    )
    final = client.state()
    assert_properties(final, node_id, length_properties, "final Trim state")
    return {
        "ok": True,
        "suite": "trim-path",
        "health": health,
        "trim_node_id": node_id,
        "menu_metadata": menu_metadata,
        "controls": controls,
        "dropdown_option": length_option,
        "wires": wires,
        "cases": {
            "default_normalized_0_to_1": {
                "properties": defaults,
                "preview": preview_evidence(default_render),
                "full_identity_hash": baseline_hash,
            },
            "normalized_start_0_3": {
                "properties": expected_properties(NORMALIZED, 0.3, 1.0, 0.0),
                "preview": preview_evidence(start_render),
            },
            "normalized_0_3_to_1_5_full": {
                "properties": full_properties,
                "preview": preview_evidence(full_render),
                "full_identity_hash": baseline_hash,
            },
            "offset_0_25": {
                "properties": offset_properties,
                "preview": preview_evidence(offset_render),
            },
            "length_px_10": {
                "properties": length_properties,
                "preview": preview_evidence(length_render),
            },
        },
        "creation_preview": preview_evidence(created_render),
        "capture": capture,
        "final_frame": final["frame"],
        "actions": client.evidence,
    }


def parse_args():
    parser = argparse.ArgumentParser()
    parser.add_argument("--base-url", default=None)
    parser.add_argument("--spawn", action="store_true")
    parser.add_argument("--evidence", default="target/qa-trim-path-e2e-evidence.json")
    parser.add_argument("--capture", default=None)
    parser.add_argument("--timeout", type=float, default=40.0)
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
                cwd=os.path.dirname(SCRIPT_DIR),
                env=environment,
                start_new_session=True,
            )
        client = QaClient(base_url, args.timeout)
        result = run_suite(client, args.capture)
        result["run_id"] = os.environ.get("RUVIE_QA_RUN_ID")
        result["git_commit"] = BASE.repository_git_commit()
        result["component_frame"] = client.component_snapshot()["frame"]
        result["action_count"] = len(result.get("actions", []))
        evidence_path = os.path.abspath(args.evidence)
        os.makedirs(os.path.dirname(evidence_path), exist_ok=True)
        with open(evidence_path, "w", encoding="utf-8") as output:
            json.dump(result, output, ensure_ascii=False, indent=2)
            output.write("\n")
        print("[qa-trim-path-e2e] PASS; evidence: {}".format(evidence_path))
        return 0
    except (QaFailure, AssertionError, KeyError, StopIteration, TypeError) as error:
        print("[qa-trim-path-e2e] FAIL: {}".format(error), file=sys.stderr)
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
