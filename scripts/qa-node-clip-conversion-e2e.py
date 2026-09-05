#!/usr/bin/env python3
"""Convert one Text clip source to a bounded Node Clip through native UI."""

from qa_support import (
    QaFailure,
    component_center,
    component_point,
    convert_timeline_item_to_node_clip,
    item_by_name,
    run_suite_main,
    settled_preview_state,
)


def _component(snapshot, component_id):
    return next(
        (component for component in snapshot["components"] if component["id"] == component_id),
        None,
    )


def _inside_scroll_area(component, scroll_area):
    if not component or not component.get("visible") or not component.get("enabled"):
        return False
    rect = component.get("rect_points") or {}
    area = scroll_area.get("rect_points") or {}
    return (
        float(rect.get("width", 0.0)) > 0.0
        and float(rect.get("height", 0.0)) > 0.0
        and float(area.get("min_y", 0.0))
        <= float(rect.get("center_y", -1.0))
        <= float(area.get("max_y", 0.0))
    )


def _bring_into_inspector(client, component_id, scroll_delta):
    for _ in range(12):
        snapshot = client.component_snapshot()
        component = _component(snapshot, component_id)
        scroll_area = _component(snapshot, "inspector.scroll_area")
        if _inside_scroll_area(component, scroll_area or {}):
            return client.wait_component_settled(component_id)[1]
        component_rect = (component or {}).get("rect_points") or {}
        scroll_rect = (scroll_area or {}).get("rect_points") or {}
        component_y = float(component_rect.get("center_y", 0.0))
        if component_y < float(scroll_rect.get("min_y", 0.0)):
            delta = abs(scroll_delta)
        elif component_y > float(scroll_rect.get("max_y", 0.0)):
            delta = -abs(scroll_delta)
        else:
            delta = scroll_delta
        client.scroll_component("inspector.scroll_area", 0.0, delta)
    raise QaFailure("could not bring {} into the Inspector".format(component_id))


def _move_effect_to_pre_transform(client, attachment_id):
    handle = _bring_into_inspector(
        client, "inspector.effect_drag_handle:" + attachment_id, -320.0
    )
    start = component_center(handle)
    client.inject(
        "press",
        {**start, "button": "primary", "coordinate_space": "points"},
    )
    client.inject(
        "move",
        {
            "x": start["x"] + 8.0,
            "y": start["y"],
            "button": "primary",
            "coordinate_space": "points",
        },
    )
    client.wait_until(
        "Effect drag payload",
        lambda: component
        if (
            component := _component(
                client.component_snapshot(), "inspector.effects.drag_state"
            )
        )
        and (component.get("metadata") or {}).get("dragging") is True
        else None,
    )
    slot = _bring_into_inspector(
        client, "inspector.effect_drop:item_pre_transform:0", 320.0
    )
    target = component_center(slot)
    for _ in range(2):
        client.inject(
            "move",
            {**target, "button": "primary", "coordinate_space": "points"},
        )
        slot = _bring_into_inspector(
            client, "inspector.effect_drop:item_pre_transform:0", 320.0
        )
        target = component_center(slot)
    metadata = slot.get("metadata") or {}
    if metadata.get("hovered") is not True:
        raise QaFailure("pre-Transform insertion slot did not preview the move")
    client.inject(
        "release",
        {**target, "button": "primary", "coordinate_space": "points"},
    )


def _node_type(node):
    return str((node.get("content") or {}).get("type", "")).replace("_", "").lower()


def _plugin_component(node):
    content = node.get("content") or {}
    if _node_type(node) != "pluginoperation":
        return None
    return (content.get("data") or {}).get("component_id")


def _constant_item_property(state, item_id, key):
    property_value = (
        state["project"]["items"][item_id].get("authored_properties") or {}
    ).get(key)
    if property_value is None or property_value.get("type") != "constant":
        return None
    return (property_value.get("properties") or {}).get("value")


def _direct_text_content(state, item_id):
    source = state["project"]["items"][item_id]["source"]
    if source.get("kind") != "text":
        raise QaFailure("direct Text content target is no longer a Text source")
    return source["value"]["text"]


