"""Reusable real-coordinate Sound graph actions and canonical-wire assertions.

The functions in this module author only through the loopback QA bridge. They
read Project state to prove outcomes, never to mutate the graph directly.
"""

import math


def connection_source_owner(connection):
    owner = connection["from"]["owner"]
    return "{}:{}".format(owner["owner_type"].lower(), owner["owner_id"])


def merge_connections(project, merge_id, port):
    return sorted(
        (
            connection
            for connection in project["connections"]
            if connection["to"]["owner"].get("owner_type") == "Node"
            and connection["to"]["owner"].get("owner_id") == merge_id
            and connection["to"].get("port") == port
        ),
        key=lambda connection: (connection["order"], connection["id"]),
    )


def assert_wire_identity(base, before, after, operation):
    before_by_id = {connection["id"]: connection for connection in before}
    after_by_id = {connection["id"]: connection for connection in after}
    if set(before_by_id) != set(after_by_id):
        raise base.QaFailure("{} changed canonical wire IDs".format(operation))
    for connection_id, original in before_by_id.items():
        current = after_by_id[connection_id]
        for field in ("id", "from", "to", "blend_mode"):
            if current[field] != original[field]:
                raise base.QaFailure(
                    "{} changed {} on wire {}".format(
                        operation, field, connection_id
                    )
                )


def _point(rect, x_fraction=0.5, y_fraction=0.5):
    return {
        "x": rect["min_x"] + rect["width"] * x_fraction,
        "y": rect["min_y"] + rect["height"] * y_fraction,
    }


def _contains(rect, point, margin=0.0):
    return (
        rect["min_x"] + margin <= point["x"] <= rect["max_x"] - margin
        and rect["min_y"] + margin <= point["y"] <= rect["max_y"] - margin
    )


def cubic_bezier_point(curve, t):
    start, control_a, control_b, end = curve
    inverse = 1.0 - t
    return {
        "x": inverse**3 * start["x"]
        + 3.0 * inverse**2 * t * control_a["x"]
        + 3.0 * inverse * t**2 * control_b["x"]
        + t**3 * end["x"],
        "y": inverse**3 * start["y"]
        + 3.0 * inverse**2 * t * control_a["y"]
        + 3.0 * inverse * t**2 * control_b["y"]
        + t**3 * end["y"],
    }


def _curve_from_metadata(metadata):
    points = (
        metadata.get("from"),
        metadata.get("control_a"),
        metadata.get("control_b"),
        metadata.get("to"),
    )
    if any(point is None for point in points):
        return None
    return points


def _distance(left, right):
    return math.hypot(left["x"] - right["x"], left["y"] - right["y"])


def _distance_to_curve(point, curve, samples=64):
    return min(
        _distance(point, cubic_bezier_point(curve, index / samples))
        for index in range(samples + 1)
    )


def unobstructed_wire_point(base, snapshot, edge_id):
    """Choose a fresh point unique to one visible Bezier and outside Node chrome."""
    components = {component["id"]: component for component in snapshot["components"]}
    try:
        edge = components[edge_id]
        canvas = components["node_editor.canvas"]
    except KeyError as error:
        raise base.QaFailure("wire selection component {} is absent".format(error)) from error
    curve = _curve_from_metadata(edge.get("metadata") or {})
    if curve is None:
        raise base.QaFailure("wire {!r} omitted cubic control points".format(edge_id))

    blocking_prefixes = (
        "node_editor.node:",
        "node_editor.node_header:",
        "node_editor.container_header.",
        "node_editor.port.",
        "node_editor.container_port.",
        "node_editor.resize_edge.",
    )
    blockers = [
        component["rect_points"]
        for component in snapshot["components"]
        if component.get("visible", False)
        and component["id"].startswith(blocking_prefixes)
    ]
    other_curves = []
    for component in snapshot["components"]:
        metadata = component.get("metadata") or {}
        if component["id"] == edge_id or metadata.get("kind") not in (
            "explicit",
            "output_binding",
        ):
            continue
        other = _curve_from_metadata(metadata)
        if other is not None:
            other_curves.append(other)

    candidates = []
    for index in range(2, 19):
        t = index / 20.0
        point = cubic_bezier_point(curve, t)
        if not _contains(canvas["rect_points"], point, margin=2.0):
            continue
        if any(_contains(rect, point) for rect in blockers):
            continue
        separation = min(
            (_distance_to_curve(point, other) for other in other_curves),
            default=float("inf"),
        )
        if separation > 10.0:
            candidates.append((separation, abs(0.5 - t), point))
    if not candidates:
        raise base.QaFailure(
            "wire {!r} has no unobstructed unique coordinate".format(edge_id)
        )
    return max(candidates, key=lambda item: (item[0], -item[1]))[2]


def select_connection_wire(base, client, connection_id):
    edge_id = "node_editor.edge:" + connection_id
    base.reveal_node_editor_component(client, edge_id)
    snapshot = client.component_snapshot()
    point = unobstructed_wire_point(base, snapshot, edge_id)
    client.inject(
        "click",
        {
            "x": point["x"],
            "y": point["y"],
            "coordinate_space": "points",
            "button": "primary",
        },
        {
            "component_id": edge_id,
            "component_frame": snapshot["frame"],
            "coordinate_reason": "fresh unobstructed unique cubic point",
            "component_hit_point": point,
        },
    )
    return client.wait_until(
        "wire coordinate selection before endpoint reconnect",
        lambda: state
        if (state := client.state())["editor"]["node_editor"][
            "selected_connection_id"
        ]
        == connection_id
        else None,
    )


