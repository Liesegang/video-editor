#!/usr/bin/env python3
"""Copy and paste a connected Module selection through the native clipboard."""

import copy
import os
import pathlib

from qa_automation_support import (
    assert_module_instance_unchanged,
    history_shortcut,
    seek_rendered,
)
from qa_node_module_support import (
    active_definition,
    connect_image_nodes,
    connection,
    create_node_from_menu,
    insert_image_opacity_in_primary_route,
    place_node_for_inline_edit,
    port,
    select_nodes,
)
from qa_property_gesture_support import (
    begin_reserved_keyframe_scrub,
    release_property_scrub,
)
from qa_support import (
    QaClient,
    QaFailure,
    activate_dock_tab,
    bring_timeline_component,
    capture_viewport,
    close_clean_native_app,
    component_center,
    component_point,
    find_clear_canvas_point,
    free_port,
    item_by_name,
    media_seconds,
    request_clean_native_close,
    run_suite_main,
    save_project_to_disk,
    spawned_authoring_app,
)


def _invocation(project, item_id):
    return project["items"][item_id]["source"]["value"]


def _node_parameter(definition, node_id):
    parameters = [
        parameter
        for parameter in definition["interface"]["parameters"]
        if parameter["target"]["node_id"] == node_id
    ]
    if len(parameters) != 1:
        raise QaFailure(
            "expected exactly one copied opacity parameter for Node {}, found {}".format(
                node_id, len(parameters)
            )
        )
    return parameters[0]


def _parameter_track(project, item_id, parameter_id):
    return _invocation(project, item_id).get("automation_tracks", {}).get(parameter_id)


def _without_identity(value, *keys):
    normalized = copy.deepcopy(value)
    for key in keys:
        normalized.pop(key, None)
    return normalized


def _node_payload(node):
    return _without_identity(node, "id", "ui_position")


def _connection_payload(value):
    return _without_identity(value, "id", "from", "to")


def _parameter_payload(parameter):
    normalized = _without_identity(parameter, "id")
    normalized["target"] = dict(normalized["target"])
    normalized["target"].pop("node_id", None)
    return normalized


def _track_payload(track):
    normalized = copy.deepcopy(track)
    for keyframe in normalized.get("keyframes", []):
        keyframe.pop("id", None)
    return normalized


def _disconnect(client, connection_id, description):
    client.click_component(
        "node_editor.connection:" + connection_id, button="secondary"
    )
    client.wait_component_settled("node_editor.wire_menu.disconnect")
    client.click_component("node_editor.wire_menu.disconnect")

    def disconnected():
        state = client.state()
        connections = active_definition(state)[1]["graph"]["connections"]
        return (
            state
            if all(candidate["id"] != connection_id for candidate in connections)
            else None
        )

    client.wait_until(
        description,
        disconnected,
    )


def _create_connected_pair(client, item_id):
    bring_timeline_component(client, "timeline.item:" + item_id, -120.0)
    client.double_click_component("timeline.item:" + item_id)
    client.wait_component_settled("node_editor.canvas")
    first = insert_image_opacity_in_primary_route(client, "node_clip")
    definition, second = create_node_from_menu(
        client,
        "node_clip",
        "Image Opacity",
        "node_editor.menu.create.image_opacity",
    )
    output = next(
        node_id
        for node_id, node in definition["graph"]["nodes"].items()
        if str((node.get("content") or {}).get("type", "")).replace("_", "").lower()
        == "moduleoutput"
    )
    outgoing = connection(definition, first, output)
    if outgoing is None:
        raise QaFailure("first Image Opacity Node is not connected to Output")
    _disconnect(client, outgoing["id"], "disconnect first Image Opacity from Output")
    connect_image_nodes(client, first, second, "first to second Image Opacity")
    connect_image_nodes(client, second, output, "second Image Opacity to Output")
    return first, second


def _duplicate_item(client, item_id):
    before = client.state()
    activate_dock_tab(client, "dock.tab:timeline", "Timeline", "Node clipboard sibling")
    bring_timeline_component(client, "timeline.item:" + item_id, -120.0)
    client.click_component("timeline.item:" + item_id, button="secondary")
    action = "timeline.item.duplicate:" + item_id
    client.wait_component_settled(action)
    client.click_component(action)

    def duplicated():
        state = client.state()
        added = set(state["project"]["items"]) - set(before["project"]["items"])
        return (state, next(iter(added))) if len(added) == 1 else None

    return before, client.wait_until("one sibling Node Clip", duplicated)


