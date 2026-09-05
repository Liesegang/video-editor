#!/usr/bin/env python3
"""Exercise one explicit Module in the production Node Editor surface."""

import copy
import math

from qa_support import (
    QaFailure,
    bring_timeline_component,
    component_center,
    component_point,
    find_clear_canvas_point,
    item_by_name,
    rendered_preview_state,
    run_suite_main,
    seek_timeline_seconds,
)


CANVAS_ID = "node_editor.canvas"


def _active_definition(state):
    document = state["editor"]["node_editor"]["document"]
    if not document or document.get("kind") != "module_definition":
        raise QaFailure("Node Editor did not open an explicit Module document")
    definition_id = document["definition_id"]
    return definition_id, state["project"]["module_definitions"][definition_id]


def _ports(snapshot, direction, node_id=None):
    result = []
    for component in snapshot["components"]:
        if component.get("type") != "node_editor_port" or not component.get("visible"):
            continue
        metadata = component.get("metadata") or {}
        if metadata.get("direction") != direction:
            continue
        if node_id is not None and metadata.get("node_id") != node_id:
            continue
        result.append(component)
    return result


def _node_content_type(node):
    content = node.get("content") or {}
    return str(content.get("type", "")).replace("_", "").lower()


def _image_port(snapshot, direction, node_id):
    return next(
        (
            port
            for port in _ports(snapshot, direction, node_id)
            if str((port.get("metadata") or {}).get("data_type", "")).lower()
            == "image"
        ),
        None,
    )


def _connection(definition, from_node, to_node):
    return next(
        (
            connection
            for connection in definition["graph"]["connections"]
            if connection["from"]["node_id"] == from_node
            and connection["to"]["node_id"] == to_node
        ),
        None,
    )


def _media_output_types(snapshot, node_id):
    return {
        str((port.get("metadata") or {}).get("data_type", "")).lower()
        for port in _ports(snapshot, "output", node_id)
        if str((port.get("metadata") or {}).get("data_type", "")).lower()
        in {"image", "audio"}
    }


def _connection_component_id(connection_id):
    return "node_editor.connection:" + connection_id


def _connection_handle_component_id(connection_id, endpoint):
    return "node_editor.connection_handle:{}:{}".format(connection_id, endpoint)


def _select_connection(client, connection_id):
    component_id = _connection_component_id(connection_id)
    _, component = client.wait_component_settled(component_id)
    metadata = component.get("metadata") or {}
    if metadata.get("connection_id") != connection_id:
        raise QaFailure("Node Editor wire QA target has the wrong connection identity")
    if metadata.get("interaction_geometry") != "node-editor-ui":
        raise QaFailure("Node Editor wire QA target did not use shared geometry")
    if metadata.get("paint_geometry") != "node-editor-ui":
        raise QaFailure("Node Editor wire paint did not use shared geometry")
    client.click_component(component_id)

    def selected():
        state = client.state()
        return (
            state
            if state["editor"]["node_editor"]["selected_connection"]
            == connection_id
            else None
        )

    client.wait_until("Module connection selection", selected)


