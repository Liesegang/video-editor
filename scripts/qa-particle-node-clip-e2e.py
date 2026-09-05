#!/usr/bin/env python3
"""Verify the production Particle System authoring path end to end."""

from qa_support import (
    QaFailure,
    activate_dock_tab,
    component_center,
    find_clear_canvas_point,
    media_seconds,
    run_suite_main,
    seek_timeline_seconds,
)


SOURCE_ID = "assets.particle_node_clip_source"
INSPECTOR_TAB_ID = "dock.tab:inspector"
TIMELINE_TAB_ID = "dock.tab:timeline"
NODE_EDITOR_TAB_ID = "dock.tab:node_editor"
NODE_EDITOR_CANVAS_ID = "node_editor.canvas"
PUBLISHED_PARAMETERS = [
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
    "Drag",
    "Color",
]
CONSTANT_ONLY_PARAMETERS = set(PUBLISHED_PARAMETERS) - {"Color"}
PARTICLE_CATALOG_IDS = {
    "native.particle.emitter",
    "native.particle.shape-location",
    "native.particle.initialize",
    "native.particle.gravity-force",
    "native.particle.drag-force",
    "native.particle.sprite-renderer",
}


def _is_interactable(component):
    rect = component.get("rect_points") or {}
    return (
        component.get("visible") is True
        and component.get("enabled") is True
        and float(rect.get("width", 0.0)) > 0.0
        and float(rect.get("height", 0.0)) > 0.0
    )


def _component(snapshot, component_id):
    return next(
        (
            component
            for component in snapshot["components"]
            if component.get("id") == component_id
        ),
        None,
    )


def _wait_registered_component(client, component_id, visible=False):
    def registered():
        component = _component(client.component_snapshot(), component_id)
        if component is None:
            return None
        rect = component.get("rect_points") or {}
        has_area = (
            float(rect.get("width", 0.0)) > 0.0
            and float(rect.get("height", 0.0)) > 0.0
        )
        if visible and (component.get("visible") is not True or not has_area):
            return None
        return component

    return client.wait_until(component_id + " registration", registered)


def _reveal_source(client):
    _, source = client.wait_until(
        "Particle Assets source registration", lambda: client.component(SOURCE_ID)
    )
    if not _is_interactable(source):
        client.scroll_component("assets.node_clip_source", delta_x=0.0, delta_y=-180.0)
    return client.wait_component_settled(SOURCE_ID)


def _native_catalog_id(node):
    content = node.get("content") or {}
    if str(content.get("type", "")).replace("_", "").lower() != "nativeoperation":
        return None
    return (content.get("data") or {}).get("catalog_id")


def _module_output_node_ids(definition):
    return {
        node_id
        for node_id, node in (definition.get("graph") or {}).get("nodes", {}).items()
        if str((node.get("content") or {}).get("type", "")).replace("_", "").lower()
        == "moduleoutput"
    }


def _active_definition(state):
    document = state["editor"]["node_editor"].get("document")
    if not document or document.get("kind") != "module_definition":
        raise QaFailure("Node Editor did not open a Module Definition document")
    definition_id = document.get("definition_id")
    definition = state["project"]["module_definitions"].get(definition_id)
    if definition is None:
        raise QaFailure("Node Editor document points at a missing Module Definition")
    return document, definition_id, definition


def _wait_particle_preview(client, revision, frame):
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


def _assert_same_frame_particle_delta(baseline, edited):
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


def _press_history_shortcut(client, redo=False):
    client.key("z", True, command=True, shift=redo)
    client.key("z", False, command=True, shift=redo)


def _reveal_inspector_control(client, control_id):
    for _ in range(8):
        component = _component(client.component_snapshot(), control_id)
        if component is not None and _is_interactable(component):
            return client.wait_component_settled(control_id)[1]
        client.scroll_component("inspector.scroll_area", delta_x=0.0, delta_y=-220.0)
    raise QaFailure("could not reveal Inspector control " + control_id)


