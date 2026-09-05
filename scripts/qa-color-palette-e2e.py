#!/usr/bin/env python3
"""Exercise the Project-owned Solid Palette through the shared color picker."""

from qa_support import (
    QaFailure,
    activate_dock_tab,
    component_center,
    component_point,
    find_clear_canvas_point,
    item_by_name,
    rendered_preview_state,
    run_suite_main,
    seek_timeline_seconds,
)


INSPECTOR_TAB_ID = "dock.tab:inspector"
TIMELINE_TAB_ID = "dock.tab:timeline"
NODE_EDITOR_TAB_ID = "dock.tab:node_editor"
NODE_EDITOR_CANVAS_ID = "node_editor.canvas"
NODE_CLIP_NAME = "QA Node Clip"


def _ensure_timeline(client):
    component_ids = {
        component["id"] for component in client.component_snapshot()["components"]
    }
    if "timeline.ruler" in component_ids:
        return
    activate_dock_tab(client, TIMELINE_TAB_ID, "Timeline", "Color Palette")
    client.wait_component_settled("timeline.ruler")


def _module_color_contract(project, item):
    module = item["source"]["value"]
    instance_id = module["instance_id"]
    instance = project["module_instances"][instance_id]
    definition = project["module_definitions"][instance["definition_id"]]
    parameters = definition["interface"]["parameters"]
    if len(parameters) != 1 or parameters[0]["name"] != "Color":
        raise QaFailure("QA Node Clip does not expose exactly one Color parameter")
    parameter_id = parameters[0]["id"]
    track = module["automation_tracks"].get(parameter_id)
    if track is None or len(track["keyframes"]) != 2:
        raise QaFailure("QA Node Clip Color does not have the expected two keys")
    keys = sorted(
        track["keyframes"],
        key=lambda keyframe: int(keyframe["time"]["value"])
        / int(keyframe["time"]["timescale"]),
    )
    control_id = "inspector.property:module_instance:{}:{}".format(
        instance_id, parameter_id
    )
    return instance_id, parameter_id, control_id, keys


def _palette(project):
    palette = project.get("palette")
    if palette is None:
        raise QaFailure("Project does not serialize its Palette")
    return palette


def _active_module_definition(state):
    document = state["editor"]["node_editor"]["document"]
    if document is None or document.get("kind") != "module_definition":
        raise QaFailure("Node Editor did not open a ModuleDefinition document")
    definition_id = document["definition_id"]
    definition = state["project"]["module_definitions"].get(definition_id)
    if definition is None:
        raise QaFailure("Node Editor document has no authoritative ModuleDefinition")
    return document, definition_id, definition


def _is_solid_node(node):
    content = node.get("content") or {}
    return (
        str(content.get("type", "")).lower() == "generator"
        and str(content.get("data", "")).lower() == "solid"
    )


def _node_property_value(definition, node_id, property_name):
    node = definition["graph"]["nodes"].get(node_id)
    if node is None:
        raise QaFailure("ModuleDefinition lost Node {}".format(node_id))
    property_value = node.get("properties", {}).get(property_name)
    if property_value is None:
        raise QaFailure(
            "Module Node {} has no {!r} property".format(node_id, property_name)
        )
    if property_value.get("type") != "constant":
        raise QaFailure("Palette Node fixture property is no longer constant")
    return property_value.get("properties", {}).get("value")


def _timeline_structure(project):
    return {
        "root_timeline_id": project["root_timeline_id"],
        "timelines": project["timelines"],
        "tracks": project["tracks"],
        "items": project["items"],
    }


def _component(snapshot, component_id):
    return next(
        (
            component
            for component in snapshot["components"]
            if component.get("id") == component_id
        ),
        None,
    )


def _place_node_for_inline_edit(client, node_id, component_id):
    """Expose an authored Node control through the production header drag."""
    snapshot = client.component_snapshot()
    component = _component(snapshot, component_id)
    if component is not None and component.get("visible"):
        return component

    _, header = client.wait_component_settled("node_editor.node_header:" + node_id)
    _, canvas = client.wait_component_settled(NODE_EDITOR_CANVAS_ID)
    bounds = canvas["rect_points"]
    client.drag(
        component_center(header),
        {
            "x": float(bounds["center_x"]),
            "y": float(bounds["min_y"]) + 42.0,
        },
        steps=10,
    )
    return client.wait_until(
        "visible Node Palette property " + component_id,
        lambda: candidate
        if (
            (candidate := _component(client.component_snapshot(), component_id))
            is not None
            and candidate.get("visible")
        )
        else None,
        timeout=5.0,
    )