def _edit_direct_text_content_and_undo(client, item_id):
    control_id = "inspector.property:item:{}:text".format(item_id)
    _bring_into_inspector(client, control_id, -280.0)
    before = client.state()
    original = _direct_text_content(before, item_id)
    edited_text = "Inspector common Content"
    client.click_component(control_id)
    client.key("a", True, command=True)
    client.key("a", False, command=True)
    client.inject("text", {"text": edited_text})
    client.key("enter", True, command=True)
    client.key("enter", False, command=True)
    edited = client.wait_until(
        "direct Text common Content edit",
        lambda: state
        if (state := client.state())["history"]["revision"]
        == before["history"]["revision"] + 1
        and _direct_text_content(state, item_id) == edited_text
        else None,
    )
    if "text" in (edited["project"]["items"][item_id].get("authored_properties") or {}):
        raise QaFailure("Text Content was duplicated into authored properties")
    # Move focus out of the multiline editor so Ctrl+Z reaches the application
    # history instead of the TextEdit widget's private typing history.
    client.click_component("timeline.item:" + item_id)
    client.key("z", True, command=True)
    client.key("z", False, command=True)
    undone = client.wait_until(
        "direct Text common Content Undo",
        lambda: state
        if _direct_text_content((state := client.state()), item_id) == original
        else None,
    )
    return {
        "before": original,
        "edited": edited_text,
        "undone_revision": undone["history"]["revision"],
    }


def _edit_direct_text_size(client, item_id):
    state = client.state()
    source = state["project"]["items"][item_id]["source"]
    appearance = source["value"].get("appearance_operations") or []
    fills = [
        operation
        for operation in appearance
        if operation["operation"].get("component_id") == "fill"
    ]
    if len(fills) != 1:
        raise QaFailure("direct Text expected one authoritative Fill Appearance")
    fill_id = fills[0]["id"]
    controls = {
        "font": "inspector.property:item:{}:font_family".format(item_id),
        "size": "inspector.property:item:{}:size".format(item_id),
        "fill": "inspector.property:appearance:{}:{}:color".format(item_id, fill_id),
        "content": "inspector.property:item:{}:text".format(item_id),
    }
    expected_kinds = {
        "content": "multiline_text",
        "font": "font",
        "size": "float",
        "fill": "managed_color",
    }
    metadata = {}
    for name, component_id in controls.items():
        component = _bring_into_inspector(client, component_id, -280.0)
        component_metadata = component.get("metadata") or {}
        if component_metadata.get("editor_kind") != expected_kinds[name]:
            raise QaFailure(
                "direct Text {} did not use its authoritative {} editor".format(
                    name, expected_kinds[name]
                )
            )
        metadata[name] = component_metadata

    before = client.state()
    before_size = _constant_item_property(before, item_id, "size")
    _bring_into_inspector(client, controls["size"], 280.0)
    client.drag_component_by(controls["size"], 24.0, 0.0, steps=10)

    def edited():
        state = client.state()
        size = _constant_item_property(state, item_id, "size")
        return (
            state
            if state["history"]["revision"] == before["history"]["revision"] + 1
            and size is not None
            and size != before_size
            else None
        )

    return client.wait_until("direct Text Font Size edit", edited), metadata


def _module_parameter(definition, name):
    matches = [
        parameter
        for parameter in definition["interface"]["parameters"]
        if parameter.get("name") == name
    ]
    if len(matches) != 1:
        raise QaFailure(
            "converted Text expected one published {!r} parameter, got {}".format(
                name, len(matches)
            )
        )
    return matches[0]


def _module_style_parameter(definition, component_id, property_key):
    node_ids = {
        node_id
        for node_id, node in definition["graph"]["nodes"].items()
        if _plugin_component(node) == component_id
    }
    matches = [
        parameter
        for parameter in definition["interface"]["parameters"]
        if parameter["target"].get("node_id") in node_ids
        and parameter["target"].get("port") == "property:" + property_key
    ]
    if len(matches) != 1:
        raise QaFailure(
            "converted Text expected one {}.{} published parameter, got {}".format(
                component_id, property_key, len(matches)
            )
        )
    return matches[0]


