#!/usr/bin/env python3
"""Real-coordinate authoring E2E for canonical Color and Path Data Nodes."""

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

COLOR_MENU_ID = "node_editor.menu.create.data:color"
PATH_MENU_ID = "node_editor.menu.create.data:path"


def activate_node_editor(client):
    client.wait_component_settled("dock.tab:node_editor")
    client.click_component("dock.tab:node_editor")
    return client.wait_until(
        "Node Editor dock activation",
        lambda: state
        if "Node Editor" in (state := client.state())["dock"]["active_tabs"]
        else None,
    )


def create_data_node(client, query, menu_id, expected_data):
    node_id, state, metadata = BASE.create_node_from_add_search(
        client,
        query,
        menu_id,
        "node_editor.container.composition:{}".format(BASE.COMPOSITION),
    )
    expected_content = {"type": "Data", "data": expected_data}
    actual = state["project"]["nodes"][node_id].get("content")
    if actual != expected_content:
        raise QaFailure(
            "{} created content {!r}, expected {!r}".format(query, actual, expected_content)
        )
    if not (
        metadata.get("catalog_id") == "native.data." + query
        and metadata.get("runtime_status") == "implemented"
        and metadata.get("category") == "Data"
    ):
        raise QaFailure("canonical Data menu metadata is incomplete: {!r}".format(metadata))
    return node_id, metadata


def select_exact_node(client, node_id):
    header_id = "node_editor.node_header:" + node_id
    BASE.reveal_node_editor_component(client, header_id)
    client.click_component(header_id)
    state = client.wait_until(
        "exact Node selection for {}".format(node_id),
        lambda: current
        if (current := client.state())["editor"]["selection"]["primary"]
        == {"kind": "node", "id": node_id}
        else None,
    )
    client.wait_component("inspector.owner.node:" + node_id)
    return state


def component_metadata(client, component_id):
    _, component = client.wait_component_settled(component_id)
    return component.get("metadata") or {}


def property_value(state, node_id):
    return BASE.property_value(state["project"]["nodes"][node_id], "value")


def assert_selected_runtime(state, node_id, expected):
    probe = state.get("runtime", {}).get("selected_metadata_output")
    if not isinstance(probe, dict):
        raise QaFailure("selected Data runtime probe is absent")
    result = probe.get("result") or {}
    expected_header = {
        "node_id": node_id,
        "port": "value",
        "evaluation_source": "authoritative_project",
    }
    mismatches = {
        key: (probe.get(key), value)
        for key, value in expected_header.items()
        if probe.get(key) != value
    }
    if mismatches:
        raise QaFailure("selected Data runtime metadata mismatch: {!r}".format(mismatches))
    if result.get("status") != "produced" or result.get("value") != expected:
        raise QaFailure(
            "runtime value {!r} does not exactly match Project value {!r}".format(
                result, expected
            )
        )
    return probe


def replace_drag_value(client, component_id, text, node_id, expected_component, index):
    client.replace_text(component_id, text)
    client.key("enter", True)
    client.key("enter", False)
    return client.wait_project(
        "{} canonical float authoring".format(expected_component),
        lambda project: BASE.property_value(project["nodes"][node_id], "value")["rgba"][
            index
        ]
        == float(text),
    )


def author_color(client, node_id):
    select_exact_node(client, node_id)
    prefix = "inspector.property_component.node:{}:value".format(node_id)
    red_id = prefix + ":r"
    green_id = prefix + ":g"
    red_metadata = component_metadata(client, red_id)
    green_metadata = component_metadata(client, green_id)
    for metadata in (red_metadata, green_metadata):
        expected = {
            "storage": "canonical_color_value",
            "numeric": "f64",
            "alpha": "straight",
            "legacy_srgba8_picker": False,
        }
        mismatches = {
            key: (metadata.get(key), value)
            for key, value in expected.items()
            if metadata.get(key) != value
        }
        if mismatches:
            raise QaFailure("canonical Color control mismatch: {!r}".format(mismatches))

    replace_drag_value(client, red_id, "-0.5", node_id, "negative R", 0)
    state = replace_drag_value(client, green_id, "4.25", node_id, "HDR G", 1)
    value = property_value(state, node_id)
    expected = {
        "$type": "color_value",
        "space": "srgb",
        "rgba": [-0.5, 4.25, 1.0, 1.0],
    }
    if value != expected:
        raise QaFailure("canonical Color Project value mismatch: {!r}".format(value))
    settled = client.wait_until(
        "canonical Color runtime evaluation",
        lambda: current
        if (
            (current := client.state())
            .get("runtime", {})
            .get("selected_metadata_output", {})
            .get("result", {})
            .get("value")
            == expected
        )
        else None,
    )
    return {
        "project_value": value,
        "runtime": assert_selected_runtime(settled, node_id, expected),
        "control_metadata": {"r": red_metadata, "g": green_metadata},
    }


def authored_path_value():
    return {
        "$type": "path_value",
        "fill_rule": "even_odd",
        "contours": [
            {
                "start": {"x": -2.5, "y": 4.0},
                "segments": [
                    {
                        "kind": "conic",
                        "control": {"x": 0.25, "y": 9.5},
                        "to": {"x": 7.0, "y": -3.0},
                        "weight": 0.375,
                    },
                    {
                        "kind": "cubic",
                        "control1": {"x": 8.0, "y": 1.0},
                        "control2": {"x": 9.0, "y": 2.0},
                        "to": {"x": 10.0, "y": 3.0},
                    },
                ],
                "closed": True,
            },
            {
                "start": {"x": 20.0, "y": 30.0},
                "segments": [
                    {
                        "kind": "quadratic",
                        "control": {"x": 22.0, "y": 35.0},
                        "to": {"x": 25.0, "y": 31.0},
                    }
                ],
                "closed": False,
            },
        ],
    }