def _publish_animated_first(client, item_id, node_id, start):
    seek_rendered(client, start + 0.5, "Node clipboard")
    bring_timeline_component(client, "timeline.item:" + item_id, -120.0)
    client.double_click_component("timeline.item:" + item_id)
    mode_id = "node_editor.property_mode.node:{}:opacity".format(node_id)
    control_id = "node_editor.property.node:{}:opacity".format(node_id)
    place_node_for_inline_edit(client, node_id, control_id)
    before = client.state()
    client.click_component(mode_id)
    client.wait_component_settled(mode_id + ".option:keyframe")
    client.click_component(mode_id + ".option:keyframe")

    def first_key():
        state = client.state()
        _, definition = active_definition(state, "node_clip")
        parameter = _node_parameter(definition, node_id)
        track = _parameter_track(state["project"], item_id, parameter["id"])
        return (
            (state, parameter, track)
            if track and len(track["keyframes"]) == 1
            else None
        )

    published, parameter, _ = client.wait_until(
        "publish first opacity key in one command", first_key
    )
    if published["history"]["revision"] != before["history"]["revision"] + 1:
        raise QaFailure("first opacity key publication was not one transaction")
    seek_rendered(client, start + 2.0, "Node clipboard")
    activate_dock_tab(
        client, "dock.tab:node_editor", "Node Editor", "copied Node animation"
    )
    place_node_for_inline_edit(client, node_id, control_id)
    reservation = begin_reserved_keyframe_scrub(
        client,
        control_id,
        control_id,
        -28.0,
        "copied Node animation",
        control_locator=lambda active, identity: place_node_for_inline_edit(
            active, node_id, identity
        ),
    )
    release_property_scrub(client, reservation)
    updated = client.wait_until(
        "second opacity key",
        lambda: (
            state
            if len(
                (
                    _parameter_track(
                        (state := client.state())["project"], item_id, parameter["id"]
                    )
                    or {}
                ).get("keyframes", [])
            )
            == 2
            else None
        ),
    )
    return updated, parameter


def _publish_constant_second(client, item_id, node_id):
    control_id = "node_editor.property.node:{}:opacity".format(node_id)
    place_node_for_inline_edit(client, node_id, control_id)
    snapshot = client.component_snapshot()
    opacity_port = port(snapshot, "input", node_id, "number", "property:opacity")
    if opacity_port is None:
        raise QaFailure("second Image Opacity Node has no published-property input")
    port_key = opacity_port["metadata"]["port"]
    before = client.state()
    client.click_component(control_id, button="secondary")
    publish = "node_editor.interface_action.node:{}.input:{}:publish_parameter".format(
        node_id, port_key
    )
    client.wait_component_settled(publish)
    client.click_component(publish)

    def published():
        state = client.state()
        _, definition = active_definition(state, "node_clip")
        matches = [
            parameter
            for parameter in definition["interface"]["parameters"]
            if parameter["target"]["node_id"] == node_id
        ]
        return (state, matches[0]) if len(matches) == 1 else None

    state, parameter = client.wait_until("publish second opacity input", published)
    place_node_for_inline_edit(client, node_id, control_id)
    client.drag_component_by(control_id, -24.0, 0.0, steps=8)

    def overridden():
        current = client.state()
        instance_id = _invocation(current["project"], item_id)["instance_id"]
        overrides = current["project"]["module_instances"][instance_id][
            "parameter_overrides"
        ]
        return current if parameter["id"] in overrides else None

    changed = client.wait_until("second opacity instance override", overridden)
    if changed["history"]["revision"] < state["history"]["revision"] + 1:
        raise QaFailure("second opacity edit did not commit")
    return changed, parameter


def _assert_sibling_unchanged(project, before, sibling_id):
    if project["items"][sibling_id] != before["items"][sibling_id]:
        raise QaFailure("Node clipboard changed sibling Timeline item")
    sibling_instance = _invocation(before, sibling_id)["instance_id"]
    assert_module_instance_unchanged(
        project, before, sibling_instance, "Node clipboard sibling"
    )


