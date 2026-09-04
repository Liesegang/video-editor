#!/usr/bin/env python3
"""Add, render, remove, and undo a first-class Timeline transition."""

from qa_support import (
    QaFailure,
    activate_dock_tab,
    component_center,
    find_clear_canvas_point,
    item_by_name,
    media_seconds,
    rendered_preview_state,
    run_suite_main,
    seek_timeline_seconds,
)


SOURCE_NAME = "QA Image"
TIMELINE_TAB_ID = "dock.tab:timeline"
INSPECTOR_TAB_ID = "dock.tab:inspector"
CURVE_TAB_ID = "dock.tab:curve_editor"


def _component(snapshot, component_id):
    return next(
        (component for component in snapshot["components"] if component["id"] == component_id),
        None,
    )


def _active_definition(state):
    document = state["editor"]["node_editor"]["document"]
    if (
        not document
        or document.get("kind") != "module_definition"
        or document.get("host") != "transition"
    ):
        raise QaFailure("Node Editor did not open the Transition Module document")
    definition_id = document["definition_id"]
    return definition_id, state["project"]["module_definitions"][definition_id]


def _node_content_type(node):
    return str((node.get("content") or {}).get("type", "")).replace("_", "").lower()


def _native_catalog_id(node):
    content = node.get("content") or {}
    return (content.get("data") or {}).get("catalog_id")


def _port(snapshot, direction, node_id, data_type=None, port_key=None):
    return next(
        (
            component
            for component in snapshot["components"]
            if component.get("type") == "node_editor_port"
            and component.get("visible")
            and (component.get("metadata") or {}).get("direction") == direction
            and (component.get("metadata") or {}).get("node_id") == node_id
            and (
                data_type is None
                or str((component.get("metadata") or {}).get("data_type", "")).lower()
                == data_type
            )
            and (
                port_key is None
                or (component.get("metadata") or {}).get("port") == port_key
            )
        ),
        None,
    )


def _visible_component(snapshot, component_type, predicate):
    return next(
        (
            component
            for component in snapshot["components"]
            if component.get("type") == component_type
            and component.get("visible")
            and predicate(component.get("metadata") or {})
        ),
        None,
    )


def _place_node_for_inline_edit(client, node_id, component_id):
    """Use the production header drag to expose a node's inline controls."""
    snapshot = client.component_snapshot()
    component = _component(snapshot, component_id)
    if component is not None and component.get("visible"):
        return component

    header_id = "node_editor.node_header:" + node_id
    _, header = client.wait_component_settled(header_id)
    _, canvas = client.wait_component_settled("node_editor.canvas")
    bounds = canvas["rect_points"]
    target = {
        "x": float(bounds["center_x"]),
        "y": float(bounds["min_y"]) + 42.0,
    }
    client.drag(component_center(header), target, steps=10)
    try:
        return client.wait_until(
            "visible Node inline property " + component_id,
            lambda: candidate
            if (
                (candidate := _component(client.component_snapshot(), component_id))
                is not None
                and candidate.get("visible")
            )
            else None,
            timeout=5.0,
        )
    except QaFailure as error:
        current = client.component_snapshot()
        diagnostics = {
            key: _component(current, key)
            for key in (component_id, header_id, "node_editor.node:" + node_id, "node_editor.canvas")
        }
        raise QaFailure("{}; geometry={!r}".format(error, diagnostics)) from error


def _transition_module(project, transition_id):
    transition = project["transitions"][transition_id]
    implementation = transition["processor"]["implementation"]
    if implementation.get("kind") != "module":
        raise QaFailure("Transition is no longer backed by a Module")
    return implementation.get("value") or {}


def _transition_parameter_keys(project, transition_id, parameter_id):
    module = _transition_module(project, transition_id)
    track = (module.get("automation_tracks") or {}).get(parameter_id)
    return [] if track is None else track.get("keyframes", [])


def _bring_into_inspector(client, component_id):
    activate_dock_tab(client, INSPECTOR_TAB_ID, "Inspector", "Transition Inspector")
    for _ in range(9):
        snapshot = client.component_snapshot()
        component = _component(snapshot, component_id)
        panel = _component(snapshot, "inspector.scroll_area")
        if component is not None and panel is not None and component.get("visible"):
            rect = component.get("rect_points") or {}
            bounds = panel.get("rect_points") or {}
            if bounds.get("min_y", 1.0) <= rect.get("center_y", 0.0) <= bounds.get(
                "max_y", -1.0
            ):
                return component
        client.scroll_component("inspector.scroll_area", 0.0, -320.0)
    raise QaFailure("could not bring {} into the Inspector".format(component_id))