def author_path(client, node_id):
    select_exact_node(client, node_id)
    prefix = "inspector.property_component.node:{}:value".format(node_id)
    toggle_id = prefix + ":toggle"
    json_id = prefix + ":json"
    apply_id = prefix + ":apply"
    client.ensure_in_scroll_area(toggle_id)
    toggle_metadata = component_metadata(client, toggle_id)
    client.click_component(toggle_id)
    path_metadata = component_metadata(client, json_id)
    expected_metadata = {
        "storage": "canonical_path_value",
        "format": "canonical_json",
        "svg_authoritative": False,
    }
    mismatches = {
        key: (path_metadata.get(key), value)
        for key, value in expected_metadata.items()
        if path_metadata.get(key) != value
    }
    if mismatches:
        raise QaFailure("canonical Path control mismatch: {!r}".format(mismatches))

    expected = authored_path_value()
    client.replace_text(json_id, json.dumps(expected, indent=2, sort_keys=True))
    client.wait_component_settled(json_id)
    snapshot, apply_component = client.wait_component_settled(apply_id)
    components = snapshot["components"]
    center = {
        "x": apply_component["rect_points"]["center_x"],
        "y": apply_component["rect_points"]["center_y"],
    }
    overlaps = [
        item["id"]
        for item in components
        if item.get("visible", False)
        and item["rect_points"]["min_x"] <= center["x"] <= item["rect_points"]["max_x"]
        and item["rect_points"]["min_y"] <= center["y"] <= item["rect_points"]["max_y"]
    ]
    shadowing_controls = [
        component_id
        for component_id in overlaps
        if component_id.startswith("inspector.property_component.")
        and component_id != apply_id
    ]
    if shadowing_controls:
        raise QaFailure(
            "canonical Path Apply is shadowed by controls: {!r}".format(
                shadowing_controls
            )
        )
    client.click_component(apply_id)

    def applied_or_rejected(project):
        value = BASE.property_value(project["nodes"][node_id], "value")
        if value == expected:
            return True
        metadata = component_metadata(client, json_id)
        error = metadata.get("validation_error")
        if error:
            raise QaFailure("canonical Path JSON was rejected: {}".format(error))
        return False

    state = client.wait_project(
        "canonical Path JSON coordinate apply",
        applied_or_rejected,
    )
    value = property_value(state, node_id)
    settled = client.wait_until(
        "canonical Path runtime evaluation",
        lambda: current
        if (
            (current := client.state())
            .get("runtime", {})
            .get("selected_metadata_output", {})
            .get("result", {})
            .get("value")
            == expected
        )
        else None,
    )
    return {
        "project_value": value,
        "runtime": assert_selected_runtime(settled, node_id, expected),
        "control_metadata": {
            "toggle": toggle_metadata,
            "json": path_metadata,
            "apply": component_metadata(client, apply_id),
        },
        "apply_geometry": {
            "rect_points": apply_component["rect_points"],
            "overlaps": overlaps,
        },
    }


def run_suite(client):
    health = client.wait_health()
    BASE.wait_fresh_fixture(client)
    activate_node_editor(client)

    color_id, color_menu = create_data_node(client, "color", COLOR_MENU_ID, "Color")
    color = author_color(client, color_id)
    path_id, path_menu = create_data_node(client, "path", PATH_MENU_ID, "Path")
    path = author_path(client, path_id)
    final_state = client.state()
    if json.loads(json.dumps(color["project_value"])) != color["project_value"]:
        raise QaFailure("canonical Color value is not a stable JSON payload")
    if json.loads(json.dumps(path["project_value"])) != path["project_value"]:
        raise QaFailure("canonical Path value is not a stable JSON payload")

    return {
        "ok": True,
        "suite": "path-color-nodes",
        "health": health,
        "final_frame": final_state["frame"],
        "color_node_id": color_id,
        "path_node_id": path_id,
        "menu_metadata": {"color": color_menu, "path": path_menu},
        "color": color,
        "path": path,
        "persistence_runtime_gate": "library data_graph roundtrip tests",
        "actions": client.evidence,
    }


def parse_args():
    parser = argparse.ArgumentParser()
    parser.add_argument("--base-url", default=None)
    parser.add_argument("--spawn", action="store_true")
    parser.add_argument(
        "--evidence", default="target/qa-path-color-nodes-e2e-evidence.json"
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
        result = run_suite(client)
        result["run_id"] = os.environ.get("RUVIE_QA_RUN_ID")
        result["git_commit"] = BASE.repository_git_commit()
        result["component_frame"] = client.component_snapshot()["frame"]
        result["action_count"] = len(result.get("actions", []))
        evidence_path = os.path.abspath(args.evidence)
        os.makedirs(os.path.dirname(evidence_path), exist_ok=True)
        with open(evidence_path, "w", encoding="utf-8") as output:
            json.dump(result, output, ensure_ascii=False, indent=2)
            output.write("\n")
        print("[qa-path-color-nodes-e2e] PASS; evidence: {}".format(evidence_path))
        return 0
    except (QaFailure, AssertionError, KeyError, StopIteration, TypeError) as error:
        print("[qa-path-color-nodes-e2e] FAIL: {}".format(error), file=sys.stderr)
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