def _edit_converted_text_size(client, instance_id, definition, converted_state):
    expected_kinds = {
        "Content": "multiline_text",
        "Font": "font",
        "Font Size": "float",
    }
    controls = {}
    for name, expected_kind in expected_kinds.items():
        parameter = _module_parameter(definition, name)
        control_id = "inspector.property:module_instance:{}:{}".format(
            instance_id, parameter["id"]
        )
        component = _bring_into_inspector(client, control_id, -280.0)
        metadata = component.get("metadata") or {}
        if metadata.get("editor_kind") != expected_kind:
            raise QaFailure(
                "converted Text {} did not preserve its {} editor".format(
                    name, expected_kind
                )
            )
        controls[name] = {"parameter": parameter, "metadata": metadata}

    fill_parameter = _module_style_parameter(definition, "fill", "color")
    fill_control_id = "inspector.property:module_instance:{}:{}".format(
        instance_id, fill_parameter["id"]
    )
    fill_component = _bring_into_inspector(client, fill_control_id, -280.0)
    fill_metadata = fill_component.get("metadata") or {}
    if fill_metadata.get("editor_kind") != "managed_color":
        raise QaFailure("converted Text Fill did not use its managed_color editor")
    controls["Fill"] = {
        "parameter": fill_parameter,
        "metadata": fill_metadata,
    }

    parameter_id = controls["Font Size"]["parameter"]["id"]
    control_id = "inspector.property:module_instance:{}:{}".format(
        instance_id, parameter_id
    )
    before_overrides = dict(
        converted_state["project"]["module_instances"][instance_id].get(
            "parameter_overrides"
        )
        or {}
    )
    _bring_into_inspector(client, control_id, 280.0)
    client.drag_component_by(control_id, 28.0, 0.0, steps=10)

    def edited():
        state = client.state()
        overrides = state["project"]["module_instances"][instance_id].get(
            "parameter_overrides"
        ) or {}
        return (
            state
            if state["history"]["revision"]
            == converted_state["history"]["revision"] + 1
            and overrides.get(parameter_id) != before_overrides.get(parameter_id)
            else None
        )

    return client.wait_until("converted Text Font Size edit", edited), controls


