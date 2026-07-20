#!/usr/bin/env python3
"""Coordinate E2E for Preview facade selection and exact Transform routing.

Every click and drag is injected through the loopback bridge at a rectangle
resolved from the latest complete egui frame.  Assertions read the
authoritative Project and transient editor state; no model-mutation QA command
is used.
"""

from __future__ import annotations

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
SPEC = importlib.util.spec_from_file_location("ruvie_qa_transform_preview_base", BASE_PATH)
if SPEC is None or SPEC.loader is None:
    raise RuntimeError("cannot load scripts/qa-e2e.py")
BASE = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(BASE)

QaClient = BASE.QaClient
QaFailure = BASE.QaFailure
free_port = BASE.free_port
repository_git_commit = BASE.repository_git_commit

ROOT_TRANSFORM_IDENTITY = ("transform", "transform", "transform.apply.v1")
ROOT_TRANSFORM_PROPERTIES = frozenset(("position", "rotation", "scale", "anchor"))
AMBIGUOUS_CLIP = "00000000-0000-0000-0000-000000000304"
AMBIGUOUS_TRANSFORM_A = "00000000-0000-0000-0000-000000000510"
AMBIGUOUS_TRANSFORM_B = "00000000-0000-0000-0000-000000000511"
AMBIGUOUS_FIXTURE_NODES = frozenset(
    {
        "00000000-0000-0000-0000-000000000410",
        "00000000-0000-0000-0000-000000000411",
        AMBIGUOUS_TRANSFORM_A,
        AMBIGUOUS_TRANSFORM_B,
        "00000000-0000-0000-0000-000000000610",
        "00000000-0000-0000-0000-000000000611",
        "00000000-0000-0000-0000-000000000612",
    }
)


def operation_identity(node):
    content = node.get("content", {})
    if content.get("type") != "PluginOperation":
        return None
    operation = content.get("data", {})
    return (
        operation.get("category"),
        operation.get("component_id"),
        operation.get("operation"),
    )


def validate_root_transform_node(node):
    """Fail closed unless a Node is the complete native root Transform contract."""
    if operation_identity(node) != ROOT_TRANSFORM_IDENTITY:
        raise QaFailure(
            "root Transform identity is {!r}, expected {!r}".format(
                operation_identity(node), ROOT_TRANSFORM_IDENTITY
            )
        )
    declared_ports = node.get("content", {}).get("data", {}).get("declared_ports")
    if not isinstance(declared_ports, list) or not declared_ports:
        raise QaFailure("root Transform omitted its persisted port contract")
    properties = node.get("properties", {})
    if set(properties) != ROOT_TRANSFORM_PROPERTIES:
        raise QaFailure(
            "root Transform properties are {}, expected {}".format(
                sorted(properties), sorted(ROOT_TRANSFORM_PROPERTIES)
            )
        )
    for name in ROOT_TRANSFORM_PROPERTIES:
        if "value" not in properties[name].get("properties", {}):
            raise QaFailure("root Transform property {} is uninitialized".format(name))
    return node


def wait_selected_tool(client, tool):
    component_id = "preview.tool." + tool

    def selected():
        snapshot, component = client.component(component_id)
        metadata = component.get("metadata") or {}
        return (snapshot, component) if metadata.get("selected") is True else None

    return client.wait_until("Preview {} tool activation".format(tool), selected)


def activate_preview_select(client, operation):
    BASE.activate_dock_tab(client, "dock.tab:preview", "Preview", operation)
    client.click_component("preview.tool.select")
    wait_selected_tool(client, "select")


def visual_component_id(spatial_node_id):
    return "preview.visual.spatial:" + spatial_node_id


def component_by_id(snapshot, component_id):
    return next(
        (item for item in snapshot["components"] if item["id"] == component_id),
        None,
    )


