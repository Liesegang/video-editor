"""Focused native-UI checks for executable Particle force parameters."""

from qa_node_module_support import place_node_for_inline_edit
from qa_automation_support import history_shortcut
from qa_support import QaFailure, activate_dock_tab, component_in_inspector


def wait_particle_preview(client, revision, frame):
    def rendered():
        state = client.state()
        preview = state["editor"]["preview"]
        if (
            state["editor"].get("error") is None
            and preview.get("rendered_revision") == revision
            and preview.get("rendered_frame") == frame
            and int(preview.get("nontransparent_pixels") or 0) > 0
            and preview.get("pixel_hash")
        ):
            return state
        return None

    return client.wait_until(
        "nontransparent Particle preview for the current revision and frame",
        rendered,
        timeout=30.0,
    )


def assert_same_frame_particle_delta(baseline, edited):
    if baseline.get("rendered_frame") != edited.get("rendered_frame"):
        raise QaFailure("Particle parameter comparison did not use one exact frame")
    if not baseline.get("pixel_hash") or not edited.get("pixel_hash"):
        raise QaFailure("Particle parameter comparison has no rendered pixel hash")
    if int(baseline.get("nontransparent_pixels") or 0) <= 0:
        raise QaFailure("Particle comparison baseline has no visible rendered pixels")
    if int(edited.get("nontransparent_pixels") or 0) <= 0:
        raise QaFailure("Particle parameter edit removed all visible rendered pixels")
    if baseline["pixel_hash"] == edited["pixel_hash"]:
        raise QaFailure(
            "editing only the Particle instance did not change same-frame Preview pixels"
        )


def reveal_inspector_control(client, control_id):
    return component_in_inspector(client, control_id)[1]


def parameter(parameters, name):
    candidate = next(
        (value for value in parameters if value.get("name") == name), None
    )
    if candidate is None:
        raise QaFailure("Particle System did not publish " + name)
    return candidate


def constant_property(node, key):
    prop = (node.get("properties") or {}).get(key) or {}
    if prop.get("type") != "constant":
        return None
    return (prop.get("properties") or {}).get("value")


def _native_catalog_id(node):
    content = node.get("content") or {}
    if str(content.get("type", "")).replace("_", "").lower() != "nativeoperation":
        return None
    return (content.get("data") or {}).get("catalog_id")


def _enter_exact_numeric(client, control_id, value):
    component_in_inspector(client, control_id)
    client.click_component(control_id)
    client.key("a", True, command=True)
    client.key("a", False, command=True)
    client.inject("text", {"text": format(float(value), "g")})
    client.key("enter", True)
    client.key("enter", False)


def _duplicate_particle_sibling(client, item_id, definition_id):
    activate_dock_tab(client, "dock.tab:timeline", "Timeline", "Particle sibling")
    before = client.state()
    before_definition = before["project"]["module_definitions"][definition_id]
    client.click_component("timeline.item:" + item_id, button="secondary")
    action = "timeline.item.duplicate:" + item_id
    client.wait_component_settled(action)
    client.click_component(action)

    def duplicated():
        state = client.state()
        added = set(state["project"]["items"]) - set(before["project"]["items"])
        return (state, next(iter(added))) if len(added) == 1 else None

    duplicated_state, sibling_id = client.wait_until(
        "one Particle sibling placement", duplicated
    )
    duplicated_definition = duplicated_state["project"]["module_definitions"][
        definition_id
    ]
    if before_definition.get("sharing") != {"kind": "private"}:
        raise QaFailure("Particle factory Definition was not private before duplication")
    if duplicated_definition.get("sharing") != {"kind": "shared_local"}:
        raise QaFailure("duplicating Particle placement did not mark its Definition shared")
    before_without_sharing = dict(before_definition)
    duplicated_without_sharing = dict(duplicated_definition)
    before_without_sharing.pop("sharing", None)
    duplicated_without_sharing.pop("sharing", None)
    if duplicated_without_sharing != before_without_sharing:
        raise QaFailure(
            "duplicating Particle placement changed more than Definition sharing"
        )
    return duplicated_state, sibling_id, duplicated_definition


def _assert_sibling_unchanged(project, before, sibling_id):
    if project["items"][sibling_id] != before["items"][sibling_id]:
        raise QaFailure("Turbulence edit changed the sibling Particle item")
    invocation = before["items"][sibling_id]["source"]["value"]
    instance_id = invocation["instance_id"]
    if project["module_instances"][instance_id] != before["module_instances"][instance_id]:
        raise QaFailure("Turbulence edit changed the sibling Particle instance")
    definition_id = before["module_instances"][instance_id]["definition_id"]
    if project["module_definitions"][definition_id] != before["module_definitions"][definition_id]:
        raise QaFailure("Turbulence edit changed the sibling Particle definition")


