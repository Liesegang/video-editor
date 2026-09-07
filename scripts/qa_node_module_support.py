"""Shared production Node Editor document, port, and route assertions."""

import os

from qa_support import (
    QA_APP_BINARY_ENV,
    QaClient,
    QaFailure,
    activate_dock_tab,
    bring_timeline_component,
    capture_viewport,
    close_clean_native_app,
    component_center,
    find_clear_canvas_point,
    free_port,
    media_seconds,
    request_clean_native_close,
    seek_timeline_seconds,
    settled_preview_state,
    spawned_authoring_app,
)


def component(snapshot, component_id):
    return next(
        (
            candidate
            for candidate in snapshot["components"]
            if candidate.get("id") == component_id
        ),
        None,
    )


def enter_exact_numeric(client, component_id, value, after_action=None):
    """Replace one production numeric editor value without losing integer precision."""

    client.click_component(component_id)
    if after_action is not None:
        after_action("click")
    client.key("a", True, command=True)
    client.key("a", False, command=True)
    client.inject("text", {"text": str(value)})
    if after_action is not None:
        after_action("text")
    client.key("enter", True)
    client.key("enter", False)


def open_timeline_item_definition(client, item_id, expected_host, description):
    """Open one Timeline Item's production Module document."""

    bring_timeline_component(client, "timeline.item:" + item_id, -120.0)
    client.double_click_component("timeline.item:" + item_id)
    activate_dock_tab(client, "dock.tab:node_editor", "Node Editor", description)
    client.wait_component_settled("node_editor.canvas")
    return active_definition(client.state(), expected_host)


def sample_rendered_preview(
    client, seconds, revision, description, require_visible=True
):
    """Render and return one exact revision/frame Preview sample."""

    activate_dock_tab(client, "dock.tab:timeline", "Timeline", description)
    sought = seek_timeline_seconds(client, seconds)
    frame = sought["editor"]["timeline"]["current_frame"]

    def rendered():
        return settled_preview_state(
            client, revision, frame, require_visible=require_visible
        )

    state = client.wait_until(description + " rendered Preview", rendered, 30.0)
    preview = state["editor"]["preview"]
    return {
        "seconds": seconds,
        "frame": frame,
        "pixel_hash": preview["pixel_hash"],
        "nontransparent_pixels": preview["nontransparent_pixels"],
    }


def reload_node_clip_project(
    client,
    project_file,
    saved,
    item_id,
    definition_id,
    samples,
    artifact_dir,
    domain,
    run_id_default,
    capture_filename,
    validate_definition,
):
    """Reload one saved Node Clip and prove Project, pixels, and graph identity."""

    initial_close = request_clean_native_close(
        client, domain + " authoring", client.timeout
    )
    port_number = free_port()
    environment = {
        "RUVIE_QA_PROJECT_PATH": str(project_file),
        "RUVIE_QA_OPEN_EXISTING_PROJECT": "1",
        "RUVIE_QA_PORT_FILE": None,
        "RUVIE_QA_RUN_ID": os.environ.get("RUVIE_QA_RUN_ID", run_id_default)
        + ":reload",
        QA_APP_BINARY_ENV: os.environ.get(QA_APP_BINARY_ENV),
    }
    with spawned_authoring_app(port_number, environment) as process:
        fresh = QaClient("http://127.0.0.1:{}".format(port_number), client.timeout)
        fresh.wait_health()
        state = fresh.state()
        if state["project"] != saved["project"]:
            raise QaFailure("fresh process changed " + domain + " Project state")
        revision = state["history"]["revision"]
        reloaded_samples = [
            sample_rendered_preview(
                fresh, sample["seconds"], revision, "reloaded " + domain
            )
            for sample in samples
        ]
        if [sample["pixel_hash"] for sample in reloaded_samples] != [
            sample["pixel_hash"] for sample in samples
        ]:
            raise QaFailure("fresh process changed " + domain + " pixels")
        reloaded_id, definition = open_timeline_item_definition(
            fresh, item_id, "node_clip", domain + " reload"
        )
        if reloaded_id != definition_id:
            raise QaFailure("fresh process changed " + domain + " Definition identity")
        validate_definition(definition)
        capture = capture_viewport(fresh, artifact_dir / capture_filename)
        reload_close = close_clean_native_app(
            fresh, process, "reloaded " + domain + " app", client.timeout
        )
    return {
        "initial_close": initial_close,
        "reload_close": reload_close,
        "samples": reloaded_samples,
        "capture": capture,
    }