def _wait_palette_count(client, count, revision):
    def ready():
        state = client.state()
        palette = _palette(state["project"])
        if (
            state["history"]["revision"] == revision
            and len(palette["definitions"]) == count
            and len(palette["ungrouped_order"]) == count
        ):
            return state
        return None

    return client.wait_until("Palette count {}".format(count), ready)


def _wait_preview_hash(
    client, revision, expected_frame, expected_hash=None, different_from=None
):
    def ready():
        state = rendered_preview_state(client, revision)
        if state is None:
            return None
        pixel_hash = state["editor"]["preview"].get("pixel_hash")
        if state["editor"]["preview"].get("rendered_frame") != expected_frame:
            return None
        if expected_hash is not None and pixel_hash != expected_hash:
            return None
        if different_from is not None and pixel_hash == different_from:
            return None
        return state

    return client.wait_until("Preview revision {}".format(revision), ready, timeout=30.0)


def _open_palette(client, color_control):
    client.click_component(color_control)
    client.click_component("color_picker.tab.palette")
    return client.wait_component_settled("color_picker.palette.add_current")


def _close_picker(client, color_control):
    client.click_component(color_control)

    def closed():
        snapshot = client.component_snapshot()
        is_open = any(
            component.get("id") == "color_picker.palette.add_current"
            for component in snapshot["components"]
        )
        return None if is_open else snapshot

    return client.wait_until(
        "closed Paint picker",
        closed,
    )


def _add_current(client, before, expected_color, expected_count):
    client.click_component("color_picker.palette.add_current")
    state = _wait_palette_count(
        client, expected_count, before["history"]["revision"] + 1
    )
    palette = _palette(state["project"])
    definition_id = palette["ungrouped_order"][-1]
    definition = palette["definitions"][definition_id]
    if definition["paint"] != {"kind": "solid", "value": expected_color}:
        raise QaFailure("Add Current did not preserve the managed ColorValue")
    return state, definition_id


def _rename(client, definition_id, name, expected_revision):
    client.click_component(
        "color_picker.palette.swatch:" + definition_id, button="secondary"
    )
    field = "color_picker.palette.rename_name:" + definition_id
    client.click_component(field)
    client.key("a", True, command=True)
    client.key("a", False, command=True)
    client.inject("text", {"text": name})
    client.click_component("color_picker.palette.rename:" + definition_id)

    def renamed():
        state = client.state()
        definition = _palette(state["project"])["definitions"].get(definition_id)
        if (
            state["history"]["revision"] == expected_revision
            and definition
            and definition["name"] == name
        ):
            return state
        return None

    return client.wait_until("renamed Palette swatch", renamed)


def _reorder_before(client, moved_id, target_id, expected_revision):
    _, moved = client.wait_component_settled(
        "color_picker.palette.swatch:" + moved_id
    )
    _, target = client.wait_component_settled(
        "color_picker.palette.swatch:" + target_id
    )
    client.drag(
        component_point(moved, 0.5, 0.45),
        component_point(target, 0.15, 0.45),
        steps=14,
    )

    def reordered():
        state = client.state()
        if (
            state["history"]["revision"] == expected_revision
            and _palette(state["project"])["ungrouped_order"]
            == [moved_id, target_id]
        ):
            return state
        return None

    return client.wait_until("drag-reordered Palette swatches", reordered)


def _key_value(project, item_id, parameter_id, key_index):
    track = project["items"][item_id]["source"]["value"]["automation_tracks"][
        parameter_id
    ]
    keys = sorted(
        track["keyframes"],
        key=lambda keyframe: int(keyframe["time"]["value"])
        / int(keyframe["time"]["timescale"]),
    )
    return keys[key_index]["value"]