def _verify_node_delete(client, node_id):
    before = client.state()
    before_project = before["project"]
    revision = before["history"]["revision"]
    definition_id, definition = _active_definition(before)
    removed_parameters = {
        parameter["id"] for parameter in definition["interface"]["parameters"]
        if parameter["target"]["node_id"] == node_id
    }
    instance_ids = {
        instance_id for instance_id, instance in before_project["module_instances"].items()
        if instance["definition_id"] == definition_id
    }
    expected_items = copy.deepcopy(before_project["items"])
    for item in expected_items.values():
        source = item["source"]
        if source["kind"] == "module" and source["value"]["instance_id"] in instance_ids:
            for parameter_id in removed_parameters:
                source["value"]["automation_tracks"].pop(parameter_id, None)

    def verify_dependents(state):
        _, current = _active_definition(state)
        remaining = {parameter["id"] for parameter in current["interface"]["parameters"]}
        if remaining & removed_parameters:
            raise QaFailure("Deleted Node retained a Published Parameter")
        for instance_id in instance_ids:
            overrides = state["project"]["module_instances"][instance_id]["parameter_overrides"]
            if set(overrides) & removed_parameters:
                raise QaFailure("Deleted Node retained instance parameter overrides")
        if state["project"]["items"] != expected_items:
            raise QaFailure("Node deletion changed Timeline items beyond its automation cleanup")
        if state["project"]["tracks"] != before_project["tracks"]:
            raise QaFailure("Node deletion changed Timeline tracks")
        if any(node_id in (edge["from"]["node_id"], edge["to"]["node_id"])
               for edge in current["graph"]["connections"]):
            raise QaFailure("Node deletion retained dangling connections")
    _, body = client.wait_component_settled("node_editor.node:" + node_id)
    # Exercise the body border, deliberately outside the header and controls.
    point = component_point(body, 0.5, 0.98)
    client.inject("click", {**point, "button": "secondary", "coordinate_space": "points"})
    action = "node_editor.node_menu:{}:delete".format(node_id)
    client.wait_component_settled(action)
    client.click_component(action)

    def deleted():
        state = client.state()
        _, definition = _active_definition(state)
        return state if node_id not in definition["graph"]["nodes"] else None

    removed = client.wait_until("body context menu deletes Node", deleted)
    verify_dependents(removed)
    if removed["history"]["revision"] != revision + 1:
        raise QaFailure("Node deletion was not a single undoable edit")
    client.key("z", True, command=True)
    client.key("z", False, command=True)
    client.wait_until(
        "Undo restores deleted Node and its connections",
        lambda: client.state()["project"] == before_project,
    )
    client.click_component("node_editor.node_header:" + node_id)
    client.key("delete", True)
    client.key("delete", False)
    verify_dependents(client.wait_until("Delete key removes selected Node", deleted))
    client.key("z", True, command=True)
    client.key("z", False, command=True)
    client.wait_until(
        "Undo keyboard Node deletion",
        lambda: client.state()["project"] == before_project,
    )
    return {"body_context_delete": True, "delete_key": True, "undo_exact": True,
            "removed_parameter_count": len(removed_parameters),
            "timeline_placement_preserved": True}


def _verify_batch_delete(client, node_ids, output_node_id):
    before = client.state()
    for index, node_id in enumerate([*node_ids, output_node_id]):
        _, header = client.wait_component_settled("node_editor.node_header:" + node_id)
        client.inject("click", {
            **component_center(header), "button": "primary",
            "coordinate_space": "points", "modifiers": {"shift": index > 0},
        })
    client.wait_until(
        "multiple Nodes selected including required Output",
        lambda: client.state()["editor"]["node_editor"]["selected_node_count"]
        == len(node_ids) + 1,
    )
    client.key("delete", True)
    client.key("delete", False)

    def removed():
        state = client.state()
        _, definition = _active_definition(state)
        return state if not set(node_ids) & set(definition["graph"]["nodes"]) else None

    after = client.wait_until("one batch Node deletion", removed)
    _, definition = _active_definition(after)
    if output_node_id not in definition["graph"]["nodes"]:
        raise QaFailure("Delete removed the required Output terminal")
    if after["history"]["revision"] != before["history"]["revision"] + 1:
        raise QaFailure("Multiple selected Nodes were not deleted in one transaction")
    client.key("z", True, command=True)
    client.key("z", False, command=True)
    client.wait_until("one Undo restores batch deletion",
                      lambda: client.state()["project"] == before["project"])
    return {"deleted_node_count": len(node_ids), "required_output_preserved": True,
            "one_transaction": True, "undo_exact": True}


def _verify_overview_delete(client, node_id):
    client.click_component("node_editor.node_header:" + node_id)
    client.scroll_component(CANVAS_ID, 0.0, -1200.0, modifiers={"command": True})
    _, canvas = client.wait_component_settled(CANVAS_ID)
    metadata = canvas.get("metadata") or {}
    if metadata.get("connect_enabled") is not False:
        raise QaFailure("Overview deletion QA did not reach the zoomed-out overview")
    before = client.state()
    client.key("backspace", True)
    client.key("backspace", False)
    client.wait_until(
        "Backspace deletes selected Node in overview",
        lambda: node_id not in _active_definition(client.state())[1]["graph"]["nodes"],
    )
    client.key("z", True, command=True)
    client.key("z", False, command=True)
    client.wait_until("Undo overview deletion",
                      lambda: client.state()["project"] == before["project"])
    client.scroll_component(CANVAS_ID, 0.0, 1200.0, modifiers={"command": True})
    return {"scale": metadata["scale"], "backspace": True, "undo_exact": True}