def _assert_paste(before, after, item_id, sibling_id, originals):
    before_id, before_definition = active_definition(before, "node_clip")
    after_id, definition = active_definition(after, "node_clip")
    if before_id != after_id:
        raise QaFailure(
            "pasting into a private Module unexpectedly changed definition identity"
        )
    old_nodes = set(before_definition["graph"]["nodes"])
    added = set(definition["graph"]["nodes"]) - old_nodes
    if len(added) != 2 or added & set(originals):
        raise QaFailure("paste did not create exactly two fresh Node identities")
    if (
        len(definition["graph"]["connections"])
        != len(before_definition["graph"]["connections"]) + 1
    ):
        raise QaFailure("paste copied an external edge or omitted the internal edge")
    internal = [
        edge
        for edge in definition["graph"]["connections"]
        if edge["from"]["node_id"] in added and edge["to"]["node_id"] in added
    ]
    if len(internal) != 1:
        raise QaFailure("paste did not preserve exactly the selected internal edge")
    pasted_from = internal[0]["from"]["node_id"]
    pasted_to = internal[0]["to"]["node_id"]
    original_edge = connection(before_definition, originals[0], originals[1])
    if original_edge is None or _connection_payload(internal[0]) != _connection_payload(
        original_edge
    ):
        raise QaFailure("pasted internal edge changed its authored order or blend")
    for original, pasted in zip(originals, (pasted_from, pasted_to)):
        if _node_payload(
            before_definition["graph"]["nodes"][original]
        ) != _node_payload(definition["graph"]["nodes"][pasted]):
            raise QaFailure("pasted Node changed its authored content or presentation")
    offsets = [
        [
            definition["graph"]["nodes"][pasted]["ui_position"][axis]
            - before_definition["graph"]["nodes"][original]["ui_position"][axis]
            for axis in range(2)
        ]
        for original, pasted in zip(originals, (pasted_from, pasted_to))
    ]
    if any(
        abs(offsets[0][axis] - offsets[1][axis]) > 0.001 for axis in range(2)
    ) or all(abs(value) <= 0.001 for value in offsets[0]):
        raise QaFailure("paste did not preserve relative Node layout at a new position")

    old_parameters = [_node_parameter(before_definition, node) for node in originals]
    new_parameters = [
        _node_parameter(definition, node) for node in (pasted_from, pasted_to)
    ]
    if (
        len(definition["interface"]["parameters"])
        != len(before_definition["interface"]["parameters"]) + 2
    ):
        raise QaFailure("paste did not copy exactly the selected Published parameters")
    if {parameter["id"] for parameter in old_parameters} & {
        parameter["id"] for parameter in new_parameters
    }:
        raise QaFailure("paste reused a PublishedParameter identity")
    for old, new in zip(old_parameters, new_parameters):
        if _parameter_payload(old) != _parameter_payload(new):
            raise QaFailure("paste changed a PublishedParameter contract")

    project_before = before["project"]
    project_after = after["project"]
    instance_id = _invocation(project_before, item_id)["instance_id"]
    before_instance = project_before["module_instances"][instance_id]
    after_instance = project_after["module_instances"][instance_id]
    for old, new in zip(old_parameters, new_parameters):
        old_override = before_instance["parameter_overrides"].get(old["id"])
        new_override = after_instance["parameter_overrides"].get(new["id"])
        if old_override != new_override:
            raise QaFailure("paste lost the current Module instance override")
        old_track = _parameter_track(project_before, item_id, old["id"])
        new_track = _parameter_track(project_after, item_id, new["id"])
        if _track_payload(old_track or {}) != _track_payload(new_track or {}):
            raise QaFailure("paste changed copied Timeline automation")
        if old_track and new_track:
            old_keys = {key["id"] for key in old_track["keyframes"]}
            new_keys = {key["id"] for key in new_track["keyframes"]}
            if old_keys & new_keys:
                raise QaFailure("paste reused a Timeline Keyframe identity")
    _assert_sibling_unchanged(project_after, project_before, sibling_id)
    if after["history"]["revision"] != before["history"]["revision"] + 1:
        raise QaFailure("pasting a Node selection was not one Undo transaction")
    return {
        "nodes": [pasted_from, pasted_to],
        "connection_id": internal[0]["id"],
        "parameter_ids": [parameter["id"] for parameter in new_parameters],
    }


