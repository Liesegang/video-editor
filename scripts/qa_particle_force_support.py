"""Focused native-UI checks for executable Particle force parameters."""

from qa_node_module_support import place_node_for_inline_edit, place_private_node_clip_source
from qa_automation_support import history_shortcut
from qa_support import (
    QaFailure,
    activate_dock_tab,
    component_in_inspector,
)


PARTICLE_SOURCE_ID = "assets.particle_node_clip_source"
PARTICLE_PUBLISHED_PARAMETERS = [
    "Capacity",
    "Emission Rate",
    "Lifetime",
    "Seed",
    "Emitter Shape",
    "Emitter Position",
    "Emitter Radius",
    "Emitter Size",
    "Emitter Surface Only",
    "Birth Velocity Min",
    "Birth Velocity Max",
    "Birth Size Min",
    "Birth Size Max",
    "Gravity",
    "Turbulence Strength",
    "Turbulence Frequency",
    "Turbulence Octaves",
    "Turbulence Evolution",
    "Turbulence Seed",
    "Drag",
    "Collision Enabled",
    "Plane Point",
    "Plane Normal",
    "Radius",
    "Bounce",
    "Friction",
    "Tint",
    "Sprites",
    "Selection Mode",
    "Selection",
]


def _is_interactable(component):
    rect = component.get("rect_points") or {}
    return (
        component.get("visible") is True
        and component.get("enabled") is True
        and float(rect.get("width", 0.0)) > 0.0
        and float(rect.get("height", 0.0)) > 0.0
    )


def _reveal_particle_source(client):
    _, source = client.wait_until(
        "Particle Assets source registration",
        lambda: client.component(PARTICLE_SOURCE_ID),
    )
    if not _is_interactable(source):
        client.scroll_component("assets.node_clip_source", delta_x=0.0, delta_y=-180.0)
    return client.wait_component_settled(PARTICLE_SOURCE_ID)


def create_particle_node_clip(client, requested_start_seconds=6.5):
    """Place the production Particle source and return its exact authored owners."""

    initial = client.state()
    project = initial["project"]
    if project.get("name") != "Authoring QA":
        raise QaFailure("Particle System QA requires authoring_e2e")
    source_snapshot, source = _reveal_particle_source(client)
    expected_metadata = {
        "kind": "particle_system",
        "creation_kind": "particle_node_clip",
        "private_definition": True,
        "draggable_to_timeline": True,
        "view_mode": "list",
    }
    metadata = source.get("metadata") or {}
    for key, expected in expected_metadata.items():
        if metadata.get(key) != expected:
            raise QaFailure("Particle Assets source omitted {}={!r}".format(key, expected))

    created = place_private_node_clip_source(
        client,
        source_snapshot,
        source,
        requested_start_seconds,
        "Particle Node Clip",
    )
    definition = created["definition"]
    parameters = (definition.get("interface") or {}).get("parameters") or []
    if [entry.get("name") for entry in parameters] != PARTICLE_PUBLISHED_PARAMETERS:
        raise QaFailure("Particle Definition omitted its curated published parameters")
    if len(definition["graph"]["nodes"]) != 9 or len(
        definition["graph"]["connections"]
    ) != 8:
        raise QaFailure("Particle Definition omitted its authoritative topology")
    created["parameters"] = parameters
    return created


def particle_factory_terminal_route(definition):
    """Return the canonical Collision-to-Sprite terminal route."""

    catalog_nodes = {}
    for node_id, node in definition["graph"]["nodes"].items():
        if catalog_id := native_catalog_id(node):
            catalog_nodes.setdefault(catalog_id, []).append(node_id)
    collision = catalog_nodes.get("native.particle.collision-plane") or []
    drag = catalog_nodes.get("native.particle.drag-force") or []
    sprite = catalog_nodes.get("native.particle.sprite-renderer") or []
    if len(drag) != 1 or len(collision) != 1 or len(sprite) != 1:
        raise QaFailure("Particle factory has no unique Drag, Collision, and Sprite Nodes")
    predecessor = next(
        (
            candidate
            for candidate in definition["graph"]["connections"]
            if candidate["from"] == {"node_id": drag[0], "port": "particles"}
            and candidate["to"] == {"node_id": collision[0], "port": "particles"}
        ),
        None,
    )
    if predecessor is None:
        raise QaFailure("Particle factory does not place Collision after its force chain")
    route = next(
        (
            candidate
            for candidate in definition["graph"]["connections"]
            if candidate["from"]
            == {"node_id": collision[0], "port": "particles"}
            and candidate["to"] == {"node_id": sprite[0], "port": "particles"}
        ),
        None,
    )
    if route is None:
        raise QaFailure("Particle factory has no exact Collision-to-Sprite route")
    return collision[0], sprite[0], route


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


def native_catalog_id(node):
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
    if native_catalog_id(definition["graph"]["nodes"][node_id]) != "native.particle.turbulence":
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