def wait_registered_component(client, component_id):
    """Wait for a visible component even when it is intentionally disabled."""

    def registered():
        snapshot = client.component_snapshot()
        component = component_by_id(snapshot, component_id)
        if component is None:
            return None
        rect = component.get("rect_points") or {}
        if (
            component.get("visible") is not True
            or rect.get("width", 0.0) <= 0.0
            or rect.get("height", 0.0) <= 0.0
        ):
            return None
        return snapshot, component

    return client.wait_until("registered component " + component_id, registered)


def require_spatial_visual(client, spatial_node_id, content_node_id):
    component_id = visual_component_id(spatial_node_id)
    _, component = client.wait_component_settled(component_id)
    metadata = component.get("metadata") or {}
    if not (
        component.get("enabled") is True
        and metadata.get("content_node_id") == content_node_id
        and metadata.get("editable_spatial_node_id") == spatial_node_id
        and metadata.get("action") == "select_or_drag_preview_visual"
    ):
        raise QaFailure("Preview visual omitted explicit content/spatial ownership")
    return component_id, component


def rect_contains(rect, point):
    return (
        rect["min_x"] <= point["x"] <= rect["max_x"]
        and rect["min_y"] <= point["y"] <= rect["max_y"]
    )


def click_topmost_visual(client, spatial_node_id, content_node_id):
    """Click a point where the requested render instance is top-most."""
    component_id, _ = require_spatial_visual(
        client, spatial_node_id, content_node_id
    )
    # Resolve geometry immediately before injection.  Instance rectangles let
    # us avoid accidentally selecting a later Merge layer that overlaps the
    # canonical content/spatial rectangle.
    snapshot = client.component_snapshot()
    canonical = component_by_id(snapshot, component_id)
    if canonical is None:
        raise QaFailure("Preview visual disappeared before coordinate click")
    target_index = (canonical.get("metadata") or {}).get("instance_index")
    instances = [
        item
        for item in snapshot["components"]
        if item["id"].startswith("preview.visual.instance:")
        and item.get("visible") is True
    ]
    rect = canonical["rect_points"]
    chosen = None
    for y_fraction in (0.15, 0.3, 0.5, 0.7, 0.85):
        for x_fraction in (0.15, 0.3, 0.5, 0.7, 0.85):
            point = client.point(rect, x_fraction, y_fraction)
            covering = [
                item for item in instances if rect_contains(item["rect_points"], point)
            ]
            if not covering:
                continue
            topmost = max(
                covering,
                key=lambda item: (item.get("metadata") or {}).get(
                    "instance_index", -1
                ),
            )
            metadata = topmost.get("metadata") or {}
            if (
                metadata.get("instance_index") == target_index
                and metadata.get("content_node_id") == content_node_id
                and metadata.get("editable_spatial_node_id") == spatial_node_id
            ):
                chosen = point
                break
        if chosen is not None:
            break
    if chosen is None:
        raise QaFailure(
            "no visible top-most coordinate found for Preview content {}".format(
                content_node_id
            )
        )
    client.inject(
        "click",
        {
            "x": chosen["x"],
            "y": chosen["y"],
            "coordinate_space": "points",
            "button": "primary",
        },
        {
            "component_id": component_id,
            "component_frame": snapshot["frame"],
            "component_rect_points": rect,
            "target_instance_index": target_index,
        },
    )
    return chosen


def expected_owner(kind, entity_id):
    return {"kind": kind, "id": entity_id}


def assert_preview_edit_target(
    state, owner_kind, owner_id, content_node_id, spatial_node_id, operation
):
    target = state["editor"]["preview"].get("edit_target")
    expected = {
        "owner": expected_owner(owner_kind, owner_id),
        "content_node_id": content_node_id,
        "spatial_node_id": spatial_node_id,
    }
    if not isinstance(target, dict) or any(
        target.get(key) != value for key, value in expected.items()
    ):
        raise QaFailure(
            "{} exposed wrong transient Preview edit target {!r}".format(
                operation, target
            )
        )
    if not target.get("instance_path"):
        raise QaFailure("{} omitted Preview instance path".format(operation))
    return target


