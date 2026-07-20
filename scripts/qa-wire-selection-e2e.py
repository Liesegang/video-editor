#!/usr/bin/env python3
"""Focused coordinate E2E for selecting and clearing a Node Editor wire."""

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


def run_suite(client):
    health = client.wait_health()
    BASE.wait_fresh_fixture(client)
    client.wait_component_settled("dock.tab:node_editor")
    client.click_component("dock.tab:node_editor")
    client.wait_until(
        "Node Editor dock activation",
        lambda: client.state()
        if "Node Editor" in client.state()["dock"]["active_tabs"]
        else None,
    )

    state = client.state()
    connection = BASE.find_project_connection(
        state["project"],
        "Node",
        BASE.SOLID,
        "image",
        "Node",
        BASE.MERGE,
        "images",
    )
    connection_id = connection["id"]
    edge_id = "node_editor.edge:" + connection_id
    BASE.reveal_node_editor_components(
        client,
        [
            "node_editor.node_header:" + BASE.SOLID,
            "node_editor.node_header:" + BASE.MERGE,
        ],
    )
    client.wait_component_settled(edge_id)
    BASE.click_node_wire_hit_point(client, edge_id, button="primary")
    selected = client.wait_until(
        "wire coordinate selection",
        lambda: client.state()
        if client.state()["editor"]["node_editor"]["selected_connection_id"]
        == connection_id
        else None,
    )

    snapshot, blank = BASE.find_free_canvas_point(client)
    client.inject(
        "click",
        {
            "x": blank["x"],
            "y": blank["y"],
            "coordinate_space": "points",
            "button": "primary",
        },
        {
            "component_id": "node_editor.canvas",
            "component_frame": snapshot["frame"],
            "coordinate_reason": "fresh unobstructed canvas point clears wire selection",
            "cleared_connection_id": connection_id,
        },
    )
    cleared = client.wait_until(
        "wire coordinate deselection",
        lambda: client.state()
        if client.state()["editor"]["node_editor"]["selected_connection_id"] is None
        else None,
    )
    if cleared["project"] != selected["project"]:
        raise QaFailure("wire deselection changed the authoritative Project")
    if cleared["history"] != selected["history"]:
        raise QaFailure("wire deselection created a Project history entry")
    if cleared["editor"]["selection"] != selected["editor"]["selection"]:
        raise QaFailure("wire deselection changed the semantic entity selection")

    return {
        "ok": True,
        "suite": "node-wire-selection",
        "health": health,
        "connection_id": connection_id,
        "selected_frame": selected["frame"],
        "cleared_frame": cleared["frame"],
        "final_history": cleared["history"],
        "actions": client.evidence,
    }


def parse_args():
    parser = argparse.ArgumentParser()
    parser.add_argument("--base-url", default=None)
    parser.add_argument("--spawn", action="store_true")
    parser.add_argument(
        "--evidence", default="target/qa-wire-selection-evidence.json"
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
        print("[qa-wire-selection-e2e] PASS; evidence: {}".format(evidence_path))
        return 0
    except (QaFailure, AssertionError, KeyError, IndexError) as error:
        print("[qa-wire-selection-e2e] FAIL: {}".format(error), file=sys.stderr)
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