def _exercise_converted_appearance(client, item_id, instance_id, baseline_state, frame):
    section_id = "inspector.appearance:" + item_id
    section = _bring_into_inspector(client, section_id, -320.0)
    section_metadata = section.get("metadata") or {}
    if section_metadata.get("owner_model") != "module_graph":
        raise QaFailure("converted Appearance is not projected from the Module graph")
    operations = section_metadata.get("operations") or []
    if len(operations) != 1 or operations[0].get("component_id") != "fill":
        raise QaFailure("converted Appearance did not expose its authoritative Fill Node")
    fill_id = operations[0]["id"]

    add_id = "inspector.appearance.add_menu:" + item_id
    _bring_into_inspector(client, add_id, -320.0)
    client.click_component(add_id)
    search_id = "inspector.appearance.menu:" + item_id + ".query"
    client.wait_component_settled(search_id)
    client.click_component(search_id)
    client.inject("text", {"text": "drop shadow"})
    choice_id = "inspector.appearance.add.drop_shadow"
    client.wait_component_settled(choice_id)
    client.click_component(choice_id)

    def added():
        state = client.state()
        component = _component(client.component_snapshot(), section_id)
        metadata = (component or {}).get("metadata") or {}
        entries = metadata.get("operations") or []
        return (
            (state, metadata, entries[-1]["id"])
            if state["history"]["revision"] == baseline_state["history"]["revision"] + 1
            and metadata.get("owner_model") == "module_graph"
            and [entry.get("component_id") for entry in entries]
            == ["fill", "drop_shadow"]
            else None
        )

    added_state, added_metadata, shadow_id = client.wait_until(
        "structured Node Clip Appearance add", added
    )
    definition_id = added_state["project"]["module_instances"][instance_id]["definition_id"]
    definition = added_state["project"]["module_definitions"][definition_id]
    if _plugin_component(definition["graph"]["nodes"].get(shadow_id) or {}) != "drop_shadow":
        raise QaFailure("structured Appearance did not insert a real Drop Shadow Node")
    distance = _module_style_parameter(definition, "drop_shadow", "distance")
    distance_id = "inspector.property:module_instance:{}:{}".format(
        instance_id, distance["id"]
    )
    distance_component = _bring_into_inspector(client, distance_id, -320.0)
    distance_metadata = distance_component.get("metadata") or {}
    if distance_metadata.get("editor_kind") != "float":
        raise QaFailure("structured Drop Shadow Distance is not a typed float control")
    before_override = (
        added_state["project"]["module_instances"][instance_id].get("parameter_overrides")
        or {}
    ).get(distance["id"])
    client.drag_component_by(distance_id, 28.0, 0.0, steps=8)

    def edited():
        state = client.state()
        override = (
            state["project"]["module_instances"][instance_id].get("parameter_overrides")
            or {}
        ).get(distance["id"])
        return (
            state
            if state["history"]["revision"] == added_state["history"]["revision"] + 1
            and override != before_override
            else None
        )

    edited_state = client.wait_until("structured Drop Shadow Distance edit", edited)
    edited_render = client.wait_until(
        "structured Appearance edited Preview",
        lambda: settled_preview_state(client, edited_state["history"]["revision"], frame),
        timeout=30.0,
    )

    move_id = "inspector.appearance.move_up:" + shadow_id
    _bring_into_inspector(client, move_id, -320.0)
    client.click_component(move_id)

    def reordered():
        state = client.state()
        component = _component(client.component_snapshot(), section_id)
        metadata = (component or {}).get("metadata") or {}
        ids = [entry.get("id") for entry in metadata.get("operations") or []]
        return (
            (state, metadata)
            if state["history"]["revision"] == edited_state["history"]["revision"] + 1
            and ids == [shadow_id, fill_id]
            else None
        )

    reordered_state, reordered_metadata = client.wait_until(
        "structured Appearance reorder", reordered
    )
    actions_id = "inspector.appearance.actions:" + shadow_id
    _bring_into_inspector(client, actions_id, -320.0)
    client.click_component(actions_id)
    remove_id = "inspector.appearance.remove:" + shadow_id
    client.wait_component_settled(remove_id)
    client.click_component(remove_id)

    def removed():
        state = client.state()
        component = _component(client.component_snapshot(), section_id)
        metadata = (component or {}).get("metadata") or {}
        entries = metadata.get("operations") or []
        current_definition_id = state["project"]["module_instances"][instance_id][
            "definition_id"
        ]
        current_definition = state["project"]["module_definitions"][current_definition_id]
        return (
            state
            if state["history"]["revision"] == reordered_state["history"]["revision"] + 1
            and [entry.get("id") for entry in entries] == [fill_id]
            and shadow_id not in current_definition["graph"]["nodes"]
            and not any(
                parameter["target"].get("node_id") == shadow_id
                for parameter in current_definition["interface"]["parameters"]
            )
            else None
        )

    removed_state = client.wait_until("structured Appearance remove", removed)
    for label in (
        "structured Appearance remove Undo",
        "structured Appearance reorder Undo",
        "structured Appearance property Undo",
        "structured Appearance add Undo",
    ):
        client.key("z", True, command=True)
        client.key("z", False, command=True)
        previous_revision = removed_state["history"]["revision"]
        removed_state = client.wait_until(
            label,
            lambda: state
            if (state := client.state())["history"]["revision"] > previous_revision
            else None,
        )
    if removed_state["project"] != baseline_state["project"]:
        raise QaFailure("structured Appearance Undo chain did not restore the converted project")
    restored_render = client.wait_until(
        "Node Clip Preview after Appearance Undo",
        lambda: state
        if (
            (state := settled_preview_state(client, removed_state["history"]["revision"], frame))
            and state["editor"]["preview"].get("pixel_hash")
            == baseline_state["editor"]["preview"].get("pixel_hash")
        )
        else None,
        timeout=30.0,
    )
    return {
        "added": added_metadata,
        "reordered": reordered_metadata,
        "shadow_id": shadow_id,
        "distance_parameter_id": distance["id"],
        "distance_editor": distance_metadata,
        "edited_preview_hash": edited_render["editor"]["preview"]["pixel_hash"],
        "restored_state": restored_render,
        "restored_preview_hash": restored_render["editor"]["preview"]["pixel_hash"],
    }