def _edit_seed_and_assert_history(client, instance_id, parameters, baseline_state):
    seed = next(
        (parameter for parameter in parameters if parameter.get("name") == "Seed"), None
    )
    if seed is None:
        raise QaFailure("Particle System did not publish Seed")
    parameter_id = seed["id"]
    control_id = "inspector.property:module_instance:{}:{}".format(
        instance_id, parameter_id
    )
    control = _reveal_inspector_control(client, control_id)
    metadata = control.get("metadata") or {}
    if metadata.get("has_definition") is not True:
        raise QaFailure("Particle Seed bypassed the shared typed Property control")

    before_instance = baseline_state["project"]["module_instances"][instance_id]
    before_overrides = dict(before_instance.get("parameter_overrides") or {})
    before_revision = baseline_state["history"]["revision"]
    baseline_preview = baseline_state["editor"]["preview"]
    frame = baseline_preview["rendered_frame"]

    client.drag_component_by(control_id, 18.0, 0.0, steps=9)

    def edited():
        state = client.state()
        overrides = state["project"]["module_instances"][instance_id].get(
            "parameter_overrides"
        ) or {}
        if (
            state["history"]["revision"] == before_revision + 1
            and parameter_id in overrides
            and overrides[parameter_id] != before_overrides.get(parameter_id)
        ):
            return state
        return None

    edited_state = client.wait_until("Particle Seed instance override", edited)
    edited_override = edited_state["project"]["module_instances"][instance_id][
        "parameter_overrides"
    ][parameter_id]
    edited_render = _wait_particle_preview(
        client, edited_state["history"]["revision"], frame
    )
    edited_preview = edited_render["editor"]["preview"]
    _assert_same_frame_particle_delta(baseline_preview, edited_preview)

    _press_history_shortcut(client)

    def undone():
        state = client.state()
        instance = state["project"]["module_instances"][instance_id]
        if (
            state["history"]["revision"] == edited_state["history"]["revision"] + 1
            and (instance.get("parameter_overrides") or {}) == before_overrides
            and state["history"].get("can_redo") is True
        ):
            return state
        return None

    undone_state = client.wait_until("Undo Particle Seed override", undone)
    undone_render = _wait_particle_preview(
        client, undone_state["history"]["revision"], frame
    )
    undone_preview = undone_render["editor"]["preview"]
    if undone_preview["pixel_hash"] != baseline_preview["pixel_hash"]:
        raise QaFailure("Undo did not restore the exact same-frame Particle Preview")

    _press_history_shortcut(client, redo=True)

    def redone():
        state = client.state()
        overrides = state["project"]["module_instances"][instance_id].get(
            "parameter_overrides"
        ) or {}
        if (
            state["history"]["revision"] == undone_state["history"]["revision"] + 1
            and overrides.get(parameter_id) == edited_override
        ):
            return state
        return None

    redone_state = client.wait_until("Redo Particle Seed override", redone)
    redone_render = _wait_particle_preview(
        client, redone_state["history"]["revision"], frame
    )
    redone_preview = redone_render["editor"]["preview"]
    if redone_preview["pixel_hash"] != edited_preview["pixel_hash"]:
        raise QaFailure("Redo did not restore the exact edited Particle Preview")

    return {
        "control": control,
        "parameter_id": parameter_id,
        "override": edited_override,
        "before_preview": baseline_preview,
        "edited_preview": edited_preview,
        "undo_preview": undone_preview,
        "redo_preview": redone_preview,
        "revisions": {
            "before": before_revision,
            "edited": edited_state["history"]["revision"],
            "undone": undone_state["history"]["revision"],
            "redone": redone_state["history"]["revision"],
        },
    }