def assert_inspector_owner(client, kind, entity_id, operation):
    component_id = "inspector.owner.{}:{}".format(kind, entity_id)
    _, component = client.wait_component_settled(component_id)
    metadata = component.get("metadata") or {}
    if metadata.get("owner") != kind or metadata.get("id") != entity_id:
        raise QaFailure("{} left Inspector on the wrong owner".format(operation))
    return component


def assert_only_transform_node_changed(before_project, after_project, node_id):
    before = copy.deepcopy(before_project)
    after = copy.deepcopy(after_project)
    before_node = before["nodes"].pop(node_id)
    after_node = after["nodes"].pop(node_id)
    if before != after:
        raise QaFailure("Preview gizmo changed Project state outside its Transform Node")

    before_properties = before_node.pop("properties")
    after_properties = after_node.pop("properties")
    if before_node != after_node:
        raise QaFailure("Preview gizmo changed non-property Transform data")
    changed = set()
    for property_name in set(before_properties) | set(after_properties):
        if before_properties.get(property_name) != after_properties.get(property_name):
            changed.add(property_name)
    if not changed or not changed <= {"position", "scale", "rotation"}:
        raise QaFailure(
            "Preview gizmo changed invalid Transform properties {}".format(
                sorted(changed)
            )
        )
    return sorted(changed)


def select_text_from_preview(client):
    activate_preview_select(client, "Preview Text facade selection")
    click_topmost_visual(client, BASE.TEXT_TRANSFORM, BASE.TEXT)
    selected = client.wait_until(
        "Preview Text click selects Clip facade",
        lambda: state
        if BASE.selection_matches((state := client.state()), "clip", BASE.CLIP_A2)
        else None,
    )
    BASE.assert_exact_selection(
        selected, "clip", BASE.CLIP_A2, "Preview Text facade selection"
    )
    target = assert_preview_edit_target(
        selected,
        "clip",
        BASE.CLIP_A2,
        BASE.TEXT,
        BASE.TEXT_TRANSFORM,
        "Preview Text facade selection",
    )
    assert_inspector_owner(
        client, "clip", BASE.CLIP_A2, "Preview Text facade selection"
    )
    return {
        "frame": selected["frame"],
        "selection": selected["editor"]["selection"],
        "edit_target": target,
    }


def timeline_select_and_drag_canonical_gizmo(client):
    BASE.activate_dock_tab(
        client, "dock.tab:timeline", "Timeline", "canonical Clip gizmo selection"
    )
    client.click_component("timeline.clip:" + BASE.CLIP_A2)
    selected = client.wait_until(
        "Timeline Clip coordinate selection",
        lambda: state
        if BASE.selection_matches((state := client.state()), "clip", BASE.CLIP_A2)
        else None,
    )
    BASE.assert_exact_selection(
        selected, "clip", BASE.CLIP_A2, "Timeline Clip coordinate selection"
    )
    assert_inspector_owner(
        client, "clip", BASE.CLIP_A2, "Timeline Clip coordinate selection"
    )

    activate_preview_select(client, "canonical Clip gizmo")
    gizmo_id = "preview.gizmo.handle:right"
    _, gizmo = client.wait_component_settled(gizmo_id)
    metadata = gizmo.get("metadata") or {}
    if not (
        metadata.get("owner") == expected_owner("clip", BASE.CLIP_A2)
        and metadata.get("content_node_id") == BASE.TEXT
        and metadata.get("spatial_node_id") == BASE.TEXT_TRANSFORM
        and metadata.get("handle") == "right"
        and metadata.get("action") == "drag_preview_gizmo_handle"
    ):
        raise QaFailure("Timeline Clip did not resolve the canonical Text Transform gizmo")

    before = client.wait_preview_settled("canonical Clip gizmo baseline")
    before_scale = BASE.property_value(
        before["project"]["nodes"][BASE.TEXT_TRANSFORM], "scale"
    )
    client.drag_component_by(gizmo_id, 24.0, 0.0, steps=12)
    edited = client.wait_project(
        "canonical gizmo coordinate drag",
        lambda project: BASE.property_value(
            project["nodes"][BASE.TEXT_TRANSFORM], "scale"
        )
        != before_scale,
    )
    changed_properties = assert_only_transform_node_changed(
        before["project"], edited["project"], BASE.TEXT_TRANSFORM
    )
    BASE.assert_history_delta(before, edited, 1, "canonical Clip gizmo drag")
    BASE.assert_exact_selection(
        edited, "clip", BASE.CLIP_A2, "canonical Clip gizmo drag"
    )
    assert_preview_edit_target(
        edited,
        "clip",
        BASE.CLIP_A2,
        BASE.TEXT,
        BASE.TEXT_TRANSFORM,
        "canonical Clip gizmo drag",
    )
    rendered = client.wait_preview_change(
        before["editor"]["preview"]["pixel_hash"],
        before["editor"]["preview"]["render_revision"],
    )
    restored = BASE.undo_project_edit(
        client,
        "canonical Clip gizmo drag",
        lambda project: project == before["project"],
    )
    BASE.wait_preview_hash_after(
        client,
        before["editor"]["preview"]["pixel_hash"],
        rendered["editor"]["preview"]["render_revision"],
        "canonical Clip gizmo Undo",
    )
    return {
        "selection_frame": selected["frame"],
        "gizmo_rect_points": gizmo["rect_points"],
        "gizmo_metadata": metadata,
        "changed_properties": changed_properties,
        "edited_frame": edited["frame"],
        "restored_frame": restored["frame"],
    }


