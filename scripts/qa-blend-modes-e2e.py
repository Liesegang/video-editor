#!/usr/bin/env python3
"""Coordinate E2E for the complete grouped Merge blend-mode catalog."""

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
SPEC = importlib.util.spec_from_file_location("ruvie_qa_blend_modes_base", BASE_PATH)
if SPEC is None or SPEC.loader is None:
    raise RuntimeError("cannot load scripts/qa-e2e.py")
BASE = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(BASE)

QaClient = BASE.QaClient
QaFailure = BASE.QaFailure


MODES = (
    ("linear_burn", "LinearBurn", "darken"),
    ("vivid_light", "VividLight", "contrast"),
    ("divide", "Divide", "comparative"),
    ("hue", "Hue", "hsl"),
    ("clear", "Clear", "normal"),
    ("dissolve", "Dissolve", "normal"),
)

GROUPS = ("Normal", "Darken", "Lighten", "Contrast", "Comparative", "HSL")


def merge_layers(project):
    return sorted(
        [
            connection
            for connection in project["connections"]
            if connection["to"]["owner"].get("owner_type") == "Node"
            and connection["to"]["owner"].get("owner_id") == BASE.MERGE
            and connection["to"]["port"] == "images"
        ],
        key=lambda connection: (connection["order"], connection["id"]),
    )


def project_connection(project, connection_id):
    try:
        return next(
            connection
            for connection in project["connections"]
            if connection["id"] == connection_id
        )
    except StopIteration as error:
        raise QaFailure("missing Merge connection {}".format(connection_id)) from error


def selector_id(connection_id):
    return "node_editor.merge_layer.blend_select:{}:{}".format(
        BASE.MERGE, connection_id
    )


def search_id(connection_id):
    return "node_editor.merge_layer.blend_search:{}:{}".format(
        BASE.MERGE, connection_id
    )


def option_id(connection_id, qa_key):
    return "node_editor.merge_layer.blend.{}:{}:{}".format(
        qa_key, BASE.MERGE, connection_id
    )


def assert_connection_identity(project, expected, operation):
    current = project_connection(project, expected["id"])
    for field in ("id", "from", "to", "order"):
        if current[field] != expected[field]:
            raise QaFailure("{} changed wire {}".format(operation, field))
    return current


def project_with_target_blend_masked(project, connection_id):
    """Clone Project JSON while masking exactly one connection-owned blend value."""
    masked = copy.deepcopy(project)
    target = project_connection(masked, connection_id)
    target["blend_mode"] = "<target-blend-mode>"
    return masked


def assert_only_target_blend_changed(
    before, after, connection_id, expected_before, expected_after, operation
):
    before_target = project_connection(before, connection_id)
    after_target = project_connection(after, connection_id)
    if before_target["blend_mode"] != expected_before:
        raise QaFailure(
            "{} started from {}, expected {}".format(
                operation, before_target["blend_mode"], expected_before
            )
        )
    if after_target["blend_mode"] != expected_after:
        raise QaFailure(
            "{} ended at {}, expected {}".format(
                operation, after_target["blend_mode"], expected_after
            )
        )
    if project_with_target_blend_masked(
        before, connection_id
    ) != project_with_target_blend_masked(after, connection_id):
        raise QaFailure(
            "{} changed Project state outside the target connection blend_mode".format(
                operation
            )
        )


