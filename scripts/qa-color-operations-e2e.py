#!/usr/bin/env python3
"""Real-coordinate E2E for lossless, explicitly transformed Color Nodes."""

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

OPERATIONS = (
    ("compose color", "compose", "Compose"),
    ("split color", "split", "Split"),
    ("mix color", "mix", "Mix"),
    ("convert color space", "convert_space", "ConvertSpace"),
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
    try:
        BASE.ensure_node_editor_ports_interactive(
            client, [source, target], max_zooms=14
        )
    except QaFailure as error:
        raise QaFailure(
            "Cannot enable Color wire {} -> {}: {}".format(output, input_port, error)
        ) from error
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
    """Compact the graph through the real all-graph layout shortcut."""
    before = client.state()
    positions_before = {
        node_id: before["project"]["nodes"][node_id]["ui_position"]
        for node_id in node_ids
    }
    execution_before = before["editor"]["node_editor"]["layout_execution_serial"]

    client.key("l", True, shift=True)
    client.key("l", False, shift=True)

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
            and execution["command"] == "NodeEditorCleanLayoutAll"
            and execution["scope"] == "all"
            and execution["changed"] is True
            and positions_changed
        ):
            return state
        return None

    arranged = client.wait_until(
        "Color Node all-graph layout",
        selection_layout_completed,
    )
    BASE.assert_history_delta(before, arranged, 1, "Color Node all-graph layout")
    return arranged


def set_node_property(client, node_id, property_key, value, description):
    """Author a Node property through its rendered egui text-entry control."""
    input_id = node_port(node_id, "input", property_key)
    component_id = "node_editor.property.node:{}:{}".format(node_id, property_key)
    header_id = "node_editor.node_header:" + node_id
    try:
        BASE.ensure_node_editor_ports_interactive(client, [input_id], max_zooms=14)
    except QaFailure as error:
        raise QaFailure(
            "Cannot reveal Color property {} on {}: {}".format(
                property_key, node_id, error
            )
        ) from error
    BASE.reveal_node_editor_component(client, component_id)
    before = client.state()
    client.replace_component_text(component_id, str(value))
    BASE.reveal_node_editor_component(client, header_id)
    client.click_component(header_id)
    state = client.wait_project(
        description,
        lambda project: project
        if BASE.property_value(project["nodes"][node_id], property_key) == value
        else None,
    )
    BASE.assert_history_delta(before, state, 1, description)
    return state


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


def signed_transfer(value, transfer):
    magnitude = transfer(abs(value))
    return -magnitude if value < 0.0 else magnitude


def srgb_to_linear(value):
    return signed_transfer(
        value,
        lambda magnitude: magnitude / 12.92
        if magnitude <= 0.04045
        else ((magnitude + 0.055) / 1.055) ** 2.4,
    )


def linear_to_srgb(value):
    return signed_transfer(
        value,
        lambda magnitude: magnitude * 12.92
        if magnitude <= 0.0031308
        else 1.055 * magnitude ** (1.0 / 2.4) - 0.055,
    )


def assert_color_probe_near(probe, space, rgba, tolerance=1.0e-12):
    result = probe.get("result") or {}
    value = result.get("value") or {}
    if result.get("status") != "produced":
        raise QaFailure("Color runtime did not produce: {!r}".format(result))
    if value.get("$type") != "color_value" or value.get("space") != space:
        raise QaFailure(
            "Color runtime tag {!r}, expected color_value @ {}".format(value, space)
        )
    actual = value.get("rgba")
    if not isinstance(actual, list) or len(actual) != 4:
        raise QaFailure("Color runtime RGBA is malformed: {!r}".format(actual))
    for index, (observed, expected) in enumerate(zip(actual, rgba)):
        if not math.isclose(observed, expected, rel_tol=0.0, abs_tol=tolerance):
            raise QaFailure(
                "Color component {} was {!r}, expected {!r}".format(
                    index, observed, expected
                )
            )