def _open_curve_editor(client):
    activate_dock_tab(client, CURVE_TAB_ID, "Curve Editor", "Transition Curve Editor")


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


def _select_connection(client, connection_id):
    component_id = "node_editor.connection:" + connection_id
    try:
        _, component = client.wait_component_settled(component_id)
    except QaFailure as error:
        snapshot = client.component_snapshot()
        wire_ids = [
            entry["id"]
            for entry in snapshot["components"]
            if entry.get("type") == "node_editor_connection"
        ]
        visible_ports = [
            {
                "id": entry["id"],
                "rect": entry.get("rect_points"),
                "metadata": entry.get("metadata"),
            }
            for entry in snapshot["components"]
            if entry.get("type") == "node_editor_port" and entry.get("visible")
        ]
        raise QaFailure(
            "{}; visible wires={!r}; visible ports={!r}".format(
                error, wire_ids, visible_ports
            )
        ) from error
    metadata = component.get("metadata") or {}
    if metadata.get("connection_id") != connection_id:
        raise QaFailure("Transition Module wire QA target has the wrong identity")
    client.click_component(component_id)
    client.wait_until(
        "Transition Module connection selection",
        lambda: state
        if (state := client.state())["editor"]["node_editor"]["selected_connection"]
        == connection_id
        else None,
    )