def place_private_node_clip_source(
    client,
    source_snapshot,
    source,
    requested_start_seconds,
    description,
):
    """Drag one production Assets source into one exact private Module Item."""

    initial = client.state()
    project = initial["project"]
    _, canvas = client.wait_component("timeline.canvas")
    track = next(
        (
            candidate
            for candidate in source_snapshot["components"]
            if candidate.get("type") == "timeline_track"
            and candidate.get("visible") is True
            and candidate.get("enabled") is True
            and float((candidate.get("rect_points") or {}).get("height", 0.0)) > 0.0
        ),
        None,
    )
    if track is None:
        raise QaFailure("Timeline exposed no production Track drop target")
    canvas_rect = canvas["rect_points"]
    timeline = initial["editor"]["timeline"]
    target = {
        "x": canvas_rect["min_x"]
        + requested_start_seconds * float(timeline["pixels_per_second"])
        - float(timeline["horizontal_scroll"]),
        "y": component_center(track)["y"],
    }
    if not canvas_rect["min_x"] < target["x"] < canvas_rect["max_x"]:
        raise QaFailure(description + " drop target is outside the Timeline viewport")

    before = {
        collection: dict(project[collection])
        for collection in ("items", "module_definitions", "module_instances")
    }
    before_revision = initial["history"]["revision"]
    start = component_center(source)
    client.drag(start, target, steps=18)

    def placed():
        state = client.state()
        current = state["project"]
        return (
            state
            if all(len(current[key]) == len(values) + 1 for key, values in before.items())
            and state["history"]["revision"] == before_revision + 1
            and state["editor"]["timeline"]["library_drag_active"] is False
            else None
        )

    placed_state = client.wait_until(description + " placement", placed)
    authored = placed_state["project"]
    added = {
        collection: set(authored[collection]) - set(previous)
        for collection, previous in before.items()
    }
    if any(len(ids) != 1 for ids in added.values()):
        raise QaFailure(description + " did not create one Item, Definition, and Instance")
    item_id = next(iter(added["items"]))
    definition_id = next(iter(added["module_definitions"]))
    instance_id = next(iter(added["module_instances"]))
    item = authored["items"][item_id]
    invocation = item.get("source") or {}
    if invocation.get("kind") != "module" or (invocation.get("value") or {}).get(
        "instance_id"
    ) != instance_id:
        raise QaFailure(description + " Item and Module Instance identities disagree")
    instance = authored["module_instances"][instance_id]
    if instance.get("definition_id") != definition_id:
        raise QaFailure(description + " Instance points at a different Definition")
    definition = authored["module_definitions"][definition_id]
    if definition.get("sharing") != {"kind": "private"}:
        raise QaFailure(description + " Definition is not private to its Timeline Item")
    for collection, previous in before.items():
        for existing_id, existing in previous.items():
            if authored[collection].get(existing_id) != existing:
                raise QaFailure(description + " mutated existing " + collection)
    if media_seconds(item["interval"]["start"]) != requested_start_seconds:
        raise QaFailure(description + " changed its exact requested start")
    return {
        "initial": initial,
        "state": placed_state,
        "source": source,
        "drag": {"from": start, "to": target},
        "item_id": item_id,
        "item": item,
        "definition_id": definition_id,
        "definition": definition,
        "instance_id": instance_id,
        "instance": instance,
    }


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


def connect_nodes(
    client,
    expected_host,
    source_id,
    source_port,
    target_id,
    target_port,
    description,
):
    """Connect two exact typed ports through the production Node surface."""

    def visible_ports():
        snapshot = client.component_snapshot()
        source = port(snapshot, "output", source_id, port_key=source_port)
        target = port(snapshot, "input", target_id, port_key=target_port)
        return (source, target) if source and target else None

    source, target = client.wait_until(description + " ports", visible_ports)
    client.drag(component_center(source), component_center(target), steps=10)

    def connected():
        definition = active_definition(client.state(), expected_host)[1]
        return next(
            (
                candidate
                for candidate in definition["graph"]["connections"]
                if candidate["from"]
                == {"node_id": source_id, "port": source_port}
                and candidate["to"]
                == {"node_id": target_id, "port": target_port}
            ),
            None,
        )

    return client.wait_until(description + " connection", connected)


