#!/usr/bin/env python3
"""Real-coordinate E2E for lossless Compose/Split/Mix Color Nodes."""

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

OPERATIONS = (
    ("compose color", "compose", "Compose"),
    ("split color", "split", "Split"),
    ("mix color", "mix", "Mix"),
)


def activate_node_editor(client):
    client.wait_component_settled("dock.tab:node_editor")
    client.click_component("dock.tab:node_editor")
    return client.wait_until(
        "Node Editor dock activation",
        lambda: state
        if "Node Editor" in (state := client.state())["dock"]["active_tabs"]
        else None,
    )


def create_color_node(client, query, operation_key, content):
    menu_id = "node_editor.menu.create.color:" + operation_key
    node_id, state, metadata = BASE.create_node_from_add_search(
        client,
        query,
        menu_id,
        "node_editor.container.composition:{}".format(BASE.COMPOSITION),
    )
    expected = {"type": "Color", "data": content}
    actual = state["project"]["nodes"][node_id].get("content")
    if actual != expected:
        raise QaFailure(
            "{} created content {!r}, expected {!r}".format(query, actual, expected)
        )
    expected_metadata = {
        "catalog_id": "native.color." + operation_key,
        "runtime_status": "implemented",
        "category": "Color",
    }
    mismatches = {
        key: (metadata.get(key), value)
        for key, value in expected_metadata.items()
        if metadata.get(key) != value
    }
    if mismatches:
        raise QaFailure("Color menu metadata mismatch: {!r}".format(mismatches))
    return node_id, metadata


def node_port(node_id, direction, port):
    return "node_editor.port.node:{}.{}:{}".format(node_id, direction, port)


def matching_connections(project, from_id, output, to_id, input_port):
    return [
        connection
        for connection in project.get("connections", ())
        if connection.get("from", {}).get("owner", {}).get("owner_type") == "Node"
        and connection["from"]["owner"].get("owner_id") == from_id
        and connection["from"].get("port") == output
        and connection.get("to", {}).get("owner", {}).get("owner_type") == "Node"
        and connection["to"]["owner"].get("owner_id") == to_id
        and connection["to"].get("port") == input_port
    ]


def connect(client, from_id, output, to_id, input_port, reverse=False):
    source = node_port(from_id, "output", output)
    target = node_port(to_id, "input", input_port)
    BASE.ensure_node_editor_ports_interactive(client, [source, target])
    before = client.state()
    if reverse:
        client.drag_components(target, source, steps=16)
    else:
        client.drag_components(source, target, steps=16)
    connected = client.wait_project(
        "Color {} -> {} coordinate wire".format(output, input_port),
        lambda project: project
        if len(matching_connections(project, from_id, output, to_id, input_port)) == 1
        else None,
    )
    BASE.assert_history_delta(before, connected, 1, "Color coordinate wire")
    return matching_connections(
        connected["project"], from_id, output, to_id, input_port
    )[0]


def arrange_color_nodes(client, node_ids):
    """Compact isolated authored Nodes through the real selection/layout UI."""
    before = client.state()
    positions_before = {
        node_id: before["project"]["nodes"][node_id]["ui_position"]
        for node_id in node_ids
    }
    execution_before = before["editor"]["node_editor"]["layout_execution_serial"]

    for index, node_id in enumerate(node_ids):
        component_id = "node_editor.node_header:" + node_id
        snapshot, component = BASE.reveal_node_editor_component(client, component_id)
        point = client.point(component["rect_points"])
        client.inject(
            "click",
            {
                "x": point["x"],
                "y": point["y"],
                "coordinate_space": "points",
                "button": "primary",
                "modifiers": {"shift": index > 0},
            },
            {
                "component_id": component_id,
                "component_frame": snapshot["frame"],
                "component_rect_points": component["rect_points"],
                "coordinate_reason": "select Color Nodes for compact layout",
            },
        )

    expected_selection = set(node_ids)

    def selected_all_nodes():
        state = client.state()
        selected = {
            target["id"]
            for target in state["editor"]["selection"]["targets"]
            if target["kind"] == "node"
        }
        return state if selected == expected_selection else None

    client.wait_until(
        "Color Node multi-selection",
        selected_all_nodes,
    )

    client.key("l", True, command=True)
    client.key("l", False, command=True)

    def selection_layout_completed():
        state = client.state()
        node_editor = state["editor"]["node_editor"]
        execution = node_editor.get("last_layout_execution")
        positions_changed = any(
            state["project"]["nodes"][node_id]["ui_position"]
            != positions_before[node_id]
            for node_id in node_ids
        )
        if (
            node_editor["layout_execution_serial"] > execution_before
            and execution is not None
            and execution["command"] == "NodeEditorCleanLayoutSelection"
            and execution["scope"] == "selection"
            and execution["changed"] is True
            and positions_changed
        ):
            return state
        return None

    arranged = client.wait_until(
        "Color Node selection layout",
        selection_layout_completed,
    )
    BASE.assert_history_delta(before, arranged, 1, "Color Node selection layout")
    return arranged