def _verify_asset_drop(client, kind, expected_outputs):
    before = client.state()
    _, definition = _active_definition(before)
    asset = next(asset for asset in before["project"]["assets"]
                 if asset["kind"].lower() == kind)
    _, source = client.wait_component_settled("assets.asset:" + asset["id"])
    destination = find_clear_canvas_point(
        client.component_snapshot(), CANVAS_ID,
        ("node_editor.node:", "node_editor.node_header:", "node_editor.port."),
    )
    client.drag(component_center(source), destination, steps=12)

    def inserted():
        state = client.state()
        _, candidate = _active_definition(state)
        nodes = set(candidate["graph"]["nodes"]) - set(definition["graph"]["nodes"])
        return (state, candidate, nodes) if len(nodes) == 1 else None

    after, candidate, nodes = client.wait_until("Asset drag creates one Media Node", inserted)
    node_id = next(iter(nodes))
    node = candidate["graph"]["nodes"][node_id]
    if _node_content_type(node) != "media" or node["content"]["data"]["asset_id"] != asset["id"]:
        raise QaFailure("Node drop did not preserve the original Asset identity")
    media = node["content"]["data"]
    if kind == "audio":
        if media.get("stream_index") is not None or media.get("audio_stream_index") != asset.get("stream_index"):
            raise QaFailure("Audio Node did not preserve its explicit audio stream identity")
    elif kind == "video":
        if media.get("stream_index") != asset.get("stream_index") or media.get("audio_stream_index") is not None:
            raise QaFailure("Video Node guessed audio or lost its explicit visual stream identity")
    elif media.get("stream_index") is not None or media.get("audio_stream_index") is not None:
        raise QaFailure("Image Node unexpectedly selected a container stream")
    if after["project"]["items"] != before["project"]["items"]:
        raise QaFailure("Node Asset drop inserted or changed a Timeline item")
    if after["project"]["assets"] != before["project"]["assets"]:
        raise QaFailure("Node Asset drop imported a duplicate Asset")
    if after["history"]["revision"] != before["history"]["revision"] + 1:
        raise QaFailure("Node Asset drop was not a single transaction")
    client.wait_until(
        kind.title() + " Media Node exposes only its selected outputs",
        lambda: _media_output_types(client.component_snapshot(), node_id)
        == set(expected_outputs),
    )
    client.key("z", True, command=True)
    client.key("z", False, command=True)
    client.wait_until("Undo Asset Node insertion",
                      lambda: client.state()["project"] == before["project"])
    return {"kind": kind, "asset_id": asset["id"], "node_id": node_id,
            "outputs": sorted(expected_outputs),
            "timeline_unchanged": True, "undo_exact": True}