def disconnect_node_connection(client, expected_host, connection_id, description):
    """Disconnect one exact production wire and wait for its authored removal."""

    client.click_component("node_editor.connection:" + connection_id, button="secondary")
    client.wait_component_settled("node_editor.wire_menu.disconnect")
    client.click_component("node_editor.wire_menu.disconnect")

    def disconnected():
        state = client.state()
        definition = active_definition(state, expected_host)[1]
        return (
            state
            if all(
                candidate.get("id") != connection_id
                for candidate in definition["graph"]["connections"]
            )
            else None
        )

    return client.wait_until(description + " disconnected", disconnected)


def publish_node_input_parameter(
    client,
    expected_host,
    node_id,
    port_key,
    trigger_component_id,
    expected_name,
    expected_default,
):
    """Publish one exact Node input through the production interface menu."""

    before = client.state()
    definition_id, definition = active_definition(before, expected_host)
    before_parameters = definition["interface"]["parameters"]
    if any(
        candidate.get("target") == {"node_id": node_id, "port": port_key}
        for candidate in before_parameters
    ):
        raise QaFailure("Node input is already a Published parameter")
    client.click_component(trigger_component_id, button="secondary")
    action_id = (
        "node_editor.interface_action.node:{}.input:{}:publish_parameter".format(
            node_id, port_key
        )
    )
    _, action = client.wait_component_settled(action_id)
    metadata = action.get("metadata") or {}
    if (
        metadata.get("action") != "publish_parameter"
        or metadata.get("node_id") != node_id
        or metadata.get("port") != port_key
        or metadata.get("label") != expected_name
        or action.get("enabled") is not True
    ):
        raise QaFailure("Publish action lost its exact Node input identity")
    client.click_component(action_id)
    before_ids = {candidate["id"] for candidate in before_parameters}

    def published():
        state = client.state()
        candidate = state["project"]["module_definitions"].get(definition_id)
        if candidate is None:
            return None
        added = [
            parameter
            for parameter in candidate["interface"]["parameters"]
            if parameter["id"] not in before_ids
        ]
        return (
            (state, added[0])
            if state["history"]["revision"] == before["history"]["revision"] + 1
            and len(added) == 1
            else None
        )

    state, parameter = client.wait_until(
        "publish exact {} Node input".format(expected_name), published
    )
    if (
        parameter.get("name") != expected_name
        or parameter.get("default_value") != expected_default
        or parameter.get("target") != {"node_id": node_id, "port": port_key}
    ):
        raise QaFailure("Published parameter changed its Node input contract")
    return state, parameter


