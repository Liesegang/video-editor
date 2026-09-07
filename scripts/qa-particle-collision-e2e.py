#!/usr/bin/env python3
"""Exercise the production Particle Collision Plane authoring path."""

import os
import pathlib

from qa_automation_support import history_shortcut
from qa_node_module_support import (
    create_node_from_menu,
    enter_exact_numeric,
    open_timeline_item_definition,
    place_node_for_inline_edit,
    reload_node_clip_project,
    sample_rendered_preview,
)
from qa_particle_force_support import (
    PARTICLE_PUBLISHED_PARAMETERS,
    constant_property,
    create_particle_node_clip,
    native_catalog_id,
    parameter,
    particle_factory_terminal_route,
    reveal_inspector_control,
)
from qa_support import (
    QaFailure,
    activate_dock_tab,
    capture_viewport,
    media_seconds,
    run_suite_main,
    save_project_to_disk,
)


COLLISION_CATALOG_ID = "native.particle.collision-plane"
COLLISION_PARAMETERS = {
    "Collision Enabled": ("active", False),
    "Plane Point": ("plane_point", {"x": 0.0, "y": 120.0, "z": 0.0}),
    "Plane Normal": ("plane_normal", {"x": 0.0, "y": -1.0, "z": 0.0}),
    "Radius": ("radius", 0.0),
    "Bounce": ("bounce", 0.5),
    "Friction": ("friction", 0.1),
}


def _definition(state, definition_id):
    return state["project"]["module_definitions"][definition_id]


def _routes(definition):
    return {
        (
            route["from"]["node_id"],
            route["from"]["port"],
            route["to"]["node_id"],
            route["to"]["port"],
        )
        for route in definition["graph"]["connections"]
    }


def _assert_node_controls_uncovered(client, node_id, control_ids):
    def uncovered():
        snapshot = client.component_snapshot()
        by_id = {candidate.get("id"): candidate for candidate in snapshot["components"]}
        controls = []
        for control_id in control_ids:
            control = by_id.get(control_id)
            rect = (control or {}).get("rect_points") or {}
            if (
                control is None
                or control.get("visible") is not True
                or float(rect.get("width", 0.0)) <= 0.0
                or float(rect.get("height", 0.0)) <= 0.0
            ):
                return None
            controls.append(control)
        for control in controls:
            point = {
                "x": float(control["rect_points"]["center_x"]),
                "y": float(control["rect_points"]["center_y"]),
            }
            for candidate in snapshot["components"]:
                candidate_id = candidate.get("id", "")
                rect = candidate.get("rect_points") or {}
                if (
                    candidate_id.startswith("node_editor.node:")
                    and candidate_id != "node_editor.node:" + node_id
                    and candidate.get("visible") is True
                    and float(rect.get("min_x", point["x"] + 1.0)) <= point["x"]
                    <= float(rect.get("max_x", point["x"] - 1.0))
                    and float(rect.get("min_y", point["y"] + 1.0)) <= point["y"]
                    <= float(rect.get("max_y", point["y"] - 1.0))
                ):
                    raise QaFailure(
                        "factory Collision control is covered by " + candidate_id
                    )
        return controls

    return client.wait_until("uncovered factory Collision controls", uncovered)


def _assert_collision_definition(definition, collision_id, expected_bypassed=False):
    nodes = definition["graph"]["nodes"]
    collision = nodes.get(collision_id)
    if collision is None or native_catalog_id(collision) != COLLISION_CATALOG_ID:
        raise QaFailure("Particle factory lost its Collision Plane Node")
    if collision.get("enabled") is not True:
        raise QaFailure("factory Collision Plane changed Node enabled semantics")
    if collision.get("bypassed") is not expected_bypassed:
        raise QaFailure("Collision Plane bypass state diverged from the authored state")
    for _, (key, expected) in COLLISION_PARAMETERS.items():
        if constant_property(collision, key) != expected:
            raise QaFailure("Collision Plane changed its {} default".format(key))
    _, _, terminal = particle_factory_terminal_route(definition)
    if len(nodes) != 9 or len(definition["graph"]["connections"]) != 8:
        raise QaFailure("Particle factory Collision topology is not authoritative")
    if (
        terminal["from"] != {"node_id": collision_id, "port": "particles"}
        or terminal["to"]["port"] != "particles"
    ):
        raise QaFailure("Collision Plane is not the final Particle stage")