def run_suite(client):
    client.wait_health()
    initial = client.state()
    node_clip = item_by_name(initial["project"], "QA Node Clip")
    seek_timeline_seconds(client, 4.5)
    bring_timeline_component(client, "timeline.item:" + node_clip["id"], -100.0)
    client.double_click_component("timeline.item:" + node_clip["id"])
    client.wait_component("dock.tab:node_editor")
    client.click_component("dock.tab:node_editor")
    _, canvas = client.wait_component_settled(CANVAS_ID)
    canvas_metadata = canvas.get("metadata") or {}
    if canvas_metadata.get("production_surface") != "egui_snarl":
        raise QaFailure("Module document did not use the production Node Editor")
    if canvas_metadata.get("timeline_graph_expansion") is not False:
        raise QaFailure("Node Editor expanded Timeline structure into Nodes")

    opened = client.state()
    definition_id, definition = _active_definition(opened)
    original_nodes = set(definition["graph"]["nodes"])
    output_nodes = {
        node_id
        for node_id, node in definition["graph"]["nodes"].items()
        if _node_content_type(node) == "moduleoutput"
    }
    if len(original_nodes) != 2 or len(output_nodes) != 1:
        raise QaFailure("fixture Module should start with one source and one Output terminal")
    output_node_id = next(iter(output_nodes))
    source_node_id = next(iter(original_nodes - output_nodes))
    if len(definition["graph"]["connections"]) != 1 or not _connection(
        definition, source_node_id, output_node_id
    ):
        raise QaFailure("fixture Module should initially route its source to Output")

    asset_drops = [
        _verify_asset_drop(client, "image", {"image"}),
        _verify_asset_drop(client, "audio", {"audio"}),
        _verify_asset_drop(client, "video", {"image"}),
    ]

    _, output_header = client.wait_component("node_editor.node_header:" + output_node_id)
    if (output_header.get("metadata") or {}).get("module_output") is not True:
        raise QaFailure("dedicated Output node was not identified by the production surface")

    snapshot = client.component_snapshot()
    menu_point = find_clear_canvas_point(
        snapshot,
        CANVAS_ID,
        ("node_editor.node:", "node_editor.node_header:", "node_editor.port."),
    )
    client.inject(
        "click",
        {**menu_point, "button": "secondary", "coordinate_space": "points"},
    )
    client.wait_component("node_editor.menu.search")
    navigation_before_menu = client.state()["editor"]["node_editor"]
    menu_navigation_point = find_clear_canvas_point(
        client.component_snapshot(), CANVAS_ID,
        ("node_editor.node:", "node_editor.node_header:", "node_editor.menu.root"),
    )
    client.inject("scroll", {**menu_navigation_point, "delta_x": 0.0, "delta_y": -60.0,
                              "modifiers": {"command": True}, "coordinate_space": "points"})
    client.drag(menu_navigation_point,
                {"x": menu_navigation_point["x"] + 30.0,
                 "y": menu_navigation_point["y"] + 12.0}, steps=10, button="middle")
    navigation_after_menu = client.state()["editor"]["node_editor"]
    for key in ("pan", "zoom"):
        if navigation_before_menu[key] != navigation_after_menu[key]:
            raise QaFailure("Node context menu allowed background " + key)
    client.click_component("node_editor.menu.search")
    client.inject("text", {"text": "diagonal clip"})
    client.wait_component_settled("node_editor.menu.create.effect:diagonal_clip")
    client.click_component("node_editor.menu.create.effect:diagonal_clip")

    def created():
        state = client.state()
        _, current = _active_definition(state)
        return state if len(current["graph"]["nodes"]) == 3 else None

    after_create = client.wait_until("a second Module Node", created)
    current_definition_id, current = _active_definition(after_create)
    new_nodes = set(current["graph"]["nodes"]) - original_nodes
    if len(new_nodes) != 1:
        raise QaFailure("Node creation did not add exactly one Module Node")
    effect_node_id = next(iter(new_nodes))
    if current_definition_id == definition_id and current == definition:
        raise QaFailure("Node creation did not update the Module definition")

    def connection_targets():
        snapshot = client.component_snapshot()
        source = _image_port(snapshot, "output", source_node_id)
        target = _image_port(snapshot, "input", effect_node_id)
        return (source, target) if source and target else None

    source, target = client.wait_until("connectable Module ports", connection_targets)
    _, source_header = client.wait_component("node_editor.node_header:" + source_node_id)
    header_point = component_center(source_header)
    _, before_wire_canvas = client.wait_component_settled(CANVAS_ID)
    gesture_scale = float((before_wire_canvas.get("metadata") or {})["scale"])
    client.inject(
        "press",
        {**header_point, "button": "primary", "coordinate_space": "points"},
    )
    # A wheel/pinch arriving while a Node owns the pointer must not change the
    # press-time transform. The same lock is consumed by port/wire gestures.
    client.inject(
        "scroll",
        {
            **header_point,
            "delta_x": 0.0,
            "delta_y": -600.0,
            "coordinate_space": "points",
            "modifiers": {"command": True},
        },
    )
    _, during_wire_canvas = client.wait_component_settled(CANVAS_ID)
    during_wire_scale = float((during_wire_canvas.get("metadata") or {})["scale"])
    if abs(during_wire_scale - gesture_scale) > 1.0e-6:
        raise QaFailure("Node Editor transform changed during a Node gesture")
    client.inject(
        "release",
        {**header_point, "button": "primary", "coordinate_space": "points"},
    )

    def original_wire_ports():
        snapshot = client.component_snapshot()
        source_port = _image_port(snapshot, "output", source_node_id)
        output_port = _image_port(snapshot, "input", output_node_id)
        return (source_port, output_port) if source_port and output_port else None

    client.wait_until(
        "original source-to-Output ports", original_wire_ports
    )
    original_connection_id = definition["graph"]["connections"][0]["id"]
    before_wire_menu = client.state()
    client.click_component(
        _connection_component_id(original_connection_id), button="secondary"
    )
    client.wait_component_settled("node_editor.wire_menu.disconnect")
    after_wire_menu = client.state()
    if after_wire_menu["project"] != before_wire_menu["project"]:
        raise QaFailure("right-clicking a wire mutated the graph before confirmation")
    if (
        after_wire_menu["editor"]["node_editor"]["selected_connection"]
        != original_connection_id
    ):
        raise QaFailure("wire context menu did not select its authoritative connection")
    client.click_component("node_editor.wire_menu.disconnect")

    def disconnected():
        state = client.state()
        _, current = _active_definition(state)
        return state if not current["graph"]["connections"] else None

    client.wait_until("source-to-Output disconnect", disconnected)
    source, target = client.wait_until("ports after frozen gesture", connection_targets)
    client.drag(component_center(source), component_center(target), steps=12)

    def source_connected_to_effect():
        state = client.state()
        _, candidate = _active_definition(state)
        return state if _connection(candidate, source_node_id, effect_node_id) else None

    client.wait_until("source connected to effect", source_connected_to_effect)

    def effect_output_targets():
        snapshot = client.component_snapshot()
        effect_output = _image_port(snapshot, "output", effect_node_id)
        module_output = _image_port(snapshot, "input", output_node_id)
        return (effect_output, module_output) if effect_output and module_output else None

    effect_output, module_output = client.wait_until(
        "effect-to-Output ports", effect_output_targets
    )
    client.drag(component_center(effect_output), component_center(module_output), steps=12)

    def completed_module_route():
        state = client.state()
        _, candidate = _active_definition(state)
        complete = (
            len(candidate["graph"]["connections"]) == 2
            and _connection(candidate, source_node_id, effect_node_id)
            and _connection(candidate, effect_node_id, output_node_id)
        )
        return state if complete else None

    after_connect = client.wait_until("source-effect-Output route", completed_module_route)
    _, routed = _active_definition(after_connect)
    source_effect = _connection(routed, source_node_id, effect_node_id)
    effect_output_connection = _connection(routed, effect_node_id, output_node_id)

    _select_connection(client, source_effect["id"])
    # The native editor accepts Delete and Backspace. The QA bridge's typed
    # key contract exposes Backspace, so exercise the same shared delete
    # interaction through that supported key instead of inventing a key name.
    client.key("backspace", True)
    client.key("backspace", False)

    def selected_wire_deleted():
        state = client.state()
        _, candidate = _active_definition(state)
        return state if not _connection(candidate, source_node_id, effect_node_id) else None

    client.wait_until("selected wire Backspace/Delete", selected_wire_deleted)
    source, target = client.wait_until(
        "ports after selected wire Backspace/Delete", connection_targets
    )
    client.drag(component_center(source), component_center(target), steps=12)
    restored_source_route = client.wait_until(
        "source route restored after Delete", source_connected_to_effect
    )
    _, restored_definition = _active_definition(restored_source_route)
    source_effect = _connection(restored_definition, source_node_id, effect_node_id)

    _, output_wire_component = client.wait_component_settled(
        _connection_component_id(effect_output_connection["id"])
    )
    output_wire_center = component_center(output_wire_component)
    client.inject(
        "drag",
        {
            "from": {
                "x": output_wire_center["x"],
                "y": output_wire_center["y"] - 28.0,
            },
            "to": {
                "x": output_wire_center["x"],
                "y": output_wire_center["y"] + 28.0,
            },
            "steps": 10,
            "button": "secondary",
            "coordinate_space": "points",
            "modifiers": {"ctrl": True},
        },
    )

    def cut_output_wire():
        state = client.state()
        _, candidate = _active_definition(state)
        return state if not _connection(candidate, effect_node_id, output_node_id) else None

    client.wait_until("Ctrl-right-drag Cut Links", cut_output_wire)
    _, effect_header = client.wait_component_settled(
        "node_editor.node_header:" + effect_node_id
    )
    _, output_header = client.wait_component_settled(
        "node_editor.node_header:" + output_node_id
    )
    client.inject(
        "drag",
        {
            "from": component_center(effect_header),
            "to": component_center(output_header),
            "steps": 12,
            "button": "secondary",
            "coordinate_space": "points",
            "modifiers": {"alt": True},
        },
    )
    restored_output_route = client.wait_until(
        "Alt-right-drag Lazy Connect", completed_module_route
    )
    _, restored_definition = _active_definition(restored_output_route)
    effect_output_connection = _connection(
        restored_definition, effect_node_id, output_node_id
    )
    source_effect_metadata = {
        key: source_effect[key] for key in ("id", "order", "blend_mode")
    }
    output_metadata = {
        key: effect_output_connection[key] for key in ("id", "order", "blend_mode")
    }

    # Add a second compatible processor so both existing-edge handles can be
    # exercised without introducing an invalid intermediate topology.
    second_snapshot = client.component_snapshot()
    second_menu_point = find_clear_canvas_point(
        second_snapshot,
        CANVAS_ID,
        (
            "node_editor.node:",
            "node_editor.node_header:",
            "node_editor.port.",
            "node_editor.connection:",
            "node_editor.connection_handle:",
        ),
    )
    client.inject(
        "click",
        {**second_menu_point, "button": "secondary", "coordinate_space": "points"},
    )
    client.wait_component("node_editor.menu.search")
    client.click_component("node_editor.menu.search")
    client.inject("text", {"text": "diagonal clip"})
    client.wait_component_settled("node_editor.menu.create.effect:diagonal_clip")
    client.click_component("node_editor.menu.create.effect:diagonal_clip")

    def second_effect_created():
        state = client.state()
        _, candidate = _active_definition(state)
        return state if len(candidate["graph"]["nodes"]) == 4 else None

    with_second_effect = client.wait_until("a second Diagonal Clip Node", second_effect_created)
    _, with_second_definition = _active_definition(with_second_effect)
    second_effect_nodes = (
        set(with_second_definition["graph"]["nodes"])
        - original_nodes
        - {effect_node_id}
    )
    if len(second_effect_nodes) != 1:
        raise QaFailure("second Diagonal Clip creation did not add exactly one Module Node")
    second_effect_node_id = next(iter(second_effect_nodes))

    def selected_target_wire_ports():
        snapshot = client.component_snapshot()
        source = _image_port(snapshot, "output", source_node_id)
        old_target = _image_port(snapshot, "input", effect_node_id)
        return (source, old_target) if source and old_target else None

    client.wait_until(
        "selected target wire ports", selected_target_wire_ports
    )
    _select_connection(client, source_effect_metadata["id"])

    def target_reconnect_ports():
        snapshot = client.component_snapshot()
        old_target = _image_port(snapshot, "input", effect_node_id)
        new_target = _image_port(snapshot, "input", second_effect_node_id)
        return (old_target, new_target) if old_target and new_target else None

    _, new_target = client.wait_until(
        "visible target reconnect ports", target_reconnect_ports
    )
    _, target_handle = client.wait_component_settled(
        _connection_handle_component_id(source_effect_metadata["id"], "target")
    )
    client.drag(component_center(target_handle), component_center(new_target), steps=12)

    def target_reconnected():
        state = client.state()
        _, candidate = _active_definition(state)
        connection = _connection(candidate, source_node_id, second_effect_node_id)
        if not connection:
            return None
        for key, expected in source_effect_metadata.items():
            if connection[key] != expected:
                raise QaFailure(
                    "target reconnect changed connection {}: {!r} != {!r}".format(
                        key, connection[key], expected
                    )
                )
        return state

    client.wait_until("target endpoint reconnect", target_reconnected)

    def source_reconnect_ports():
        snapshot = client.component_snapshot()
        old_source = _image_port(snapshot, "output", effect_node_id)
        new_source = _image_port(snapshot, "output", second_effect_node_id)
        target = _image_port(snapshot, "input", output_node_id)
        return (
            (old_source, new_source, target)
            if old_source and new_source and target
            else None
        )

    _, new_source, _ = client.wait_until(
        "source reconnect ports", source_reconnect_ports
    )
    _select_connection(client, output_metadata["id"])
    _, source_handle = client.wait_component_settled(
        _connection_handle_component_id(output_metadata["id"], "source")
    )
    client.drag(component_center(source_handle), component_center(new_source), steps=12)

    def source_reconnected():
        state = client.state()
        _, candidate = _active_definition(state)
        connection = _connection(candidate, second_effect_node_id, output_node_id)
        if not connection:
            return None
        for key, expected in output_metadata.items():
            if connection[key] != expected:
                raise QaFailure(
                    "source reconnect changed connection {}: {!r} != {!r}".format(
                        key, connection[key], expected
                    )
                )
        return state

    after_reconnect = client.wait_until("source endpoint reconnect", source_reconnected)
    _, reconnected_definition = _active_definition(after_reconnect)
    if len(reconnected_definition["graph"]["connections"]) != 2:
        raise QaFailure("endpoint reconnect duplicated or dropped an authored connection")

    # The create-menu placement is intentionally pointer-local, so the second
    # effect can be partially covered by the first one. Move that foreground
    # Node through its real visible header before targeting the lower Node's
    # compact status control; an occluded click would truthfully hit the
    # foreground body instead.
    _, occluder_header = client.wait_component_settled(
        "node_editor.node_header:" + effect_node_id
    )
    occluder_header_before = component_center(occluder_header)
    client.drag_component_by(
        "node_editor.node_header:" + effect_node_id,
        320.0,
        0.0,
        steps=12,
    )

    def target_status_is_unoccluded():
        snapshot = client.component_snapshot()
        by_id = {component["id"]: component for component in snapshot["components"]}
        occluder_header = by_id.get("node_editor.node_header:" + effect_node_id)
        target_node = by_id.get("node_editor.node:" + second_effect_node_id)
        status = by_id.get("node_editor.node_state:" + second_effect_node_id)
        if not occluder_header or not target_node or not status:
            return None
        if component_center(occluder_header)["x"] < occluder_header_before["x"] + 200.0:
            return None
        target_rect = status["rect_points"]
        for component in snapshot["components"]:
            if component.get("type") != "node_editor_node":
                continue
            if (component.get("metadata") or {}).get("node_id") == second_effect_node_id:
                continue
            rect = component["rect_points"]
            if (
                target_rect["min_x"] < rect["max_x"]
                and target_rect["max_x"] > rect["min_x"]
                and target_rect["min_y"] < rect["max_y"]
                and target_rect["max_y"] > rect["min_y"]
            ):
                return None
        return snapshot

    client.wait_until("unoccluded bypass control after header drag", target_status_is_unoccluded)

    baseline_revision = client.state()["history"]["revision"]
    baseline_render = client.wait_until(
        "active effect Preview render",
        lambda: rendered_preview_state(client, baseline_revision),
        timeout=30.0,
    )
    baseline_preview = baseline_render["editor"]["preview"]
    client.click_component("node_editor.node_state:" + second_effect_node_id)

    def header_bypassed_effect():
        state = client.state()
        _, candidate = _active_definition(state)
        node = candidate["graph"]["nodes"][second_effect_node_id]
        return (
            state
            if node.get("enabled") is True
            and node.get("bypassed") is True
            and state["history"]["revision"] == baseline_revision + 1
            else None
        )

    bypassed_state = client.wait_until("header bypass action", header_bypassed_effect)
    bypassed_render = client.wait_until(
        "bypassed effect Preview render",
        lambda: rendered_preview_state(
            client, bypassed_state["history"]["revision"]
        ),
        timeout=30.0,
    )
    bypassed_preview = bypassed_render["editor"]["preview"]
    if bypassed_preview["rendered_frame"] != baseline_preview["rendered_frame"]:
        raise QaFailure("header bypass comparison changed the Preview frame")
    if bypassed_preview["pixel_hash"] == baseline_preview["pixel_hash"]:
        raise QaFailure("header bypass changed model state but not rendered Preview pixels")

    client.click_component("node_editor.node_state:" + second_effect_node_id)

    def header_resumed_effect():
        state = client.state()
        _, candidate = _active_definition(state)
        node = candidate["graph"]["nodes"][second_effect_node_id]
        return (
            state
            if node.get("enabled") is True
            and node.get("bypassed") is False
            and state["history"]["revision"]
            == bypassed_state["history"]["revision"] + 1
            else None
        )

    resumed_state = client.wait_until("header resume action", header_resumed_effect)
    resumed_render = client.wait_until(
        "resumed effect Preview render",
        lambda: rendered_preview_state(client, resumed_state["history"]["revision"]),
        timeout=30.0,
    )
    resumed_preview = resumed_render["editor"]["preview"]
    if resumed_preview["pixel_hash"] != baseline_preview["pixel_hash"]:
        raise QaFailure("header resume did not restore the original Preview pixels")

    node_delete = _verify_node_delete(client, effect_node_id)
    published_node_delete = _verify_node_delete(client, source_node_id)
    if published_node_delete["removed_parameter_count"] == 0:
        raise QaFailure("Published Node deletion QA did not exercise any parameter dependents")
    batch_delete = _verify_batch_delete(client, [effect_node_id, source_node_id], output_node_id)
    overview_delete = _verify_overview_delete(client, effect_node_id)
    after_connect = client.state()
    project_before_navigation = after_connect["project"]
    history_before_navigation = after_connect["history"]

    _, before_pan_canvas = client.wait_component_settled(CANVAS_ID)
    before_pan_metadata = before_pan_canvas.get("metadata") or {}
    if before_pan_metadata.get("viewport_controller") != "shared":
        raise QaFailure("Node Editor did not use the shared ViewportController")
    before_pan = before_pan_metadata.get("pan") or {}
    pan_start = find_clear_canvas_point(
        client.component_snapshot(),
        CANVAS_ID,
        ("node_editor.node:", "node_editor.node_header:", "node_editor.port."),
    )
    pan_delta = {"x": 72.0, "y": 44.0}
    client.drag(
        pan_start,
        {"x": pan_start["x"] + pan_delta["x"], "y": pan_start["y"] + pan_delta["y"]},
        steps=8,
        button="middle",
    )
    _, after_pan_canvas = client.wait_component_settled(CANVAS_ID)
    after_pan = (after_pan_canvas.get("metadata") or {}).get("pan") or {}
    applied_pan = {
        "x": float(after_pan["x"]) - float(before_pan["x"]),
        "y": float(after_pan["y"]) - float(before_pan["y"]),
    }
    if abs(applied_pan["x"] - pan_delta["x"]) > 2.0 or abs(
        applied_pan["y"] - pan_delta["y"]
    ) > 2.0:
        raise QaFailure(
            "Node Editor pan was ignored or applied more than once: {!r}".format(applied_pan)
        )

    _, before_canvas = client.wait_component_settled(CANVAS_ID)
    scale_before = float((before_canvas.get("metadata") or {})["scale"])
    if not math.isfinite(scale_before) or scale_before <= 0:
        raise QaFailure("Node Editor published an invalid initial zoom")

    observed_scales = [scale_before]
    # Exercise the formerly freezing extreme zoom path repeatedly. Every
    # injected step must publish a fresh frame and remain health-responsive.
    for delta in (-1200.0, -1200.0, -1200.0, 1200.0, 1200.0):
        client.scroll_component(CANVAS_ID, 0.0, delta, modifiers={"command": True})
        _, candidate = client.wait_component_settled(CANVAS_ID)
        scale = float((candidate.get("metadata") or {})["scale"])
        if not math.isfinite(scale) or scale <= 0:
            raise QaFailure("Node Editor zoom became non-finite")
        observed_scales.append(scale)
        if client.request("/health").get("ok") is not True:
            raise QaFailure("Node Editor zoom made the app unresponsive")
    if len(set(observed_scales)) < 2:
        raise QaFailure("Node Editor ignored every zoom gesture")

    final = client.state()
    if (
        final["project"] != project_before_navigation
        or final["history"] != history_before_navigation
    ):
        raise QaFailure("Node Editor navigation mutated the Module or undo history")
    return {
        "suite": "node-editor",
        "item_id": node_clip["id"],
        "definition_id": current_definition_id,
        "created_node_id": effect_node_id,
        "reconnect_node_id": second_effect_node_id,
        "connection_count": 2,
        "reconnected_connection_ids": [
            source_effect_metadata["id"],
            output_metadata["id"],
        ],
        "wire_actions": {
            "context_menu_disconnect": True,
            "selected_delete": True,
            "ctrl_right_cut": True,
            "alt_right_lazy_connect": True,
        },
        "asset_drops": asset_drops,
        "node_actions": node_delete,
        "published_node_actions": published_node_delete,
        "batch_delete": batch_delete,
        "overview_delete": overview_delete,
        "header_bypass_pixel_hashes": {
            "active": baseline_preview["pixel_hash"],
            "bypassed": bypassed_preview["pixel_hash"],
            "resumed": resumed_preview["pixel_hash"],
        },
        "direct_gesture_scale": gesture_scale,
        "pan_delta": applied_pan,
        "zoom_scales": observed_scales,
        "canvas": canvas_metadata,
        "history": final["history"],
        "actions": client.evidence,
    }


if __name__ == "__main__":
    raise SystemExit(
        run_suite_main(
            "qa-node-editor-e2e",
            run_suite,
            "target/qa-node-editor-e2e-evidence.json",
        )
    )