def _exercise_picker_drag_isolation(client, property_id, edited_node_id):
    """Place a real Node header under the picker, then drag across its edge."""
    client.click_component("color_picker.tab.picker")
    _, canvas = client.wait_component_settled(NODE_EDITOR_CANVAS_ID)
    canvas_rect = canvas["rect_points"]
    field_id = None
    start = None
    for candidate in ("color_picker.saturation_value", "color_picker.hue", "color_picker.alpha"):
        _, field = client.wait_component_settled(candidate)
        field_rect = field["rect_points"]
        left = max(float(field_rect["min_x"]), float(canvas_rect["min_x"])) + 4.0
        right = min(float(field_rect["max_x"]), float(canvas_rect["max_x"])) - 4.0
        top = max(float(field_rect["min_y"]), float(canvas_rect["min_y"])) + 4.0
        bottom = min(float(field_rect["max_y"]), float(canvas_rect["max_y"])) - 4.0
        if right > left and bottom > top:
            field_id = candidate
            start = {"x": (left + right) / 2.0, "y": (top + bottom) / 2.0}
            break
    if field_id is None:
        raise QaFailure("picker does not overlap the Node canvas for drag isolation QA")
    client.click_component(property_id)
    client.wait_until(
        "closed Picker before placing the underlying Node",
        lambda: not _component(client.component_snapshot(), "color_picker.saturation_value"),
    )
    _, _, definition = _active_module_definition(client.state())
    underlying_id = next(
        node_id for node_id, node in definition["graph"]["nodes"].items()
        if str((node.get("content") or {}).get("type", "")).replace("_", "").lower()
        == "moduleoutput"
    )
    header_id = "node_editor.node_header:" + underlying_id
    _, header = client.wait_component_settled(header_id)
    client.drag(component_center(header), start, steps=12)
    _, placed = client.wait_component_settled(header_id)
    placed_rect = placed["rect_points"]
    if not (float(placed_rect["min_x"]) < start["x"] < float(placed_rect["max_x"])
            and float(placed_rect["min_y"]) < start["y"] < float(placed_rect["max_y"])):
        raise QaFailure("underlying Node header did not reach the Picker drag origin")
    client.click_component(property_id)
    _, field = client.wait_component_settled(field_id)
    before = client.state()
    _, definition_id, definition = _active_module_definition(before)
    positions = {
        node_id: node["ui_position"] for node_id, node in definition["graph"]["nodes"].items()
    }
    before_color = _node_property_value(definition, edited_node_id, "color")
    before_editor = before["editor"]["node_editor"]
    # Begin precisely over the underlying header and continue outside the
    # picker field. The slider owns this entire gesture, including its tail.
    end = {"x": float(field["rect_points"]["min_x"]) - 45.0,
           "y": start["y"] - 16.0}
    client.drag(start, end, steps=16)
    client.scroll_component(field_id, 0.0, -60.0,
                            modifiers={"command": True})
    during = client.state()
    during_definition = during["project"]["module_definitions"][definition_id]
    actual_positions = {
        node_id: node["ui_position"]
        for node_id, node in during_definition["graph"]["nodes"].items()
    }
    if actual_positions != positions:
        raise QaFailure("color picker drag moved a Node behind the popup")
    for key in ("pan", "zoom", "selected_node_count", "selected_connection"):
        if during["editor"]["node_editor"][key] != before_editor[key]:
            raise QaFailure("color picker input leaked into Node Editor " + key)
    if during["editor"]["node_editor"]["gesture_active"]:
        raise QaFailure("color picker drag left a background graph gesture active")
    client.click_component(property_id)

    def committed():
        state = client.state()
        current_definition = state["project"]["module_definitions"][definition_id]
        color = _node_property_value(current_definition, edited_node_id, "color")
        return state if color != before_color else None

    after = client.wait_until("color edit committed without moving background Nodes", committed)
    after_definition = after["project"]["module_definitions"][definition_id]
    if {node_id: node["ui_position"] for node_id, node in
            after_definition["graph"]["nodes"].items()} != positions:
        raise QaFailure("closing the picker committed an unintended Node move")
    # Also dismiss by clicking a different Node, not the original color
    # button. That click belongs to the popup, including batched press/release.
    other_id = next(node_id for node_id in positions
                    if node_id not in (edited_node_id, underlying_id))
    client.click_component(property_id)
    client.wait_component_settled(field_id)

    def selected_headers():
        return sorted(component["id"] for component in client.component_snapshot()["components"]
                      if component["id"].startswith("node_editor.node_header:")
                      and (component.get("metadata") or {}).get("selected"))

    selection_before_close = selected_headers()
    client.click_component("node_editor.node_header:" + other_id)
    client.wait_until("picker dismissed by outside Node click",
                      lambda: not _component(client.component_snapshot(), field_id))
    if selected_headers() != selection_before_close:
        raise QaFailure("click-to-dismiss selected a different Node behind the picker")
    return {"underlying_node_id": underlying_id, "field_id": field_id,
            "start": start, "end": end,
            "node_positions_unchanged": True, "canvas_unchanged": True,
            "outside_dismissal_preserved_selection": True,
            "color_before": before_color,
            "color_after": _node_property_value(after_definition, edited_node_id, "color")}