def _assert_published_collision(parameters, collision_id):
    if [entry.get("name") for entry in parameters] != PARTICLE_PUBLISHED_PARAMETERS:
        raise QaFailure("Particle Collision parameters changed publication order")
    result = {}
    for name, (key, expected) in COLLISION_PARAMETERS.items():
        published = parameter(parameters, name)
        target = published.get("target") or {}
        if target != {"node_id": collision_id, "port": key}:
            raise QaFailure(name + " targets a different Particle Node input")
        if published.get("default_value") != expected:
            raise QaFailure(name + " lost its exact factory default")
        result[name] = published
    return result


def _set_collision_active(client, instance_id, parameter_id):
    activate_dock_tab(client, "dock.tab:timeline", "Timeline", "Particle Collision")
    activate_dock_tab(client, "dock.tab:inspector", "Inspector", "Particle Collision")
    control_id = "inspector.property:module_instance:{}:{}".format(
        instance_id, parameter_id
    )
    control = reveal_inspector_control(client, control_id)
    metadata = control.get("metadata") or {}
    if metadata.get("editor_kind") != "boolean" or metadata.get("value") is not False:
        raise QaFailure("Collision Enabled bypassed the shared Boolean editor")
    before = client.state()
    client.click_component(control_id)

    def active():
        state = client.state()
        overrides = state["project"]["module_instances"][instance_id].get(
            "parameter_overrides", {}
        )
        return (
            state
            if state["history"]["revision"] == before["history"]["revision"] + 1
            and overrides.get(parameter_id) is True
            else None
        )

    return control_id, control, before, client.wait_until(
        "Collision Enabled instance override", active
    )


def _configure_inactive_collision(
    client, instance_id, definition_id, published, definition_baseline
):
    activate_dock_tab(client, "dock.tab:inspector", "Inspector", "Collision settings")
    edits = [
        ("Plane Point", "y", 120.0, 20.0, {"x": 0.0, "y": 20.0, "z": 0.0}),
        ("Radius", None, 0.0, 2.0, 2.0),
        ("Bounce", None, 0.5, 0.7, 0.7),
        ("Friction", None, 0.1, 0.3, 0.3),
    ]
    evidence = []
    expected_overrides = {}
    for name, axis, initial, value, expected in edits:
        parameter_id = published[name]["id"]
        base_id = "inspector.property:module_instance:{}:{}".format(
            instance_id, parameter_id
        )
        control_id = base_id + ":" + axis if axis else base_id
        control = reveal_inspector_control(client, control_id)
        metadata = control.get("metadata") or {}
        if axis:
            if metadata.get("axis") != axis.upper() or metadata.get("value") != initial:
                raise QaFailure(name + " bypassed the shared vector component editor")
        elif metadata.get("editor_kind") != "float" or metadata.get("value") != initial:
            raise QaFailure(name + " bypassed the shared Number editor")
        before = client.state()
        enter_exact_numeric(client, control_id, value)

        def edited():
            state = client.state()
            overrides = state["project"]["module_instances"][instance_id].get(
                "parameter_overrides", {}
            )
            return (
                state
                if state["history"]["revision"] == before["history"]["revision"] + 1
                and overrides.get(parameter_id) == expected
                and _definition(state, definition_id) == definition_baseline
                else None
            )

        state = client.wait_until("exact inactive " + name + " override", edited)
        expected_overrides[parameter_id] = expected
        actual_overrides = state["project"]["module_instances"][instance_id].get(
            "parameter_overrides", {}
        )
        if actual_overrides != expected_overrides:
            raise QaFailure(name + " changed another Particle instance parameter")
        evidence.append(
            {
                "name": name,
                "parameter_id": parameter_id,
                "control_id": control_id,
                "value": expected,
                "revision": state["history"]["revision"],
            }
        )
    return evidence, state


def _wait_exact_project(client, revision, project, description):
    return client.wait_until(
        description,
        lambda: state
        if (state := client.state())["history"]["revision"] == revision
        and state["project"] == project
        else None,
    )