def unpublish_node_input_parameter(
    client,
    definition_id,
    instance_id,
    node_id,
    port_key,
    parameter,
    before,
):
    """Unpublish one exact Node input through its shared production row menu."""

    port_id = "node_editor.interface_port.node:{}.input:{}".format(node_id, port_key)
    action_id = (
        "node_editor.interface_action.node:{}.input:{}:unpublish_parameter".format(
            node_id, port_key
        )
    )
    client.click_component(port_id, button="secondary")
    _, action = client.wait_component_settled(action_id)
    metadata = action.get("metadata") or {}
    if (
        metadata.get("action") != "unpublish_parameter"
        or metadata.get("node_id") != node_id
        or metadata.get("port") != port_key
        or metadata.get("parameter_id") != parameter["id"]
    ):
        raise QaFailure("Unpublish action lost its exact Node input identity")

    reset_id = "node_editor.interface_action.node:{}.input:{}:reset_parameter".format(
        node_id, port_key
    )
    reset = component(client.component_snapshot(), reset_id)
    reset_metadata = (reset or {}).get("metadata") or {}
    if (
        reset is None
        or reset.get("visible") is not True
        or reset.get("enabled") is not False
        or reset_metadata.get("action") != "reset_parameter"
        or reset_metadata.get("node_id") != node_id
        or reset_metadata.get("port") != port_key
        or reset_metadata.get("parameter_id") != parameter["id"]
        or reset_metadata.get("instance_id") != instance_id
    ):
        raise QaFailure("Published Node input menu lost its disabled exact Reset action")
    client.click_component(action_id)

    expected = [
        candidate
        for candidate in before["interface"]["parameters"]
        if candidate["id"] != parameter["id"]
    ]

    def unpublished():
        state = client.state()
        definition = state["project"]["module_definitions"][definition_id]
        return state if definition["interface"]["parameters"] == expected else None

    state = client.wait_until("unpublish exact Node input parameter", unpublished)
    _, socket = client.wait_component_settled(
        "node_editor.port.node:{}.input:{}".format(node_id, port_key)
    )
    socket_metadata = socket.get("metadata") or {}
    if (
        socket_metadata.get("input_ownership") != "internal"
        or socket_metadata.get("connectable") is not True
        or socket_metadata.get("connected") is not False
    ):
        raise QaFailure("Unpublished Node input socket did not become internal")
    return state


def reset_published_node_input_parameter(
    client, definition_id, instance_id, node_id, port_key, parameter, before
):
    """Reset one exact published Node input before changing its interface ownership."""

    port_id = "node_editor.interface_port.node:{}.input:{}".format(node_id, port_key)
    action_id = (
        "node_editor.interface_action.node:{}.input:{}:reset_parameter".format(
            node_id, port_key
        )
    )
    client.click_component(port_id, button="secondary")
    _, action = client.wait_component_settled(action_id)
    metadata = action.get("metadata") or {}
    if (
        action.get("enabled") is not True
        or metadata.get("action") != "reset_parameter"
        or metadata.get("node_id") != node_id
        or metadata.get("port") != port_key
        or metadata.get("parameter_id") != parameter["id"]
        or metadata.get("instance_id") != instance_id
    ):
        raise QaFailure("Published Node input Reset lost its exact identity")
    client.click_component(action_id)

    def reset():
        state = client.state()
        definition = state["project"]["module_definitions"].get(definition_id)
        overrides = state["project"]["module_instances"][instance_id].get(
            "parameter_overrides", {}
        )
        return (
            state
            if state["history"]["revision"] == before["history"]["revision"] + 1
            and parameter["id"] not in overrides
            and definition == before["project"]["module_definitions"][definition_id]
            else None
        )

    return client.wait_until("reset exact published Node input", reset)


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


def ensure_node_editor_authoring_scale(client, minimum=0.4, maximum=0.75):
    """Reach a detail-visible authoring scale through the shared canvas controller."""

    before = client.state()
    previous = None
    for _ in range(8):
        _, canvas = client.wait_component_settled("node_editor.canvas")
        scale = float((canvas.get("metadata") or {}).get("scale", 0.0))
        if minimum <= scale <= maximum:
            after = client.state()
            if after["project"] != before["project"] or after["history"] != before["history"]:
                raise QaFailure("Node Editor zoom mutated the Project or undo history")
            return canvas
        delta = 100.0 if scale < minimum else -100.0
        client.scroll_component(
            "node_editor.canvas", 0.0, delta, modifiers={"command": True}
        )
        _, changed = client.wait_component_settled("node_editor.canvas")
        changed_scale = float((changed.get("metadata") or {}).get("scale", 0.0))
        if (delta > 0.0 and changed_scale <= scale) or (
            delta < 0.0 and changed_scale >= scale
        ):
            raise QaFailure("Node Editor authoring zoom moved in the wrong direction")
        if previous == changed_scale:
            raise QaFailure("Node Editor authoring zoom stopped making progress")
        previous = changed_scale
    raise QaFailure("Node Editor could not reach a usable authoring scale")