def _assert_open_particle_definition(
    client, item_id, instance_id, definition_id, expected_definition
):
    activate_dock_tab(client, TIMELINE_TAB_ID, "Timeline", "Particle Timeline")
    client.double_click_component("timeline.item:" + item_id)
    activate_dock_tab(
        client, NODE_EDITOR_TAB_ID, "Node Editor", "Particle Module Node Editor"
    )
    _, canvas = client.wait_component_settled(NODE_EDITOR_CANVAS_ID)
    canvas_metadata = canvas.get("metadata") or {}
    if canvas_metadata.get("production_surface") != "egui_snarl":
        raise QaFailure("Particle Definition did not use the production Node Editor")
    if canvas_metadata.get("timeline_graph_expansion") is not False:
        raise QaFailure("opening Particle Definition expanded Timeline structure into Nodes")
    if canvas_metadata.get("module_definition_id") != definition_id:
        raise QaFailure("Node Editor canvas exposed a different Particle Definition")

    opened = client.state()
    document, opened_definition_id, opened_definition = _active_definition(opened)
    if opened_definition_id != definition_id or document.get("instance_id") != instance_id:
        raise QaFailure("Node Editor lost the Particle Item's Definition/Instance identity")
    if opened_definition != expected_definition:
        changed_fields = sorted(
            key
            for key in set(opened_definition) | set(expected_definition)
            if opened_definition.get(key) != expected_definition.get(key)
        )
        before_nodes = (expected_definition.get("graph") or {}).get("nodes") or {}
        after_nodes = (opened_definition.get("graph") or {}).get("nodes") or {}
        changed_nodes = {
            node_id: sorted(
                key
                for key in set(before_nodes.get(node_id) or {})
                | set(after_nodes.get(node_id) or {})
                if (before_nodes.get(node_id) or {}).get(key)
                != (after_nodes.get(node_id) or {}).get(key)
            )
            for node_id in set(before_nodes) | set(after_nodes)
            if before_nodes.get(node_id) != after_nodes.get(node_id)
        }
        raise QaFailure(
            "opening the Particle Definition mutated fields {}; node fields {}; connections changed {}".format(
                changed_fields,
                changed_nodes,
                (expected_definition.get("graph") or {}).get("connections")
                != (opened_definition.get("graph") or {}).get("connections"),
            )
        )

    nodes = (opened_definition.get("graph") or {}).get("nodes") or {}
    native_ids = {_native_catalog_id(node) for node in nodes.values()}
    native_ids.discard(None)
    if native_ids != PARTICLE_CATALOG_IDS:
        raise QaFailure("production Node Editor opened the wrong Particle topology")
    output_ids = _module_output_node_ids(opened_definition)
    if len(nodes) != 7 or len(output_ids) != 1:
        raise QaFailure("Particle Node Editor omitted its six Nodes or Output terminal")
    for node_id in nodes:
        component = _wait_registered_component(client, "node_editor.node:" + node_id)
        metadata = component.get("metadata") or {}
        if (
            metadata.get("node_id") != node_id
            or metadata.get("production_surface") != "egui_snarl"
        ):
            raise QaFailure("Particle topology QA metadata did not come from production")
    return opened, canvas, sorted(native_ids), next(iter(output_ids))