def connect_node_wire(
    base,
    client,
    source_node_id,
    source_port,
    target_node_id,
    target_port,
    description,
    steps=16,
):
    source_id = "node_editor.port.node:{}.output:{}".format(
        source_node_id, source_port
    )
    target_id = "node_editor.port.node:{}.input:{}".format(
        target_node_id, target_port
    )
    base.ensure_node_editor_ports_interactive(client, [source_id, target_id])
    before = client.state()
    connection_ids_before = {
        connection["id"] for connection in before["project"]["connections"]
    }
    client.drag_components(source_id, target_id, steps=steps)

    def connected(project):
        try:
            return base.find_project_connection(
                project,
                "Node",
                source_node_id,
                source_port,
                "Node",
                target_node_id,
                target_port,
            )
        except base.QaFailure:
            return None

    state = client.wait_project(description, connected)
    connection = connected(state["project"])
    if connection["id"] in connection_ids_before:
        raise base.QaFailure(
            "{} did not create a new canonical connection".format(description)
        )
    base.assert_history_delta(before, state, 1, description)
    return connection, state


def set_node_number(base, client, node_id, property_key, value, description):
    """Edit a numeric Node control through its real egui text-entry gesture."""
    component_id = "node_editor.property.node:{}:{}".format(node_id, property_key)
    header_id = "node_editor.node_header:" + node_id
    input_id = "node_editor.port.node:{}.input:{}".format(node_id, property_key)
    # At overview zoom the body controls are intentionally not rendered. Use
    # the always-published typed input socket to zoom into detail first.
    base.ensure_node_editor_ports_interactive(client, [input_id])
    base.reveal_node_editor_component(client, component_id)
    before = client.state()
    client.replace_component_text(component_id, str(value))
    # Moving focus to the same Node header closes the DragValue text editor and
    # commits the continuous edit as exactly one undoable Project mutation.
    base.reveal_node_editor_component(client, header_id)
    client.click_component(header_id)

    def authored(project):
        property_value = (
            project["nodes"][node_id]
            .get("properties", {})
            .get(property_key, {})
            .get("properties", {})
            .get("value")
        )
        return project if property_value == value else None

    state = client.wait_project(description, authored)
    base.assert_history_delta(before, state, 1, description)
    return state


def connect_audio_source(base, client, source_node_id, merge_id, description):
    source_id = "node_editor.port.node:{}.output:audio".format(source_node_id)
    target_id = "node_editor.port.node:{}.input:sounds".format(merge_id)
    base.ensure_node_editor_ports_interactive(client, [source_id, target_id])
    before = client.state()
    before_ids = {
        connection["id"]
        for connection in merge_connections(before["project"], merge_id, "sounds")
    }
    client.drag_components(source_id, target_id, steps=16)

    def added(project):
        matches = [
            connection
            for connection in merge_connections(project, merge_id, "sounds")
            if connection["id"] not in before_ids
            and connection_source_owner(connection) == "node:" + source_node_id
            and connection["from"]["port"] == "audio"
        ]
        return matches[0] if len(matches) == 1 else None

    state = client.wait_project(description, added)
    connection = added(state["project"])
    base.assert_history_delta(before, state, 1, description)
    return connection, state


def reconnect_sound_source(
    base, client, connection, original_source_id, alternate_source_id
):
    connection_id = connection["id"]
    edge_id = "node_editor.edge:" + connection_id
    source_handle = edge_id + ".from_handle"
    alternate_output = "node_editor.port.node:{}.output:audio".format(
        alternate_source_id
    )
    select_connection_wire(base, client, connection_id)
    base.reveal_node_editor_components(client, [source_handle, alternate_output])
    before = client.state()
    original = base.project_connection(before["project"], connection_id)
    if original is None or connection_source_owner(original) != "node:" + original_source_id:
        raise base.QaFailure("Sound reconnect baseline endpoint is stale")
    client.drag_components(source_handle, alternate_output, steps=16)

    def uses_alternate(project):
        current = base.project_connection(project, connection_id)
        return (
            current
            if current is not None
            and connection_source_owner(current) == "node:" + alternate_source_id
            else None
        )

    reconnected = client.wait_project("Sound wire source endpoint reconnect", uses_alternate)
    moved = base.project_connection(reconnected["project"], connection_id)
    if not (
        moved["id"] == original["id"]
        and moved["to"] == original["to"]
        and moved["order"] == original["order"]
        and moved["blend_mode"] == original["blend_mode"]
    ):
        raise base.QaFailure("Sound endpoint reconnect changed UUID/order/target/blend")
    base.assert_history_delta(before, reconnected, 1, "Sound wire endpoint reconnect")
    undone = base.undo_project_edit(
        client,
        "Sound wire endpoint reconnect",
        lambda project: base.project_connection(project, connection_id) == original,
    )
    redone = base.redo_project_edit(
        client,
        "Sound wire endpoint reconnect",
        lambda project: uses_alternate(project) is not None,
    )
    final = base.undo_project_edit(
        client,
        "Sound wire endpoint reconnect final restore",
        lambda project: base.project_connection(project, connection_id) == original,
    )
    if not (
        undone["history"]["undo_depth"] == before["history"]["undo_depth"]
        and redone["history"]["undo_depth"]
        == reconnected["history"]["undo_depth"]
        and final["history"]["undo_depth"] == before["history"]["undo_depth"]
    ):
        raise base.QaFailure(
            "Sound reconnect undo/redo changed more than one history entry"
        )
    return {
        "connection_id": connection_id,
        "original_source": original_source_id,
        "alternate_source": alternate_source_id,
        "uuid_preserved": True,
        "history_entries": 1,
        "undo_redo": True,
    }
