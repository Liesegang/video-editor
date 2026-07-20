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
import time


SCRIPT_DIR = os.path.dirname(os.path.abspath(__file__))
BASE_PATH = os.path.join(SCRIPT_DIR, "qa-e2e.py")
SPEC = importlib.util.spec_from_file_location("ruvie_qa_blend_modes_base", BASE_PATH)
if SPEC is None or SPEC.loader is None:
    raise RuntimeError("cannot load scripts/qa-e2e.py")
BASE = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(BASE)

QaClient = BASE.QaClient
QaFailure = BASE.QaFailure


CATALOG = (
    ("normal", "Normal", "normal"),
    ("dissolve", "Dissolve", "normal"),
    ("behind", "Behind", "normal"),
    ("clear", "Clear", "normal"),
    ("darken", "Darken", "darken"),
    ("multiply", "Multiply", "darken"),
    ("color_burn", "ColorBurn", "darken"),
    ("linear_burn", "LinearBurn", "darken"),
    ("darker_color", "DarkerColor", "darken"),
    ("lighten", "Lighten", "lighten"),
    ("screen", "Screen", "lighten"),
    ("color_dodge", "ColorDodge", "lighten"),
    ("linear_dodge", "LinearDodge", "lighten"),
    ("lighter_color", "LighterColor", "lighten"),
    ("overlay", "Overlay", "contrast"),
    ("soft_light", "SoftLight", "contrast"),
    ("hard_light", "HardLight", "contrast"),
    ("vivid_light", "VividLight", "contrast"),
    ("linear_light", "LinearLight", "contrast"),
    ("pin_light", "PinLight", "contrast"),
    ("hard_mix", "HardMix", "contrast"),
    ("difference", "Difference", "comparative"),
    ("exclusion", "Exclusion", "comparative"),
    ("subtract", "Subtract", "comparative"),
    ("divide", "Divide", "comparative"),
    ("hue", "Hue", "hsl"),
    ("saturation", "Saturation", "hsl"),
    ("color", "Color", "hsl"),
    ("luminosity", "Luminosity", "hsl"),
)

# The fixture starts at Normal. Rotate the complete catalog so every selection
# changes the Project exactly once, while Dissolve remains last for a focused
# deterministic Undo/Redo check.
MODES = CATALOG[2:] + CATALOG[:2]

# Each shard starts from the fresh fixture's Normal mode, performs only real
# coordinate-driven edits, and exactly restores its own Project. Their union
# is the complete catalog with no overlap.
MODE_SHARDS = {
    "normal-darken": CATALOG[2:9] + CATALOG[:1],
    "lighten": CATALOG[9:14] + CATALOG[1:2],
    "contrast": CATALOG[14:21],
    "comparative-hsl": CATALOG[21:],
}

PREVIEW_REPRESENTATIVES = {
    "Clear",
    "Multiply",
    "Screen",
    "Overlay",
    "Difference",
    "Hue",
    "Dissolve",
}

GROUPS = ("Normal", "Darken", "Lighten", "Contrast", "Comparative", "HSL")


def modes_for_shard(shard):
    if shard == "all":
        return MODES
    try:
        return MODE_SHARDS[shard]
    except KeyError as error:
        raise QaFailure("unknown blend-mode shard {!r}".format(shard)) from error


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


def wire_search_id(connection_id):
    return "node_editor.wire_menu.blend_search:{}".format(connection_id)


def wire_menu_id(connection_id):
    return "node_editor.wire_menu:{}".format(connection_id)


def wire_option_id(connection_id, qa_key):
    return "node_editor.wire_menu.blend.{}:{}".format(qa_key, connection_id)


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
    validate_categories=False,
    render_preview=False,
):
    print("[qa-blend-modes-e2e] selecting {}".format(qa_key), flush=True)
    connection_id = connection["id"]
    select = selector_id(connection_id)
    query = search_id(connection_id)
    _, selector = client.component(select)
    selector_metadata = selector.get("metadata") or {}
    if not (
        selector_metadata.get("action") == "open_authored_blend"
        and selector_metadata.get("connection_id") == connection_id
        and selector_metadata.get("authored_blend_mode") == current_key
        and selector_metadata.get("blend_ownership") == "connection"
    ):
        raise QaFailure("Merge selector omitted authoritative wire metadata")
    client.click_component(select)

    client.wait_component(query)
    if validate_categories:
        for group in GROUPS:
            category = query + ".category:" + group
            _, component = client.wait_component(category)
            metadata = component.get("metadata") or {}
            if not (
                metadata.get("action") == "toggle_category"
                and metadata.get("category_path") == [group]
                and metadata.get("inline") is True
            ):
                raise QaFailure(
                    "blend category {} omitted accordion metadata".format(group)
                )

    # The search click and result click are both actual egui coordinate events.
    # Search flattens the otherwise grouped six-accordion catalog.
    search_terms = qa_key if qa_key == group_key else "{} {}".format(qa_key, group_key)
    client.replace_component_text(query, search_terms)
    option = option_id(connection_id, qa_key)
    _, option_component = client.wait_component(option)
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
    if changed["editor"]["preview"]["modal_error"] is not None:
        raise QaFailure("{} surfaced a Preview error".format(qa_key))
    rendered = None
    if render_preview:
        rendered = client.wait_preview_render_after(
            changed, "Merge blend {}".format(qa_key)
        )
        if rendered["editor"]["preview"]["modal_error"] is not None:
            raise QaFailure("{} surfaced a Preview error".format(qa_key))
    return before, changed, rendered


