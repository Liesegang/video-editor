#!/usr/bin/env python3
"""Focused real-coordinate QA for Node Editor layout commands."""

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
TEXT = BASE.TEXT
COMPOSITION = BASE.COMPOSITION
NODE_TAB = "dock.tab:node_editor"
TIMELINE_TAB = "dock.tab:timeline"
SMART_LAYOUT = "node_editor.layout.smart"


def execution_id(state):
    return int(state["editor"]["node_editor"]["layout_execution_serial"])


def wait_execution(client, previous_id, command, scope):
    def completed():
        state = client.state()
        execution = state["editor"]["node_editor"]["last_layout_execution"]
        if execution is None or int(execution["execution_id"]) <= previous_id:
            return None
        if execution["command"] != command or execution["scope"] != scope:
            raise QaFailure(
                "layout execution mismatch: expected {}/{}, got {!r}".format(
                    command, scope, execution
                )
            )
        if execution["changed"] is not True:
            raise QaFailure("layout command reported no authoritative change")
        return state

    return client.wait_until("new {} {} execution".format(scope, command), completed)


def inject_layout_icon(client, shift=False):
    snapshot, component = client.component(SMART_LAYOUT)
    point = client.point(component["rect_points"])
    client.inject(
        "click",
        {
            "x": point["x"],
            "y": point["y"],
            "coordinate_space": "points",
            "button": "primary",
            "modifiers": {"shift": shift},
        },
        {
            "component_id": SMART_LAYOUT,
            "component_frame": snapshot["frame"],
            "component_rect_points": component["rect_points"],
            "coordinate_reason": "real TREE_STRUCTURE icon modifier click",
        },
    )


def inject_layout_key(client, shift=False):
    client.key("l", True, shift=shift)
    client.key("l", False, shift=shift)


def undo_and_assert_restored(client, before, operation):
    expected_project = before["project"]
    expected_depth = BASE.history_depth(before)
    client.key("z", True, command=True)
    client.key("z", False, command=True)

    def restored():
        state = client.state()
        if (
            BASE.history_depth(state) == expected_depth
            and state["project"] == expected_project
        ):
            return state
        return None

    return client.wait_until("{} undo restoration".format(operation), restored)


def assert_single_history(before, after, operation):
    BASE.assert_history_delta(before, after, 1, operation)


def assert_only_text_moved(before, after, operation):
    old = BASE.node_editor_layout_positions(before["project"])
    new = BASE.node_editor_layout_positions(after["project"])
    changed_nodes = sorted(
        node_id for node_id, position in old["nodes"].items() if new["nodes"][node_id] != position
    )
    if changed_nodes != [TEXT]:
        raise QaFailure(
            "{} moved nodes outside selection: {!r}".format(operation, changed_nodes)
        )
    for kind in ("compositions", "tracks", "clips"):
        if old[kind] != new[kind]:
            raise QaFailure("{} moved unselected {}".format(operation, kind))


def run_and_undo(client, previous_id, command, scope, inject, selection_only=False):
    before = client.state()
    inject()
    after = wait_execution(client, previous_id, command, scope)
    assert_single_history(before, after, "{} {}".format(command, scope))
    if selection_only:
        assert_only_text_moved(before, after, "{} {}".format(command, scope))
    undo_and_assert_restored(client, before, "{} {}".format(command, scope))
    return int(after["editor"]["node_editor"]["last_layout_execution"]["execution_id"])


def ensure_editable_node(client):
    def detail_enabled():
        _, canvas = client.component("node_editor.canvas")
        return (canvas.get("metadata") or {}).get("detail_enabled") is True

    for _ in range(3):
        if detail_enabled():
            break
        client.scroll_component(
            "node_editor.canvas", 0.0, 1000.0, modifiers={"command": True}
        )
    if not detail_enabled():
        raise QaFailure("Node Editor did not enter detailed interaction scale")

    header = "node_editor.node_header:" + TEXT
    BASE.reveal_node_editor_component(client, header)
    before = client.state()
    original = before["project"]["nodes"][TEXT]["ui_position"]
    client.drag_component_by(header, 180.0, 0.0, steps=10)

    def moved_and_selected():
        state = client.state()
        selection = state["editor"]["selection"]
        if (
            state["project"]["nodes"][TEXT]["ui_position"] != original
            and selection["primary"] == {"kind": "node", "id": TEXT}
        ):
            return state
        return None

    moved = client.wait_until("real Node header perturbation", moved_and_selected)
    assert_single_history(before, moved, "Node header perturbation")
    return moved


def select_current_container(client):
    header = "node_editor.container_header.composition:" + COMPOSITION
    BASE.reveal_node_editor_component(client, header)
    before = client.state()
    client.click_component(header)
    selected = client.wait_until(
        "Composition container selection",
        lambda: state
        if (state := client.state())["editor"]["selection"]["primary"]
        == {"kind": "composition", "id": COMPOSITION}
        else None,
    )
    if selected["project"] != before["project"] or selected["history"] != before["history"]:
        raise QaFailure("Composition container selection mutated Project/history")
    return selected