def choose_mode(
    client,
    connection,
    current_key,
    current_serialized,
    qa_key,
    serialized,
    group_key,
):
    print("[qa-blend-modes-e2e] selecting {}".format(qa_key), flush=True)
    connection_id = connection["id"]
    select = selector_id(connection_id)
    query = search_id(connection_id)
    BASE.reveal_node_editor_components(client, [select])
    _, selector = client.wait_component_settled(select)
    selector_metadata = selector.get("metadata") or {}
    if not (
        selector_metadata.get("action") == "open_authored_blend"
        and selector_metadata.get("connection_id") == connection_id
        and selector_metadata.get("authored_blend_mode") == current_key
        and selector_metadata.get("blend_ownership") == "connection"
    ):
        raise QaFailure("Merge selector omitted authoritative wire metadata")
    popup_is_open = any(
        component["id"] == query
        and component.get("enabled", False)
        and component.get("visible", False)
        for component in client.component_snapshot()["components"]
    )
    if not popup_is_open:
        client.click_component(select)

    client.wait_component_settled(query)
    for group in GROUPS:
        category = query + ".category:" + group
        _, component = client.wait_component(category)
        metadata = component.get("metadata") or {}
        if not (
            metadata.get("action") == "toggle_category"
            and metadata.get("category_path") == [group]
            and metadata.get("inline") is True
        ):
            raise QaFailure("blend category {} omitted accordion metadata".format(group))

    # The search click and result click are both actual egui coordinate events.
    # Search flattens the otherwise grouped six-accordion catalog.
    client.replace_component_text(query, qa_key)
    option = option_id(connection_id, qa_key)
    _, option_component = client.wait_component_settled(option)
    option_metadata = option_component.get("metadata") or {}
    if not (
        option_metadata.get("action") == "set_authored_blend"
        and option_metadata.get("connection_id") == connection_id
        and option_metadata.get("blend_mode") == qa_key
        and option_metadata.get("blend_group") == group_key
        and option_metadata.get("coordinate_space") == "screen_points"
    ):
        raise QaFailure("{} option omitted catalog/coordinate metadata".format(qa_key))

    before = client.state()
    client.click_component(option)
    changed = client.wait_project(
        "Merge blend {}".format(qa_key),
        lambda project: project_connection(project, connection_id)["blend_mode"]
        == serialized,
    )
    BASE.assert_history_delta(before, changed, 1, "Merge blend {}".format(qa_key))
    if changed["history"]["redo_depth"] != 0:
        raise QaFailure("{} did not clear redo history".format(qa_key))
    if changed["editor"]["selection"] != before["editor"]["selection"]:
        raise QaFailure("{} unexpectedly changed typed selection".format(qa_key))
    assert_only_target_blend_changed(
        before["project"],
        changed["project"],
        connection_id,
        current_serialized,
        serialized,
        qa_key,
    )
    current = assert_connection_identity(changed["project"], connection, qa_key)
    if current["blend_mode"] != serialized:
        raise QaFailure("{} did not persist on its ProjectConnection".format(qa_key))
    rendered = client.wait_preview_render_after(changed, "Merge blend {}".format(qa_key))
    if rendered["editor"]["preview"]["modal_error"] is not None:
        raise QaFailure("{} surfaced a Preview error".format(qa_key))
    return before, changed, rendered


