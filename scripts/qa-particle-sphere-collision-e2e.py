#!/usr/bin/env python3
"""Exercise Particle Collision Sphere through the production Node Editor."""

import os
import pathlib

from qa_automation_support import history_shortcut
from qa_node_module_support import (
    connect_nodes,
    create_node_from_menu,
    disconnect_node_connection,
    enter_exact_numeric,
    ensure_node_editor_authoring_scale,
    open_timeline_item_definition,
    place_created_node,
    place_node_for_inline_edit,
    publish_node_input_parameter,
    reload_node_clip_project,
    sample_rendered_preview,
)
from qa_particle_force_support import (
    constant_property,
    create_particle_node_clip,
    native_catalog_id,
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


SPHERE_CATALOG_ID = "native.particle.collision-sphere"
SPHERE_PROPERTIES = {
    "active": True,
    "center": {"x": 0.0, "y": 120.0, "z": 0.0},
    "radius": 100.0,
    "particle_radius": 0.0,
    "mode": "Solid",
    "bounce": 0.5,
    "friction": 0.1,
}
NODE_EDITS = {
    "center": {"x": 0.0, "y": 20.0, "z": 0.0},
    "radius": 80.0,
    "friction": 0.3,
}
INSTANCE_EDITS = {
    "center": {"x": 0.0, "y": 10.0, "z": 0.0},
    "radius": 60.0,
    "friction": 0.6,
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


def _assert_sphere_definition(
    definition, sphere_id, expected_routes, expected_properties, bypassed=False
):
    sphere = definition["graph"]["nodes"].get(sphere_id)
    if sphere is None or native_catalog_id(sphere) != SPHERE_CATALOG_ID:
        raise QaFailure("Collision Sphere lost its native catalog identity")
    if sphere.get("enabled") is not True or sphere.get("bypassed") is not bypassed:
        raise QaFailure("Collision Sphere changed its enabled/bypass semantics")
    for key, expected in expected_properties.items():
        if constant_property(sphere, key) != expected:
            raise QaFailure("Collision Sphere changed its {} value".format(key))
    if (
        len(definition["graph"]["nodes"]) != 10
        or len(definition["graph"]["connections"]) != len(expected_routes)
        or _routes(definition) != expected_routes
    ):
        raise QaFailure("Collision Sphere changed the exact Particle routes")


def _wait_project(client, revision, project, description):
    return client.wait_until(
        description,
        lambda: state
        if (state := client.state())["history"]["revision"] == revision
        and state["project"] == project
        else None,
    )


def _edit_node_number(client, definition_id, sphere_id, key, axis, value, expected):
    base = "node_editor.property.node:{}:{}".format(sphere_id, key)
    control_id = base + ":" + axis if axis else base
    control = place_node_for_inline_edit(client, sphere_id, control_id)
    metadata = control.get("metadata") or {}
    if axis and metadata.get("axis") != axis.upper():
        raise QaFailure("Collision Sphere vector axis lost its QA identity")
    if not axis and metadata.get("editor_kind") != "float":
        raise QaFailure("Collision Sphere Number did not use the shared editor")
    before = client.state()
    enter_exact_numeric(client, control_id, value)

    def edited():
        state = client.state()
        node = _definition(state, definition_id)["graph"]["nodes"][sphere_id]
        return (
            state
            if state["history"]["revision"] == before["history"]["revision"] + 1
            and constant_property(node, key) == expected
            else None
        )

    state = client.wait_until("exact Collision Sphere {} edit".format(key), edited)
    return control_id, control, state


def _set_node_mode(client, definition_id, sphere_id, value):
    activate_dock_tab(client, "dock.tab:node_editor", "Node Editor", "Sphere mode")
    control_id = "node_editor.property.node:{}:mode".format(sphere_id)
    control = place_node_for_inline_edit(client, sphere_id, control_id)
    metadata = control.get("metadata") or {}
    if metadata.get("editor_kind") != "dropdown" or metadata.get("value") not in (
        "Solid",
        "Container",
    ):
        raise QaFailure("Collision Sphere mode bypassed the shared enum editor")
    before = client.state()
    client.click_component(control_id)
    option_id = control_id + ".option:" + value
    _, option = client.wait_component_settled(option_id)
    if (option.get("metadata") or {}).get("value") != value:
        raise QaFailure("Collision Sphere mode option lost its exact value")
    client.click_component(option_id)

    def changed():
        state = client.state()
        node = _definition(state, definition_id)["graph"]["nodes"][sphere_id]
        return (
            state
            if state["history"]["revision"] == before["history"]["revision"] + 1
            and constant_property(node, "mode") == value
            else None
        )

    return control, client.wait_until("Collision Sphere " + value + " mode", changed)


def _publish_edited_inputs(client, sphere_id):
    activate_dock_tab(
        client, "dock.tab:node_editor", "Node Editor", "Sphere publication"
    )
    published = {}
    expected = {
        "center": ("Center", NODE_EDITS["center"]),
        "radius": ("Radius", NODE_EDITS["radius"]),
        "friction": ("Friction", NODE_EDITS["friction"]),
    }
    for key, (name, default) in expected.items():
        control_id = "node_editor.property.node:{}:{}".format(sphere_id, key)
        place_node_for_inline_edit(client, sphere_id, control_id)
        state, parameter = publish_node_input_parameter(
            client,
            "node_clip",
            sphere_id,
            key,
            control_id,
            name,
            default,
        )
        published[key] = parameter
    return state, published


def _edit_instance_inputs(
    client, item_id, instance_id, definition_id, definition_baseline, published
):
    activate_dock_tab(client, "dock.tab:timeline", "Timeline", "Sphere instance")
    client.click_component("timeline.item:" + item_id)
    activate_dock_tab(client, "dock.tab:inspector", "Inspector", "Sphere instance")
    expected_overrides = {}
    evidence = []
    for key, expected in INSTANCE_EDITS.items():
        parameter_id = published[key]["id"]
        base = "inspector.property:module_instance:{}:{}".format(
            instance_id, parameter_id
        )
        control_id = base + ":y" if key == "center" else base
        control = reveal_inspector_control(client, control_id)
        metadata = control.get("metadata") or {}
        if key == "center" and metadata.get("axis") != "Y":
            raise QaFailure("published Sphere Center lost its shared vector editor")
        if key != "center" and metadata.get("editor_kind") != "float":
            raise QaFailure("published Sphere Number lost its shared editor")
        before = client.state()
        value = expected["y"] if key == "center" else expected
        enter_exact_numeric(client, control_id, value)

        def overridden():
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

        state = client.wait_until("Sphere {} instance override".format(key), overridden)
        expected_overrides[parameter_id] = expected
        overrides = state["project"]["module_instances"][instance_id].get(
            "parameter_overrides", {}
        )
        if overrides != expected_overrides:
            raise QaFailure("Sphere instance edit changed another parameter override")
        evidence.append({"key": key, "control": control, "value": expected})
    return state, evidence


def _assert_effective_node_values(client, sphere_id, instance_id, published):
    evidence = []
    for key, expected in INSTANCE_EDITS.items():
        control_id = "node_editor.property.node:{}:{}".format(sphere_id, key)
        control = place_node_for_inline_edit(client, sphere_id, control_id)
        metadata = control.get("metadata") or {}
        if (
            metadata.get("value") != expected
            or metadata.get("property_scope") != "module_instance"
            or metadata.get("parameter_id") != published[key]["id"]
            or metadata.get("instance_id") != instance_id
        ):
            raise QaFailure("Sphere Node lost its effective published {} value".format(key))
        evidence.append(control)
    return evidence


def run_suite(client):
    client.wait_health()
    created = create_particle_node_clip(client, 6.5)
    item_id = created["item_id"]
    instance_id = created["instance_id"]
    definition_id = created["definition_id"]
    start = media_seconds(created["item"]["interval"]["start"])
    plane_id, sprite_id, terminal = particle_factory_terminal_route(
        created["definition"]
    )
    factory_routes = _routes(created["definition"])
    sample_time = start + 2.5
    baseline = sample_rendered_preview(
        client,
        sample_time,
        created["state"]["history"]["revision"],
        "factory Particle",
    )

    opened_id, _ = open_timeline_item_definition(
        client, item_id, "node_clip", "Sphere Collision"
    )
    if opened_id != definition_id:
        raise QaFailure("Node Editor opened a different Particle Definition")
    disconnect_node_connection(
        client, "node_clip", terminal["id"], "Collision Plane to Sprite"
    )
    definition, sphere_id = create_node_from_menu(
        client,
        "node_clip",
        "Collision Sphere",
        "node_editor.menu.create.particle_collision_sphere",
    )
    sphere = definition["graph"]["nodes"][sphere_id]
    if native_catalog_id(sphere) != SPHERE_CATALOG_ID:
        raise QaFailure("Collision Sphere menu created a different catalog Node")
    for key, expected in SPHERE_PROPERTIES.items():
        if constant_property(sphere, key) != expected:
            raise QaFailure("Collision Sphere changed its {} catalog default".format(key))
    connect_nodes(
        client, "node_clip", plane_id, "particles", sphere_id, "particles", "Plane to Sphere"
    )
    connect_nodes(
        client, "node_clip", sphere_id, "particles", sprite_id, "particles", "Sphere to Sprite"
    )
    ensure_node_editor_authoring_scale(client)
    place_created_node(client, sphere_id, 0.50)
    expected_routes = (factory_routes - {
        (plane_id, "particles", sprite_id, "particles")
    }) | {
        (plane_id, "particles", sphere_id, "particles"),
        (sphere_id, "particles", sprite_id, "particles"),
    }
    current_properties = dict(SPHERE_PROPERTIES)
    _assert_sphere_definition(
        _definition(client.state(), definition_id),
        sphere_id,
        expected_routes,
        current_properties,
    )

    controls = []
    for key, axis, value, expected in (
        ("center", "y", 20.0, NODE_EDITS["center"]),
        ("radius", None, 80.0, NODE_EDITS["radius"]),
        ("friction", None, 0.3, NODE_EDITS["friction"]),
    ):
        control_id, control, state = _edit_node_number(
            client, definition_id, sphere_id, key, axis, value, expected
        )
        controls.append({"id": control_id, "component": control})
        current_properties[key] = expected
    solid = sample_rendered_preview(
        client, sample_time, state["history"]["revision"], "Solid Sphere"
    )
    if solid["frame"] != baseline["frame"] or solid["pixel_hash"] == baseline["pixel_hash"]:
        raise QaFailure("Solid Sphere did not change same-frame Particle pixels")

    mode_control, container_state = _set_node_mode(
        client, definition_id, sphere_id, "Container"
    )
    container = sample_rendered_preview(
        client,
        sample_time,
        container_state["history"]["revision"],
        "Container Sphere",
    )
    if container["frame"] != solid["frame"] or container["pixel_hash"] == solid["pixel_hash"]:
        raise QaFailure("Container mode did not change same-frame Sphere pixels")
    _, solid_state = _set_node_mode(client, definition_id, sphere_id, "Solid")
    solid_again = sample_rendered_preview(
        client, sample_time, solid_state["history"]["revision"], "restored Solid Sphere"
    )
    if solid_again["pixel_hash"] != solid["pixel_hash"]:
        raise QaFailure("restoring Solid mode did not restore deterministic pixels")

    published_state, published = _publish_edited_inputs(client, sphere_id)
    published_definition = _definition(published_state, definition_id)
    _assert_sphere_definition(
        published_definition, sphere_id, expected_routes, current_properties
    )
    instance_state, inspector = _edit_instance_inputs(
        client,
        item_id,
        instance_id,
        definition_id,
        published_definition,
        published,
    )
    overridden = sample_rendered_preview(
        client,
        sample_time,
        instance_state["history"]["revision"],
        "Sphere instance overrides",
    )
    if overridden["pixel_hash"] == baseline["pixel_hash"]:
        raise QaFailure("Sphere instance overrides did not affect Particle pixels")

    open_timeline_item_definition(client, item_id, "node_clip", "Sphere Node values")
    effective_controls = _assert_effective_node_values(
        client, sphere_id, instance_id, published
    )
    bypass_before = client.state()
    bypass_id = "node_editor.node_state:" + sphere_id
    _, bypass_control = client.wait_component_settled(bypass_id)
    metadata = bypass_control.get("metadata") or {}
    if metadata.get("supports_bypass") is not True or metadata.get("bypassed") is not False:
        raise QaFailure("Collision Sphere lost its production bypass control")
    client.click_component(bypass_id)

    def bypassed():
        state = client.state()
        node = _definition(state, definition_id)["graph"]["nodes"][sphere_id]
        return (
            state
            if state["history"]["revision"] == bypass_before["history"]["revision"] + 1
            and node.get("bypassed") is True
            else None
        )

    bypassed_state = client.wait_until("Collision Sphere bypass", bypassed)
    bypassed_pixels = sample_rendered_preview(
        client,
        sample_time,
        bypassed_state["history"]["revision"],
        "bypassed Sphere",
    )
    if bypassed_pixels["pixel_hash"] != baseline["pixel_hash"]:
        raise QaFailure("Sphere bypass did not restore the factory Particle pixels")
    history_shortcut(client)
    resumed = _wait_project(
        client,
        bypassed_state["history"]["revision"] + 1,
        bypass_before["project"],
        "Undo exact Sphere bypass",
    )
    resumed_pixels = sample_rendered_preview(
        client, sample_time, resumed["history"]["revision"], "Undo Sphere bypass"
    )
    if resumed_pixels["pixel_hash"] != overridden["pixel_hash"]:
        raise QaFailure("Undo Sphere bypass did not restore active pixels")
    history_shortcut(client, redo=True)
    redone = _wait_project(
        client,
        resumed["history"]["revision"] + 1,
        bypassed_state["project"],
        "Redo exact Sphere bypass",
    )
    history_shortcut(client)
    final_state = _wait_project(
        client,
        redone["history"]["revision"] + 1,
        bypass_before["project"],
        "restore active Sphere after Redo proof",
    )

    samples = [
        sample_rendered_preview(
            client, start + offset, final_state["history"]["revision"], "saved Sphere"
        )
        for offset in (2.5, 3.0)
    ]
    open_timeline_item_definition(client, item_id, "node_clip", "Sphere capture")
    artifact_dir = pathlib.Path(os.environ["RUVIE_QA_ARTIFACT_DIR"])
    capture = capture_viewport(client, artifact_dir / "capture.png")
    project_path = os.environ.get("RUVIE_QA_PROJECT_PATH")
    if not project_path:
        raise QaFailure("Sphere Collision persistence QA requires a Project file")
    project_file = pathlib.Path(project_path)
    saved, saved_file = save_project_to_disk(client, project_file, "Sphere Collision")
    reloaded = reload_node_clip_project(
        client,
        project_file,
        saved,
        item_id,
        definition_id,
        samples,
        artifact_dir,
        "Sphere Collision",
        "particle-sphere-collision",
        "sphere-collision-reloaded.png",
        lambda candidate: _assert_sphere_definition(
            candidate, sphere_id, expected_routes, current_properties
        ),
    )
    return {
        "suite": "particle-sphere-collision",
        "item_id": item_id,
        "instance_id": instance_id,
        "definition_id": definition_id,
        "sphere_id": sphere_id,
        "plane_id": plane_id,
        "sprite_id": sprite_id,
        "controls": controls,
        "mode_control": mode_control,
        "published": published,
        "inspector": inspector,
        "effective_controls": effective_controls,
        "baseline": baseline,
        "solid": solid,
        "container": container,
        "overridden": overridden,
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
            "qa-particle-sphere-collision-e2e",
            run_suite,
            "target/qa-particle-sphere-collision-e2e-evidence.json",
        )
    )