def _create_unpublished_emitter_and_assert_locked_rate(
    client, definition_id, before_definition, before_revision
):
    menu_point = find_clear_canvas_point(
        client.component_snapshot(),
        NODE_EDITOR_CANVAS_ID,
        (
            "node_editor.node:",
            "node_editor.node_header:",
            "node_editor.port.",
            "node_editor.connection:",
        ),
    )
    client.inject(
        "click",
        {**menu_point, "button": "secondary", "coordinate_space": "points"},
    )
    client.wait_component("node_editor.menu.search")
    client.click_component("node_editor.menu.search")
    client.inject("text", {"text": "particle emitter"})
    client.wait_component_settled("node_editor.menu.create.particle_emitter")
    client.click_component("node_editor.menu.create.particle_emitter")

    before_nodes = (before_definition.get("graph") or {}).get("nodes") or {}

    def created():
        state = client.state()
        document = state["editor"]["node_editor"].get("document") or {}
        definition = state["project"]["module_definitions"].get(definition_id) or {}
        nodes = (definition.get("graph") or {}).get("nodes") or {}
        if (
            document.get("definition_id") == definition_id
            and state["history"]["revision"] == before_revision + 1
            and len(nodes) == len(before_nodes) + 1
        ):
            return state, definition
        return None

    created_state, created_definition = client.wait_until(
        "one unpublished Particle Emitter", created
    )
    new_node_ids = set(created_definition["graph"]["nodes"]) - set(before_nodes)
    if len(new_node_ids) != 1:
        raise QaFailure("Particle Emitter action did not add exactly one Node")
    emitter_id = next(iter(new_node_ids))
    emitter = created_definition["graph"]["nodes"][emitter_id]
    if _native_catalog_id(emitter) != "native.particle.emitter":
        raise QaFailure("Particle Emitter menu action created a different Node type")

    property_id = "node_editor.property.node:{}:rate".format(emitter_id)
    _, property_control = client.wait_component_settled(property_id)
    property_metadata = property_control.get("metadata") or {}
    reason = property_metadata.get("dynamic_value_disabled_reason")
    if (
        property_metadata.get("descriptor_available") is not True
        or property_metadata.get("allow_expression") is not False
        or not isinstance(reason, str)
        or "fixed-step" not in reason
        or property_control.get("enabled") is not True
    ):
        raise QaFailure("Particle Rate did not remain a typed editable constant Property")

    mode_id = "node_editor.property_mode.node:{}:rate".format(emitter_id)
    mode_control = _wait_registered_component(client, mode_id, visible=True)
    mode_metadata = mode_control.get("metadata") or {}
    if (
        mode_control.get("enabled") is not False
        or mode_metadata.get("mode") != "constant"
        or mode_metadata.get("allow_keyframe") is not False
        or mode_metadata.get("allow_expression") is not False
        or mode_metadata.get("mode_switch_available") is not False
        or mode_metadata.get("locked") is not True
        or mode_metadata.get("keyframe_disabled_reason") != reason
    ):
        raise QaFailure("Particle Rate mode switch did not expose its locked contract")

    port_id = "node_editor.port.node:{}.input:rate".format(emitter_id)
    disabled_port = _wait_registered_component(client, port_id, visible=True)
    port_metadata = disabled_port.get("metadata") or {}
    if (
        disabled_port.get("enabled") is not False
        or port_metadata.get("connectable") is not False
        or port_metadata.get("visual_state") != "disabled"
        or port_metadata.get("disabled_reason") != reason
    ):
        raise QaFailure("Particle Rate socket did not visibly expose its disabled contract")

    lock_id = "node_editor.port_lock.node:{}.input:rate".format(emitter_id)
    lock = _wait_registered_component(client, lock_id, visible=True)
    if (lock.get("metadata") or {}).get("disabled_reason") != reason:
        raise QaFailure("Particle Rate lock tooltip and descriptor reason diverged")

    output_id = "node_editor.port.node:{}.output:particles".format(emitter_id)
    _, output_port = client.wait_component_settled(output_id)
    if (output_port.get("metadata") or {}).get("connectable") is not True:
        raise QaFailure("Particle Emitter output unexpectedly became non-connectable")

    before_invalid_drag = client.state()
    before_invalid_definition = before_invalid_drag["project"]["module_definitions"][
        definition_id
    ]
    client.drag(
        component_center(disabled_port), component_center(output_port), steps=12
    )

    def gesture_finished():
        state = client.state()
        node_editor = state["editor"]["node_editor"]
        if (
            node_editor.get("gesture_active") is False
            and node_editor.get("gesture_transform_locked") is False
        ):
            return state
        return None

    after_invalid_drag = client.wait_until(
        "disabled Particle Rate drag to remain inactive", gesture_finished
    )
    if after_invalid_drag["history"]["revision"] != before_invalid_drag["history"]["revision"]:
        raise QaFailure("disabled Particle Rate drag committed an edit")
    if (
        after_invalid_drag["project"]["module_definitions"][definition_id]
        != before_invalid_definition
    ):
        raise QaFailure("disabled Particle Rate drag mutated the Module graph")
    if after_invalid_drag["editor"].get("error") is not None:
        raise QaFailure("disabled Particle Rate drag reached connection validation")

    _press_history_shortcut(client)

    def emitter_undone():
        state = client.state()
        definition = state["project"]["module_definitions"].get(definition_id)
        if (
            definition == before_definition
            and state["history"]["revision"]
            == before_invalid_drag["history"]["revision"] + 1
        ):
            return state
        return None

    restored = client.wait_until("one Undo restoring the Particle topology", emitter_undone)
    restored_graph = restored["project"]["module_definitions"][definition_id]["graph"]
    if (
        len(restored_graph["nodes"]) != 7
        or len(restored_graph["connections"]) != 6
        or restored["history"].get("can_redo") is not True
    ):
        raise QaFailure("Particle Emitter creation was not one Undo boundary")
    if restored["editor"].get("error") is not None:
        raise QaFailure("Particle Emitter Undo reported an editor error")

    return {
        "menu_point": menu_point,
        "node_id": emitter_id,
        "catalog_id": _native_catalog_id(emitter),
        "property": property_control,
        "property_mode": mode_control,
        "disabled_port": disabled_port,
        "lock": lock,
        "output_port": output_port,
        "invalid_drag": {
            "from": component_center(disabled_port),
            "to": component_center(output_port),
            "revision_before": before_invalid_drag["history"]["revision"],
            "revision_after": after_invalid_drag["history"]["revision"],
            "gesture_active": after_invalid_drag["editor"]["node_editor"][
                "gesture_active"
            ],
            "error": after_invalid_drag["editor"].get("error"),
        },
        "undo": {
            "restored_node_count": len(restored_graph["nodes"]),
            "restored_connection_count": len(restored_graph["connections"]),
            "revision_before": before_invalid_drag["history"]["revision"],
            "revision_after": restored["history"]["revision"],
            "can_redo": restored["history"].get("can_redo"),
        },
    }


