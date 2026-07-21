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


def create_from_search(client, query, menu_key, expected_type, expected_analysis=None):
    node_id, state, metadata = BASE.create_node_from_add_search(
        client, query, MENU_ITEMS[menu_key]
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
        client, "sound merge", "sound_merge", "SoundMerge"
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