def _exercise_node_editor_palette(
    client,
    before,
    item_id,
    instance_id,
    parameter_id,
    first_id,
    second_id,
    expected_frame,
    expected_hash,
):
    project_before = before["project"]
    definition_id = project_before["module_instances"][instance_id]["definition_id"]
    definition_before = project_before["module_definitions"][definition_id]
    nodes_before = definition_before["graph"]["nodes"]
    source_node_id = next(
        parameter["target"]["node_id"]
        for parameter in definition_before["interface"]["parameters"]
        if parameter["id"] == parameter_id
    )
    source_node = nodes_before.get(source_node_id)
    if source_node is None or not _is_solid_node(source_node):
        raise QaFailure("QA Node Clip published Color does not target its Solid source")
    _node_property_value(definition_before, source_node_id, "color")

    palette_before = _palette(project_before)
    timeline_before = _timeline_structure(project_before)
    instances_before = project_before["module_instances"]
    interface_before = definition_before["interface"]
    definition_ids_before = set(project_before["module_definitions"])

    _close_picker(
        client,
        "inspector.property:module_instance:{}:{}".format(
            instance_id, parameter_id
        ),
    )
    _ensure_timeline(client)
    client.double_click_component("timeline.item:" + item_id)
    client.wait_component(NODE_EDITOR_TAB_ID)
    client.click_component(NODE_EDITOR_TAB_ID)
    _, canvas = client.wait_component_settled(NODE_EDITOR_CANVAS_ID)
    canvas_metadata = canvas.get("metadata") or {}
    if canvas_metadata.get("production_surface") != "egui_snarl":
        raise QaFailure("Palette QA did not use the production Node Editor surface")
    if canvas_metadata.get("timeline_graph_expansion") is not False:
        raise QaFailure("opening a Node Clip expanded Timeline structure into Nodes")

    opened = client.state()
    document, opened_definition_id, opened_definition = _active_module_definition(opened)
    if opened_definition_id != definition_id:
        raise QaFailure("Node Editor opened a different ModuleDefinition")
    if document.get("instance_id") != instance_id:
        raise QaFailure("Node Editor lost the concrete Node Clip instance")
    if opened["history"] != before["history"] or opened["project"] != project_before:
        raise QaFailure("opening the Node Editor mutated the Project")
    if len(opened_definition["graph"]["nodes"]) != len(nodes_before):
        raise QaFailure("opening the Node Editor synthesized Timeline Nodes")

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
    client.inject("text", {"text": "solid"})
    client.wait_component_settled("node_editor.menu.create.solid")
    client.click_component("node_editor.menu.create.solid")

    created_revision = before["history"]["revision"] + 1

    def created():
        state = client.state()
        _, active_definition_id, definition = _active_module_definition(state)
        if (
            state["history"]["revision"] == created_revision
            and active_definition_id == definition_id
            and len(definition["graph"]["nodes"]) == len(nodes_before) + 1
        ):
            return state, definition
        return None

    created_state, created_definition = client.wait_until(
        "one unpublished Solid Color Node", created
    )
    new_node_ids = set(created_definition["graph"]["nodes"]) - set(nodes_before)
    if len(new_node_ids) != 1:
        raise QaFailure("Solid creation did not add exactly one Module Node")
    node_id = next(iter(new_node_ids))
    if not _is_solid_node(created_definition["graph"]["nodes"][node_id]):
        raise QaFailure("Node Editor Solid menu created the wrong Node type")
    if created_definition["topology_revision"] != definition_before["topology_revision"] + 1:
        raise QaFailure("Solid creation did not bump only one topology revision")
    if _timeline_structure(created_state["project"]) != timeline_before:
        raise QaFailure("adding a Module Node changed Timeline structure")
    if created_state["project"]["module_instances"] != instances_before:
        raise QaFailure("adding a private Module Node changed ModuleInstance ownership")
    if created_definition["interface"] != interface_before:
        raise QaFailure("adding an unpublished Node changed the Published Interface")
    if set(created_state["project"]["module_definitions"]) != definition_ids_before:
        raise QaFailure("adding to a private Module created a parallel definition")
    if _palette(created_state["project"]) != palette_before:
        raise QaFailure("adding a Module Node changed Project Palette data")
    _wait_preview_hash(
        client,
        created_revision,
        expected_frame,
        expected_hash=expected_hash,
    )

    property_id = "node_editor.property.node:{}:color".format(node_id)
    _place_node_for_inline_edit(client, node_id, property_id)
    initial_color = _node_property_value(created_definition, node_id, "color")
    candidates = [
        (definition_id, palette_before["definitions"][definition_id]["paint"]["value"])
        for definition_id in (second_id, first_id)
    ]
    paint_id, expected_color = next(
        (
            (candidate_id, candidate_color)
            for candidate_id, candidate_color in candidates
            if candidate_color != initial_color
        ),
        (None, None),
    )
    if paint_id is None:
        raise QaFailure("Palette has no swatch different from the Solid default")

    _open_palette(client, property_id)
    for index, paint_definition_id in enumerate(palette_before["ungrouped_order"]):
        _, swatch = client.wait_component_settled(
            "color_picker.palette.swatch:" + paint_definition_id
        )
        metadata = swatch.get("metadata") or {}
        if (
            metadata.get("paint_definition_id") != paint_definition_id
            or metadata.get("index") != index
            or metadata.get("action") != "apply_copy"
        ):
            raise QaFailure("Node Editor Palette swatch metadata is not authoritative")

    client.click_component("color_picker.palette.swatch:" + paint_id)
    applied_revision = created_revision + 1

    def applied():
        state = client.state()
        _, active_definition_id, definition = _active_module_definition(state)
        if (
            state["history"]["revision"] == applied_revision
            and active_definition_id == definition_id
            and _node_property_value(definition, node_id, "color") == expected_color
        ):
            return state, definition
        return None

    try:
        applied_state, applied_definition = client.wait_until(
            "lossless Node Editor Palette copy application", applied
        )
    except QaFailure as error:
        failed = client.state()
        _, failed_definition_id, failed_definition = _active_module_definition(failed)
        raise QaFailure(
            "{}; revision={!r}; definition_id={!r}; node_color={!r}; "
            "expected={!r}; editor_error={!r}".format(
                error,
                failed["history"]["revision"],
                failed_definition_id,
                _node_property_value(failed_definition, node_id, "color"),
                expected_color,
                failed["editor"].get("error"),
            )
        ) from error
    if applied_definition["topology_revision"] != created_definition["topology_revision"] + 1:
        raise QaFailure("Node Palette application did not bump one topology revision")
    if _timeline_structure(applied_state["project"]) != timeline_before:
        raise QaFailure("Node Palette application changed Timeline structure")
    if applied_state["project"]["module_instances"] != instances_before:
        raise QaFailure("Node Palette application escaped its private ModuleDefinition")
    if applied_definition["interface"] != interface_before:
        raise QaFailure("Node Palette application changed the Published Interface")
    if _palette(applied_state["project"]) != palette_before:
        raise QaFailure("copy-applying a swatch mutated Project Palette data")
    _wait_preview_hash(
        client,
        applied_revision,
        expected_frame,
        expected_hash=expected_hash,
    )

    picker_drag_isolation = _exercise_picker_drag_isolation(client, property_id, node_id)

    return {
        "definition_id": definition_id,
        "instance_id": instance_id,
        "published_source_node_id": source_node_id,
        "authored_solid_node_id": node_id,
        "paint_definition_id": paint_id,
        "color": expected_color,
        "created_revision": created_revision,
        "applied_revision": applied_revision,
        "topology_revision": applied_definition["topology_revision"],
        "canvas": canvas_metadata,
        "picker_drag_isolation": picker_drag_isolation,
    }