def _keyboard_copy_paste(client, item_id, sibling_id, originals):
    select_nodes(client, originals, "clipboard source Nodes")
    before = client.state()
    client.key("c", True, command=True)
    client.key("c", False, command=True)
    client.wait_until(
        "native Node copy",
        lambda: (
            state
            if (state := client.state())["editor"].get("status")
            == "Copied selected processing nodes"
            else None
        ),
    )
    if client.state()["project"] != before["project"]:
        raise QaFailure("Copy changed the authored Project")
    client.wait_component_settled("node_editor.canvas")
    client.key("v", True, command=True)
    client.key("v", False, command=True)
    after = client.wait_until(
        "native clipboard Paste event",
        lambda: (
            state
            if len(active_definition(state := client.state())[1]["graph"]["nodes"])
            == len(active_definition(before)[1]["graph"]["nodes"]) + 2
            else None
        ),
        30.0,
    )
    result = _assert_paste(before, after, item_id, sibling_id, originals)
    if after["editor"]["node_editor"]["selected_node_count"] != 2:
        raise QaFailure("paste did not select the newly created Nodes")
    result["capture"] = capture_viewport(
        client,
        pathlib.Path(os.environ["RUVIE_QA_ARTIFACT_DIR"]) / "clipboard-pasted.png",
    )
    client.key("delete", True)
    client.key("delete", False)
    client.wait_until(
        "pasted selection Delete",
        lambda: (
            state
            if not set(result["nodes"])
            & set(active_definition(state := client.state())[1]["graph"]["nodes"])
            else None
        ),
    )
    history_shortcut(client)
    client.wait_until(
        "Undo pasted selection Delete",
        lambda: client.state()["project"] == after["project"],
    )
    history_shortcut(client)
    paste_undone = client.wait_until(
        "Undo keyboard Node paste",
        lambda: (
            state if (state := client.state())["project"] == before["project"] else None
        ),
    )
    history_shortcut(client, redo=True)
    redone = client.wait_until(
        "Redo keyboard Node paste",
        lambda: (
            state if (state := client.state())["project"] == after["project"] else None
        ),
    )
    if redone["history"]["revision"] != paste_undone["history"]["revision"] + 1:
        raise QaFailure("Node paste Redo was not one history command")
    history_shortcut(client)
    client.wait_until(
        "Undo keyboard Node paste after Redo",
        lambda: client.state()["project"] == before["project"],
    )
    return result


def _context_copy_paste(client, item_id, sibling_id, originals):
    select_nodes(client, originals, "context clipboard source Nodes")
    client.click_component(
        "node_editor.node_header:" + originals[0], button="secondary"
    )
    copy_id = "node_editor.node_menu:{}:copy".format(originals[0])
    _, copy_action = client.wait_component_settled(copy_id)
    if set((copy_action.get("metadata") or {}).get("node_ids", [])) != set(originals):
        raise QaFailure("Node context Copy omitted part of the selected subgraph")
    before = client.state()
    client.click_component(copy_id)
    client.wait_until(
        "context Node copy",
        lambda: (
            state
            if (state := client.state())["editor"].get("status")
            == "Copied selected processing nodes"
            else None
        ),
    )
    client.wait_component_settled("node_editor.canvas")
    point = find_clear_canvas_point(
        client.component_snapshot(),
        "node_editor.canvas",
        ("node_editor.node:", "node_editor.node_header:"),
    )
    client.inject(
        "click", {**point, "button": "secondary", "coordinate_space": "points"}
    )
    client.wait_component_settled("node_editor.menu.paste")
    client.click_component("node_editor.menu.paste")
    after = client.wait_until(
        "context-menu Node paste",
        lambda: (
            state
            if len(active_definition(state := client.state())[1]["graph"]["nodes"])
            == len(active_definition(before)[1]["graph"]["nodes"]) + 2
            else None
        ),
        30.0,
    )
    result = _assert_paste(before, after, item_id, sibling_id, originals)
    history_shortcut(client)
    paste_undone = client.wait_until(
        "Undo context Node paste",
        lambda: (
            state if (state := client.state())["project"] == before["project"] else None
        ),
    )
    history_shortcut(client, redo=True)
    redone = client.wait_until(
        "Redo context Node paste",
        lambda: (
            state if (state := client.state())["project"] == after["project"] else None
        ),
    )
    if redone["history"]["revision"] != paste_undone["history"]["revision"] + 1:
        raise QaFailure("context Node paste Redo was not one history command")
    return result


