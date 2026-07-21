#!/usr/bin/env python3
"""Coordinate E2E for native heterogeneous List Nodes.

All graph mutations enter through rendered egui coordinates: the suite creates
Nodes from the searchable Add menu, authors duplicate wires through the vacant
physical List input, and drags a row handle to reorder the persisted edges.
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
SPEC = importlib.util.spec_from_file_location("ruvie_qa_e2e_base", BASE_PATH)
if SPEC is None or SPEC.loader is None:
    raise RuntimeError("cannot load scripts/qa-e2e.py")
BASE = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(BASE)

QaClient = BASE.QaClient
QaFailure = BASE.QaFailure

ADD_MENU_ID = "node_editor.menu.create.value:add"
MAKE_LIST_MENU_ID = "node_editor.menu.create.list:make"


def activate_node_editor(client):
    client.wait_component_settled("dock.tab:node_editor")
    client.click_component("dock.tab:node_editor")
    return client.wait_until(
        "Node Editor dock activation",
        lambda: state
        if "Node Editor" in (state := client.state())["dock"]["active_tabs"]
        else None,
    )


def create_native_node(client, query, menu_id, expected_content):
    node_id, state, metadata = BASE.create_node_from_add_search(
        client,
        query,
        menu_id,
        "node_editor.container.composition:{}".format(BASE.COMPOSITION),
    )
    content = state["project"]["nodes"][node_id].get("content")
    if content != expected_content:
        raise QaFailure(
            "{} created content {!r}, expected {!r}".format(query, content, expected_content)
        )
    return node_id, state, metadata


def list_connections(project, make_id):
    connections = [
        connection
        for connection in project.get("connections", ())
        if connection.get("to", {}).get("owner", {}).get("owner_type") == "Node"
        and connection["to"]["owner"].get("owner_id") == make_id
        and connection["to"].get("port") == "item"
    ]
    return sorted(connections, key=lambda item: (item["order"], item["id"]))


def ensure_list_ports(client, source_id, make_id):
    source = "node_editor.port.node:{}.output:result".format(source_id)
    vacant = "node_editor.port.node:{}.input:item".format(make_id)
    BASE.ensure_node_editor_ports_interactive(client, [source, vacant])
    return source, vacant


def connect_duplicate_item(client, source_id, make_id, expected_count):
    source, vacant = ensure_list_ports(client, source_id, make_id)
    before = client.state()
    if expected_count == 1:
        client.drag_components(source, vacant, steps=16)
    else:
        # Once an output owns a wire, its endpoint handle intentionally wins
        # the exact hit over the underlying pin for reconnect gestures. Start
        # at the still-vacant List socket to author an independent fan-out.
        client.drag_components(vacant, source, steps=16)
    connected = client.wait_project(
        "Make List item {} coordinate connection".format(expected_count),
        lambda project: len(list_connections(project, make_id)) == expected_count,
    )
    BASE.assert_history_delta(
        before,
        connected,
        1,
        "Make List item {} coordinate connection".format(expected_count),
    )
    return connected


def row_ids(make_id, connection_id):
    return (
        "node_editor.merge_layer.drag_handle:{}:{}".format(make_id, connection_id),
        "node_editor.merge_layer:{}:{}".format(make_id, connection_id),
    )


def validate_row(client, make_id, connection, expected_index, expected_count):
    handle_id, row_id = row_ids(make_id, connection["id"])
    BASE.reveal_node_editor_components(client, [handle_id, row_id])
    _, row = client.wait_component_settled(row_id)
    metadata = row.get("metadata") or {}
    source = metadata.get("source") or {}
    expected = {
        "ordered_input_node_id": make_id,
        "ordered_input": True,
        "input_kind": "list",
        "merge_kind": "list",
        "port": "item",
        "connection_id": connection["id"],
        "canonical_index": expected_index,
        "visual_index": expected_index,
        "layer_count": expected_count,
        "canonical_order_semantics": "top_to_bottom",
        "visual_order_semantics": "top_to_bottom",
        "authored_order": expected_index,
        "authored_blend_available": False,
    }
    mismatches = {
        key: (metadata.get(key), value)
        for key, value in expected.items()
        if metadata.get(key) != value
    }
    if mismatches:
        raise QaFailure("List row metadata mismatch: {!r}".format(mismatches))
    if not (
        source.get("kind") == "node"
        and source.get("owner") == "node:" + connection["from"]["owner"]["owner_id"]
        and source.get("port") == connection["from"]["port"]
    ):
        raise QaFailure("List row source metadata mismatch: {!r}".format(source))
    return {"handle_id": handle_id, "row_id": row_id, "metadata": metadata}


def run_suite(client):
    health = client.wait_health()
    BASE.wait_fresh_fixture(client)
    activate_node_editor(client)

    source_id, _, source_menu = create_native_node(
        client,
        "add",
        ADD_MENU_ID,
        {"type": "Value", "data": "Add"},
    )
    make_id, created, list_menu = create_native_node(
        client,
        "make list",
        MAKE_LIST_MENU_ID,
        {"type": "List", "data": "Make"},
    )
    if not (
        source_menu.get("catalog_id") == "native.math.add"
        and list_menu.get("catalog_id") == "native.list.make"
        and list_menu.get("runtime_status") == "implemented"
        and list_menu.get("category") == "Logic"
    ):
        raise QaFailure(
            "searchable native menu metadata is incomplete: {!r} {!r}".format(
                source_menu, list_menu
            )
        )

    first_state = connect_duplicate_item(client, source_id, make_id, 1)
    first = list_connections(first_state["project"], make_id)[0]
    second_state = connect_duplicate_item(client, source_id, make_id, 2)
    original = list_connections(second_state["project"], make_id)
    if not (
        original[0]["id"] == first["id"]
        and original[0]["id"] != original[1]["id"]
        and original[0]["from"] == original[1]["from"]
        and [item["order"] for item in original] == [0, 1]
    ):
        raise QaFailure("duplicate List sources lost slot identity/order: {!r}".format(original))

    first_ui = validate_row(client, make_id, original[0], 0, 2)
    second_ui = validate_row(client, make_id, original[1], 1, 2)
    before_reorder = client.state()
    BASE.reveal_node_editor_components(
        client, [first_ui["handle_id"], second_ui["row_id"]]
    )
    client.drag_components(first_ui["handle_id"], second_ui["row_id"], steps=16)
    reordered = client.wait_project(
        "physical Make List row reorder",
        lambda project: [item["id"] for item in list_connections(project, make_id)]
        == [original[1]["id"], original[0]["id"]],
    )
    BASE.assert_history_delta(before_reorder, reordered, 1, "physical Make List row reorder")
    persisted = list_connections(reordered["project"], make_id)
    if [item["order"] for item in persisted] != [0, 1]:
        raise QaFailure("reordered List connections were not canonically normalized")

    after_rows = [
        validate_row(client, make_id, connection, index, 2)["metadata"]
        for index, connection in enumerate(persisted)
    ]
    serialized_connections = json.loads(json.dumps(persisted, sort_keys=True))
    if serialized_connections != persisted:
        raise QaFailure("List connection evidence is not JSON serializable")
    final_state = client.state()
    if list_connections(final_state["project"], make_id) != persisted:
        raise QaFailure("List order changed after the post-drag metadata frame")

    return {
        "ok": True,
        "suite": "list-nodes",
        "health": health,
        "created_frame": created["frame"],
        "final_frame": final_state["frame"],
        "source_node_id": source_id,
        "make_list_node_id": make_id,
        "menu_metadata": {"source": source_menu, "make_list": list_menu},
        "original_connection_ids": [item["id"] for item in original],
        "reordered_connection_ids": [item["id"] for item in persisted],
        "serialized_connections": serialized_connections,
        "row_metadata_after_reorder": after_rows,
        "actions": client.evidence,
    }


def parse_args():
    parser = argparse.ArgumentParser()
    parser.add_argument("--base-url", default=None)
    parser.add_argument("--spawn", action="store_true")
    parser.add_argument("--evidence", default="target/qa-list-nodes-e2e-evidence.json")
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
        result["action_count"] = len(result.get("actions", []))
        evidence_path = os.path.abspath(args.evidence)
        os.makedirs(os.path.dirname(evidence_path), exist_ok=True)
        with open(evidence_path, "w", encoding="utf-8") as output:
            json.dump(result, output, ensure_ascii=False, indent=2)
            output.write("\n")
        print("[qa-list-nodes-e2e] PASS; evidence: {}".format(evidence_path))
        return 0
    except (QaFailure, AssertionError, KeyError, StopIteration, TypeError) as error:
        print("[qa-list-nodes-e2e] FAIL: {}".format(error), file=sys.stderr)
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