def prove_ambiguous_owner_has_no_arbitrary_gizmo(client):
    baseline = client.state()
    BASE.activate_dock_tab(
        client, "dock.tab:timeline", "Timeline", "ambiguous Clip facade"
    )
    client.click_component("timeline.clip:" + AMBIGUOUS_CLIP)
    selected = client.wait_until(
        "ambiguous Clip coordinate selection",
        lambda: state
        if BASE.selection_matches((state := client.state()), "clip", AMBIGUOUS_CLIP)
        else None,
    )
    BASE.assert_exact_selection(
        selected, "clip", AMBIGUOUS_CLIP, "ambiguous Clip coordinate selection"
    )
    assert_inspector_owner(
        client, "clip", AMBIGUOUS_CLIP, "ambiguous Clip coordinate selection"
    )
    activate_preview_select(client, "ambiguous Clip facade")
    snapshot, badge = wait_registered_component(client, "preview.facade.ambiguous")
    metadata = badge.get("metadata") or {}
    candidates = set(metadata.get("candidate_node_ids") or [])
    expected_candidates = {AMBIGUOUS_TRANSFORM_A, AMBIGUOUS_TRANSFORM_B}
    if not (
        metadata.get("reason") == "multiple independent spatial transforms"
        and candidates == expected_candidates
    ):
        raise QaFailure(
            "advanced facade exposed wrong ambiguity candidates {}".format(
                sorted(candidates)
            )
        )
    ambiguous_state = client.state()
    if ambiguous_state["editor"]["preview"].get("edit_target") is not None:
        raise QaFailure("ambiguous Clip facade chose an arbitrary Preview edit target")
    if component_by_id(snapshot, "preview.gizmo.bounds") is not None:
        raise QaFailure("ambiguous Clip facade exposed an arbitrary gizmo")
    if (
        ambiguous_state["project"] != baseline["project"]
        or ambiguous_state["history"] != baseline["history"]
    ):
        raise QaFailure("ambiguity display mutated Project or history")
    return {
        "authored_transform_node_ids": sorted(expected_candidates),
        "badge_rect_points": badge["rect_points"],
        "candidate_node_ids": sorted(candidates),
        "ambiguous_frame": ambiguous_state["frame"],
    }