def place_created_node(client, node_id, horizontal_fraction, vertical_offset=26.0):
    """Move a just-created Node to a stable visible canvas fraction."""

    _, canvas = client.wait_component_settled("node_editor.canvas")
    _bring_node_header_into_view(client, node_id)
    snapshot, header = client.wait_component_settled(
        "node_editor.node_header:" + node_id
    )
    bounds = canvas["rect_points"]
    header_bounds = header["rect_points"]
    margin = 4.0
    half_width = float(header_bounds.get("width", 0.0)) * 0.5
    half_height = float(header_bounds.get("height", 0.0)) * 0.5
    if (
        half_width * 2.0 + margin * 2.0 > float(bounds["width"])
        or half_height * 2.0 + margin * 2.0 > float(bounds["height"])
    ):
        raise QaFailure("Node header is larger than the production canvas")
    requested_x = float(bounds["min_x"]) + float(bounds["width"]) * horizontal_fraction
    requested_y = float(bounds["min_y"]) + vertical_offset
    target = {
        "x": max(
            float(bounds["min_x"]) + half_width + margin,
            min(float(bounds["max_x"]) - half_width - margin, requested_x),
        ),
        "y": max(
            float(bounds["min_y"]) + half_height + margin,
            min(float(bounds["max_y"]) - half_height - margin, requested_y),
        ),
    }
    origin = component_center(header)
    blocked = []
    for candidate in snapshot["components"]:
        candidate_id = candidate.get("id", "")
        rect = candidate.get("rect_points") or {}
        if (
            candidate_id.startswith("node_editor.node:")
            and candidate_id != "node_editor.node:" + node_id
            and candidate.get("visible")
            and float(rect.get("min_x", origin["x"] + 1.0)) <= origin["x"]
            <= float(rect.get("max_x", origin["x"] - 1.0))
            and float(rect.get("min_y", origin["y"] + 1.0)) <= origin["y"]
            <= float(rect.get("max_y", origin["y"] - 1.0))
        ):
            blocked.append({"id": candidate_id, "rect_points": rect})
    if blocked:
        raise QaFailure(
            "Node header drag origin is occluded: node={!r}, origin={!r}, blockers={!r}".format(
                node_id, origin, blocked
            )
        )
    client.drag(origin, target, steps=10)
    _, moved = client.wait_component_settled("node_editor.node_header:" + node_id)
    if abs(float(moved["rect_points"]["center_x"]) - target["x"]) > 3.0:
        raise QaFailure("created Node did not follow its production header drag")
    return moved


def _node_editor_layout_violations(canvas_rect, rects):
    clipped = []
    for node_id, rect in rects.items():
        if (
            float(rect["min_x"]) < float(canvas_rect["min_x"]) - 0.5
            or float(rect["max_x"]) > float(canvas_rect["max_x"]) + 0.5
            or float(rect["min_y"]) < float(canvas_rect["min_y"]) - 0.5
            or float(rect["max_y"]) > float(canvas_rect["max_y"]) + 0.5
        ):
            clipped.append(node_id)

    overlaps = []
    ordered_ids = list(rects)
    for index, left_id in enumerate(ordered_ids):
        left = rects[left_id]
        for right_id in ordered_ids[index + 1 :]:
            right = rects[right_id]
            overlap_x = min(float(left["max_x"]), float(right["max_x"])) - max(
                float(left["min_x"]), float(right["min_x"])
            )
            overlap_y = min(float(left["max_y"]), float(right["max_y"])) - max(
                float(left["min_y"]), float(right["min_y"])
            )
            if overlap_x > 0.5 and overlap_y > 0.5:
                overlaps.append(
                    {
                        "left": left_id,
                        "right": right_id,
                        "width": overlap_x,
                        "height": overlap_y,
                    }
                )
    return {"clipped": clipped, "overlaps": overlaps}