def run_suite(client):
    health = client.wait_health()
    BASE.wait_fresh_fixture(client)
    activate_node_editor(client)

    compose_id, compose_menu = create_color_node(client, *OPERATIONS[0])
    second_compose_id, second_compose_menu = create_color_node(client, *OPERATIONS[0])
    split_id, split_menu = create_color_node(client, *OPERATIONS[1])
    mix_id, mix_menu = create_color_node(client, *OPERATIONS[2])
    linear_a_id, linear_a_menu = create_color_node(client, *OPERATIONS[3])
    linear_b_id, linear_b_menu = create_color_node(client, *OPERATIONS[3])
    display_id, display_menu = create_color_node(client, *OPERATIONS[3])

    arrange_color_nodes(
        client,
        (
            compose_id,
            second_compose_id,
            linear_a_id,
            linear_b_id,
            mix_id,
            display_id,
            split_id,
        ),
    )

    set_node_property(client, compose_id, "r", 0.5, "encoded source R authoring")
    set_node_property(
        client, display_id, "target_space", "srgb", "display transform target"
    )

    connections = [
        connect(client, compose_id, "color", linear_a_id, "color"),
        connect(client, second_compose_id, "color", linear_b_id, "color"),
        connect(client, linear_a_id, "color", mix_id, "a", reverse=True),
        connect(client, linear_b_id, "color", mix_id, "b"),
        connect(client, mix_id, "color", display_id, "color"),
        connect(client, display_id, "color", split_id, "color"),
    ]
    inspector = {
        "compose": select_and_assert_inspector(
            client, compose_id, ("space", "r", "g", "b", "a")
        ),
        "mix": select_and_assert_inspector(client, mix_id, ("factor",)),
        "convert": select_and_assert_inspector(
            client, display_id, ("color", "target_space")
        ),
    }

    encoded_source = {
        "$type": "color_value",
        "space": "srgb",
        "rgba": [0.5, 1.0, 1.0, 1.0],
    }
    compose_probe = metadata_probe(client, compose_id, "color")
    linear_probe = metadata_probe(client, linear_a_id, "color")
    linear_mix_probe = metadata_probe(client, mix_id, "color")
    display_probe = metadata_probe(client, display_id, "color")
    split_probe = metadata_probe(client, split_id, "space")
    assert_probe_value(compose_probe, encoded_source)
    source_linear_r = srgb_to_linear(0.5)
    mixed_linear_r = (source_linear_r + 1.0) * 0.5
    display_r = linear_to_srgb(mixed_linear_r)
    assert_color_probe_near(
        linear_probe, "linear-srgb", [source_linear_r, 1.0, 1.0, 1.0]
    )
    assert_color_probe_near(
        linear_mix_probe, "linear-srgb", [mixed_linear_r, 1.0, 1.0, 1.0]
    )
    assert_color_probe_near(display_probe, "srgb", [display_r, 1.0, 1.0, 1.0])
    assert_probe_value(split_probe, "srgb")
    if math.isclose(display_r, 0.75, rel_tol=0.0, abs_tol=1.0e-6):
        raise QaFailure("Mix occurred in encoded sRGB instead of linear-sRGB")

    return {
        "ok": True,
        "suite": "color-operations",
        "health": health,
        "node_ids": {
            "compose": compose_id,
            "second_compose": second_compose_id,
            "split": split_id,
            "mix": mix_id,
            "linear_a": linear_a_id,
            "linear_b": linear_b_id,
            "display": display_id,
        },
        "menu_metadata": {
            "compose": compose_menu,
            "second_compose": second_compose_menu,
            "split": split_menu,
            "mix": mix_menu,
            "linear_a": linear_a_menu,
            "linear_b": linear_b_menu,
            "display": display_menu,
        },
        "connections": connections,
        "inspector": inspector,
        "runtime": {
            "compose": compose_probe,
            "split_space": split_probe,
            "linear": linear_probe,
            "linear_mix": linear_mix_probe,
            "display": display_probe,
            "oracle": {
                "source_linear_r": source_linear_r,
                "mixed_linear_r": mixed_linear_r,
                "display_r": display_r,
            },
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