WIRE_GRAPH_ITEM_PREFIXES = (
    "node_editor.node:",
    "node_editor.container_header.",
    "node_editor.port.",
    "node_editor.container_port.",
)


def choose_unobstructed_merge_wire(client, connections):
    diagnostics = []
    selected = None
    for connection in connections:
        connection_id = connection["id"]
        edge = "node_editor.edge:" + connection_id
        BASE.reveal_node_editor_components(client, [edge])
        snapshot, component = client.component(edge)
        metadata = component.get("metadata") or {}
        hit_point = metadata.get("hit_point")
        overlap_candidates = []
        if hit_point is not None:
            overlap_candidates = [
                item["id"]
                for item in snapshot["components"]
                if item.get("visible", False)
                and item["id"].startswith(WIRE_GRAPH_ITEM_PREFIXES)
                and BASE.point_in_component_rect(hit_point, item["rect_points"])
            ]
        candidate = {
            "connection_id": connection_id,
            "edge_id": edge,
            "component_frame": snapshot["frame"],
            "hit_point": hit_point,
            "overlap_candidates": overlap_candidates,
            "authored_blend_available": metadata.get("authored_blend_available"),
        }
        diagnostics.append(candidate)
        if selected is None and (
            metadata.get("kind") == "explicit"
            and metadata.get("connection_id") == connection_id
            and metadata.get("authored_blend_available") is True
            and hit_point is not None
            and not overlap_candidates
        ):
            selected = (connection, edge)
    if selected is None:
        raise QaFailure(
            "no Merge wire has an unobstructed real secondary-click point: {}".format(
                diagnostics
            )
        )
    connection, edge = selected
    BASE.reveal_node_editor_components(client, [edge])
    return connection, edge, diagnostics