def run_suite(client):
    client.wait_health()
    initial = client.state()
    source = item_by_name(initial["project"], SOURCE_NAME)
    source_id = source["id"]
    source_component_id = "timeline.item:" + source_id
    source_end = media_seconds(source["interval"]["start"]) + media_seconds(
        source["interval"]["duration"]
    )
    initial_item_ids = set(initial["project"]["items"])
    initial_transition_ids = set(initial["project"].get("transitions", {}))
    initial_revision = initial["history"]["revision"]

    client.click_component(source_component_id, button="secondary")
    duplicate_id = "timeline.item.duplicate:" + source_id
    client.wait_component(duplicate_id)
    client.click_component(duplicate_id)

    def duplicated():
        state = client.state()
        new_ids = set(state["project"]["items"]) - initial_item_ids
        if len(new_ids) != 1 or state["history"]["revision"] != initial_revision + 1:
            return None
        item_id = next(iter(new_ids))
        item = state["project"]["items"][item_id]
        if (
            item["track_id"] == source["track_id"]
            and abs(media_seconds(item["interval"]["start"]) - source_end) < 1e-9
        ):
            return state, item_id, item
        return None

    after_duplicate, target_id, target = client.wait_until(
        "adjacent duplicate Timeline clip", duplicated
    )
    target_component_id = "timeline.item:" + target_id
    client.wait_component_settled(target_component_id)

    client.click_component(source_component_id, button="secondary")
    client.wait_component("timeline.item.add_transition_menu:" + source_id)
    client.click_component("timeline.item.add_transition_menu:" + source_id)
    add_id = "timeline.item.add_transition:{}:{}:image".format(source_id, target_id)
    client.wait_component(add_id)
    client.click_component(add_id)

    def transition_added():
        state = client.state()
        transitions = state["project"].get("transitions", {})
        new_ids = set(transitions) - initial_transition_ids
        if len(new_ids) != 1 or state["history"]["revision"] != initial_revision + 2:
            return None
        transition_id = next(iter(new_ids))
        transition = transitions[transition_id]
        processor = transition["processor"]
        contract = processor["contract"]
        implementation = processor["implementation"]
        operation = implementation.get("value") or {}
        if (
            transition["from_item_id"] == source_id
            and transition["to_item_id"] == target_id
            and transition["alignment"] == "centered_on_edit"
            and contract["media_type"] == "image"
            and implementation.get("kind") == "operation"
            and operation.get("component_id") == "cross_dissolve"
        ):
            return state, transition_id, transition
        return None

    added, transition_id, transition = client.wait_until(
        "first-class Cross Dissolve authoring state", transition_added
    )
    overlay_id = "timeline.transition:" + transition_id
    _, overlay = client.wait_component_settled(overlay_id)
    metadata = overlay.get("metadata") or {}
    if (
        metadata.get("from_item_id") != source_id
        or metadata.get("to_item_id") != target_id
        or metadata.get("output") != "image"
    ):
        raise QaFailure("Timeline transition overlay metadata does not match its model")

    client.click_component(overlay_id)
    client.wait_until(
        "Transition overlay primary selection",
        lambda: state
        if (state := client.state())["editor"]["selection"]["primary"]
        == {"kind": "transition", "id": transition_id}
        else None,
    )
    client.wait_component_settled(
        "inspector.property:transition:{}:duration".format(transition_id)
    )
    client.wait_component_settled(
        "inspector.transition.alignment:{}".format(transition_id)
    )
    inspector_open_id = "inspector.transition.open_editor:" + transition_id
    client.wait_component_settled(inspector_open_id)

    duration = media_seconds(transition["duration"])
    edit_point = media_seconds(transition["edit_point"])
    if duration <= 0.0 or duration > min(
        media_seconds(source["interval"]["duration"]),
        media_seconds(target["interval"]["duration"]),
    ):
        raise QaFailure("default transition duration is outside both clip bounds")
    seek_timeline_seconds(client, edit_point)
    rendered = client.wait_until(
        "Cross Dissolve Preview publication",
        lambda: rendered_preview_state(client, added["history"]["revision"]),
        timeout=30.0,
    )

    definitions_before = set(added["project"]["module_definitions"])
    instances_before = set(added["project"]["module_instances"])
    items_before_promotion = added["project"]["items"]
    client.click_component(inspector_open_id)

    def promoted_to_module():
        state = client.state()
        current = state["project"].get("transitions", {}).get(transition_id)
        if not current or state["history"]["revision"] != added["history"]["revision"] + 1:
            return None
        implementation = current["processor"]["implementation"]
        if implementation.get("kind") != "module":
            return None
        instance_id = (implementation.get("value") or {}).get("instance_id")
        instance = state["project"]["module_instances"].get(instance_id)
        if not instance:
            return None
        definition_id = instance["definition_id"]
        definition = state["project"]["module_definitions"].get(definition_id)
        if not definition:
            return None
        return state, current, instance_id, definition_id, definition

    (
        promoted,
        module_transition,
        module_instance_id,
        module_definition_id,
        module_definition,
    ) = client.wait_until("Transition promotion to a finite Node Module", promoted_to_module)

    if promoted["project"]["items"] != items_before_promotion:
        raise QaFailure("Transition promotion changed Timeline clips")
    if set(promoted["project"]["module_definitions"]) - definitions_before != {
        module_definition_id
    }:
        raise QaFailure("Transition promotion did not create exactly one Module definition")
    if set(promoted["project"]["module_instances"]) - instances_before != {
        module_instance_id
    }:
        raise QaFailure("Transition promotion did not create exactly one Module instance")
    for field in (
        "timeline_id",
        "from_item_id",
        "to_item_id",
        "edit_point",
        "duration",
        "alignment",
    ):
        if module_transition[field] != transition[field]:
            raise QaFailure("Transition promotion changed Timeline-owned " + field)

    host_contract = module_definition.get("host_contract") or {}
    contract = host_contract.get("contract") or {}
    if host_contract.get("kind") != "transition" or contract.get("media_type") != "image":
        raise QaFailure("Transition Module did not persist its protected Image host contract")
    if len(module_definition["graph"]["nodes"]) != 5:
        raise QaFailure("Transition Module did not start with finite A/B/Progress/Mix/Output nodes")
    if len(module_definition["graph"]["connections"]) != 4:
        raise QaFailure("Transition Module starter topology is incomplete")
    catalog_ids = {
        _native_catalog_id(node)
        for node in module_definition["graph"]["nodes"].values()
        if _node_content_type(node) == "nativeoperation"
    }
    if not {
        "native.transition.image_input",
        "native.transition.progress_input",
        "native.transition.image_mix",
    }.issubset(catalog_ids):
        raise QaFailure("Transition Module is missing a protected host or Mix node")

    client.wait_component("dock.tab:node_editor")
    client.click_component("dock.tab:node_editor")
    _, canvas = client.wait_component_settled("node_editor.canvas")
    canvas_metadata = canvas.get("metadata") or {}
    if canvas_metadata.get("timeline_graph_expansion") is not False:
        raise QaFailure("Transition logic expanded Timeline clips into Nodes")
    opened = client.state()
    active_definition_id, active_definition = _active_definition(opened)
    if active_definition_id != module_definition_id or active_definition != module_definition:
        raise QaFailure("Node Editor opened a different Module than the Transition processor")
    if opened["editor"]["node_editor"]["document"].get("instance_id") != module_instance_id:
        raise QaFailure("Node Editor lost the concrete Transition Module instance")

    protected_headers = [
        component
        for component in client.component_snapshot()["components"]
        if component.get("type") == "node_header"
        and (component.get("metadata") or {}).get("host_boundary") is True
    ]
    if len(protected_headers) != 4:
        raise QaFailure("A/B/Progress/Output were not visibly protected by the host contract")

    module_rendered = client.wait_until(
        "Node-authored Transition Preview publication",
        lambda: rendered_preview_state(client, promoted["history"]["revision"]),
        timeout=30.0,
    )

    original_nodes = set(module_definition["graph"]["nodes"])
    output_node_id = next(
        node_id
        for node_id, node in module_definition["graph"]["nodes"].items()
        if _node_content_type(node) == "moduleoutput"
    )
    mix_node_id = next(
        node_id
        for node_id, node in module_definition["graph"]["nodes"].items()
        if _native_catalog_id(node) == "native.transition.image_mix"
    )
    mix_output = _connection(module_definition, mix_node_id, output_node_id)
    if not mix_output:
        raise QaFailure("starter Transition Module does not route Mix to Output")

    snapshot = client.component_snapshot()
    menu_point = find_clear_canvas_point(
        snapshot,
        "node_editor.canvas",
        ("node_editor.node:", "node_editor.node_header:", "node_editor.port."),
    )
    client.inject(
        "click",
        {**menu_point, "button": "secondary", "coordinate_space": "points"},
    )
    client.wait_component("node_editor.menu.search")
    client.click_component("node_editor.menu.search")
    client.inject("text", {"text": "blur"})
    client.wait_component_settled("node_editor.menu.create.effect:blur")
    client.click_component("node_editor.menu.create.effect:blur")

    def blur_created():
        state = client.state()
        _, definition = _active_definition(state)
        new_nodes = set(definition["graph"]["nodes"]) - original_nodes
        return (state, definition, next(iter(new_nodes))) if len(new_nodes) == 1 else None

    _, _, blur_node_id = client.wait_until(
        "Blur Node inside Transition Module", blur_created
    )
    sigma_property_id = "node_editor.property.node:{}:sigma_x".format(blur_node_id)
    _place_node_for_inline_edit(client, blur_node_id, sigma_property_id)
    _select_connection(client, mix_output["id"])
    client.key("backspace", True)
    client.key("backspace", False)

    def mix_disconnected():
        state = client.state()
        _, definition = _active_definition(state)
        return state if not _connection(definition, mix_node_id, output_node_id) else None
    client.wait_until("Mix disconnected from Transition Output", mix_disconnected)

    def mix_to_blur_ports():
        current = client.component_snapshot()
        source = _port(current, "output", mix_node_id, "image")
        target = _port(current, "input", blur_node_id, "image")
        return (source, target) if source and target else None

    source_port, target_port = client.wait_until(
        "visible Mix and Blur ports", mix_to_blur_ports
    )
    client.drag(component_center(source_port), component_center(target_port), steps=12)
    try:
        client.wait_until(
            "Mix connected to Blur",
            lambda: state
            if _connection(
                _active_definition(state := client.state())[1],
                mix_node_id,
                blur_node_id,
            )
            else None,
        )
    except QaFailure as error:
        state = client.state()
        _, failed_definition = _active_definition(state)
        canvas = _component(client.component_snapshot(), "node_editor.canvas") or {}
        raise QaFailure(
            "{}; source={!r}; target={!r}; connections={!r}; canvas={!r}; editor_error={!r}; last_action={!r}".format(
                error,
                source_port,
                target_port,
                failed_definition["graph"]["connections"],
                canvas.get("metadata"),
                state["editor"].get("error"),
                client.evidence[-1] if client.evidence else None,
            )
        ) from error

    def blur_to_output_ports():
        current = client.component_snapshot()
        source = _port(current, "output", blur_node_id, "image")
        target = _port(current, "input", output_node_id, "image")
        return (source, target) if source and target else None

    source_port, target_port = client.wait_until(
        "visible Blur and Output ports", blur_to_output_ports
    )
    client.drag(component_center(source_port), component_center(target_port), steps=12)

    def custom_route_complete():
        state = client.state()
        _, definition = _active_definition(state)
        if (
            len(definition["graph"]["nodes"]) == 6
            and len(definition["graph"]["connections"]) == 5
            and _connection(definition, mix_node_id, blur_node_id)
            and _connection(definition, blur_node_id, output_node_id)
        ):
            return state, definition
        return None

    custom_state, custom_definition = client.wait_until(
        "Mix to Blur to Output Transition route", custom_route_complete
    )
    if custom_state["project"]["items"] != items_before_promotion:
        raise QaFailure("Node editing a Transition changed Timeline clips")
    for field in (
        "timeline_id",
        "from_item_id",
        "to_item_id",
        "edit_point",
        "duration",
        "alignment",
    ):
        if custom_state["project"]["transitions"][transition_id][field] != transition[field]:
            raise QaFailure("Node editing changed Timeline-owned " + field)
    custom_rendered = client.wait_until(
        "custom-node Transition Preview publication",
        lambda: rendered_preview_state(client, custom_state["history"]["revision"]),
        timeout=30.0,
    )

    published_before = {
        parameter["id"] for parameter in custom_definition["interface"]["parameters"]
    }

    def publishable_sigma_port():
        snapshot = client.component_snapshot()
        port = _port(snapshot, "input", blur_node_id, "number", "property:sigma_x")
        return (snapshot, port) if port is not None else None

    _, sigma_port = client.wait_until(
        "publishable Blur sigma_x port", publishable_sigma_port
    )
    sigma_metadata = sigma_port.get("metadata") or {}
    sigma_port_key = sigma_metadata.get("port")
    try:
        _, sigma_property = client.wait_component_settled(sigma_property_id)
    except QaFailure as error:
        node_components = [
            {
                "id": component.get("id"),
                "type": component.get("type"),
                "visible": component.get("visible"),
                "enabled": component.get("enabled"),
                "metadata": component.get("metadata"),
            }
            for component in client.component_snapshot()["components"]
            if (component.get("metadata") or {}).get("node_id") == blur_node_id
        ]
        raise QaFailure(
            "{}; Blur components={!r}".format(error, node_components)
        ) from error
    if sigma_property.get("id") != sigma_property_id:
        raise QaFailure("Blur sigma_x property QA identity is not stable")
    client.click_component(sigma_property_id, button="secondary")
    publish_action_id = (
        "node_editor.interface_action.node:{}.input:{}:publish_parameter".format(
            blur_node_id, sigma_port_key
        )
    )
    try:
        _, publish_action = client.wait_component_settled(publish_action_id)
    except QaFailure as error:
        snapshot = client.component_snapshot()
        interface_components = [
            {
                "id": component.get("id"),
                "type": component.get("type"),
                "metadata": component.get("metadata"),
            }
            for component in snapshot["components"]
            if "interface" in str(component.get("id", ""))
        ]
        raise QaFailure(
            "{}; interface components={!r}".format(error, interface_components)
        ) from error
    publish_metadata = publish_action.get("metadata") or {}
    if (
        publish_metadata.get("node_id") != blur_node_id
        or publish_metadata.get("port") != sigma_port_key
        or publish_metadata.get("action") != "publish_parameter"
    ):
        raise QaFailure("Published parameter action lost its Node port identity")
    client.click_component(publish_action_id)

    def parameter_published():
        state = client.state()
        _, definition = _active_definition(state)
        parameters = definition["interface"]["parameters"]
        new_parameters = [
            parameter for parameter in parameters if parameter["id"] not in published_before
        ]
        if (
            len(new_parameters) == 1
            and state["history"]["revision"] == custom_state["history"]["revision"] + 1
        ):
            return state, definition, new_parameters[0]
        return None

    published_state, published_definition, published_parameter = client.wait_until(
        "Blur sigma_x Published parameter", parameter_published
    )
    parameter_id = published_parameter["id"]
    if published_parameter.get("target") != {
        "node_id": blur_node_id,
        "port": sigma_port_key,
    }:
        raise QaFailure("Published parameter targets an internal port other than sigma_x")

    activate_dock_tab(client, TIMELINE_TAB_ID, "Timeline", "Transition automation owner")
    seek_timeline_seconds(client, edit_point)
    client.wait_component_settled(overlay_id)
    client.click_component(overlay_id)
    client.wait_until(
        "Transition selected for parameter automation",
        lambda: state
        if (state := client.state())["editor"]["selection"]["primary"]
        == {"kind": "transition", "id": transition_id}
        else None,
    )
    property_control_id = "transition:{}:module_parameter:{}".format(
        transition_id, parameter_id
    )
    property_mode_id = "inspector.property_mode:" + property_control_id
    mode_control = _bring_into_inspector(client, property_mode_id)
    mode_metadata = mode_control.get("metadata") or {}
    if (
        mode_metadata.get("mode") != "constant"
        or mode_metadata.get("allow_keyframe") is not True
        or mode_metadata.get("keyframe_count") != 0
    ):
        raise QaFailure("Published Transition parameter did not use the shared timer state")
    client.click_component(property_mode_id)
    keyframe_option_id = property_mode_id + ".option:keyframe"
    client.wait_component_settled(keyframe_option_id)
    before_keyframe = client.state()
    client.click_component(keyframe_option_id)

    def transition_key_created():
        state = client.state()
        keys = _transition_parameter_keys(state["project"], transition_id, parameter_id)
        if (
            len(keys) == 1
            and state["history"]["revision"]
            == before_keyframe["history"]["revision"] + 1
        ):
            return state, keys[0]
        return None

    keyframed_state, authored_key = client.wait_until(
        "Inspector Transition parameter keyframe", transition_key_created
    )
    keyframe_id = authored_key["id"]
    expected_local_time = duration * 0.5
    if abs(media_seconds(authored_key["time"]) - expected_local_time) > 0.001:
        raise QaFailure(
            "Transition keyframe did not use interval-local time: {}".format(
                authored_key["time"]
            )
        )
    keyed_mode = _bring_into_inspector(client, property_mode_id)
    keyed_metadata = keyed_mode.get("metadata") or {}
    if (
        keyed_metadata.get("mode") != "keyframe"
        or keyed_metadata.get("key_at_current_time") is not True
        or keyed_metadata.get("keyframe_count") != 1
    ):
        raise QaFailure("Transition Inspector did not expose its authored diamond state")

    lane_owner = {
        "kind": "transition_definition",
        "transition_id": transition_id,
    }
    lane_target = {"kind": "module_parameter", "id": parameter_id}
    expected_lane = {"owner": lane_owner, "target": lane_target}
    activate_dock_tab(client, TIMELINE_TAB_ID, "Timeline", "Transition Dope Sheet")
    if target_id not in client.state()["editor"]["timeline"]["expanded_items"]:
        client.wait_component_settled("timeline.item_expand:" + target_id)
        client.click_component("timeline.item_expand:" + target_id)
        client.wait_until(
            "expanded Transition B clip",
            lambda: state
            if target_id in (state := client.state())["editor"]["timeline"]["expanded_items"]
            else None,
        )

    def transition_dope_lane():
        client.state()
        return _visible_component(
            client.component_snapshot(),
            "timeline_property_label",
            lambda metadata: metadata.get("item_id") == target_id
            and metadata.get("lane") == expected_lane
            and keyframe_id in metadata.get("keyframe_ids", []),
        )

    dope_lane = None
    for _ in range(8):
        dope_lane = transition_dope_lane()
        if dope_lane is not None:
            break
        client.scroll_component("timeline.canvas", 0.0, -100.0)
    if dope_lane is None:
        raise QaFailure("Transition parameter is missing from the Timeline Dope Sheet")
    if (dope_lane.get("metadata") or {}).get("target") != lane_target:
        raise QaFailure("Dope Sheet changed the PublishedParameter identity")
    client.click_component(dope_lane["id"])
    client.wait_until(
        "Dope Sheet lane preserves Transition selection",
        lambda: state
        if (state := client.state())["editor"]["selection"]["primary"]
        == {"kind": "transition", "id": transition_id}
        else None,
    )

    _open_curve_editor(client)

    def transition_curve_key():
        client.state()
        return _visible_component(
            client.component_snapshot(),
            "curve_editor_keyframe",
            lambda metadata: metadata.get("lane") == expected_lane
            and metadata.get("target") == lane_target
            and metadata.get("keyframe_id") == keyframe_id
            and metadata.get("component") == "value",
        )

    curve_key = client.wait_until(
        "same Transition owner and PublishedParameter in Curve Editor",
        transition_curve_key,
    )
    before_curve_drag = client.state()
    original_key = _transition_parameter_keys(
        before_curve_drag["project"], transition_id, parameter_id
    )[0]
    client.drag_component_by(curve_key["id"], 28.0, -14.0, steps=12)

    def transition_curve_dragged():
        state = client.state()
        keys = _transition_parameter_keys(state["project"], transition_id, parameter_id)
        if len(keys) != 1:
            return None
        key = keys[0]
        if (
            key["time"] != original_key["time"]
            and key["value"] != original_key["value"]
            and state["history"]["revision"]
            == before_curve_drag["history"]["revision"] + 1
            and not state["editor"]["curve_editor"]["drag_active"]
        ):
            return state, key
        return None

    dragged_state, dragged_key = client.wait_until(
        "atomic Transition Curve point drag", transition_curve_dragged
    )
    client.key("z", True, command=True)
    client.key("z", False, command=True)
    automation_state = client.wait_until(
        "one Undo restoring Transition Curve point",
        lambda: state
        if (state := client.state())["project"] == before_curve_drag["project"]
        else None,
    )
    if automation_state["history"]["can_redo"] is not True:
        raise QaFailure("Transition Curve drag was not one undoable command")

    activate_dock_tab(client, TIMELINE_TAB_ID, "Timeline", "Transition removal")
    client.wait_component_settled(overlay_id)
    client.click_component(overlay_id, button="secondary")
    remove_id = "timeline.transition.remove:" + transition_id
    client.wait_component(remove_id)
    client.click_component(remove_id)

    removed = client.wait_until(
        "one-step Timeline transition removal",
        lambda: state
        if transition_id not in (state := client.state())["project"].get("transitions", {})
        and module_instance_id not in state["project"]["module_instances"]
        and module_definition_id not in state["project"]["module_definitions"]
        and state["history"]["revision"] == automation_state["history"]["revision"] + 1
        else None,
    )
    if _component(client.component_snapshot(), overlay_id) is not None:
        raise QaFailure("removed transition still has a Timeline overlay")

    client.key("z", True, command=True)
    client.key("z", False, command=True)
    restored = client.wait_until(
        "single Undo restoring Timeline transition",
        lambda: state
        if transition_id in (state := client.state())["project"].get("transitions", {})
        else None,
    )
    client.wait_component_settled(overlay_id)
    if (
        restored["project"]["transitions"][transition_id]
        != automation_state["project"]["transitions"][transition_id]
    ):
        raise QaFailure("Undo did not restore the exact Node-authored transition")
    if (
        module_instance_id not in restored["project"]["module_instances"]
        or module_definition_id not in restored["project"]["module_definitions"]
    ):
        raise QaFailure("Undo did not restore the Transition Module ownership graph")
    if (
        restored["project"]["module_definitions"][module_definition_id]
        != automation_state["project"]["module_definitions"][module_definition_id]
    ):
        raise QaFailure("Undo did not restore the edited Transition Module topology")

    return {
        "suite": "timeline-transition",
        "source_item_id": source_id,
        "target_item_id": target_id,
        "transition_id": transition_id,
        "transition": module_transition,
        "overlay": metadata,
        "rendered_revision": rendered["editor"]["preview"]["rendered_revision"],
        "module_rendered_revision": module_rendered["editor"]["preview"]["rendered_revision"],
        "custom_rendered_revision": custom_rendered["editor"]["preview"]["rendered_revision"],
        "module_definition_id": module_definition_id,
        "module_instance_id": module_instance_id,
        "published_parameter_id": parameter_id,
        "published_parameter": published_parameter,
        "dope_sheet_lane": dope_lane.get("metadata"),
        "curve_editor_lane": curve_key.get("metadata"),
        "curve_dragged_key": dragged_key,
        "curve_dragged_revision": dragged_state["history"]["revision"],
        "removed_revision": removed["history"]["revision"],
        "restored_revision": restored["history"]["revision"],
        "duplicate_revision": after_duplicate["history"]["revision"],
        "actions": client.evidence,
    }


if __name__ == "__main__":
    raise SystemExit(
        run_suite_main(
            "qa-transition-e2e",
            run_suite,
            "target/qa-transition-e2e-evidence.json",
        )
    )
