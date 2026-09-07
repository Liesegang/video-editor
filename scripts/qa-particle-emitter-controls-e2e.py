#!/usr/bin/env python3
"""Reproduce published Particle Emitter Shape authoring through real UI controls."""

import os
import pathlib

from qa_node_module_support import (
    open_timeline_item_definition,
    place_node_for_inline_edit,
    sample_rendered_preview,
)
from qa_particle_force_support import (
    create_particle_node_clip,
    parameter,
    reveal_inspector_control,
)
from qa_support import (
    QaFailure,
    activate_dock_tab,
    capture_viewport,
    media_seconds,
    run_suite_main,
)


def _select_shape(client, control_id, instance_id, parameter_id, value):
    before = client.state()
    option_id = control_id + ".option:" + value
    _, control = client.wait_component_settled(control_id)
    rect = control["rect_points"]
    artifact_dir = pathlib.Path(os.environ["RUVIE_QA_ARTIFACT_DIR"])
    capture_viewport(client, artifact_dir / ("shape-{}-before.png".format(value.lower())))
    option = None
    clicked_region = None
    regions = (
        ("right", float(rect["max_x"]) - 8.0),
        ("left", float(rect["min_x"]) + 8.0),
        ("center", float(rect["center_x"])),
    )
    for region, x in regions:
        client.inject(
            "click",
            {
                "x": x,
                "y": float(rect["center_y"]),
                "button": "primary",
                "coordinate_space": "points",
            },
        )

        def opened():
            snapshot = client.component_snapshot()
            return next(
                (
                    candidate
                    for candidate in snapshot["components"]
                    if candidate.get("id") == option_id
                    and candidate.get("visible") is True
                ),
                None,
            )

        try:
            option = client.wait_until(
                "Emitter Shape {} option after {} click".format(value, region),
                opened,
                2.0,
            )
            clicked_region = region
            break
        except QaFailure:
            capture_viewport(
                client,
                artifact_dir
                / ("shape-{}-after-{}.png".format(value.lower(), region)),
            )
    if option is None:
        raise QaFailure("Emitter Shape dropdown did not open from left/center/right")
    capture_viewport(client, artifact_dir / ("shape-{}-open.png".format(value.lower())))
    metadata = option.get("metadata") or {}
    if metadata.get("value") != value or option.get("enabled") is not True:
        raise QaFailure("Emitter Shape option lost its exact value")
    client.click_component(option_id)

    def committed():
        state = client.state()
        overrides = state["project"]["module_instances"][instance_id].get(
            "parameter_overrides", {}
        )
        return (
            state
            if state["history"]["revision"] == before["history"]["revision"] + 1
            and overrides.get(parameter_id) == value
            and state["editor"].get("error") is None
            else None
        )

    return (
        {"component": option, "clicked_region": clicked_region},
        client.wait_until("Emitter Shape " + value + " commit", committed),
    )