def select_and_assert_inspector(client, node_id, property_names):
    header_id = "node_editor.node_header:" + node_id
    BASE.reveal_node_editor_component(client, header_id)
    client.click_component(header_id)
    client.wait_until(
        "Color Node exact selection",
        lambda: state
        if (state := client.state())["editor"]["selection"]["primary"]
        == {"kind": "node", "id": node_id}
        else None,
    )
    client.wait_component("inspector.owner.node:" + node_id)
    properties = []
    for name in property_names:
        component_id = "inspector.property.node:{}:{}".format(node_id, name)
        client.ensure_in_scroll_area(component_id)
        _, component = client.wait_component_settled(component_id)
        properties.append(
            {
                "id": component_id,
                "metadata": component.get("metadata") or {},
                "rect_points": component.get("rect_points"),
            }
        )
    return properties


def metadata_probe(client, node_id, port):
    return client.request(
        "/v1/probes/metadata-output",
        {"node_id": node_id, "port": port, "global_time": 0.5},
        method="POST",
    )


def assert_probe_value(probe, expected):
    result = probe.get("result") or {}
    if result.get("status") != "produced" or result.get("value") != expected:
        raise QaFailure(
            "Color runtime probe {!r}, expected {!r}".format(result, expected)
        )


def run_suite(client):
    health = client.wait_health()
    BASE.wait_fresh_fixture(client)
    activate_node_editor(client)

    compose_id, compose_menu = create_color_node(client, *OPERATIONS[0])
    second_compose_id, second_compose_menu = create_color_node(client, *OPERATIONS[0])
    split_id, split_menu = create_color_node(client, *OPERATIONS[1])
    mix_id, mix_menu = create_color_node(client, *OPERATIONS[2])

    arrange_color_nodes(
        client, (compose_id, second_compose_id, split_id, mix_id)
    )

    connections = [
        connect(client, compose_id, "color", split_id, "color"),
        connect(client, compose_id, "color", mix_id, "a", reverse=True),
        connect(client, second_compose_id, "color", mix_id, "b"),
    ]
    inspector = {
        "compose": select_and_assert_inspector(
            client, compose_id, ("space", "r", "g", "b", "a")
        ),
        "mix": select_and_assert_inspector(client, mix_id, ("factor",)),
    }

    white = {
        "$type": "color_value",
        "space": "srgb",
        "rgba": [1.0, 1.0, 1.0, 1.0],
    }
    compose_probe = metadata_probe(client, compose_id, "color")
    split_probe = metadata_probe(client, split_id, "space")
    mix_probe = metadata_probe(client, mix_id, "color")
    assert_probe_value(compose_probe, white)
    assert_probe_value(split_probe, "srgb")
    assert_probe_value(mix_probe, white)

    return {
        "ok": True,
        "suite": "color-operations",
        "health": health,
        "node_ids": {
            "compose": compose_id,
            "second_compose": second_compose_id,
            "split": split_id,
            "mix": mix_id,
        },
        "menu_metadata": {
            "compose": compose_menu,
            "second_compose": second_compose_menu,
            "split": split_menu,
            "mix": mix_menu,
        },
        "connections": connections,
        "inspector": inspector,
        "runtime": {
            "compose": compose_probe,
            "split_space": split_probe,
            "mix": mix_probe,
        },
        "actions": client.evidence,
    }


def parse_args():
    parser = argparse.ArgumentParser()
    parser.add_argument("--base-url", default=None)
    parser.add_argument("--spawn", action="store_true")
    parser.add_argument(
        "--evidence", default="target/qa-color-operations-e2e-evidence.json"
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
        print("[qa-color-operations-e2e] PASS; evidence: {}".format(evidence_path))
        return 0
    except (QaFailure, AssertionError, KeyError, StopIteration, TypeError) as error:
        print("[qa-color-operations-e2e] FAIL: {}".format(error), file=sys.stderr)
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