def node_editor_selection_and_repaint(client):
    BASE.activate_dock_tab(
        client, "dock.tab:node_editor", "Node Editor", "direct Node selection"
    )
    header_id = "node_editor.node_header:" + BASE.TEXT
    BASE.reveal_node_editor_component(client, header_id)
    before_selection = client.state()
    client.click_component(header_id)
    selected = client.wait_until(
        "Node Editor direct Node selection",
        lambda: state
        if BASE.selection_matches((state := client.state()), "node", BASE.TEXT)
        else None,
    )
    BASE.assert_exact_selection(selected, "node", BASE.TEXT, "Node Editor selection")
    if selected["editor"]["preview"].get("edit_target") is not None:
        raise QaFailure("Node Editor Node selection retained a Preview edit target")
    assert_inspector_owner(client, "node", BASE.TEXT, "Node Editor selection")
    if (
        selected["project"] != before_selection["project"]
        or selected["history"] != before_selection["history"]
    ):
        raise QaFailure("Node Editor selection mutated Project or history")

    # This context command travels through the queued Node-edit path.  A valid
    # changed Preview revision after the coordinate click proves the UI was
    # woken for the authoritative graph mutation.
    before_edit = client.wait_preview_settled("queued Node edit baseline")
    client.click_component(header_id, button="secondary")
    toggle_id = "node_editor.menu.toggle_enabled.node:" + BASE.TEXT
    client.wait_component(toggle_id)
    client.click_component(toggle_id)
    disabled = client.wait_project(
        "queued Text disable",
        lambda project: project["nodes"][BASE.TEXT]["enabled"] is False,
    )
    BASE.assert_history_delta(before_edit, disabled, 1, "queued Text disable")
    BASE.assert_only_node_enabled_changed(
        before_edit["project"], disabled["project"], BASE.TEXT, False
    )
    BASE.assert_exact_selection(disabled, "node", BASE.TEXT, "queued Text disable")
    rendered = client.wait_preview_change(
        before_edit["editor"]["preview"]["pixel_hash"],
        before_edit["editor"]["preview"]["render_revision"],
    )
    restored = BASE.undo_project_edit(
        client,
        "queued Text disable",
        lambda project: project == before_edit["project"],
    )
    BASE.wait_preview_hash_after(
        client,
        before_edit["editor"]["preview"]["pixel_hash"],
        rendered["editor"]["preview"]["render_revision"],
        "queued Text disable Undo",
    )

    # End on a fresh real coordinate click and assert post-frame state rather
    # than treating successful event injection as successful interaction.
    BASE.reveal_node_editor_component(client, header_id)
    client.click_component(header_id)
    final = client.wait_until(
        "final direct Node coordinate selection",
        lambda: state
        if BASE.selection_matches((state := client.state()), "node", BASE.TEXT)
        else None,
    )
    BASE.assert_exact_selection(final, "node", BASE.TEXT, "final coordinate click")
    return {
        "selection_frame": selected["frame"],
        "disabled_frame": disabled["frame"],
        "render_revision_before": before_edit["editor"]["preview"]["render_revision"],
        "render_revision_after": rendered["editor"]["preview"]["render_revision"],
        "restored_frame": restored["frame"],
        "final_frame": final["frame"],
    }


def wait_transform_preview_fixture(client):
    def ready():
        state = client.state()
        project = state["project"]
        if project.get("name") != "RuViE QA E2E":
            raise QaFailure(
                "the transform_preview_e2e fixture is required; start with "
                "RUVIE_QA_FIXTURE=transform_preview_e2e"
            )
        nodes = set(project.get("nodes", {}))
        if not (BASE.EXPECTED_FIXTURE_NODES | AMBIGUOUS_FIXTURE_NODES) <= nodes:
            return None
        if AMBIGUOUS_CLIP not in project.get("clips", {}):
            return None
        preview = state["editor"]["preview"]
        if preview["modal_error"] is not None:
            raise QaFailure("initial Preview failed: {}".format(preview["modal_error"]))
        return state if preview["render_revision"] > 0 else None

    initial = client.wait_until("the Transform Preview fixture render", ready)
    BASE.validate_canonical_ownership(initial["project"])
    for transform_id in (AMBIGUOUS_TRANSFORM_A, AMBIGUOUS_TRANSFORM_B):
        validate_root_transform_node(initial["project"]["nodes"][transform_id])
    BASE.assert_valid_preview(initial, "Transform Preview fixture")
    return initial


