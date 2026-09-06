"""Shared production Node Editor document, port, and route assertions."""

from qa_support import QaFailure, component_center, find_clear_canvas_point


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


def node_content_type(node):
    return str((node.get("content") or {}).get("type", "")).replace("_", "").lower()


def connect_image_nodes(client, from_node, to_node, description="Module Image"):
    def visible_ports():
        snapshot = client.component_snapshot()
        source = port(snapshot, "output", from_node, "image")
        target = port(snapshot, "input", to_node, "image")
        return (source, target) if source and target else None

    source, target = client.wait_until(description + " ports", visible_ports)
    client.drag(component_center(source), component_center(target), steps=10)
    client.wait_until(
        description + " connection",
        lambda: connection(active_definition(client.state())[1], from_node, to_node),
    )


def create_node_from_menu(client, expected_host, query, choice_id):
    """Create one Node through the production searchable canvas menu."""

    _, before = active_definition(client.state(), expected_host)
    nodes_before = set(before["graph"]["nodes"])
    point = find_clear_canvas_point(
        client.component_snapshot(),
        "node_editor.canvas",
        ("node_editor.node:", "node_editor.node_header:"),
    )
    client.inject(
        "click",
        {**point, "button": "secondary", "coordinate_space": "points"},
    )
    client.wait_component_settled("node_editor.menu.search")
    client.click_component("node_editor.menu.search")
    client.inject("text", {"text": query})
    client.wait_component_settled(choice_id)
    client.click_component(choice_id)

    def created():
        definition = active_definition(client.state(), expected_host)[1]
        added = set(definition["graph"]["nodes"]) - nodes_before
        return (definition, next(iter(added))) if len(added) == 1 else None

    return client.wait_until(query + " Node creation", created)


def insert_image_opacity_in_primary_route(client, expected_host):
    """Insert the real Image Opacity Node into a starter Image→Output route."""

    client.wait_component_settled("node_editor.canvas")
    _, before = active_definition(client.state(), expected_host)
    nodes = before["graph"]["nodes"]
    output_ids = [
        node_id
        for node_id, node in nodes.items()
        if node_content_type(node) == "moduleoutput"
    ]
    if len(output_ids) != 1 or len(nodes) != 2:
        raise QaFailure("Image Opacity setup requires one starter source and Output")
    output = output_ids[0]
    source = next(node_id for node_id in nodes if node_id != output)
    original = connection(before, source, output)
    if original is None:
        raise QaFailure("starter Module has no primary Image route")

    created_definition, opacity = create_node_from_menu(
        client,
        expected_host,
        "Image Opacity",
        "node_editor.menu.create.image_opacity",
    )
    if len(created_definition["graph"]["nodes"]) != len(nodes) + 1:
        raise QaFailure("Image Opacity action changed more than one Node")
    client.click_component(
        "node_editor.connection:" + original["id"], button="secondary"
    )
    client.wait_component_settled("node_editor.wire_menu.disconnect")
    client.click_component("node_editor.wire_menu.disconnect")
    client.wait_until(
        "disconnect starter Image route",
        lambda: not connection(active_definition(client.state())[1], source, output),
    )
    connect_image_nodes(client, source, opacity, "source to Image Opacity")
    connect_image_nodes(client, opacity, output, "Image Opacity to Output")
    return opacity


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
            lambda: (
                current
                if (
                    (current := component(client.component_snapshot(), component_id))
                    is not None
                    and current.get("visible")
                )
                else None
            ),
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


def select_nodes(client, node_ids, description="Module Nodes"):
    """Select exactly the requested Nodes through their production headers."""

    for index, node_id in enumerate(node_ids):
        _, header = client.wait_component_settled("node_editor.node_header:" + node_id)
        client.inject(
            "click",
            {
                **component_center(header),
                "button": "primary",
                "coordinate_space": "points",
                "modifiers": {"shift": index > 0},
            },
        )

    def selected():
        state = client.state()
        return (
            state
            if state["editor"]["node_editor"]["selected_node_count"] == len(node_ids)
            else None
        )

    return client.wait_until(description + " exact selection", selected)