def _exercise_converted_ensemble(client, item_id, instance_id, baseline_state, frame):
    add_id = "inspector.text_ensemble.add_menu:" + item_id
    _bring_into_inspector(client, add_id, -320.0)
    client.click_component(add_id)
    search_id = "inspector.text_ensemble.menu:" + item_id + ".query"
    client.wait_component_settled(search_id)
    client.click_component(search_id)
    client.inject("text", {"text": "opacity"})
    choice_id = "inspector.text_ensemble.add.effector:opacity"
    client.wait_component_settled(choice_id)
    client.click_component(choice_id)

    before_nodes = set(
        baseline_state["project"]["module_definitions"][
            baseline_state["project"]["module_instances"][instance_id]["definition_id"]
        ]["graph"]["nodes"]
    )

    def added():
        state = client.state()
        component = _component(client.component_snapshot(), "inspector.text_ensemble:" + item_id)
        metadata = (component or {}).get("metadata") or {}
        definition_id = state["project"]["module_instances"][instance_id]["definition_id"]
        nodes = state["project"]["module_definitions"][definition_id]["graph"]["nodes"]
        new_nodes = set(nodes) - before_nodes
        return (
            (state, component, next(iter(new_nodes)))
            if state["history"]["revision"] == baseline_state["history"]["revision"] + 1
            and len(new_nodes) == 1
            and metadata.get("owner_model") == "module_graph"
            and metadata.get("operation_count") == 1
            else None
        )

    added_state, section, operation_id = client.wait_until(
        "structured Node Clip Text Ensemble add", added
    )
    definition_id = added_state["project"]["module_instances"][instance_id]["definition_id"]
    definition = added_state["project"]["module_definitions"][definition_id]
    operation_node = definition["graph"]["nodes"][operation_id]
    if _plugin_component(operation_node) != "opacity":
        raise QaFailure("structured Text Ensemble inserted the wrong graph Node")
    parameter = next(
        (
            parameter
            for parameter in definition["interface"]["parameters"]
            if parameter["target"]["node_id"] == operation_id
            and parameter["target"].get("port") == "property:opacity"
        ),
        None,
    )
    if parameter is None:
        raise QaFailure("structured Text Ensemble did not publish Opacity")
    parameter_id = parameter["id"]
    control_id = "inspector.property:module_instance:{}:{}".format(
        instance_id, parameter_id
    )
    control = _bring_into_inspector(client, control_id, -280.0)
    if (control.get("metadata") or {}).get("editor_kind") != "float":
        raise QaFailure("structured Text Ensemble bypassed the published typed control")
    added_render = client.wait_until(
        "Node Clip Text Ensemble added Preview",
        lambda: settled_preview_state(client, added_state["history"]["revision"], frame),
        timeout=30.0,
    )
    before_override = (
        added_state["project"]["module_instances"][instance_id].get("parameter_overrides")
        or {}
    ).get(parameter_id)
    client.drag_component_by(control_id, 56.0, 0.0, steps=12)

    def property_edited():
        state = client.state()
        override = (
            state["project"]["module_instances"][instance_id].get("parameter_overrides")
            or {}
        ).get(parameter_id)
        return (
            state
            if state["history"]["revision"] == added_state["history"]["revision"] + 1
            and override != before_override
            else None
        )

    edited_state = client.wait_until("structured Text Ensemble Opacity edit", property_edited)
    edited_render = client.wait_until(
        "structured Text Ensemble edited Preview",
        lambda: state
        if (
            (state := settled_preview_state(client, edited_state["history"]["revision"], frame))
            and state["editor"]["preview"].get("pixel_hash")
            != added_render["editor"]["preview"].get("pixel_hash")
        )
        else None,
        timeout=30.0,
    )

    client.key("z", True, command=True)
    client.key("z", False, command=True)
    property_undone = client.wait_until(
        "structured Text Ensemble property Undo",
        lambda: state
        if (
            (state := client.state())["history"]["revision"]
            == edited_state["history"]["revision"] + 1
            and state["project"] == added_state["project"]
        )
        else None,
    )
    client.key("z", True, command=True)
    client.key("z", False, command=True)
    restored = client.wait_until(
        "structured Text Ensemble add Undo",
        lambda: state
        if (
            (state := client.state())["history"]["revision"]
            == property_undone["history"]["revision"] + 1
            and state["project"] == baseline_state["project"]
        )
        else None,
    )
    client.wait_until(
        "Node Clip Preview after Ensemble Undo",
        lambda: state
        if (
            (state := settled_preview_state(client, restored["history"]["revision"], frame))
            and state["editor"]["preview"].get("pixel_hash")
            == baseline_state["editor"]["preview"].get("pixel_hash")
        )
        else None,
        timeout=30.0,
    )
    return {
        "section": section.get("metadata"),
        "operation_id": operation_id,
        "parameter_id": parameter_id,
        "edited_preview_hash": edited_render["editor"]["preview"]["pixel_hash"],
        "restored_state": restored,
    }