def assert_node_editor_nodes_do_not_overlap(client, node_ids, description):
    """Assert fit-rendered Node bodies are complete and have disjoint interiors."""

    _, canvas = client.wait_component_settled("node_editor.canvas")
    snapshot = client.component_snapshot()
    components = {entry["id"]: entry for entry in snapshot["components"]}
    rects = {}
    for node_id in dict.fromkeys(node_ids):
        component_id = "node_editor.node:" + node_id
        node_component = components.get(component_id)
        if not node_component or not node_component.get("visible"):
            raise QaFailure("{} Node is not visibly rendered: {}".format(description, node_id))
        bounds = ((node_component.get("metadata") or {}).get("screen_bounds") or {})
        try:
            x = float(bounds["x"])
            y = float(bounds["y"])
            width = float(bounds["width"])
            height = float(bounds["height"])
        except (KeyError, TypeError, ValueError) as error:
            raise QaFailure(
                "{} Node omitted authoritative unclipped screen bounds: {}".format(
                    description, node_id
                )
            ) from error
        if width <= 0.0 or height <= 0.0:
            raise QaFailure("{} Node has no rendered body: {}".format(description, node_id))
        rect = {
            "min_x": x,
            "min_y": y,
            "max_x": x + width,
            "max_y": y + height,
            "width": width,
            "height": height,
        }
        rects[node_id] = rect

    violations = _node_editor_layout_violations(canvas["rect_points"], rects)
    if violations["clipped"]:
        raise QaFailure(
            "{} rendered Nodes are clipped outside the fitted canvas: {!r}".format(
                description, violations["clipped"]
            )
        )
    if violations["overlaps"]:
        raise QaFailure(
            "{} rendered Node bodies overlap: {!r}".format(
                description, violations["overlaps"]
            )
        )
    return {
        "frame": snapshot.get("frame"),
        "canvas_rect": canvas["rect_points"],
        "node_rects": rects,
    }


def _bring_node_header_into_view(client, node_id):
    """Pan the production canvas until one authored Node header is interactable."""

    header_id = "node_editor.node_header:" + node_id
    for _ in range(12):
        _, canvas = client.wait_component_settled("node_editor.canvas")
        definition = active_definition(client.state())[1]
        node = definition["graph"]["nodes"].get(node_id)
        if node is None:
            raise QaFailure("Node Editor cannot reveal a missing Node " + node_id)
        metadata = canvas.get("metadata") or {}
        scale = float(metadata.get("scale", 0.0))
        translation = metadata.get("translation") or {}
        position = node.get("ui_position") or []
        size = node.get("ui_size") or []
        if scale <= 0.0 or len(position) != 2 or len(size) != 2:
            raise QaFailure("Node Editor omitted the Canvas transform or Node geometry")
        bounds = canvas["rect_points"]
        current = {
            "x": float(translation["x"]) + float(position[0]) * scale,
            "y": float(translation["y"]) + float(position[1]) * scale,
        }
        model_width = float(size[0]) * scale
        snapshot = client.component_snapshot()
        header = component(snapshot, header_id)
        if header is not None and header.get("visible"):
            rect = header.get("rect_points") or {}
            # QA response rects are clipped to the viewport. Use the authored
            # Node width through the same Canvas transform to distinguish a
            # genuinely visible header from a clipped fragment.
            fully_inside = (
                current["x"] >= float(bounds["min_x"])
                and current["x"] + model_width + 12.0 <= float(bounds["max_x"])
                and current["y"] >= float(bounds["min_y"])
                and float(rect.get("max_y", current["y"])) <= float(bounds["max_y"])
            )
            if (
                float(rect.get("width", 0.0)) > 0.0
                and float(rect.get("height", 0.0)) > 0.0
                and fully_inside
            ):
                return header
        target = {
            "x": float(bounds["center_x"]) - model_width * 0.5,
            "y": float(bounds["min_y"]) + 42.0,
        }
        limit_x = float(bounds["width"]) * 0.35
        limit_y = float(bounds["height"]) * 0.35
        delta = {
            "x": max(-limit_x, min(limit_x, target["x"] - current["x"])),
            "y": max(-limit_y, min(limit_y, target["y"] - current["y"])),
        }
        origin = {"x": float(bounds["center_x"]), "y": float(bounds["center_y"])}
        client.drag(
            origin,
            {"x": origin["x"] + delta["x"], "y": origin["y"] + delta["y"]},
            steps=10,
            button="middle",
        )
    raise QaFailure("Node Editor could not pan the requested Node header into view")


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

    ensure_node_editor_authoring_scale(client)
    header_id = "node_editor.node_header:" + node_id
    header = _bring_node_header_into_view(client, node_id)
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