def run_suite(client):
    client.wait_health()
    _ensure_timeline(client)
    initial = client.state()
    if _palette(initial["project"])["definitions"]:
        raise QaFailure("QA Project Palette must start empty")

    item = item_by_name(initial["project"], NODE_CLIP_NAME)
    item_id = item["id"]
    instance_id, parameter_id, color_control, keys = _module_color_contract(
        initial["project"], item
    )
    first_color = keys[0]["value"]
    second_color = keys[1]["value"]

    client.click_component("timeline.item:" + item_id)
    first_frame = seek_timeline_seconds(client, 4.0)
    activate_dock_tab(client, INSPECTOR_TAB_ID, "Inspector", "Color Palette")
    first_render = _wait_preview_hash(
        client, first_frame["history"]["revision"], 120
    )
    first_hash = first_render["editor"]["preview"]["pixel_hash"]
    _open_palette(client, color_control)

    before_first_add = client.state()
    first_added, first_id = _add_current(
        client, before_first_add, first_color, 1
    )
    _wait_preview_hash(
        client, first_added["history"]["revision"], 120, expected_hash=first_hash
    )

    _close_picker(client, color_control)
    _ensure_timeline(client)
    second_frame = seek_timeline_seconds(client, 6.0)
    activate_dock_tab(client, INSPECTOR_TAB_ID, "Inspector", "Color Palette")
    second_render = _wait_preview_hash(
        client,
        second_frame["history"]["revision"],
        180,
        different_from=first_hash,
    )
    second_hash = second_render["editor"]["preview"]["pixel_hash"]
    _open_palette(client, color_control)

    before_second_add = client.state()
    second_added, second_id = _add_current(
        client, before_second_add, second_color, 2
    )
    _wait_preview_hash(
        client, second_added["history"]["revision"], 180, expected_hash=second_hash
    )

    renamed = _rename(
        client,
        first_id,
        "Brand Red",
        second_added["history"]["revision"] + 1,
    )
    _wait_preview_hash(
        client, renamed["history"]["revision"], 180, expected_hash=second_hash
    )

    reordered = _reorder_before(
        client,
        second_id,
        first_id,
        renamed["history"]["revision"] + 1,
    )
    _wait_preview_hash(
        client, reordered["history"]["revision"], 180, expected_hash=second_hash
    )

    # Copy-apply the original first color to the second key. This is the only
    # Palette gesture in the scenario that edits the selected property.
    client.click_component("color_picker.palette.swatch:" + first_id)
    applied_revision = reordered["history"]["revision"] + 1

    def applied():
        state = client.state()
        if (
            state["history"]["revision"] == applied_revision
            and _key_value(state["project"], item_id, parameter_id, 1) == first_color
        ):
            return state
        return None

    applied_state = client.wait_until("lossless Palette copy application", applied)
    applied_render = _wait_preview_hash(
        client, applied_revision, 180, different_from=second_hash
    )
    applied_hash = applied_render["editor"]["preview"]["pixel_hash"]

    # Deleting a swatch changes only Project Palette data, never the already
    # copied property or the rendered frame.
    client.click_component(
        "color_picker.palette.swatch:" + second_id, button="secondary"
    )
    client.click_component("color_picker.palette.delete:" + second_id)
    deleted = _wait_palette_count(client, 1, applied_revision + 1)
    if _key_value(deleted["project"], item_id, parameter_id, 1) != first_color:
        raise QaFailure("deleting a swatch changed an authored property")
    _wait_preview_hash(
        client, deleted["history"]["revision"], 180, expected_hash=applied_hash
    )

    client.key("z", True, command=True)
    client.key("z", False, command=True)
    restored = _wait_palette_count(
        client, 2, deleted["history"]["revision"] + 1
    )
    if restored["history"].get("can_redo") is not True:
        raise QaFailure("Palette delete was not restored by one Undo")
    _wait_preview_hash(
        client, restored["history"]["revision"], 180, expected_hash=applied_hash
    )

    node_editor = _exercise_node_editor_palette(
        client,
        restored,
        item_id,
        instance_id,
        parameter_id,
        first_id,
        second_id,
        180,
        applied_hash,
    )
    final = client.state()

    return {
        "suite": "color-palette",
        "item_id": item_id,
        "published_parameter_id": parameter_id,
        "paint_definition_ids": [first_id, second_id],
        "order": _palette(restored["project"])["ungrouped_order"],
        "pixel_hashes": {
            "first": first_hash,
            "second": second_hash,
            "applied": applied_hash,
        },
        "node_editor": node_editor,
        "history": final["history"],
        "actions": client.evidence,
    }


if __name__ == "__main__":
    raise SystemExit(
        run_suite_main(
            "qa-color-palette-e2e",
            run_suite,
            "target/qa-color-palette-e2e-evidence.json",
        )
    )