def _focused_text_edit_owns_clipboard(client):
    """A Node TextEdit must keep native copy/paste away from graph actions."""

    _, text_node_id = create_node_from_menu(
        client,
        "node_clip",
        "Text",
        "node_editor.menu.create.text",
    )
    control_id = "node_editor.property.node:{}:text".format(text_node_id)
    place_node_for_inline_edit(client, text_node_id, control_id)
    client.click_component("node_editor.node_header:" + text_node_id)
    client.wait_until(
        "Text Node selected before Content focus",
        lambda: (
            component
            if (
                component := next(
                    (
                        candidate
                        for candidate in client.component_snapshot()["components"]
                        if candidate.get("id")
                        == "node_editor.node_header:" + text_node_id
                    ),
                    None,
                )
            )
            and (component.get("metadata") or {}).get("selected") is True
            else None
        ),
    )
    _, control = client.wait_component_settled(control_id)
    before = client.state()
    text_node = active_definition(before, "node_clip")[1]["graph"]["nodes"][
        text_node_id
    ]
    original_text = text_node["properties"]["text"]["properties"]["value"]
    if not isinstance(original_text, str) or not original_text:
        raise QaFailure("Text Node did not expose its real editable Content")
    node_ids = set(active_definition(before, "node_clip")[1]["graph"]["nodes"])

    focus_point = component_point(control, fraction_x=0.75, fraction_y=0.5)
    client.inject(
        "click", {**focus_point, "button": "primary", "coordinate_space": "points"}
    )
    client.key("a", True, command=True)
    client.key("a", False, command=True)
    copied_text = "Node TextEdit clipboard sentinel"
    client.inject("text", {"text": copied_text})

    def text_edit_focused():
        state = client.state()
        value = active_definition(state, "node_clip")[1]["graph"]["nodes"][
            text_node_id
        ]["properties"]["text"]["properties"]["value"]
        return state if value == copied_text else None

    focused = client.wait_until("focused Node TextEdit input", text_edit_focused)
    client.key("a", True, command=True)
    client.key("a", False, command=True)
    client.key("c", True, command=True)
    client.key("c", False, command=True)
    after_copy = client.state()
    if set(active_definition(after_copy, "node_clip")[1]["graph"]["nodes"]) != node_ids:
        raise QaFailure("focused TextEdit copy was routed into the Node graph")
    if (
        after_copy["project"] != focused["project"]
        or after_copy["history"] != focused["history"]
    ):
        raise QaFailure("focused TextEdit copy changed Project history")
    if after_copy["editor"].get("status") != focused["editor"].get("status"):
        raise QaFailure(
            "focused TextEdit copy was captured by the Node clipboard router"
        )
    client.key("a", True, command=True)
    client.key("a", False, command=True)
    client.key("backspace", True)
    client.key("backspace", False)

    def cleared_text():
        state = client.state()
        value = active_definition(state, "node_clip")[1]["graph"]["nodes"][
            text_node_id
        ]["properties"]["text"]["properties"]["value"]
        return state if value == "" else None

    cleared = client.wait_until("focused Node TextEdit clear", cleared_text)
    client.key("v", True, command=True)
    client.key("v", False, command=True)
    after_paste_shortcut = client.state()
    if (
        after_paste_shortcut["project"] != cleared["project"]
        or after_paste_shortcut["history"] != cleared["history"]
    ):
        raise QaFailure("focused TextEdit paste shortcut changed Project history")
    if (
        set(active_definition(after_paste_shortcut, "node_clip")[1]["graph"]["nodes"])
        != node_ids
    ):
        raise QaFailure("focused TextEdit paste was routed into the Node graph")
    current_text = active_definition(after_paste_shortcut, "node_clip")[1]["graph"][
        "nodes"
    ][text_node_id]["properties"]["text"]["properties"]["value"]
    if current_text != "":
        raise QaFailure("raw QA paste shortcut unexpectedly supplied Text content")
    # Loopback key injection enters egui after winit's native clipboard
    # translation, so Ctrl+V normally has no Paste payload here. The production
    # Event::Paste path has a focused-TextEdit unit test; restore through the
    # existing real Text input action without adding QA transport.
    paste_payload_observed = False
    client.key("a", True, command=True)
    client.key("a", False, command=True)
    client.inject("text", {"text": original_text})
    client.click_component("node_editor.node_header:" + text_node_id)

    def restored_after_focus_out():
        state = client.state()
        current_text = active_definition(state, "node_clip")[1]["graph"]["nodes"][
            text_node_id
        ]["properties"]["text"]["properties"]["value"]
        return state if current_text == original_text else None

    restored = client.wait_until(
        "focused Node TextEdit restored and committed", restored_after_focus_out
    )
    return {
        "node_id": text_node_id,
        "value": original_text,
        "paste_payload_observed": paste_payload_observed,
        "revision": restored["history"]["revision"],
    }