def run_suite(client):
    health = client.wait_health()
    initial = BASE.wait_fresh_fixture(client)
    BASE.activate_dock_tab(
        client, "dock.tab:node_editor", "Node Editor", "blend-mode catalog"
    )
    layers = merge_layers(initial["project"])
    if len(layers) != 3:
        raise QaFailure("blend fixture must expose three physical Merge wires")
    target = dict(layers[1])
    # Resolve the target once before taking the exact restore baseline. This
    # allows any first-view layout canonicalization to finish without hiding a
    # mutation performed by a blend-mode command.
    BASE.reveal_node_editor_components(client, [selector_id(target["id"])])
    baseline = client.state()
    original_project = copy.deepcopy(baseline["project"])
    original_connections = copy.deepcopy(original_project["connections"])
    baseline_selection = copy.deepcopy(baseline["editor"]["selection"])
    baseline_history = dict(baseline["history"])
    if baseline_history["redo_depth"] != 0:
        raise QaFailure("fresh blend fixture unexpectedly has redo history")
    current_key = "normal"
    current_serialized = target["blend_mode"]
    mutations = []
    hashes = []

    for qa_key, serialized, group_key in MODES:
        before, changed, rendered = choose_mode(
            client,
            target,
            current_key,
            current_serialized,
            qa_key,
            serialized,
            group_key,
        )
        mutations.append(
            {
                "mode": serialized,
                "before_connections": before["project"]["connections"],
                "after_connections": changed["project"]["connections"],
                "preview_hash": rendered["editor"]["preview"]["pixel_hash"],
            }
        )
        hashes.append(rendered["editor"]["preview"]["pixel_hash"])
        current_key = qa_key
        current_serialized = serialized

    if len(set(hashes)) < 4:
        raise QaFailure("representative blend modes did not produce distinct Preview pixels")
    if hashes[-1] == hashes[-2]:
        raise QaFailure("Clear and Dissolve produced the same Preview checksum")

    dissolve_connections = list(mutations[-1]["after_connections"])
    clear_connections = list(mutations[-1]["before_connections"])
    undone = BASE.undo_project_edit(
        client,
        "Dissolve blend",
        lambda project: project["connections"] == clear_connections,
    )
    undone_rendered = client.wait_preview_render_after(undone, "Dissolve Undo")
    redone = BASE.redo_project_edit(
        client,
        "Dissolve blend",
        lambda project: project["connections"] == dissolve_connections,
    )
    redone_rendered = client.wait_preview_render_after(redone, "Dissolve Redo")
    if redone_rendered["editor"]["preview"]["pixel_hash"] != hashes[-1]:
        raise QaFailure("Dissolve Redo was not pixel deterministic")
    if redone["editor"]["selection"] != baseline_selection:
        raise QaFailure("Dissolve Undo/Redo changed typed selection")
    if not (
        redone["history"]["undo_depth"] == baseline_history["undo_depth"] + len(MODES)
        and redone["history"]["redo_depth"] == 0
    ):
        raise QaFailure("Dissolve Undo/Redo left unexpected history depths")

    # Restore the shared fixture: one Undo for redone Dissolve plus the five
    # earlier catalog mutations. Each command still travels through egui.
    restored = redone
    for index in range(len(MODES)):
        expected = (
            original_connections
            if index == len(MODES) - 1
            else mutations[len(MODES) - index - 2]["after_connections"]
        )
        restored = BASE.undo_project_edit(
            client,
            "blend catalog restore {}".format(index + 1),
            lambda project, expected=expected: project["connections"] == expected,
        )
    if restored["project"] != original_project:
        raise QaFailure("blend suite did not exactly restore the authoritative Project")
    if restored["editor"]["selection"] != baseline_selection:
        raise QaFailure("blend suite changed typed selection while restoring Project")
    if not (
        restored["history"]["undo_depth"] == baseline_history["undo_depth"]
        and restored["history"]["redo_depth"]
        == baseline_history["redo_depth"] + len(MODES)
    ):
        raise QaFailure("blend suite restore left unexpected history depths")

    return {
        "ok": True,
        "suite": "blend-modes",
        "health": health,
        "initial_frame": initial["frame"],
        "final_frame": restored["frame"],
        "connection_id": target["id"],
        "modes": [mode for _, mode, _ in MODES],
        "preview_hashes": hashes,
        "undo_preview_hash": undone_rendered["editor"]["preview"]["pixel_hash"],
        "redo_preview_hash": redone_rendered["editor"]["preview"]["pixel_hash"],
        "actions": client.evidence,
    }


def parse_args():
    parser = argparse.ArgumentParser()
    parser.add_argument("--base-url", default=None)
    parser.add_argument("--spawn", action="store_true")
    parser.add_argument("--evidence", default="target/qa-blend-modes-evidence.json")
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
        print("[qa-blend-modes-e2e] PASS; evidence: {}".format(evidence_path))
        return 0
    except (QaFailure, AssertionError, KeyError, IndexError) as error:
        print("[qa-blend-modes-e2e] FAIL: {}".format(error), file=sys.stderr)
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