def run_suite(client):
    client.wait_health()
    initial = client.state()
    text_item = item_by_name(initial["project"], "QA Text")
    item_id = text_item["id"]
    client.click_component("timeline.item:" + item_id)

    def fixture_effects():
        state = client.state()
        effects = [
            (attachment_id, attachment)
            for attachment_id, attachment in state["project"]["attachments"].items()
            if attachment["owner"]
            == {"kind": "item", "item_id": item_id}
        ]
        return (state, effects) if len(effects) == 2 else None

    _, effects = client.wait_until("fixture Text Effects", fixture_effects)
    tile_id, _ = next(
        pair
        for pair in effects
        if pair[1]["processor"]["value"]["operation"]["component_id"] == "tile"
    )
    blur_id, _ = next(pair for pair in effects if pair[0] != tile_id)
    _move_effect_to_pre_transform(client, tile_id)

    moved = client.wait_until(
        "one pre- and one post-Transform Effect",
        lambda: state
        if (
            (state := client.state())["project"]["attachments"][tile_id]["stage"]
            == "item_pre_transform"
            and state["project"]["attachments"][blur_id]["stage"]
            == "item_post_transform"
        )
        else None,
    )

    # Seek inside the Text interval so the pixel hash validates the converted
    # source and both sides of the host Transform boundary.
    _, ruler = client.wait_component("timeline.ruler")
    client.inject(
        "click",
        {
            **component_point(ruler, 0.16, 0.5),
            "button": "primary",
            "coordinate_space": "points",
        },
    )
    seeked = client.wait_until(
        "Timeline seek publication",
        lambda: state
        if (state := client.state())["editor"]["timeline"]["current_frame"]
        != moved["editor"]["timeline"]["current_frame"]
        else None,
    )
    expected_frame = seeked["editor"]["timeline"]["current_frame"]
    direct_baseline = client.wait_until(
        "direct Text baseline Preview",
        lambda: settled_preview_state(
            client, moved["history"]["revision"], expected_frame
        ),
        timeout=30.0,
    )
    direct_content = _edit_direct_text_content_and_undo(client, item_id)
    direct_edited, direct_controls = _edit_direct_text_size(client, item_id)
    before = client.wait_until(
        "direct Text edited Preview",
        lambda: state
        if (
            (state := settled_preview_state(
                client, direct_edited["history"]["revision"], expected_frame
            ))
            and state["editor"]["preview"].get("pixel_hash")
            != direct_baseline["editor"]["preview"].get("pixel_hash")
        )
        else None,
        timeout=30.0,
    )
    project_before = before["project"]
    item_before = project_before["items"][item_id]
    attachments_before = project_before["attachments"]
    preview_hash = before["editor"]["preview"]["pixel_hash"]
    revision_before = before["history"]["revision"]

    after = convert_timeline_item_to_node_clip(client, item_id, revision_before)
    item_after = after["project"]["items"][item_id]
    source = item_after["source"]["value"]
    document = after["editor"]["node_editor"]["document"]
    definition_id = document["definition_id"]
    instance_id = document["instance_id"]
    if "Node Editor" not in after["dock"]["active_tabs"]:
        raise QaFailure("conversion did not focus the existing Node Editor panel")
    if "moved 1 pre-Transform effect(s), kept 1 post-Transform effect(s) outside" not in after[
        "editor"
    ]["status"]:
        raise QaFailure("conversion status did not explain the preserved stage boundary")
    if source["instance_id"] != instance_id:
        raise QaFailure("opened Node Editor does not target the converted instance")
    placement_before = {
        key: value
        for key, value in item_before["authored_properties"].items()
        if key not in ("font_family", "font", "size", "font_size", "color")
    }
    if item_after["authored_properties"] != placement_before:
        raise QaFailure("conversion moved Timeline-owned placement Keyframes")
    for key in ("track_id", "interval", "time_map", "layer", "parent", "blend_mode"):
        if item_after[key] != item_before[key]:
            raise QaFailure("conversion changed Timeline placement field {}".format(key))
    for sibling_id, sibling in project_before["items"].items():
        if sibling_id != item_id and after["project"]["items"][sibling_id] != sibling:
            raise QaFailure("conversion mutated sibling {}".format(sibling_id))
    if tile_id in after["project"]["attachments"]:
        raise QaFailure("pre-Transform Effect was not moved into the Node Clip")
    if after["project"]["attachments"].get(blur_id) != attachments_before[blur_id]:
        raise QaFailure("post-Transform Effect was not retained unchanged outside the Module")

    definition = after["project"]["module_definitions"][definition_id]
    nodes = definition["graph"]["nodes"]
    output_nodes = [node_id for node_id, node in nodes.items() if _node_type(node) == "moduleoutput"]
    text_nodes = [
        node_id
        for node_id, node in nodes.items()
        if _node_type(node) == "generator"
        and str((node.get("content") or {}).get("data", "")).lower() == "text"
    ]
    tile_nodes = [node_id for node_id, node in nodes.items() if _plugin_component(node) == "tile"]
    fill_nodes = [node_id for node_id, node in nodes.items() if _plugin_component(node) == "fill"]
    stack_nodes = [
        node_id
        for node_id, node in nodes.items()
        if _node_type(node) == "nativeoperation"
        and ((node.get("content") or {}).get("data") or {}).get("catalog_id")
        == "native.appearance-stack"
    ]
    if not (
        len(nodes) == 5
        and len(output_nodes) == 1
        and len(text_nodes) == 1
        and tile_nodes == [tile_id]
        and len(fill_nodes) == 1
        and len(stack_nodes) == 1
        and len(definition["graph"]["connections"]) == 4
    ):
        raise QaFailure("converted topology must combine Text + Fill in one Appearance Stack")
    links = {
        (edge["from"]["node_id"], edge["from"]["port"],
         edge["to"]["node_id"], edge["to"]["port"])
        for edge in definition["graph"]["connections"]
    }
    required_links = {
        (text_nodes[0], "shape", stack_nodes[0], "shape_in"),
        (fill_nodes[0], "style", stack_nodes[0], "styles"),
        (stack_nodes[0], "image", tile_id, "image_in"),
    }
    output_links = links - required_links
    if not required_links.issubset(links) or len(output_links) != 1 or not any(
        source == tile_id and port == "image" and target == output_nodes[0]
        for source, port, target, _ in output_links
    ):
        raise QaFailure("conversion lost the typed Shape/Style boundary or Effect/Output order")

    _, canvas = client.wait_component_settled("node_editor.canvas")
    metadata = canvas.get("metadata") or {}
    if metadata.get("production_surface") != "egui_snarl":
        raise QaFailure("conversion did not open the production Node Editor")
    if metadata.get("timeline_graph_expansion") is not False:
        raise QaFailure("conversion expanded Timeline structure into Nodes")
    for node_id in nodes:
        client.wait_component("node_editor.node:" + node_id)

    rendered_after = client.wait_until(
        "converted Preview publication",
        lambda: settled_preview_state(client, revision_before + 1, expected_frame),
        timeout=30.0,
    )
    rendered_hash = rendered_after["editor"]["preview"]["pixel_hash"]
    if rendered_hash != preview_hash:
        raise QaFailure(
            "Node Clip conversion changed Preview pixels: before={}, after={}".format(
                preview_hash, rendered_hash
            )
        )

    module_edited, module_controls = _edit_converted_text_size(
        client, instance_id, definition, rendered_after
    )
    module_rendered = client.wait_until(
        "converted Text edited Preview",
        lambda: state
        if (
            (state := settled_preview_state(
                client, module_edited["history"]["revision"], expected_frame
            ))
            and state["editor"]["preview"].get("pixel_hash") != rendered_hash
        )
        else None,
        timeout=30.0,
    )

    client.key("z", True, command=True)
    client.key("z", False, command=True)
    module_undone = client.wait_until(
        "converted Text property Undo",
        lambda: state
        if (
            (state := client.state())["history"]["revision"]
            == module_edited["history"]["revision"] + 1
            and state["project"] == rendered_after["project"]
        )
        else None,
    )
    module_undo_rendered = client.wait_until(
        "converted Text Preview after property Undo",
        lambda: state
        if (
            (state := settled_preview_state(
                client, module_undone["history"]["revision"], expected_frame
            ))
            and state["editor"]["preview"].get("pixel_hash") == rendered_hash
        )
        else None,
        timeout=30.0,
    )
    appearance = _exercise_converted_appearance(
        client, item_id, instance_id, module_undo_rendered, expected_frame
    )
    ensemble = _exercise_converted_ensemble(
        client,
        item_id,
        instance_id,
        appearance["restored_state"],
        expected_frame,
    )

    client.key("z", True, command=True)
    client.key("z", False, command=True)
    undone = client.wait_until(
        "one-step conversion Undo",
        lambda: state if (state := client.state())["project"] == project_before else None,
    )
    restored_preview = client.wait_until(
        "Preview after conversion Undo",
        lambda: state
        if (
            (state := settled_preview_state(
                client, undone["history"]["revision"], expected_frame
            ))
            and state["editor"]["preview"].get("pixel_hash") == preview_hash
        )
        else None,
        timeout=30.0,
    )
    if not undone["history"]["can_redo"]:
        raise QaFailure("single Undo did not preserve the conversion as one redo step")
    return {
        "suite": "node-clip-conversion",
        "item_id": item_id,
        "definition_id": definition_id,
        "instance_id": instance_id,
        "topology": {
            "nodes": list(nodes),
            "connections": definition["graph"]["connections"],
            "output_node": output_nodes[0],
            "pre_effect_node": tile_nodes[0],
            "appearance_stack_node": stack_nodes[0],
        },
        "preview_hash": preview_hash,
        "direct_text_controls": direct_controls,
        "direct_text_content": direct_content,
        "converted_text_controls": module_controls,
        "converted_edit_preview_hash": module_rendered["editor"]["preview"]["pixel_hash"],
        "structured_appearance": {
            key: value for key, value in appearance.items() if key != "restored_state"
        },
        "structured_text_ensemble": {
            key: value for key, value in ensemble.items() if key != "restored_state"
        },
        "rendered_frame": rendered_after["editor"]["preview"]["rendered_frame"],
        "rendered_revision": rendered_after["editor"]["preview"]["rendered_revision"],
        "undo_revision": restored_preview["history"]["revision"],
        "node_editor_canvas": metadata,
        "actions": client.evidence,
    }


if __name__ == "__main__":
    raise SystemExit(
        run_suite_main(
            "qa-node-clip-conversion-e2e",
            run_suite,
            "target/qa-node-clip-conversion-e2e-evidence.json",
        )
    )
