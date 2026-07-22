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

LAYOUT_PATH = os.path.join(SCRIPT_DIR, "qa_node_editor_layout.py")
LAYOUT_SPEC = importlib.util.spec_from_file_location(
    "ruvie_qa_node_editor_layout", LAYOUT_PATH
)
if LAYOUT_SPEC is None or LAYOUT_SPEC.loader is None:
    raise RuntimeError("cannot load scripts/qa_node_editor_layout.py")
LAYOUT_MODULE = importlib.util.module_from_spec(LAYOUT_SPEC)
LAYOUT_SPEC.loader.exec_module(LAYOUT_MODULE)
NODE_LAYOUT = LAYOUT_MODULE.NodeEditorLayoutQa(BASE)

QaClient = BASE.QaClient
QaFailure = BASE.QaFailure

OPERATIONS = (
    ("compose color", "compose", "Compose"),
    ("split color", "split", "Split"),
    ("mix color", "mix", "Mix"),
    ("convert color space", "convert_space", "ConvertSpace"),
)
FIXTURE_NAME = "color_operations_e2e"
FIXTURE_PROJECT_NAME = "RuViE Color Operations QA"
FIXTURE_TRACK = "00000000-0000-0000-0000-0000000c0201"


def wait_color_fixture(client):
    """Wait for the isolated empty graph without requiring rendered content."""

    def ready():
        state = client.state()
        project = state["project"]
        if project.get("name") != FIXTURE_PROJECT_NAME:
            raise QaFailure(
                "the isolated Color fixture is required; start with "
                "RUVIE_QA_FIXTURE={}".format(FIXTURE_NAME)
            )
        if state["editor"]["navigation"]["active_composition_id"] != BASE.COMPOSITION:
            raise QaFailure("Color fixture did not activate its Composition")
        preview = state["editor"]["preview"]
        if preview["modal_error"] is not None:
            raise QaFailure("initial Color Preview failed: {}".format(preview["modal_error"]))
        return state if preview["render_revision"] > 0 else None

    initial = client.wait_until("the isolated Color fixture", ready)
    project = initial["project"]
    if len(project["compositions"]) != 1:
        raise QaFailure("Color fixture must contain exactly one Composition")
    composition = project["compositions"][0]
    if composition["id"] != BASE.COMPOSITION or composition["track_ids"] != [
        FIXTURE_TRACK
    ]:
        raise QaFailure("Color fixture Composition containment is not canonical")
    if set(project["tracks"]) != {FIXTURE_TRACK}:
        raise QaFailure("Color fixture must contain exactly one Track")
    if project["tracks"][FIXTURE_TRACK]["clip_ids"]:
        raise QaFailure("Color fixture Track must not contain Clips")
    if project["clips"]:
        raise QaFailure("Color fixture must not contain Clips")
    if len(project["nodes"]) != 4:
        raise QaFailure("Color fixture must contain only four structural Merge Nodes")
    return initial


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
    before = client.state()
    menu_id = "node_editor.menu.create.color:" + operation_key
    node_id, state, metadata = BASE.create_node_from_add_search(
        client,
        query,
        menu_id,
        "node_editor.container.track:{}".format(FIXTURE_TRACK),
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
    BASE.assert_history_delta(before, state, 1, "{} Node creation".format(query))
    owners = BASE.validate_canonical_ownership(state["project"])
    expected_owner = "track:" + FIXTURE_TRACK
    actual_owner = owners["node_owners"].get(node_id)
    if actual_owner != expected_owner:
        raise QaFailure(
            "{} Node owner was {!r}, expected {!r}".format(
                query, actual_owner, expected_owner
            )
        )
    track = state["project"]["tracks"][FIXTURE_TRACK]
    if track["node_ids"].count(node_id) != 1:
        raise QaFailure("{} Node is not contained exactly once".format(query))
    return node_id, metadata, before, state, {
        "node_id": node_id,
        "owner": actual_owner,
        "undo_depth_before": before["history"]["undo_depth"],
        "undo_depth_after": state["history"]["undo_depth"],
    }


def exercise_node_compose_picker_noop(client, node_id):
    """Open and close the compact Node-local picker through real clicks."""
    picker_id = "node_editor.color_picker.node:{}:compose".format(node_id)
    try:
        BASE.reveal_node_editor_component(client, picker_id)
    except QaFailure as error:
        state = client.state()
        snapshot = client.component_snapshot()
        rendered_nodes = sorted(
            component["id"]
            for component in snapshot["components"]
            if component["id"].startswith("node_editor.node:")
        )
        raise QaFailure(
            "{}; Compose node={!r}; track={!r}; rendered_nodes={!r}".format(
                error,
                state["project"]["nodes"].get(node_id),
                state["project"]["tracks"].get(FIXTURE_TRACK),
                rendered_nodes,
            )
        ) from error
    snapshot, picker = client.wait_component_settled(picker_id)
    metadata = picker.get("metadata") or {}
    expected = {
        "operation": "native.color.compose",
        "presentation": "compact_swatch_status",
        "editable": True,
        "popup_open": False,
        "transform_authority": "ruvie-color-management",
    }
    mismatches = {
        key: (metadata.get(key), value)
        for key, value in expected.items()
        if metadata.get(key) != value
    }
    if mismatches:
        raise QaFailure("compact Compose picker metadata mismatch: {!r}".format(mismatches))
    components = {component["id"]: component for component in snapshot["components"]}
    node = components.get("node_editor.node:" + node_id)
    if node is None:
        raise QaFailure("Compose picker frame omitted its owning Node card")
    picker_center = client.point(picker["rect_points"])
    if not BASE.point_in_component_rect(picker_center, node["rect_points"]):
        raise QaFailure("Compose picker is outside its physical Node card")

    before = client.state()
    client.click_component(picker_id)

    def picker_opened():
        state = client.state()
        try:
            opened_snapshot, opened_picker = client.component(picker_id)
        except QaFailure:
            return None
        if (opened_picker.get("metadata") or {}).get("popup_open") is True:
            return state, opened_snapshot, opened_picker
        return None

    opened, opened_snapshot, opened_picker = client.wait_until(
        "compact Compose picker popup", picker_opened
    )
    if opened["project"] != before["project"] or opened["history"] != before["history"]:
        raise QaFailure("opening the compact Compose picker mutated Project/history")

    client.click_component(picker_id)

    def picker_closed():
        state = client.state()
        try:
            closed_snapshot, closed_picker = client.component(picker_id)
        except QaFailure:
            return None
        if (closed_picker.get("metadata") or {}).get("popup_open") is False:
            return state, closed_snapshot, closed_picker
        return None

    closed, closed_snapshot, closed_picker = client.wait_until(
        "compact Compose picker close", picker_closed
    )
    if closed["project"] != before["project"] or closed["history"] != before["history"]:
        raise QaFailure("closing the compact Compose picker mutated Project/history")
    return {
        "picker_id": picker_id,
        "initial_frame": snapshot["frame"],
        "opened_frame": opened_snapshot["frame"],
        "closed_frame": closed_snapshot["frame"],
        "picker_rect": picker["rect_points"],
        "node_rect": node["rect_points"],
        "initial_metadata": metadata,
        "opened_metadata": opened_picker.get("metadata") or {},
        "closed_metadata": closed_picker.get("metadata") or {},
        "project_unchanged": closed["project"] == before["project"],
        "history_unchanged": closed["history"] == before["history"],
    }


def connect(client, from_id, output, to_id, input_port, reverse=False):
    source = NODE_LAYOUT.node_port(from_id, "output", output)
    target = NODE_LAYOUT.node_port(to_id, "input", input_port)
    connection_label = "{}:{} -> {}:{}".format(
        from_id, output, to_id, input_port
    )
    NODE_LAYOUT.compact_wire_endpoints(client, from_id, to_id)
    try:
        port_snapshot, port_components = BASE.ensure_node_editor_ports_interactive(
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
    try:
        connected = client.wait_project(
            "Color {} coordinate wire".format(connection_label),
            lambda project: project
            if len(
                NODE_LAYOUT.matching_connections(
                    project, from_id, output, to_id, input_port
                )
            )
            == 1
            else None,
        )
    except QaFailure as error:
        failed = client.state()
        component_geometry = {
            component["id"]: {
                "rect_points": component.get("rect_points"),
                "metadata": component.get("metadata"),
            }
            for component in port_components
        }
        raise QaFailure(
            "{}; authored connections after drag: {!r}; port frame: {!r}; "
            "port geometry: {!r}; Node positions: {!r}; last action: {!r}".format(
                error,
                failed["project"].get("connections", ()),
                port_snapshot.get("frame"),
                component_geometry,
                {
                    from_id: failed["project"]["nodes"][from_id]["ui_position"],
                    to_id: failed["project"]["nodes"][to_id]["ui_position"],
                },
                client.evidence[-1] if client.evidence else None,
            )
        ) from error
    BASE.assert_history_delta(before, connected, 1, "Color coordinate wire")
    return (
        NODE_LAYOUT.matching_connections(
            connected["project"], from_id, output, to_id, input_port
        )[0],
        before,
        connected,
    )


def assert_undo_redo_roundtrip(client, description, before, after):
    """Prove one representative edit restores exact Project snapshots."""
    undone = BASE.undo_project_edit(
        client,
        description,
        lambda project: project == before["project"],
    )
    if not (
        undone["history"]["undo_depth"]
        == after["history"]["undo_depth"] - 1
        and undone["history"]["redo_depth"]
        == after["history"]["redo_depth"] + 1
    ):
        raise QaFailure("{} Undo changed an unexpected history span".format(description))
    redone = BASE.redo_project_edit(
        client,
        description,
        lambda project: project == after["project"],
    )
    if redone["history"] != after["history"]:
        raise QaFailure("{} Redo did not restore history depths".format(description))
    return {
        "project_exact_after_undo": undone["project"] == before["project"],
        "project_exact_after_redo": redone["project"] == after["project"],
        "history_before": before["history"],
        "history_after_edit": after["history"],
        "history_after_undo": undone["history"],
        "history_after_redo": redone["history"],
    }


def set_node_property(client, node_id, property_key, value, description):
    """Author a Node property through its rendered egui text-entry control."""
    input_id = NODE_LAYOUT.node_port(node_id, "input", property_key)
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
    return before, state


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
    initial = wait_color_fixture(client)
    activate_node_editor(client)

    creation_events = []
    (
        compose_id,
        compose_menu,
        compose_creation_before,
        compose_created,
        compose_creation,
    ) = create_color_node(client, *OPERATIONS[0])
    creation_events.append(compose_creation)
    history_roundtrips = {
        "creation": assert_undo_redo_roundtrip(
            client,
            "Compose Node creation",
            compose_creation_before,
            compose_created,
        )
    }
    compose_picker = exercise_node_compose_picker_noop(client, compose_id)
    created = []
    for operation in (
        OPERATIONS[0],
        OPERATIONS[1],
        OPERATIONS[2],
        OPERATIONS[3],
        OPERATIONS[3],
        OPERATIONS[3],
    ):
        node_id, metadata, _before, _after, event = create_color_node(
            client, *operation
        )
        creation_events.append(event)
        created.append((node_id, metadata))
    (
        (second_compose_id, second_compose_menu),
        (split_id, split_menu),
        (mix_id, mix_menu),
        (linear_a_id, linear_a_menu),
        (linear_b_id, linear_b_menu),
        (display_id, display_menu),
    ) = created

    compose_property_before, compose_property_after = set_node_property(
        client, compose_id, "r", 0.5, "encoded source R authoring"
    )
    history_roundtrips["property"] = assert_undo_redo_roundtrip(
        client,
        "encoded source R authoring",
        compose_property_before,
        compose_property_after,
    )
    set_node_property(
        client, display_id, "target_space", "srgb", "display transform target"
    )

    edge_specs = [
        (compose_id, "color", linear_a_id, "color", False),
        (second_compose_id, "color", linear_b_id, "color", False),
        (linear_a_id, "color", mix_id, "a", True),
        (linear_b_id, "color", mix_id, "b", False),
        (mix_id, "color", display_id, "color", False),
        (display_id, "color", split_id, "color", False),
    ]
    connections = []
    first_connection, first_wire_before, first_wire_after = connect(
        client, *edge_specs[0]
    )
    connections.append(first_connection)
    history_roundtrips["wire"] = assert_undo_redo_roundtrip(
        client,
        "Compose to linear Color wire",
        first_wire_before,
        first_wire_after,
    )
    for edge_spec in edge_specs[1:]:
        connection, _before, _after = connect(client, *edge_spec)
        connections.append(connection)

    # Physical header placement above exists only to make disconnected setup
    # ports reachable.  Once every authored wire exists, the production clean
    # layout owns all Node positions; no Node header is dragged after this.
    arranged = NODE_LAYOUT.clean_layout_all(
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
        label="Color Node",
    )
    layout = NODE_LAYOUT.assert_forward_layout(
        arranged,
        [(source, output, target, input_port) for source, output, target, input_port, _ in edge_specs],
    )
    all_layout_node_ids = tuple(sorted(arranged["project"]["nodes"]))
    rendered_layout = NODE_LAYOUT.assert_rendered_non_overlap(
        client, all_layout_node_ids
    )
    reconnected, post_layout_wire = NODE_LAYOUT.reconnect_wire_after_layout(
        client,
        compose_id,
        "color",
        linear_a_id,
        "color",
        connections[0],
        all_layout_node_ids,
    )
    connections[0] = reconnected
    port_interactions = NODE_LAYOUT.assert_ports_interactive(
        client,
        (
            (
                "post-layout branch wire",
                NODE_LAYOUT.node_port(linear_a_id, "output", "color"),
                NODE_LAYOUT.node_port(mix_id, "input", "a"),
            ),
            (
                "post-layout final wire",
                NODE_LAYOUT.node_port(display_id, "output", "color"),
                NODE_LAYOUT.node_port(split_id, "input", "color"),
            ),
        ),
    )
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

    final = client.state()
    expected_color_nodes = {
        compose_id,
        second_compose_id,
        split_id,
        mix_id,
        linear_a_id,
        linear_b_id,
        display_id,
    }
    final_owners = BASE.validate_canonical_ownership(final["project"])
    expected_owner = "track:" + FIXTURE_TRACK
    color_owners = {
        node_id: final_owners["node_owners"].get(node_id)
        for node_id in sorted(expected_color_nodes)
    }
    if set(color_owners.values()) != {expected_owner}:
        raise QaFailure("final Color Node containment drifted: {!r}".format(color_owners))
    final_positions = {
        node_id: final["project"]["nodes"][node_id]["ui_position"]
        for node_id in sorted(expected_color_nodes)
    }
    if final_positions != layout["positions"]:
        raise QaFailure("Node positions changed after the final clean-layout command")
    if final["history"]["redo_depth"] != 0:
        raise QaFailure("final Color validation left redo history")
    verified_mutation_count = len(creation_events) + len(edge_specs) + 2 + 1 + 2
    history_delta = (
        final["history"]["undo_depth"] - initial["history"]["undo_depth"]
    )
    if history_delta < verified_mutation_count:
        raise QaFailure(
            "final history delta {} is below {} verified edits".format(
                history_delta, verified_mutation_count
            )
        )
    final_validation = {
        "canonical_owner": expected_owner,
        "color_node_owners": color_owners,
        "color_node_count": len(expected_color_nodes),
        "authored_connection_count": len(connections),
        "all_connection_ids_unique": len({item["id"] for item in connections})
        == len(connections),
        "positions_unchanged_after_clean_layout": final_positions
        == layout["positions"],
        "rendered_node_overlap_count": len(rendered_layout["overlaps"]),
        "post_layout_wire_reconnected": post_layout_wire[
            "reconnected_connection_id"
        ]
        == connections[0]["id"],
    }
    if not final_validation["all_connection_ids_unique"]:
        raise QaFailure("Color graph contains duplicate authored connection IDs")

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
        "creation_history": creation_events,
        "history_roundtrips": history_roundtrips,
        "compose_node_picker": compose_picker,
        "layout": layout,
        "rendered_layout": rendered_layout,
        "post_layout_wire_interaction": post_layout_wire,
        "post_layout_port_interactions": port_interactions,
        "final_validation": final_validation,
        "final_history": {
            "baseline": initial["history"],
            "final": final["history"],
            "verified_mutation_count": verified_mutation_count,
            "actual_undo_delta": history_delta,
            "setup_header_drag_entries": history_delta - verified_mutation_count,
        },
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
            environment["RUVIE_QA_FIXTURE"] = FIXTURE_NAME
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