def run_suite(client):
    health = client.wait_health()
    client.wait_until(
        "node layout fixture",
        lambda: state
        if TEXT in (state := client.state())["project"]["nodes"]
        else None,
    )
    BASE.activate_dock_tab(client, NODE_TAB, "Node Editor", "layout command QA")
    client.wait_component_settled(SMART_LAYOUT)
    _, smart = client.component(SMART_LAYOUT)
    metadata = smart.get("metadata") or {}
    if metadata.get("command_id") != "node_editor.clean_layout":
        raise QaFailure("TREE_STRUCTURE command metadata is missing")
    if metadata.get("label") != "Clean layout" or metadata.get("shortcut") != "L":
        raise QaFailure("TREE_STRUCTURE accessibility/shortcut metadata is incomplete")

    ensure_editable_node(client)
    last_id = execution_id(client.state())
    last_id = run_and_undo(
        client,
        last_id,
        "NodeEditorCleanLayout",
        "selection",
        lambda: inject_layout_icon(client),
        selection_only=True,
    )
    last_id = run_and_undo(
        client,
        last_id,
        "NodeEditorCleanLayout",
        "selection",
        lambda: inject_layout_key(client),
        selection_only=True,
    )
    last_id = run_and_undo(
        client,
        last_id,
        "NodeEditorCleanLayoutAll",
        "all",
        lambda: inject_layout_icon(client, shift=True),
    )
    last_id = run_and_undo(
        client,
        last_id,
        "NodeEditorCleanLayoutAll",
        "all",
        lambda: inject_layout_key(client, shift=True),
    )

    select_current_container(client)
    last_id = run_and_undo(
        client,
        last_id,
        "NodeEditorCleanLayout",
        "container",
        lambda: inject_layout_icon(client),
    )

    # Preserve the stale focused Node Editor, but move the real pointer into
    # the separately rendered Preview panel. Area-scoped L must follow the
    # pointer and must not fall back to the old dock focus.
    preview_snapshot, preview = client.component("preview.canvas")
    preview_point = client.point(preview["rect_points"])
    client.inject(
        "move",
        {
            "x": preview_point["x"],
            "y": preview_point["y"],
            "coordinate_space": "points",
        },
        {
            "component_id": "preview.canvas",
            "component_frame": preview_snapshot["frame"],
            "component_rect_points": preview["rect_points"],
            "coordinate_reason": "stale Node focus with pointer over real Preview",
        },
    )
    preview_before = client.state()
    inject_layout_key(client)
    preview_after = client.state()
    if preview_after["project"] != preview_before["project"]:
        raise QaFailure("L over Preview with stale Node focus mutated Project")
    if preview_after["history"] != preview_before["history"]:
        raise QaFailure("L over Preview with stale Node focus changed history")
    if execution_id(preview_after) != last_id:
        raise QaFailure("L over Preview advanced Node layout execution serial")

    node_snapshot, node_point = BASE.find_free_canvas_point(client)
    client.inject(
        "move",
        {
            "x": node_point["x"],
            "y": node_point["y"],
            "coordinate_space": "points",
        },
        {
            "component_id": "node_editor.canvas",
            "component_frame": node_snapshot["frame"],
            "coordinate_reason": "real Node canvas pointer restores Node shortcut scope",
        },
    )
    last_id = run_and_undo(
        client,
        last_id,
        "NodeEditorCleanLayout",
        "container",
        lambda: inject_layout_key(client),
    )

    BASE.activate_dock_tab(client, TIMELINE_TAB, "Timeline", "layout scope guard")
    outside_before = client.state()
    inject_layout_key(client)
    outside_after = client.state()
    if outside_after["project"] != outside_before["project"]:
        raise QaFailure("L outside Node Editor mutated Project")
    if outside_after["history"] != outside_before["history"]:
        raise QaFailure("L outside Node Editor changed history")
    if execution_id(outside_after) != last_id:
        raise QaFailure("L outside Node Editor executed a Node layout command")

    return {
        "ok": True,
        "suite": "node-layout-command",
        "health": health,
        "last_execution_id": last_id,
        "final_history": outside_after["history"],
        "actions": client.evidence,
    }


def parse_args():
    parser = argparse.ArgumentParser()
    parser.add_argument("--base-url", default=None)
    parser.add_argument("--spawn", action="store_true")
    parser.add_argument("--timeout", type=float, default=30.0)
    parser.add_argument(
        "--evidence",
        default="target/qa-node-layout-e2e-evidence.json",
    )
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
        result = run_suite(QaClient(base_url, args.timeout))
        result["run_id"] = os.environ.get("RUVIE_QA_RUN_ID")
        evidence_path = os.path.abspath(args.evidence)
        os.makedirs(os.path.dirname(evidence_path), exist_ok=True)
        with open(evidence_path, "w", encoding="utf-8") as output:
            json.dump(result, output, ensure_ascii=False, indent=2)
            output.write("\n")
        print("[qa-node-layout-e2e] PASS; evidence: {}".format(evidence_path))
        return 0
    except (QaFailure, AssertionError, KeyError, StopIteration, TypeError) as error:
        print("[qa-node-layout-e2e] FAIL: {}".format(error), file=sys.stderr)
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
