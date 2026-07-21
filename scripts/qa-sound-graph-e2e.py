#!/usr/bin/env python3
"""Coordinate E2E for native Sound Merge and Sound-analysis Nodes.

Every menu selection, Node selection, and wire connection enters through the
loopback QA bridge using rectangles from the latest completed egui frame.
Project-state reads are assertions only; they never author the graph.
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
free_port = BASE.free_port

MENU_ITEMS = {
    "sound_merge": "node_editor.menu.create.sound_merge",
    "rms": "node_editor.menu.create.sound_rms",
    "peak": "node_editor.menu.create.sound_peak",
    "spectrum": "node_editor.menu.create.sound_spectrum",
    "band_energy": "node_editor.menu.create.sound_band_energy",
}

AUDIO_A = BASE.AUDIO_A
AUDIO_B = BASE.AUDIO_B
CLIP_A1 = BASE.CLIP_A1
CLIP_A2 = BASE.CLIP_A2
TRACK_A = BASE.TRACK_A


def node_content_type(project, node_id):
    return project["nodes"][node_id].get("content", {}).get("type")


def sound_analysis_kind(project, node_id):
    content = project["nodes"][node_id].get("content", {})
    return content.get("data") if content.get("type") == "SoundAnalysis" else None


def activate_node_editor(client):
    client.wait_component_settled("dock.tab:node_editor")
    client.click_component("dock.tab:node_editor")
    return client.wait_until(
        "Node Editor dock activation",
        lambda: state
        if "Node Editor" in (state := client.state())["dock"]["active_tabs"]
        else None,
    )


def create_rms_through_hierarchy(client):
    before = client.state()
    node_ids_before = set(before["project"]["nodes"])
    BASE.open_create_menu(client, operation="hierarchical Sound Add menu")
    client.wait_component("node_editor.menu.search")

    sound_category = "node_editor.menu.search.category:Sound"
    analysis_category = "node_editor.menu.search.category:Sound/Analysis"
    client.wait_component_settled(sound_category)
    client.click_component(sound_category)
    client.wait_component_settled(analysis_category)
    client.click_component(analysis_category)

    menu_metadata = {}
    for name, component_id in MENU_ITEMS.items():
        _, component = client.wait_component_settled(component_id)
        metadata = component.get("metadata") or {}
        expected_category = "Sound" if name == "sound_merge" else "Sound / Analysis"
        if metadata.get("category") != expected_category:
            raise QaFailure(
                "{} has category {!r}, expected {!r}".format(
                    component_id, metadata.get("category"), expected_category
                )
            )
        menu_metadata[name] = metadata

    client.click_component(MENU_ITEMS["rms"])
    created = client.wait_project(
        "RMS creation through nested Sound/Analysis menu",
        lambda project: len(project["nodes"]) == len(node_ids_before) + 1,
    )
    node_id = (set(created["project"]["nodes"]) - node_ids_before).pop()
    if sound_analysis_kind(created["project"], node_id) != "Rms":
        raise QaFailure("nested RMS menu did not create an RMS analysis Node")
    return node_id, menu_metadata


def create_from_search(
    client,
    query,
    menu_key,
    expected_type,
    expected_analysis=None,
    scope_component_id=None,
):
    node_id, state, metadata = BASE.create_node_from_add_search(
        client, query, MENU_ITEMS[menu_key], scope_component_id
    )
    project = state["project"]
    if node_content_type(project, node_id) != expected_type:
        raise QaFailure(
            "{} search created {}, expected {}".format(
                query, node_content_type(project, node_id), expected_type
            )
        )
    if expected_analysis is not None and sound_analysis_kind(project, node_id) != expected_analysis:
        raise QaFailure(
            "{} search created analysis {}, expected {}".format(
                query, sound_analysis_kind(project, node_id), expected_analysis
            )
        )
    return node_id, metadata


def validate_port_contract(client, node_id, definitions):
    component_ids = [
        "node_editor.port.node:{}.{}:{}".format(node_id, direction, key)
        for direction, key, _data_type in definitions
    ]
    _snapshot, components = BASE.ensure_node_editor_ports_interactive(
        client, component_ids
    )
    evidence = {}
    for component, (direction, key, data_type) in zip(components, definitions):
        metadata = component.get("metadata") or {}
        expected_owner = "node:" + node_id
        address = metadata.get("address") or {}
        if not (
            metadata.get("direction") == direction
            and metadata.get("data_type") == data_type
            and metadata.get("normal_interaction_enabled") is True
            and address.get("owner") == expected_owner
            and address.get("port") == key
        ):
            raise QaFailure(
                "typed port contract mismatch for {}: {!r}".format(
                    component["id"], metadata
                )
            )
        evidence[component["id"]] = metadata
    return evidence


def connect_spectrum_to_band_energy(client, spectrum_id, band_energy_id):
    source_id = "node_editor.port.node:{}.output:spectrum".format(spectrum_id)
    target_id = "node_editor.port.node:{}.input:spectrum_in".format(band_energy_id)
    BASE.ensure_node_editor_ports_interactive(client, [source_id, target_id])
    before = client.state()
    connection_ids_before = {
        connection["id"] for connection in before["project"]["connections"]
    }
    client.drag_components(source_id, target_id, steps=14)

    def connected(project):
        try:
            return BASE.find_project_connection(
                project,
                "Node",
                spectrum_id,
                "spectrum",
                "Node",
                band_energy_id,
                "spectrum_in",
            )
        except QaFailure:
            return None

    state = client.wait_project("FFT Spectrum to Band Energy wire", connected)
    connection = connected(state["project"])
    if connection["id"] in connection_ids_before:
        raise QaFailure("Spectrum wire did not create a new canonical connection")
    if state["history"]["undo_depth"] != before["history"]["undo_depth"] + 1:
        raise QaFailure("Spectrum wire did not produce exactly one history entry")
    return connection


def merge_connections(project, merge_id, port):
    return sorted(
        (
            connection
            for connection in project["connections"]
            if connection["to"]["owner"].get("owner_type") == "Node"
            and connection["to"]["owner"].get("owner_id") == merge_id
            and connection["to"].get("port") == port
        ),
        key=lambda connection: (connection["order"], connection["id"]),
    )


def connection_source_owner(connection):
    owner = connection["from"]["owner"]
    return "{}:{}".format(owner["owner_type"].lower(), owner["owner_id"])


def assert_wire_identity(before, after, operation):
    before_by_id = {connection["id"]: connection for connection in before}
    after_by_id = {connection["id"]: connection for connection in after}
    if set(before_by_id) != set(after_by_id):
        raise QaFailure("{} changed canonical wire IDs".format(operation))
    for connection_id, original in before_by_id.items():
        current = after_by_id[connection_id]
        for field in ("id", "from", "to", "blend_mode"):
            if current[field] != original[field]:
                raise QaFailure(
                    "{} changed {} on wire {}".format(
                        operation, field, connection_id
                    )
                )


def connect_audio_source(client, source_node_id, merge_id, description):
    source_id = "node_editor.port.node:{}.output:audio".format(source_node_id)
    target_id = "node_editor.port.node:{}.input:sounds".format(merge_id)
    BASE.ensure_node_editor_ports_interactive(client, [source_id, target_id])
    before = client.state()
    before_ids = {
        connection["id"]
        for connection in merge_connections(before["project"], merge_id, "sounds")
    }
    client.drag_components(source_id, target_id, steps=16)

    def added(project):
        matches = [
            connection
            for connection in merge_connections(project, merge_id, "sounds")
            if connection["id"] not in before_ids
            and connection_source_owner(connection) == "node:" + source_node_id
            and connection["from"]["port"] == "audio"
        ]
        return matches[0] if len(matches) == 1 else None

    state = client.wait_project(description, added)
    connection = added(state["project"])
    BASE.assert_history_delta(before, state, 1, description)
    return connection, state


def row_component_ids(merge_id, connection_id):
    return (
        "node_editor.merge_layer.drag_handle:{}:{}".format(
            merge_id, connection_id
        ),
        "node_editor.merge_layer:{}:{}".format(merge_id, connection_id),
    )


def validate_sound_row(client, merge_id, connection, expected_min, expected_max):
    handle_id, row_id = row_component_ids(merge_id, connection["id"])
    BASE.reveal_node_editor_components(client, [handle_id, row_id])
    _, row = client.wait_component_settled(row_id)
    metadata = row.get("metadata") or {}
    if not (
        metadata.get("merge_kind") == "sound"
        and metadata.get("port") == "sounds"
        and metadata.get("canonical_order_semantics") == "top_to_bottom"
        and metadata.get("visual_order_semantics") == "top_to_bottom"
        and metadata.get("authored_blend_available") is False
        and metadata.get("blend_ownership") is None
        and metadata.get("reorder_min_canonical_index") == expected_min
        and metadata.get("reorder_max_canonical_index") == expected_max
    ):
        raise QaFailure("Sound Merge row metadata is stale: {!r}".format(metadata))
    return {"handle_id": handle_id, "row_id": row_id, "metadata": metadata}


def reorder_custom_sound_rows(client, merge_id, first, second):
    original = merge_connections(client.state()["project"], merge_id, "sounds")
    if [connection["id"] for connection in original] != [first["id"], second["id"]]:
        raise QaFailure("standalone Sound Merge did not append inputs top-to-bottom")
    first_ui = validate_sound_row(client, merge_id, first, 0, 1)
    second_ui = validate_sound_row(client, merge_id, second, 0, 1)
    before = client.state()
    client.drag_components(first_ui["handle_id"], second_ui["row_id"], steps=14)
    reordered = client.wait_project(
        "physical Sound Merge custom row reorder",
        lambda project: [
            connection["id"]
            for connection in merge_connections(project, merge_id, "sounds")
        ]
        == [second["id"], first["id"]],
    )
    BASE.assert_history_delta(before, reordered, 1, "Sound Merge custom row reorder")
    assert_wire_identity(
        original,
        merge_connections(reordered["project"], merge_id, "sounds"),
        "Sound Merge custom row reorder",
    )
    undone = BASE.undo_project_edit(
        client,
        "Sound Merge custom row reorder",
        lambda project: [
            connection["id"]
            for connection in merge_connections(project, merge_id, "sounds")
        ]
        == [first["id"], second["id"]],
    )
    redone = BASE.redo_project_edit(
        client,
        "Sound Merge custom row reorder",
        lambda project: [
            connection["id"]
            for connection in merge_connections(project, merge_id, "sounds")
        ]
        == [second["id"], first["id"]],
    )
    assert_wire_identity(
        original,
        merge_connections(redone["project"], merge_id, "sounds"),
        "Sound Merge custom row reorder redo",
    )
    return {
        "before": [first["id"], second["id"]],
        "after": [second["id"], first["id"]],
        "rows": [first_ui["metadata"], second_ui["metadata"]],
        "undo_depth": undone["history"]["undo_depth"],
        "redo_depth": redone["history"]["undo_depth"],
    }


def structural_prefix(project, merge_id, port, owner_type):
    return [
        connection
        for connection in merge_connections(project, merge_id, port)
        if connection["from"]["owner"].get("owner_type") == owner_type
    ]


def track_order_is(project, clip_ids, sound_merge_id, image_merge_id):
    expected_sources = ["clip:" + clip_id for clip_id in clip_ids]
    sound_sources = [
        connection_source_owner(connection)
        for connection in structural_prefix(
            project, sound_merge_id, "sounds", "Clip"
        )
    ]
    image_sources = [
        connection_source_owner(connection)
        for connection in structural_prefix(
            project, image_merge_id, "images", "Clip"
        )
    ]
    return (
        project["tracks"][TRACK_A]["clip_ids"] == clip_ids
        and sound_sources == expected_sources
        and image_sources == expected_sources
    )


def exercise_structural_sound_rows(client):
    initial = client.state()
    track = initial["project"]["tracks"][TRACK_A]
    sound_merge_id = track["structural_sound_merge_node_id"]
    image_merge_id = track["structural_merge_node_id"]
    if not track_order_is(
        initial["project"], [CLIP_A1, CLIP_A2], sound_merge_id, image_merge_id
    ):
        raise QaFailure("Track A typed structural prefixes do not match Timeline order")

    custom_a, _ = connect_audio_source(
        client,
        AUDIO_A,
        sound_merge_id,
        "Audio A custom input on structural Sound Merge",
    )
    custom_b, custom_state = connect_audio_source(
        client,
        AUDIO_B,
        sound_merge_id,
        "Audio B custom input on structural Sound Merge",
    )
    sound_rows = merge_connections(
        custom_state["project"], sound_merge_id, "sounds"
    )
    if [connection_source_owner(connection) for connection in sound_rows] != [
        "clip:" + CLIP_A1,
        "clip:" + CLIP_A2,
        "node:" + AUDIO_A,
        "node:" + AUDIO_B,
    ]:
        raise QaFailure("custom Sound inputs did not remain behind structural prefix")

    custom_a_ui = validate_sound_row(client, sound_merge_id, custom_a, 2, 3)
    custom_b_ui = validate_sound_row(client, sound_merge_id, custom_b, 2, 3)
    structural_first = sound_rows[0]
    structural_second = sound_rows[1]
    structural_first_ui = validate_sound_row(
        client, sound_merge_id, structural_first, 0, 1
    )
    structural_second_ui = validate_sound_row(
        client, sound_merge_id, structural_second, 0, 1
    )
    if not all(
        (row.get("structural_child") or {}).get("reorders_timeline") is True
        for row in (structural_first_ui["metadata"], structural_second_ui["metadata"])
    ):
        raise QaFailure("structural Sound rows did not advertise Timeline ownership")
    if any(
        row.get("structural_child") is not None
        for row in (custom_a_ui["metadata"], custom_b_ui["metadata"])
    ):
        raise QaFailure("custom Sound rows were misclassified as Timeline children")

    # A real drag into the structural prefix is outside this custom row's
    # legal [2, 3] range. It must cancel without a Project/history mutation.
    boundary_before = client.state()
    client.drag_components(
        custom_a_ui["handle_id"], structural_first_ui["row_id"], steps=14
    )
    boundary_after = client.state()
    if (
        boundary_after["project"] != boundary_before["project"]
        or boundary_after["history"] != boundary_before["history"]
    ):
        raise QaFailure("custom Sound row crossed the mandatory structural prefix")

    sound_before = client.state()
    before_sound_rows = merge_connections(
        sound_before["project"], sound_merge_id, "sounds"
    )
    before_image_rows = merge_connections(
        sound_before["project"], image_merge_id, "images"
    )
    client.drag_components(
        structural_first_ui["handle_id"], structural_second_ui["row_id"], steps=14
    )
    sound_reordered = client.wait_project(
        "structural Sound row Timeline reorder",
        lambda project: track_order_is(
            project, [CLIP_A2, CLIP_A1], sound_merge_id, image_merge_id
        ),
    )
    BASE.assert_history_delta(
        sound_before, sound_reordered, 1, "structural Sound row Timeline reorder"
    )
    assert_wire_identity(
        before_sound_rows,
        merge_connections(sound_reordered["project"], sound_merge_id, "sounds"),
        "structural Sound row Timeline reorder",
    )
    assert_wire_identity(
        before_image_rows,
        merge_connections(sound_reordered["project"], image_merge_id, "images"),
        "structural Sound row synchronized Image prefix",
    )
    BASE.undo_project_edit(
        client,
        "structural Sound row Timeline reorder",
        lambda project: track_order_is(
            project, [CLIP_A1, CLIP_A2], sound_merge_id, image_merge_id
        ),
    )
    BASE.redo_project_edit(
        client,
        "structural Sound row Timeline reorder",
        lambda project: track_order_is(
            project, [CLIP_A2, CLIP_A1], sound_merge_id, image_merge_id
        ),
    )

    # Exercise the existing Image physical-row path against the same typed
    # structural transaction. Image's display is Front->Back, while the
    # canonical row index remains the Timeline child index.
    image_state = client.state()
    image_rows = structural_prefix(
        image_state["project"], image_merge_id, "images", "Clip"
    )
    if [connection_source_owner(connection) for connection in image_rows] != [
        "clip:" + CLIP_A2,
        "clip:" + CLIP_A1,
    ]:
        raise QaFailure("Image structural prefix did not follow Sound row reorder")
    image_first_handle, _ = row_component_ids(image_merge_id, image_rows[0]["id"])
    _, image_second_row = row_component_ids(image_merge_id, image_rows[1]["id"])
    BASE.reveal_node_editor_components(
        client, [image_first_handle, image_second_row]
    )
    image_before = client.state()
    client.drag_components(image_first_handle, image_second_row, steps=14)
    image_reordered = client.wait_project(
        "Image structural row Timeline reorder regression",
        lambda project: track_order_is(
            project, [CLIP_A1, CLIP_A2], sound_merge_id, image_merge_id
        ),
    )
    BASE.assert_history_delta(
        image_before,
        image_reordered,
        1,
        "Image structural row Timeline reorder regression",
    )
    BASE.undo_project_edit(
        client,
        "Image structural row Timeline reorder regression",
        lambda project: track_order_is(
            project, [CLIP_A2, CLIP_A1], sound_merge_id, image_merge_id
        ),
    )
    image_redone = BASE.redo_project_edit(
        client,
        "Image structural row Timeline reorder regression",
        lambda project: track_order_is(
            project, [CLIP_A1, CLIP_A2], sound_merge_id, image_merge_id
        ),
    )
    return {
        "sound_merge": sound_merge_id,
        "image_merge": image_merge_id,
        "custom_connections": [custom_a["id"], custom_b["id"]],
        "structural_sound_connections": [
            structural_first["id"],
            structural_second["id"],
        ],
        "custom_prefix_boundary_noop": True,
        "final_timeline_clip_ids": image_redone["project"]["tracks"][TRACK_A][
            "clip_ids"
        ],
    }


def inspect_band_energy(client, node_id):
    header_id = "node_editor.node_header:" + node_id
    BASE.reveal_node_editor_component(client, header_id)
    client.click_component(header_id)
    client.wait_until(
        "Band Energy Node selection",
        lambda: state
        if BASE.selection_matches((state := client.state()), "node", node_id)
        else None,
    )
    owner_id = "inspector.owner.node:" + node_id
    _, owner = client.wait_component_settled(owner_id)
    owner_metadata = owner.get("metadata") or {}
    if owner_metadata.get("node_type") != "Band Energy":
        raise QaFailure("Inspector did not resolve exact Band Energy Node metadata")
    property_ids = [
        "inspector.property.node:{}:low_hz".format(node_id),
        "inspector.property.node:{}:high_hz".format(node_id),
    ]
    property_metadata = {}
    for component_id in property_ids:
        component = client.ensure_in_scroll_area(component_id)
        metadata = component.get("metadata") or {}
        ui = ((metadata.get("definition") or {}).get("ui") or {})
        if not (
            ui.get("kind") == "float"
            and ui.get("min") == 0.0
            and ui.get("max") == 96000.0
            and ui.get("step") == 1.0
            and ui.get("suffix") == " Hz"
            and ui.get("min_hard_limit") is True
            and ui.get("max_hard_limit") is True
        ):
            raise QaFailure(
                "Band Energy Inspector lost canonical frequency metadata: {!r}".format(
                    metadata
                )
            )
        property_metadata[component_id] = metadata
    return {
        "owner": owner_metadata,
        "properties": property_metadata,
    }


def run_suite(client):
    health = client.wait_health()
    initial = BASE.wait_fresh_fixture(client)
    activate_node_editor(client)

    rms_id, hierarchy_metadata = create_rms_through_hierarchy(client)
    sound_merge_id, sound_merge_menu = create_from_search(
        client,
        "sound merge",
        "sound_merge",
        "SoundMerge",
        scope_component_id="node_editor.container.clip:" + CLIP_A1,
    )
    peak_id, peak_menu = create_from_search(
        client, "peak amplitude", "peak", "SoundAnalysis", "Peak"
    )
    spectrum_id, spectrum_menu = create_from_search(
        client, "fft spectrum", "spectrum", "SoundAnalysis", "Spectrum"
    )
    band_energy_id, band_energy_menu = create_from_search(
        client, "band energy", "band_energy", "SoundAnalysis", "BandEnergy"
    )

    port_metadata = {}
    for node_id, definitions in (
        (
            sound_merge_id,
            (("input", "sounds", "audio"), ("output", "audio", "audio")),
        ),
        (
            rms_id,
            (
                ("input", "sound", "audio"),
                ("input", "window_ms", "numeric"),
                ("input", "hop_ms", "numeric"),
                ("input", "sample_rate", "numeric"),
                ("output", "result", "number"),
            ),
        ),
        (
            peak_id,
            (
                ("input", "sound", "audio"),
                ("input", "window_ms", "numeric"),
                ("input", "hop_ms", "numeric"),
                ("input", "sample_rate", "numeric"),
                ("output", "result", "number"),
            ),
        ),
        (
            spectrum_id,
            (
                ("input", "sound", "audio"),
                ("input", "window_ms", "numeric"),
                ("input", "hop_ms", "numeric"),
                ("input", "sample_rate", "numeric"),
                ("output", "spectrum", "spectrum"),
            ),
        ),
        (
            band_energy_id,
            (
                ("input", "spectrum_in", "spectrum"),
                ("input", "low_hz", "numeric"),
                ("input", "high_hz", "numeric"),
                ("output", "result", "number"),
            ),
        ),
    ):
        port_metadata.update(validate_port_contract(client, node_id, definitions))

    connection = connect_spectrum_to_band_energy(
        client, spectrum_id, band_energy_id
    )
    inspector = inspect_band_energy(client, band_energy_id)

    audio_a_connection, _ = connect_audio_source(
        client, AUDIO_A, sound_merge_id, "Audio A to standalone Sound Merge"
    )
    audio_b_connection, _ = connect_audio_source(
        client, AUDIO_B, sound_merge_id, "Audio B to standalone Sound Merge"
    )
    standalone_reorder = reorder_custom_sound_rows(
        client, sound_merge_id, audio_a_connection, audio_b_connection
    )
    structural_reorder = exercise_structural_sound_rows(client)
    final = client.state()
    if not any(action.get("endpoint") == "drag" for action in client.evidence):
        raise QaFailure("Sound graph E2E produced no coordinate wire drag evidence")

    return {
        "ok": True,
        "suite": "sound-graph",
        "health_frame": health["frame"],
        "initial_frame": initial["frame"],
        "final_frame": final["frame"],
        "nodes": {
            "sound_merge": sound_merge_id,
            "rms": rms_id,
            "peak": peak_id,
            "spectrum": spectrum_id,
            "band_energy": band_energy_id,
        },
        "menus": {
            "hierarchy": hierarchy_metadata,
            "sound_merge_search": sound_merge_menu,
            "peak_search": peak_menu,
            "spectrum_search": spectrum_menu,
            "band_energy_search": band_energy_menu,
        },
        "ports": port_metadata,
        "standalone_sound_merge": {
            "connections": [audio_a_connection, audio_b_connection],
            "reorder": standalone_reorder,
        },
        "structural_sound_merge": structural_reorder,
        "spectrum_connection": connection,
        "inspector": inspector,
        "final_history": final["history"],
        "actions": client.evidence,
    }


def parse_args():
    parser = argparse.ArgumentParser()
    parser.add_argument("--base-url", default=None)
    parser.add_argument("--spawn", action="store_true")
    parser.add_argument(
        "--evidence",
        default="target/qa-sound-graph-e2e-evidence.json",
    )
    parser.add_argument("--timeout", type=float, default=30.0)
    return parser.parse_args()


def main():
    args = parse_args()
    process = None
    port = free_port() if args.spawn else 39091
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
        print("[qa-sound-graph-e2e] PASS; evidence: {}".format(evidence_path))
        return 0
    except (QaFailure, AssertionError, KeyError, StopIteration, TypeError) as error:
        print("[qa-sound-graph-e2e] FAIL: {}".format(error), file=sys.stderr)
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
