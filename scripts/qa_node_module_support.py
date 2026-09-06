"""Shared production Node Editor document and port assertions."""

from qa_support import QaFailure, component_center


def component(snapshot, component_id):
    return next(
        (
            candidate
            for candidate in snapshot["components"]
            if candidate.get("id") == component_id
        ),
        None,
    )


def active_definition(state, expected_host=None):
    document = state["editor"]["node_editor"]["document"]
    if not document or document.get("kind") != "module_definition":
        raise QaFailure("Node Editor did not open an explicit Module document")
    if expected_host is not None and document.get("host") != expected_host:
        raise QaFailure("Node Editor opened the wrong Module host")
    definition_id = document["definition_id"]
    return definition_id, state["project"]["module_definitions"][definition_id]


def port(snapshot, direction, node_id, data_type=None, port_key=None):
    return next(
        (
            candidate
            for candidate in snapshot["components"]
            if candidate.get("type") == "node_editor_port"
            and candidate.get("visible")
            and (candidate.get("metadata") or {}).get("direction") == direction
            and (candidate.get("metadata") or {}).get("node_id") == node_id
            and (
                data_type is None
                or str((candidate.get("metadata") or {}).get("data_type", "")).lower()
                == data_type
            )
            and (
                port_key is None
                or (candidate.get("metadata") or {}).get("port") == port_key
            )
        ),
        None,
    )


def connection(definition, from_node, to_node):
    return next(
        (
            candidate
            for candidate in definition["graph"]["connections"]
            if candidate["from"]["node_id"] == from_node
            and candidate["to"]["node_id"] == to_node
        ),
        None,
    )


def place_node_for_inline_edit(client, node_id, component_id):
    """Use the production header drag to expose one inline Node control."""

    snapshot = client.component_snapshot()
    candidate = component(snapshot, component_id)
    if candidate is not None and candidate.get("visible"):
        return candidate

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
            lambda: current
            if (
                (current := component(client.component_snapshot(), component_id))
                is not None
                and current.get("visible")
            )
            else None,
            timeout=5.0,
        )
    except QaFailure as error:
        current = client.component_snapshot()
        diagnostics = {
            key: component(current, key)
            for key in (
                component_id,
                header_id,
                "node_editor.node:" + node_id,
                "node_editor.canvas",
            )
        }
        raise QaFailure("{}; geometry={!r}".format(error, diagnostics)) from error