def _assert_inspector_capabilities(client, instance_id, parameters):
    expected_ids = {
        parameter["name"]: "inspector.property_mode:module_instance:{}:{}".format(
            instance_id, parameter["id"]
        )
        for parameter in parameters
    }

    def controls_ready():
        snapshot = client.component_snapshot()
        components = {
            name: _component(snapshot, component_id)
            for name, component_id in expected_ids.items()
        }
        return components if all(components.values()) else None

    controls = client.wait_until("all Particle Inspector controls", controls_ready)
    evidence = {}
    for name, control in controls.items():
        metadata = control.get("metadata") or {}
        expected_keyframes = name == "Color"
        if metadata.get("mode") != "constant":
            raise QaFailure("{} did not start as a constant control".format(name))
        if metadata.get("allow_keyframe") is not expected_keyframes:
            raise QaFailure("{} exposed the wrong keyframe capability".format(name))
        if metadata.get("allow_expression") is not False:
            raise QaFailure("{} unexpectedly exposed Expression authoring".format(name))
        if expected_keyframes:
            if (
                metadata.get("keyframe_disabled_reason") is not None
                or metadata.get("mode_switch_available") is not True
                or metadata.get("locked") is not False
                or control.get("enabled") is not True
            ):
                raise QaFailure("Color did not retain frame-sampled authoring")
        else:
            reason = metadata.get("keyframe_disabled_reason")
            if (
                not isinstance(reason, str)
                or "fixed-step" not in reason
                or metadata.get("mode_switch_available") is not False
                or metadata.get("locked") is not True
                or control.get("enabled") is not False
            ):
                raise QaFailure(
                    "{} did not expose its locked fixed-step contract".format(name)
                )
        evidence[name] = {
            "enabled": control.get("enabled"),
            "metadata": metadata,
        }
    if set(evidence) != set(PUBLISHED_PARAMETERS):
        raise QaFailure("Inspector and Published parameter names diverged")
    if set(name for name in evidence if name != "Color") != CONSTANT_ONLY_PARAMETERS:
        raise QaFailure("Inspector constant-only parameter set changed")
    return evidence