def run_suite(client):
    health = client.wait_health()
    wait_transform_preview_fixture(client)
    # First Node Editor presentation may normalize only persisted UI layout.
    # Establish the exact restoration baseline after that normal view lifecycle
    # so later assertions remain about semantic edits, not one-time layout.
    BASE.activate_dock_tab(
        client, "dock.tab:node_editor", "Node Editor", "fixture layout baseline"
    )
    client.wait_component_settled("node_editor.canvas")
    initial = client.state()
    text_selection = select_text_from_preview(client)
    canonical_gizmo = timeline_select_and_drag_canonical_gizmo(client)
    ambiguity = prove_ambiguous_owner_has_no_arbitrary_gizmo(client)
    node_editor = node_editor_selection_and_repaint(client)
    final = client.state()
    if final["project"] != initial["project"]:
        changed_sections = sorted(
            key
            for key in set(initial["project"]) | set(final["project"])
            if initial["project"].get(key) != final["project"].get(key)
        )
        changed_nodes = sorted(
            node_id
            for node_id in set(initial["project"]["nodes"])
            | set(final["project"]["nodes"])
            if initial["project"]["nodes"].get(node_id)
            != final["project"]["nodes"].get(node_id)
        )
        raise QaFailure(
            "Transform/Preview suite did not restore the exact fixture; "
            "sections={}, nodes={}".format(changed_sections, changed_nodes)
        )
    BASE.assert_valid_preview(final, "Transform/Preview coordinate suite")
    return {
        "ok": True,
        "suite": "transform-preview",
        "health_frame": health["frame"],
        "initial_frame": initial["frame"],
        "preview_text_selection": text_selection,
        "timeline_canonical_gizmo": canonical_gizmo,
        "advanced_ambiguity": ambiguity,
        "node_editor_selection_repaint": node_editor,
        "final_frame": final["frame"],
        "final_selection": final["editor"]["selection"],
        "final_history": final["history"],
        "actions": client.evidence,
    }


def parse_args():
    parser = argparse.ArgumentParser()
    parser.add_argument("--base-url", default=None)
    parser.add_argument(
        "--spawn",
        action="store_true",
        help="launch a fresh headful app with the deterministic fixture",
    )
    parser.add_argument(
        "--evidence",
        default="target/qa-transform-preview-e2e-evidence.json",
        help="JSON evidence output path",
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
            environment["RUVIE_QA_FIXTURE"] = "transform_preview_e2e"
            process = subprocess.Popen(
                ["cargo", "run", "-p", "app", "--locked"],
                cwd=os.path.dirname(SCRIPT_DIR),
                env=environment,
                start_new_session=True,
            )
        client = QaClient(base_url, args.timeout)
        result = run_suite(client)
        result["run_id"] = os.environ.get("RUVIE_QA_RUN_ID")
        result["git_commit"] = repository_git_commit()
        result["component_frame"] = client.component_snapshot()["frame"]
        result["action_count"] = len(result.get("actions", []))
        evidence_path = os.path.abspath(args.evidence)
        os.makedirs(os.path.dirname(evidence_path), exist_ok=True)
        with open(evidence_path, "w", encoding="utf-8") as output:
            json.dump(result, output, ensure_ascii=False, indent=2)
            output.write("\n")
        print("[qa-transform-preview-e2e] PASS; evidence: {}".format(evidence_path))
        return 0
    except (QaFailure, AssertionError, KeyError, IndexError, TypeError) as error:
        print("[qa-transform-preview-e2e] FAIL: {}".format(error), file=sys.stderr)
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