def run_suite(client):
    client.wait_health()
    created = create_particle_node_clip(client, 6.5)
    item_id = created["item_id"]
    instance_id = created["instance_id"]
    definition_id = created["definition_id"]
    parameters = created["parameters"]
    start = media_seconds(created["item"]["interval"]["start"])
    collision_id, sprite_id, terminal = particle_factory_terminal_route(
        created["definition"]
    )
    _assert_collision_definition(created["definition"], collision_id)
    published = _assert_published_collision(parameters, collision_id)
    expected_routes = _routes(created["definition"])

    sample_time = start + 2.5
    inactive = sample_rendered_preview(
        client,
        sample_time,
        created["state"]["history"]["revision"],
        "inactive factory Collision",
    )
    activate_dock_tab(client, "dock.tab:timeline", "Timeline", "Collision selection")
    client.click_component("timeline.item:" + item_id)
    settings, configured_state = _configure_inactive_collision(
        client, instance_id, definition_id, published, created["definition"]
    )
    configured_inactive = sample_rendered_preview(
        client,
        sample_time,
        configured_state["history"]["revision"],
        "configured inactive Collision",
    )
    if configured_inactive["pixel_hash"] != inactive["pixel_hash"]:
        raise QaFailure("inactive Collision settings changed pass-through Particle pixels")
    activate_dock_tab(client, "dock.tab:timeline", "Timeline", "Collision selection")
    client.click_component("timeline.item:" + item_id)
    active_id = published["Collision Enabled"]["id"]
    control_id, inspector_control, inactive_state, active_state = _set_collision_active(
        client, instance_id, active_id
    )
    active = sample_rendered_preview(
        client, sample_time, active_state["history"]["revision"], "active Collision"
    )
    if active["frame"] != inactive["frame"] or active["pixel_hash"] == inactive["pixel_hash"]:
        raise QaFailure("enabling Collision did not change the same-frame Particle pixels")

    history_shortcut(client)
    undone = _wait_exact_project(
        client,
        active_state["history"]["revision"] + 1,
        inactive_state["project"],
        "Undo exact Collision Enabled override",
    )
    undone_pixels = sample_rendered_preview(
        client, sample_time, undone["history"]["revision"], "undone Collision"
    )
    if undone_pixels["pixel_hash"] != inactive["pixel_hash"]:
        raise QaFailure("Undo did not restore inactive Collision pixels")
    history_shortcut(client, redo=True)
    redone = _wait_exact_project(
        client,
        undone["history"]["revision"] + 1,
        active_state["project"],
        "Redo exact Collision Enabled override",
    )
    redone_pixels = sample_rendered_preview(
        client, sample_time, redone["history"]["revision"], "redone Collision"
    )
    if redone_pixels["pixel_hash"] != active["pixel_hash"]:
        raise QaFailure("Redo did not restore active Collision pixels")

    opened_id, definition = open_timeline_item_definition(
        client, item_id, "node_clip", "Particle Collision Node"
    )
    if opened_id != definition_id:
        raise QaFailure("Node Editor opened a different Particle Definition")
    _assert_collision_definition(definition, collision_id)
    node_control_id = "node_editor.property.node:{}:active".format(collision_id)
    _assert_node_controls_uncovered(
        client,
        collision_id,
        ["node_editor.node_state:" + collision_id, node_control_id],
    )
    node_control = place_node_for_inline_edit(client, collision_id, node_control_id)
    node_metadata = node_control.get("metadata") or {}
    if (
        node_metadata.get("value") is not True
        or node_metadata.get("property_scope") != "module_instance"
        or node_metadata.get("parameter_id") != active_id
        or node_metadata.get("instance_id") != instance_id
        or node_metadata.get("target") != {"kind": "module_parameter", "id": active_id}
    ):
        raise QaFailure("Collision Node diverged from its published instance value")
    plane_y_id = "node_editor.property.node:{}:plane_point:y".format(collision_id)
    plane_y = place_node_for_inline_edit(client, collision_id, plane_y_id)
    plane_y_metadata = plane_y.get("metadata") or {}
    if plane_y_metadata.get("axis") != "Y" or plane_y_metadata.get("value") != 20.0:
        raise QaFailure("Collision Node lost the effective Plane Point instance value")
    bypass_before = client.state()
    _, bypass_control = client.wait_component_settled(
        "node_editor.node_state:" + collision_id
    )
    bypass_metadata = bypass_control.get("metadata") or {}
    if (
        bypass_metadata.get("node_id") != collision_id
        or bypass_metadata.get("supports_bypass") is not True
        or bypass_metadata.get("bypassed") is not False
    ):
        raise QaFailure("Collision Plane lost its production bypass control")
    client.click_component("node_editor.node_state:" + collision_id)

    def bypassed():
        state = client.state()
        node = _definition(state, definition_id)["graph"]["nodes"][collision_id]
        return (
            state
            if state["history"]["revision"] == bypass_before["history"]["revision"] + 1
            and node.get("enabled") is True
            and node.get("bypassed") is True
            else None
        )

    bypassed_state = client.wait_until("Collision Plane header bypass", bypassed)
    bypassed_definition = _definition(bypassed_state, definition_id)
    _assert_collision_definition(bypassed_definition, collision_id, True)
    if _routes(bypassed_definition) != expected_routes:
        raise QaFailure("Collision bypass changed Particle routes")
    bypassed_pixels = sample_rendered_preview(
        client,
        sample_time,
        bypassed_state["history"]["revision"],
        "bypassed Collision",
    )
    if bypassed_pixels["pixel_hash"] != inactive["pixel_hash"]:
        raise QaFailure("Collision bypass did not restore pass-through Particle pixels")
    history_shortcut(client)
    resumed = _wait_exact_project(
        client,
        bypassed_state["history"]["revision"] + 1,
        bypass_before["project"],
        "Undo exact Collision bypass",
    )
    resumed_pixels = sample_rendered_preview(
        client, sample_time, resumed["history"]["revision"], "resumed Collision"
    )
    if resumed_pixels["pixel_hash"] != active["pixel_hash"]:
        raise QaFailure("Undo bypass did not restore active Collision pixels")

    open_timeline_item_definition(client, item_id, "node_clip", "Collision catalog")
    before_add = client.state()
    added_definition, added_id = create_node_from_menu(
        client,
        "node_clip",
        "Collision Plane",
        "node_editor.menu.create.particle_collision_plane",
    )
    added = added_definition["graph"]["nodes"][added_id]
    if (
        native_catalog_id(added) != COLLISION_CATALOG_ID
        or constant_property(added, "active") is not True
    ):
        raise QaFailure("new Collision Plane did not use its active catalog default")
    if len(added_definition["graph"]["nodes"]) != 10:
        raise QaFailure("Collision catalog action changed more than one Node")
    added_state = client.state()
    history_shortcut(client)
    removed = _wait_exact_project(
        client,
        added_state["history"]["revision"] + 1,
        before_add["project"],
        "Undo new Collision Plane",
    )
    _assert_collision_definition(_definition(removed, definition_id), collision_id)

    samples = [
        sample_rendered_preview(
            client, start + offset, removed["history"]["revision"], "saved Collision"
        )
        for offset in (2.5, 3.0)
    ]
    open_timeline_item_definition(client, item_id, "node_clip", "Collision capture")
    artifact_dir = pathlib.Path(os.environ["RUVIE_QA_ARTIFACT_DIR"])
    capture = capture_viewport(client, artifact_dir / "capture.png")
    project_path = os.environ.get("RUVIE_QA_PROJECT_PATH")
    if not project_path:
        raise QaFailure("Particle Collision persistence QA requires a Project file")
    project_file = pathlib.Path(project_path)
    saved, saved_file = save_project_to_disk(
        client, project_file, "Particle Collision"
    )
    reloaded = reload_node_clip_project(
        client,
        project_file,
        saved,
        item_id,
        definition_id,
        samples,
        artifact_dir,
        "Particle Collision",
        "particle-collision",
        "particle-collision-reloaded.png",
        lambda candidate: _assert_collision_definition(candidate, collision_id),
    )
    return {
        "suite": "particle-collision",
        "item_id": item_id,
        "instance_id": instance_id,
        "definition_id": definition_id,
        "collision_id": collision_id,
        "sprite_id": sprite_id,
        "terminal_route": terminal,
        "published": published,
        "settings": settings,
        "inspector_control": {"id": control_id, "component": inspector_control},
        "node_controls": {"active": node_control, "plane_point_y": plane_y},
        "inactive": inactive,
        "configured_inactive": configured_inactive,
        "active": active,
        "bypassed": bypassed_pixels,
        "samples": samples,
        "capture": capture,
        "saved": saved_file,
        "reloaded": reloaded,
        "actions": client.evidence,
    }


if __name__ == "__main__":
    raise SystemExit(
        run_suite_main(
            "qa-particle-collision-e2e",
            run_suite,
            "target/qa-particle-collision-e2e-evidence.json",
        )
    )