def edit_turbulence_and_assert_history(
    client, item_id, instance_id, definition_id, parameters, baseline_state
):
    published = parameter(parameters, "Turbulence Strength")
    parameter_id = published["id"]
    default_value = published.get("default_value")
    if not isinstance(default_value, (int, float)) or float(default_value) != 0.0:
        raise QaFailure("Particle factory Turbulence Strength is not neutral by default")

    duplicated, sibling_id, definition_baseline = _duplicate_particle_sibling(
        client, item_id, definition_id
    )
    sibling_before = duplicated["project"]
    frame = baseline_state["editor"]["preview"]["rendered_frame"]
    sibling_baseline = wait_particle_preview(
        client, duplicated["history"]["revision"], frame
    )
    activate_dock_tab(client, "dock.tab:timeline", "Timeline", "Particle selection")
    client.click_component("timeline.item:" + item_id)
    activate_dock_tab(client, "dock.tab:inspector", "Inspector", "Particle Turbulence")
    control_id = "inspector.property:module_instance:{}:{}".format(
        instance_id, parameter_id
    )
    control = reveal_inspector_control(client, control_id)
    before = client.state()
    _enter_exact_numeric(client, control_id, 120.0)

    def edited():
        state = client.state()
        value = state["project"]["module_instances"][instance_id].get(
            "parameter_overrides", {}
        ).get(parameter_id)
        return (
            state
            if state["history"]["revision"] == before["history"]["revision"] + 1
            and isinstance(value, (int, float))
            and float(value) == 120.0
            else None
        )

    edited_state = client.wait_until("Turbulence Strength 120 instance edit", edited)
    edited_render = wait_particle_preview(
        client, edited_state["history"]["revision"], frame
    )
    assert_same_frame_particle_delta(
        sibling_baseline["editor"]["preview"], edited_render["editor"]["preview"]
    )
    _assert_sibling_unchanged(edited_state["project"], sibling_before, sibling_id)

    activate_dock_tab(client, "dock.tab:timeline", "Timeline", "Particle Node check")
    client.double_click_component("timeline.item:" + item_id)
    activate_dock_tab(client, "dock.tab:node_editor", "Node Editor", "Turbulence Node")
    definition = edited_state["project"]["module_definitions"][definition_id]
    node_id = published["target"]["node_id"]
    node_control_id = "node_editor.property.node:{}:strength".format(node_id)
    node_control = place_node_for_inline_edit(client, node_id, node_control_id)
    node_value = (node_control.get("metadata") or {}).get("value")
    if not isinstance(node_value, (int, float)) or float(node_value) != 120.0:
        raise QaFailure("Node Turbulence Strength diverged from its Inspector parameter")
    if _native_catalog_id(definition["graph"]["nodes"][node_id]) != "native.particle.turbulence":
        raise QaFailure("Turbulence parameter targets the wrong native Node")

    activate_dock_tab(client, "dock.tab:timeline", "Timeline", "Particle reset")
    client.click_component("timeline.item:" + item_id)
    activate_dock_tab(client, "dock.tab:inspector", "Inspector", "Particle reset")
    reveal_inspector_control(client, control_id)
    before_reset = client.state()
    client.click_component(control_id, button="secondary")
    reset_id = "inspector.module_parameter.reset:{}:{}".format(
        instance_id, parameter_id
    )
    client.wait_component_settled(reset_id)
    client.click_component(reset_id)

    def reset():
        state = client.state()
        overrides = state["project"]["module_instances"][instance_id].get(
            "parameter_overrides", {}
        )
        return (
            state
            if state["history"]["revision"] == before_reset["history"]["revision"] + 1
            and parameter_id not in overrides
            else None
        )

    reset_state = client.wait_until("Turbulence Strength reset", reset)
    _assert_sibling_unchanged(reset_state["project"], sibling_before, sibling_id)
    reset_render = wait_particle_preview(client, reset_state["history"]["revision"], frame)
    if (
        reset_render["editor"]["preview"]["pixel_hash"]
        != sibling_baseline["editor"]["preview"]["pixel_hash"]
    ):
        raise QaFailure("Turbulence reset did not restore neutral same-frame pixels")
    history_shortcut(client)
    undone = client.wait_until(
        "Undo Turbulence reset",
        lambda: state
        if isinstance(
            value := (state := client.state())["project"]["module_instances"][
                instance_id
            ]
            .get("parameter_overrides", {})
            .get(parameter_id),
            (int, float),
        )
        and float(value) == 120.0
        else None,
    )
    undo_render = wait_particle_preview(client, undone["history"]["revision"], frame)
    if (
        undo_render["editor"]["preview"]["pixel_hash"]
        != edited_render["editor"]["preview"]["pixel_hash"]
    ):
        raise QaFailure("Undo Turbulence reset did not restore edited pixels")
    _assert_sibling_unchanged(undone["project"], sibling_before, sibling_id)
    return {
        "parameter_id": parameter_id,
        "node_id": node_id,
        "sibling_id": sibling_id,
        "sibling_baseline": sibling_baseline["editor"]["preview"],
        "control": control,
        "node_control": node_control,
        "edited_preview": edited_render["editor"]["preview"],
        "reset_preview": reset_render["editor"]["preview"],
        "undo_preview": undo_render["editor"]["preview"],
        "shared_definition": definition_baseline,
    }