def run_suite(client):
    client.wait_health()
    initial = client.state()
    project = initial["project"]
    if project.get("name") != "Authoring QA":
        raise QaFailure("Particle System QA requires authoring_e2e")
    if initial["editor"]["assets"].get("view_mode") != "list":
        raise QaFailure("Particle source must be reachable from the production Assets list")

    source_snapshot, source = _reveal_source(client)
    source_metadata = source.get("metadata") or {}
    expected_metadata = {
        "kind": "particle_system",
        "creation_kind": "particle_node_clip",
        "private_definition": True,
        "draggable_to_timeline": True,
        "view_mode": "list",
    }
    for key, expected in expected_metadata.items():
        if source_metadata.get(key) != expected:
            raise QaFailure(
                "Particle Assets source omitted {}={!r}".format(key, expected)
            )

    _, canvas = client.wait_component("timeline.canvas")
    track = next(
        (
            component
            for component in source_snapshot["components"]
            if component.get("type") == "timeline_track" and _is_interactable(component)
        ),
        None,
    )
    if track is None:
        raise QaFailure("Timeline exposed no production Track drop target")

    start = component_center(source)
    canvas_rect = canvas["rect_points"]
    # At 6.5s the fixture's remaining background clips are static through the
    # 200ms sample window. This lets the hash delta isolate Particle emission
    # while landing on an exact frame at the fixture's 30fps rate.
    requested_start_seconds = 6.5
    timeline_view = initial["editor"]["timeline"]
    target = {
        "x": canvas_rect["min_x"]
        + requested_start_seconds * float(timeline_view["pixels_per_second"])
        - float(timeline_view["horizontal_scroll"]),
        "y": component_center(track)["y"],
    }
    if not canvas_rect["min_x"] < target["x"] < canvas_rect["max_x"]:
        raise QaFailure("6.5s Particle drop target is outside the Timeline viewport")
    before_items = dict(project["items"])
    before_definitions = dict(project["module_definitions"])
    before_instances = dict(project["module_instances"])
    before_revision = initial["history"]["revision"]
    client.drag(start, target, steps=18)

    def placed():
        state = client.state()
        current = state["project"]
        if (
            len(current["items"]) == len(before_items) + 1
            and len(current["module_definitions"]) == len(before_definitions) + 1
            and len(current["module_instances"]) == len(before_instances) + 1
            and state["history"]["revision"] == before_revision + 1
            and state["editor"]["timeline"]["library_drag_active"] is False
        ):
            return state
        return None

    final = client.wait_until("Particle Node Clip placement", placed)
    final_project = final["project"]
    new_item_ids = set(final_project["items"]) - set(before_items)
    new_definition_ids = set(final_project["module_definitions"]) - set(before_definitions)
    new_instance_ids = set(final_project["module_instances"]) - set(before_instances)
    if not (
        len(new_item_ids) == 1
        and len(new_definition_ids) == 1
        and len(new_instance_ids) == 1
    ):
        raise QaFailure("Particle drag did not create one Item, Definition, and Instance")

    item_id = new_item_ids.pop()
    definition_id = new_definition_ids.pop()
    instance_id = new_instance_ids.pop()
    item = final_project["items"][item_id]
    invocation = item.get("source") or {}
    if invocation.get("kind") != "module":
        raise QaFailure("Particle Timeline item is not a production Module invocation")
    invocation_value = invocation.get("value") or {}
    if invocation_value.get("instance_id") != instance_id:
        raise QaFailure("Particle Item and Module Instance identities do not agree")

    instance = final_project["module_instances"][instance_id]
    if instance.get("definition_id") != definition_id:
        raise QaFailure("Particle Instance does not own the new Definition")
    definition = final_project["module_definitions"][definition_id]
    if definition.get("sharing") != {"kind": "private"}:
        raise QaFailure("Particle Definition is not private to its Timeline Item")
    graph = definition.get("graph") or {}
    parameters = (definition.get("interface") or {}).get("parameters") or []
    if len(graph.get("nodes") or {}) != 7 or len(graph.get("connections") or []) != 6:
        raise QaFailure("Particle Definition omitted its authoritative seven-node topology")
    if [parameter.get("name") for parameter in parameters] != PUBLISHED_PARAMETERS:
        raise QaFailure("Particle Definition omitted its curated published parameters")

    if item.get("name") != "Particle System":
        raise QaFailure("Particle Timeline item has an unexpected production label")
    duration_value = (item.get("interval") or {}).get("duration") or {}
    duration = float(duration_value.get("value", 0.0)) / max(
        float(duration_value.get("timescale", 1.0)), 1.0
    )
    if duration != 5.0:
        raise QaFailure("Particle Timeline item did not use the five-second clip default")
    for existing_id, before in before_items.items():
        if final_project["items"].get(existing_id) != before:
            raise QaFailure("Particle creation mutated ordinary clip {}".format(existing_id))
    for existing_id, before in before_definitions.items():
        if final_project["module_definitions"].get(existing_id) != before:
            raise QaFailure("Particle creation mutated Module {}".format(existing_id))
    for existing_id, before in before_instances.items():
        if final_project["module_instances"].get(existing_id) != before:
            raise QaFailure("Particle creation mutated Instance {}".format(existing_id))

    expected_selection = {"kind": "timeline_item", "id": item_id}
    if final["editor"]["selection"].get("primary") != expected_selection:
        raise QaFailure("Particle placement did not select the new Timeline Item")
    if final["editor"].get("error") is not None:
        raise QaFailure(
            "Particle interaction reported an error: {}".format(final["editor"]["error"])
        )
    _, rendered_item = client.wait_component_settled("timeline.item:" + item_id)
    rendered_metadata = rendered_item.get("metadata") or {}
    if float(rendered_metadata.get("duration_seconds", 0.0)) != 5.0:
        raise QaFailure("rendered Particle clip metadata disagrees with Project state")

    start_seconds = media_seconds((item.get("interval") or {})["start"])
    if abs(start_seconds - requested_start_seconds) > 0.02:
        raise QaFailure("Particle drop did not preserve its requested Timeline start")

    activate_dock_tab(
        client, INSPECTOR_TAB_ID, "Inspector", "Particle Node Clip Inspector"
    )
    inspector_controls = _assert_inspector_capabilities(
        client, instance_id, parameters
    )

    activate_dock_tab(client, TIMELINE_TAB_ID, "Timeline", "Particle Timeline")
    baseline_seek = seek_timeline_seconds(client, start_seconds)
    baseline_frame = baseline_seek["editor"]["timeline"]["current_frame"]
    baseline = _wait_particle_preview(
        client, final["history"]["revision"], baseline_frame
    )
    baseline_preview = baseline["editor"]["preview"]

    active_seconds = start_seconds + 0.2
    active_seek = seek_timeline_seconds(client, active_seconds)
    active_frame = active_seek["editor"]["timeline"]["current_frame"]
    active = _wait_particle_preview(client, final["history"]["revision"], active_frame)
    active_preview = active["editor"]["preview"]
    if active_preview["pixel_hash"] == baseline_preview["pixel_hash"]:
        raise QaFailure("active Particle time did not change rendered Preview content")

    replay_seek = seek_timeline_seconds(client, start_seconds)
    replay_frame = replay_seek["editor"]["timeline"]["current_frame"]
    replay = _wait_particle_preview(client, final["history"]["revision"], replay_frame)
    replay_preview = replay["editor"]["preview"]
    if replay_preview["pixel_hash"] != baseline_preview["pixel_hash"]:
        raise QaFailure("Particle Preview was not deterministic after seeking back")

    replay_at_active = seek_timeline_seconds(client, active_seconds)
    active_frame = replay_at_active["editor"]["timeline"]["current_frame"]
    active_before_edit = _wait_particle_preview(
        client, final["history"]["revision"], active_frame
    )
    activate_dock_tab(client, INSPECTOR_TAB_ID, "Inspector", "Particle Inspector")
    parameter_edit = _edit_seed_and_assert_history(
        client, instance_id, parameters, active_before_edit
    )
    before_open_definition = client.state()["project"]["module_definitions"][definition_id]
    if before_open_definition != definition:
        raise QaFailure("Particle parameter history changed Definition presentation")

    opened, node_canvas, native_catalog_ids, output_node_id = (
        _assert_open_particle_definition(
            client, item_id, instance_id, definition_id, before_open_definition
        )
    )
    node_editor_evidence = _create_unpublished_emitter_and_assert_locked_rate(
        client,
        definition_id,
        definition,
        opened["history"]["revision"],
    )
    completed = client.state()

    return {
        "suite": "particle-node-clip-authoring",
        "source": source,
        "drag": {"from": start, "to": target},
        "created_item": item,
        "created_instance_id": instance_id,
        "created_definition_id": definition_id,
        "published_parameters": PUBLISHED_PARAMETERS,
        "rendered_item": rendered_item,
        "inspector_controls": inspector_controls,
        "parameter_edit": parameter_edit,
        "preview": {
            "baseline": baseline_preview,
            "active": active_preview,
            "replay": replay_preview,
            "hash_changed_during_emission": (
                active_preview["pixel_hash"] != baseline_preview["pixel_hash"]
            ),
            "seek_back_is_deterministic": (
                replay_preview["pixel_hash"] == baseline_preview["pixel_hash"]
            ),
        },
        "node_editor": {
            "canvas": node_canvas,
            "native_catalog_ids": native_catalog_ids,
            "output_node_id": output_node_id,
            "unpublished_emitter": node_editor_evidence,
        },
        "history": completed["history"],
        "final_editor": {
            "error": completed["editor"].get("error"),
            "status": completed["editor"].get("status"),
            "selection": completed["editor"]["selection"],
        },
        "actions": client.evidence,
    }


if __name__ == "__main__":
    raise SystemExit(
        run_suite_main(
            "qa-particle-node-clip-e2e",
            run_suite,
            "target/qa-particle-node-clip-e2e-evidence.json",
        )
    )