def choose_wire_mode_and_restore(client, connections, original_project):
    connection, edge, wire_candidates = choose_unobstructed_merge_wire(
        client, connections
    )
    connection_id = connection["id"]
    _, edge_component = client.wait_component(edge)
    edge_metadata = edge_component.get("metadata") or {}
    if not (
        edge_metadata.get("kind") == "explicit"
        and edge_metadata.get("connection_id") == connection_id
    ):
        raise QaFailure("Merge wire omitted explicit connection identity")

    # Open the real wire menu at the rendered Bezier hit point, then use the
    # same production searchable menu as a human right-click interaction.
    pre_context = client.state()
    pre_context_selected_connection = pre_context["editor"]["node_editor"].get(
        "selected_connection_id"
    )
    BASE.click_node_wire_hit_point(client, edge, button="secondary")
    query = wire_search_id(connection_id)
    client.wait_component(query)
    _, menu_component = client.wait_component(wire_menu_id(connection_id))
    opened = client.state()
    if opened["project"] != pre_context["project"]:
        raise QaFailure("opening the wire menu mutated Project")
    if opened["history"] != pre_context["history"]:
        raise QaFailure("opening the wire menu changed history")
    if len(merge_layers(opened["project"])) != 3:
        raise QaFailure("opening the wire menu disconnected a physical Merge wire")
    if not opened["editor"]["node_editor"].get("wire_context_menu_open"):
        raise QaFailure("real secondary click did not keep the wire menu open")
    if (
        opened["editor"]["node_editor"].get("selected_connection_id")
        != connection_id
    ):
        raise QaFailure("wire menu did not expose its transient connection selection")
    if (menu_component.get("metadata") or {}).get("actions_armed") is not False:
        raise QaFailure("opening secondary gesture prematurely armed wire commands")

    client.replace_component_text(query, "screen lighten")
    option = wire_option_id(connection_id, "screen")
    _, option_component = client.wait_component(option)
    metadata = option_component.get("metadata") or {}
    if not (
        metadata.get("action") == "set_authored_blend"
        and metadata.get("connection_id") == connection_id
        and metadata.get("blend_mode") == "screen"
        and metadata.get("blend_group") == "lighten"
    ):
        raise QaFailure("wire Screen option omitted authoritative blend metadata")

    before = client.state()
    if before["project"] != pre_context["project"]:
        raise QaFailure("wire blend search mutated Project before option selection")
    if before["history"] != pre_context["history"]:
        raise QaFailure("wire blend search changed history before option selection")
    client.click_component(option)
    changed = client.wait_project(
        "wire context Screen blend",
        lambda project: project_connection(project, connection_id)["blend_mode"]
        == "Screen",
    )
    BASE.assert_history_delta(before, changed, 1, "wire context Screen blend")
    if changed["history"]["redo_depth"] != 0:
        raise QaFailure("wire context blend did not clear redo history")
    if changed["editor"]["selection"] != before["editor"]["selection"]:
        raise QaFailure("wire context blend changed typed selection")
    if (
        changed["editor"]["node_editor"].get("selected_connection_id")
        != pre_context_selected_connection
    ):
        raise QaFailure("wire context blend did not restore the prior wire selection")
    assert_only_target_blend_changed(
        before["project"],
        changed["project"],
        connection_id,
        project_connection(before["project"], connection_id)["blend_mode"],
        "Screen",
        "wire context Screen blend",
    )
    current = assert_connection_identity(
        changed["project"], connection, "wire context Screen blend"
    )
    if current["blend_mode"] != "Screen":
        raise QaFailure("wire context Screen did not persist on ProjectConnection")
    rendered = client.wait_preview_render_after(changed, "wire context Screen blend")
    if rendered["editor"]["preview"]["modal_error"] is not None:
        raise QaFailure("wire context Screen surfaced a Preview error")

    undone = BASE.undo_project_edit(
        client,
        "wire context Screen blend",
        lambda project: project == original_project,
    )
    undone_rendered = client.wait_preview_render_after(
        undone, "wire context Screen Undo"
    )
    if undone["project"] != original_project:
        raise QaFailure("wire context Undo did not exactly restore Project")
    if undone["editor"]["selection"] != before["editor"]["selection"]:
        raise QaFailure("wire context Undo changed typed selection")
    if (
        undone["editor"]["node_editor"].get("selected_connection_id")
        != pre_context_selected_connection
    ):
        raise QaFailure("wire context Undo changed wire selection")
    if not (
        undone["history"]["undo_depth"] == before["history"]["undo_depth"]
        and undone["history"]["redo_depth"] == 1
    ):
        raise QaFailure("wire context Undo left unexpected history depths")
    return {
        "mode": "Screen",
        "wire_candidates": wire_candidates,
        "selected_edge_id": edge,
        "opening_project_unchanged": opened["project"] == pre_context["project"],
        "opening_history_unchanged": opened["history"] == pre_context["history"],
        "opening_layer_count": len(merge_layers(opened["project"])),
        "before_connections": before["project"]["connections"],
        "after_connections": changed["project"]["connections"],
        "preview_hash": rendered["editor"]["preview"]["pixel_hash"],
        "undo_preview_hash": undone_rendered["editor"]["preview"]["pixel_hash"],
    }, undone