def _save_and_reload(client, item_id, persisted_paste, artifact_dir):
    project_file = pathlib.Path(os.environ["RUVIE_QA_PROJECT_PATH"])
    saved, file_evidence = save_project_to_disk(client, project_file, "Node clipboard")
    initial_close = request_clean_native_close(
        client, "saved Node clipboard app", client.timeout
    )
    reload_port = free_port()
    with spawned_authoring_app(
        reload_port,
        {
            "RUVIE_QA_PROJECT_PATH": str(project_file),
            "RUVIE_QA_OPEN_EXISTING_PROJECT": "1",
            "RUVIE_QA_PORT_FILE": None,
            "RUVIE_QA_RUN_ID": os.environ.get("RUVIE_QA_RUN_ID", "node-copy-paste")
            + ":reload",
        },
    ) as process:
        fresh = QaClient("http://127.0.0.1:{}".format(reload_port), client.timeout)
        fresh.wait_health()
        reloaded = fresh.state()
        if reloaded["project"] != saved["project"]:
            raise QaFailure(
                "fresh process changed pasted Nodes, edges, parameters, or automation"
            )
        bring_timeline_component(fresh, "timeline.item:" + item_id, -120.0)
        fresh.double_click_component("timeline.item:" + item_id)
        fresh.wait_component_settled("node_editor.canvas")
        definition = active_definition(fresh.state(), "node_clip")[1]
        if not set(persisted_paste["nodes"]).issubset(definition["graph"]["nodes"]):
            raise QaFailure("fresh process omitted the pasted Node identities")
        if all(
            connection["id"] != persisted_paste["connection_id"]
            for connection in definition["graph"]["connections"]
        ):
            raise QaFailure("fresh process omitted the pasted internal edge")
        capture = capture_viewport(client=fresh, path=artifact_dir / "capture.png")
        reload_actions = list(fresh.evidence)
        reload_close = close_clean_native_app(
            fresh, process, "reloaded Node clipboard app", client.timeout
        )
    return {
        "saved": file_evidence,
        "initial_close": initial_close,
        "reload_close": reload_close,
        "capture": capture,
        "actions": reload_actions,
    }


def run_suite(client):
    client.wait_health()
    initial = client.state()
    item = item_by_name(initial["project"], "QA Node Clip")
    item_id = item["id"]
    start = media_seconds(item["interval"]["start"])
    first, second = _create_connected_pair(client, item_id)
    _, (_, sibling_id) = _duplicate_item(client, item_id)
    animated, _ = _publish_animated_first(client, item_id, first, start)
    sibling_before = copy.deepcopy(animated["project"])
    prepared, _ = _publish_constant_second(client, item_id, second)
    _assert_sibling_unchanged(prepared["project"], sibling_before, sibling_id)
    _, prepared_definition = active_definition(prepared, "node_clip")
    first_parameter = _node_parameter(prepared_definition, first)
    second_parameter = _node_parameter(prepared_definition, second)
    first_track = _parameter_track(prepared["project"], item_id, first_parameter["id"])
    instance_id = _invocation(prepared["project"], item_id)["instance_id"]
    overrides = prepared["project"]["module_instances"][instance_id][
        "parameter_overrides"
    ]
    if first_track is None or len(first_track["keyframes"]) != 2:
        raise QaFailure("clipboard source lacks the two-key Timeline automation")
    if second_parameter["id"] not in overrides:
        raise QaFailure("clipboard source lacks the current instance override")
    keyboard = _keyboard_copy_paste(client, item_id, sibling_id, (first, second))
    context = _context_copy_paste(client, item_id, sibling_id, (first, second))
    text_edit = _focused_text_edit_owns_clipboard(client)
    artifact_dir = pathlib.Path(os.environ["RUVIE_QA_ARTIFACT_DIR"])
    before_save_capture = capture_viewport(
        client, artifact_dir / "clipboard-before-save.png"
    )
    reload = _save_and_reload(client, item_id, context, artifact_dir)
    return {
        "suite": "node-copy-paste",
        "item_id": item_id,
        "sibling_id": sibling_id,
        "source_nodes": [first, second],
        "keyboard": keyboard,
        "context": context,
        "text_edit": text_edit,
        "before_save_capture": before_save_capture,
        "saved": reload["saved"],
        "initial_close": reload["initial_close"],
        "reload_close": reload["reload_close"],
        "capture": reload["capture"],
        "actions": client.evidence,
        "reload_actions": reload["actions"],
    }


if __name__ == "__main__":
    raise SystemExit(
        run_suite_main(
            "qa-node-copy-paste-e2e",
            run_suite,
            "target/qa-node-copy-paste-evidence.json",
        )
    )