def run_suite(client):
    client.wait_health()
    created = create_particle_node_clip(client, 6.5)
    item_id = created["item_id"]
    instance_id = created["instance_id"]
    definition_id = created["definition_id"]
    start = media_seconds(created["item"]["interval"]["start"])
    shape = parameter(created["parameters"], "Emitter Shape")
    surface = parameter(created["parameters"], "Emitter Surface Only")
    if (
        shape.get("target", {}).get("port") != "shape"
        or surface.get("target", {}).get("port") != "surface_only"
        or shape["target"]["node_id"] != surface["target"]["node_id"]
    ):
        raise QaFailure("Emitter Shape controls do not target one production Shape Node")
    shape_node_id = shape["target"]["node_id"]
    sample_time = start + 2.5
    point = sample_rendered_preview(
        client,
        sample_time,
        created["state"]["history"]["revision"],
        "Point emitter",
    )

    activate_dock_tab(client, "dock.tab:timeline", "Timeline", "Emitter selection")
    client.click_component("timeline.item:" + item_id)
    activate_dock_tab(client, "dock.tab:inspector", "Inspector", "Emitter Shape")
    shape_control_id = "inspector.property:module_instance:{}:{}".format(
        instance_id, shape["id"]
    )
    shape_control = reveal_inspector_control(client, shape_control_id)
    shape_metadata = shape_control.get("metadata") or {}
    if (
        shape_metadata.get("editor_kind") != "dropdown"
        or shape_metadata.get("value") != "Point"
    ):
        raise QaFailure("Emitter Shape did not expose its shared dropdown control")

    box_option, box_state = _select_shape(
        client, shape_control_id, instance_id, shape["id"], "Box"
    )
    box = sample_rendered_preview(
        client, sample_time, box_state["history"]["revision"], "Box emitter"
    )
    if box["frame"] != point["frame"] or box["pixel_hash"] == point["pixel_hash"]:
        raise QaFailure("Box did not change the same-frame Emitter pixels")

    activate_dock_tab(client, "dock.tab:timeline", "Timeline", "Emitter selection")
    client.click_component("timeline.item:" + item_id)
    activate_dock_tab(client, "dock.tab:inspector", "Inspector", "Emitter Shape")
    reveal_inspector_control(client, shape_control_id)
    sphere_option, sphere_state = _select_shape(
        client, shape_control_id, instance_id, shape["id"], "Sphere"
    )
    sphere = sample_rendered_preview(
        client,
        sample_time,
        sphere_state["history"]["revision"],
        "Sphere emitter",
    )
    if sphere["frame"] != box["frame"] or sphere["pixel_hash"] == box["pixel_hash"]:
        raise QaFailure("Sphere did not change the same-frame Emitter pixels")

    activate_dock_tab(client, "dock.tab:timeline", "Timeline", "Emitter selection")
    client.click_component("timeline.item:" + item_id)
    activate_dock_tab(client, "dock.tab:inspector", "Inspector", "Emitter Surface")
    surface_control_id = "inspector.property:module_instance:{}:{}".format(
        instance_id, surface["id"]
    )
    surface_control = reveal_inspector_control(client, surface_control_id)
    surface_metadata = surface_control.get("metadata") or {}
    if (
        surface_metadata.get("editor_kind") != "boolean"
        or surface_metadata.get("value") is not False
    ):
        raise QaFailure("Emitter Surface Only did not expose its shared Boolean control")
    before_surface = client.state()
    client.click_component(surface_control_id)

    def surface_committed():
        state = client.state()
        overrides = state["project"]["module_instances"][instance_id].get(
            "parameter_overrides", {}
        )
        return (
            state
            if state["history"]["revision"]
            == before_surface["history"]["revision"] + 1
            and overrides.get(surface["id"]) is True
            and state["editor"].get("error") is None
            else None
        )

    surface_state = client.wait_until(
        "Emitter Surface Only Boolean commit", surface_committed
    )
    surface_pixels = sample_rendered_preview(
        client,
        sample_time,
        surface_state["history"]["revision"],
        "Sphere surface emitter",
    )
    if surface_pixels["pixel_hash"] == sphere["pixel_hash"]:
        raise QaFailure("Surface Only did not change same-frame Sphere pixels")

    opened_id, _ = open_timeline_item_definition(
        client, item_id, "node_clip", "Emitter Shape Node"
    )
    if opened_id != definition_id:
        raise QaFailure("Emitter controls opened a different Definition")
    node_shape_id = "node_editor.property.node:{}:shape".format(shape_node_id)
    node_shape = place_node_for_inline_edit(client, shape_node_id, node_shape_id)
    node_surface_id = "node_editor.property.node:{}:surface_only".format(shape_node_id)
    node_surface = place_node_for_inline_edit(client, shape_node_id, node_surface_id)
    for control, parameter_id, expected in (
        (node_shape, shape["id"], "Sphere"),
        (node_surface, surface["id"], True),
    ):
        metadata = control.get("metadata") or {}
        if (
            metadata.get("value") != expected
            or metadata.get("property_scope") != "module_instance"
            or metadata.get("parameter_id") != parameter_id
            or metadata.get("instance_id") != instance_id
        ):
            raise QaFailure("Node inline Emitter control lost its published value")

    artifact_dir = pathlib.Path(os.environ["RUVIE_QA_ARTIFACT_DIR"])
    capture = capture_viewport(client, artifact_dir / "capture.png")
    return {
        "suite": "particle-emitter-controls",
        "item_id": item_id,
        "instance_id": instance_id,
        "definition_id": definition_id,
        "shape_node_id": shape_node_id,
        "controls": {"shape": shape_control, "surface": surface_control},
        "options": {"box": box_option, "sphere": sphere_option},
        "samples": {
            "point": point,
            "box": box,
            "sphere": sphere,
            "surface": surface_pixels,
        },
        "node_controls": {"shape": node_shape, "surface": node_surface},
        "capture": capture,
        "actions": client.evidence,
    }


if __name__ == "__main__":
    raise SystemExit(
        run_suite_main(
            "qa-particle-emitter-controls-e2e",
            run_suite,
            "target/qa-particle-emitter-controls-e2e-evidence.json",
        )
    )