def run_suite(client, shard="all"):
    suite_started = time.monotonic()
    phase_started = suite_started
    timings = {}
    modes = modes_for_shard(shard)
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
    timings["setup"] = round(time.monotonic() - phase_started, 3)
    current_key = "normal"
    current_serialized = target["blend_mode"]
    mutations = []
    hashes_by_mode = {}
    mode_timings = []

    phase_started = time.monotonic()
    for index, (qa_key, serialized, group_key) in enumerate(modes):
        mode_started = time.monotonic()
        render_preview = serialized in PREVIEW_REPRESENTATIVES
        before, changed, rendered = choose_mode(
            client,
            target,
            current_key,
            current_serialized,
            qa_key,
            serialized,
            group_key,
            validate_categories=shard in ("all", "normal-darken") and index == 0,
            render_preview=render_preview,
        )
        preview_hash = (
            rendered["editor"]["preview"]["pixel_hash"]
            if rendered is not None
            else None
        )
        mutations.append(
            {
                "mode": serialized,
                "before_connections": before["project"]["connections"],
                "after_connections": changed["project"]["connections"],
                "preview_hash": preview_hash,
            }
        )
        if preview_hash is not None:
            hashes_by_mode[serialized] = preview_hash
        mode_timings.append(
            {
                "mode": serialized,
                "seconds": round(time.monotonic() - mode_started, 3),
                "preview_waited": render_preview,
            }
        )
        current_key = qa_key
        current_serialized = serialized
    timings["catalog_mutations"] = round(time.monotonic() - phase_started, 3)

    if shard == "all" and len(set(hashes_by_mode.values())) < 4:
        raise QaFailure("representative blend modes did not produce distinct Preview pixels")
    if shard == "all" and hashes_by_mode["Clear"] == hashes_by_mode["Dissolve"]:
        raise QaFailure("Clear and Dissolve produced the same Preview checksum")

    undone_rendered = None
    redone_rendered = None
    restored = changed
    if modes[-1][1] == "Dissolve":
        phase_started = time.monotonic()
        dissolve_connections = list(mutations[-1]["after_connections"])
        pre_dissolve_connections = list(mutations[-1]["before_connections"])
        undone = BASE.undo_project_edit(
            client,
            "Dissolve blend",
            lambda project: project["connections"] == pre_dissolve_connections,
        )
        undone_rendered = client.wait_preview_render_after(undone, "Dissolve Undo")
        restored = BASE.redo_project_edit(
            client,
            "Dissolve blend",
            lambda project: project["connections"] == dissolve_connections,
        )
        redone_rendered = client.wait_preview_render_after(restored, "Dissolve Redo")
        if (
            redone_rendered["editor"]["preview"]["pixel_hash"]
            != hashes_by_mode["Dissolve"]
        ):
            raise QaFailure("Dissolve Redo was not pixel deterministic")
        if restored["editor"]["selection"] != baseline_selection:
            raise QaFailure("Dissolve Undo/Redo changed typed selection")
        if not (
            restored["history"]["undo_depth"]
            == baseline_history["undo_depth"] + len(modes)
            and restored["history"]["redo_depth"] == 0
        ):
            raise QaFailure("Dissolve Undo/Redo left unexpected history depths")
        timings["dissolve_undo_redo"] = round(
            time.monotonic() - phase_started, 3
        )

    # Restore this shard's authoritative fixture. Every command still travels
    # through the real egui keyboard path.
    phase_started = time.monotonic()
    for index in range(len(modes)):
        expected = (
            original_connections
            if index == len(modes) - 1
            else mutations[len(modes) - index - 2]["after_connections"]
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
        == baseline_history["redo_depth"] + len(modes)
    ):
        raise QaFailure("blend suite restore left unexpected history depths")
    timings["catalog_restore"] = round(time.monotonic() - phase_started, 3)

    wire_context = None
    if shard in ("all", "contrast"):
        phase_started = time.monotonic()
        BASE.wait_wire_snapshot_for_project(
            client, [item["id"] for item in original_project["connections"]]
        )
        wire_context, restored = choose_wire_mode_and_restore(
            client, merge_layers(original_project), original_project
        )
        timings["wire_context"] = round(time.monotonic() - phase_started, 3)
        if restored["project"] != original_project:
            raise QaFailure("wire context phase did not leave the exact original Project")
        if restored["editor"]["selection"] != baseline_selection:
            raise QaFailure("wire context phase changed typed selection")

    timings["total"] = round(time.monotonic() - suite_started, 3)

    return {
        "ok": True,
        "suite": "blend-modes-{}".format(shard),
        "shard": shard,
        "health": health,
        "initial_frame": initial["frame"],
        "final_frame": restored["frame"],
        "connection_id": target["id"],
        "modes": [mode for _, mode, _ in modes],
        "preview_hashes": hashes_by_mode,
        "undo_preview_hash": (
            undone_rendered["editor"]["preview"]["pixel_hash"]
            if undone_rendered is not None
            else None
        ),
        "redo_preview_hash": (
            redone_rendered["editor"]["preview"]["pixel_hash"]
            if redone_rendered is not None
            else None
        ),
        "wire_context": wire_context,
        "timings_seconds": timings,
        "mode_timings": mode_timings,
        "actions": client.evidence,
    }


def parse_args():
    parser = argparse.ArgumentParser()
    parser.add_argument("--base-url", default=None)
    parser.add_argument("--spawn", action="store_true")
    parser.add_argument("--evidence", default="target/qa-blend-modes-evidence.json")
    parser.add_argument("--timeout", type=float, default=30.0)
    parser.add_argument(
        "--shard", choices=("all", *MODE_SHARDS), default="all"
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
                ["cargo", "run", "-p", "app", "--locked"],
                env=environment,
                start_new_session=True,
            )
        client = QaClient(base_url, args.timeout)
        result = run_suite(client, args.shard)
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
