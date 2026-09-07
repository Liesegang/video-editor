"""Save/reopen/restart checks shared by the native Appearance QA suite."""

import os
import pathlib

from qa_node_module_support import (
    assert_node_editor_nodes_do_not_overlap,
    open_timeline_item_definition,
)
from qa_support import (
    AUTHORING_FIXTURE,
    QaClient,
    QaFailure,
    activate_dock_tab,
    bring_timeline_component,
    capture_viewport,
    close_clean_native_app,
    convert_timeline_item_to_node_clip,
    create_basic_timeline_clip,
    free_port,
    media_seconds,
    project_file_evidence,
    request_clean_native_close,
    save_project_to_disk,
    seek_timeline_seconds,
    settled_preview_state,
    spawned_authoring_app,
)


OPEN_EXISTING_PROJECT_ENV = "RUVIE_QA_OPEN_EXISTING_PROJECT"


def assert_canonical_appearance_graph(project, definition_id, output_id, operations):
    definition = project["module_definitions"][definition_id]
    nodes = (definition.get("graph") or {}).get("nodes") or {}
    connections = (definition.get("graph") or {}).get("connections") or []
    operation_ids = [operation["id"] for operation in operations]
    expected_operations = {
        operation["id"]: {
            "component_id": operation["component_id"],
            "category": operation.get("category", "style"),
            "operation": operation.get("operation", "style.apply.v1"),
        }
        for operation in operations
    }

    def content_data(node):
        data = (node.get("content") or {}).get("data")
        return data if isinstance(data, dict) else {}

    def reject_style_ports(value):
        if isinstance(value, dict):
            if value.get("data_type") == "Style":
                raise QaFailure("converted Appearance retained a Style-typed port")
            for child in value.values():
                reject_style_ports(child)
        elif isinstance(value, list):
            for child in value:
                reject_style_ports(child)

    reject_style_ports(definition)
    if any(
        content_data(node).get("catalog_id") == "native.appearance-stack"
        for node in nodes.values()
    ):
        raise QaFailure("converted Appearance retained native.appearance-stack")

    def address(connection, side):
        endpoint = connection.get(side) or {}
        return endpoint.get("node_id"), endpoint.get("port")

    def matches(connection, source, target, order):
        return (
            address(connection, "from") == source
            and address(connection, "to") == target
            and connection.get("order") == order
            and connection.get("blend_mode") == "Normal"
        )

    used = set()

    def require_connection(source, target, order=0):
        found = [
            connection
            for connection in connections
            if matches(connection, source, target, order)
        ]
        if len(found) != 1:
            raise QaFailure(
                "Appearance graph expected one {}:{} -> {}:{} order {} Normal connection, got {}".format(
                    source[0], source[1], target[0], target[1], order, len(found)
                )
            )
        used.add(found[0]["id"])

    merge_ids = {
        node_id
        for node_id, node in nodes.items()
        if (node.get("content") or {}).get("type") == "Merge"
    }
    accumulated = None
    shape_source = None
    used_merges = set()
    for node_id in operation_ids:
        node = nodes.get(node_id)
        if node is None:
            raise QaFailure("converted Appearance operation Node is missing: " + node_id)
        content = node.get("content") or {}
        data = content_data(node)
        expected = expected_operations[node_id]
        component_id = expected["component_id"]
        if (
            content.get("type") != "PluginOperation"
            or data.get("category") != expected["category"]
            or data.get("component_id") != component_id
            or data.get("operation") != expected["operation"]
        ):
            raise QaFailure(
                "converted Appearance Node {} is not the expected {} Plugin operation".format(
                    node_id, component_id
                )
            )
        ports = data.get("declared_ports") or []
        port_contract = {
            (port.get("key"), port.get("direction"), port.get("data_type"))
            for port in ports
        }
        if ("image", "Output", "Image") not in port_contract:
            raise QaFailure(component_id + " does not expose its canonical Image output")
        output = (node_id, "image")
        if component_id in {"fill", "stroke"}:
            if ("shape_in", "Input", "Shape") not in port_contract:
                raise QaFailure(component_id + " is not a Shape-to-Image branch")
            inbound = [
                connection
                for connection in connections
                if address(connection, "to") == (node_id, "shape_in")
            ]
            if len(inbound) != 1:
                raise QaFailure(component_id + " must have exactly one Shape input")
            candidate_source = address(inbound[0], "from")
            if candidate_source[1] != "shape":
                raise QaFailure(component_id + " is not fed from a Shape output")
            if shape_source is None:
                shape_source = candidate_source
            elif shape_source != candidate_source:
                raise QaFailure("Fill/Stroke branches do not share one Shape source")
            require_connection(shape_source, (node_id, "shape_in"))
            if accumulated is None:
                accumulated = output
            else:
                candidates = [
                    merge_id
                    for merge_id in merge_ids - used_merges
                    if any(
                        matches(connection, accumulated, (merge_id, "images"), 0)
                        for connection in connections
                    )
                    and any(
                        matches(connection, output, (merge_id, "images"), 1)
                        for connection in connections
                    )
                ]
                if len(candidates) != 1:
                    raise QaFailure("Shape Appearance branches lack one ordered Normal Merge")
                merge_id = candidates[0]
                if nodes[merge_id].get("name") != "Appearance Merge":
                    raise QaFailure("Appearance raster branches use an unrelated Merge Node")
                require_connection(accumulated, (merge_id, "images"), 0)
                require_connection(output, (merge_id, "images"), 1)
                used_merges.add(merge_id)
                accumulated = (merge_id, "image")
        else:
            if ("image_in", "Input", "Image") not in port_contract:
                raise QaFailure(component_id + " is not an Image-to-Image operation")
            if accumulated is None:
                if any(
                    address(connection, "to") == (node_id, "image_in")
                    for connection in connections
                ):
                    raise QaFailure("leading Image Appearance unexpectedly has an input")
            else:
                require_connection(accumulated, (node_id, "image_in"))
            accumulated = output

    output_nodes = [
        node_id
        for node_id, node in nodes.items()
        if (node.get("content") or {}).get("type") == "ModuleOutput"
        and content_data(node).get("id") == output_id
    ]
    if accumulated is None or len(output_nodes) != 1:
        raise QaFailure("converted Appearance has no unique Image output")
    require_connection(accumulated, (output_nodes[0], "image_in"))
    if used_merges != merge_ids:
        raise QaFailure("converted Appearance retained an unrelated Merge Node")
    if used != {connection["id"] for connection in connections}:
        raise QaFailure("converted Appearance graph contains connections outside its ordered chain")
    return {
        "operation_ids": operation_ids,
        "component_ids": [
            expected_operations[node_id]["component_id"] for node_id in operation_ids
        ],
        "shape_source": shape_source,
        "merge_ids": sorted(merge_ids),
        "connection_ids": sorted(used),
        "output_node_id": output_nodes[0],
    }


def _constant(operation, key):
    prop = (operation.get("properties") or {}).get(key) or {}
    if prop.get("type") != "constant":
        return None
    return (prop.get("properties") or {}).get("value")


def _project_file():
    value = os.environ.get("RUVIE_QA_PROJECT_PATH")
    if not value:
        raise QaFailure("Appearance persistence QA requires RUVIE_QA_PROJECT_PATH")
    return pathlib.Path(value).resolve()


def _preview_at(client, seconds, revision):
    sought = seek_timeline_seconds(client, seconds)
    frame = sought["editor"]["timeline"]["current_frame"]
    rendered = client.wait_until(
        "Appearance persistence Preview at {:.3f}s".format(seconds),
        lambda: settled_preview_state(client, revision, frame),
        timeout=30.0,
    )
    return {
        "seconds": seconds,
        "frame": frame,
        "pixel_hash": rendered["editor"]["preview"]["pixel_hash"],
        "nontransparent_pixels": rendered["editor"]["preview"][
            "nontransparent_pixels"
        ],
    }


def _save_and_reopen(client, label, preview_seconds):
    before = client.state()
    expected_project = before["project"]
    project_file = _project_file()
    expected_previews = [
        _preview_at(client, seconds, before["history"]["revision"])
        for seconds in preview_seconds
    ]
    saved, saved_file = save_project_to_disk(client, project_file, label)

    client.key("n", True, command=True)
    client.key("n", False, command=True)
    client.wait_until(
        label + " temporary New Project",
        lambda: state
        if (state := client.state())["project"].get("name") == "Untitled Project"
        else None,
    )
    client.key("o", True, command=True)
    client.key("o", False, command=True)
    reopened = client.wait_until(
        label + " reopen",
        lambda: state
        if (state := client.state())["project"] == expected_project
        else None,
    )
    actual_previews = [
        _preview_at(client, expected["seconds"], reopened["history"]["revision"])
        for expected in expected_previews
    ]
    if actual_previews != expected_previews:
        raise QaFailure(label + " Preview pixels changed after reopen")
    return reopened, {
        **saved_file,
        "saved_revision": saved["history"]["revision"],
        "reopened_revision": reopened["history"]["revision"],
        "previews": actual_previews,
    }


def _author_direct_appearance(
    client, kind, expected_name, description, appearance, bring_into_inspector
):
    created_state, item = create_basic_timeline_clip(client, kind, expected_name)
    item_id = item["id"]
    initial = appearance(created_state, item_id)
    if len(initial) != 1 or initial[0]["operation"].get("component_id") != "fill":
        raise QaFailure(
            "new {} did not start with one authoritative Fill".format(description)
        )
    add_id = "inspector.appearance.add_menu:" + item_id
    bring_into_inspector(client, add_id)
    client.click_component(add_id)
    query_id = "inspector.appearance.menu:" + item_id + ".query"
    client.wait_component_settled(query_id)
    client.click_component(query_id)
    client.inject("text", {"text": "stroke"})
    client.click_component("inspector.appearance.add.stroke")

    def added():
        state = client.state()
        operations = appearance(state, item_id)
        return (
            (state, operations[-1])
            if state["history"]["revision"] == created_state["history"]["revision"] + 1
            and [operation["operation"].get("component_id") for operation in operations]
            == ["fill", "stroke"]
            else None
        )

    added_state, stroke = client.wait_until(description + " Stroke add", added)
    stroke_id = stroke["id"]
    width_id = "inspector.property:appearance:{}:{}:width".format(item_id, stroke_id)
    bring_into_inspector(client, width_id)
    before_width = _constant(stroke, "width")
    client.drag_component_by(width_id, 28.0, 0.0, steps=8)

    def edited():
        state = client.state()
        operation = next(
            (
                candidate
                for candidate in appearance(state, item_id)
                if candidate["id"] == stroke_id
            ),
            None,
        )
        return (
            (state, operation)
            if operation is not None
            and state["history"]["revision"] == added_state["history"]["revision"] + 1
            and _constant(operation, "width") != before_width
            else None
        )

    edited_state, edited_stroke = client.wait_until(
        description + " Stroke width edit", edited
    )
    return edited_state, {
        "item_id": item_id,
        "fill_id": initial[0]["id"],
        "stroke_id": stroke_id,
        "stroke_width": _constant(edited_stroke, "width"),
        "active_seconds": media_seconds(item["interval"]["start"]) + 0.2,
    }


def _fresh_process_reload(
    project_file,
    expected_project,
    expected_file,
    expected_previews,
    converted_item_id,
    definition_id,
    output_id,
    expected_facade_operations,
    direct_operations,
    appearance,
    bring_into_inspector,
    timeout,
):
    port = free_port()
    run_id = os.environ.get("RUVIE_QA_RUN_ID", "appearance")
    environment = {
        "RUVIE_QA_FIXTURE": AUTHORING_FIXTURE,
        "RUVIE_QA_PROJECT_PATH": str(project_file),
        OPEN_EXISTING_PROJECT_ENV: "1",
        "RUVIE_QA_PORT_FILE": None,
        "RUVIE_QA_RUN_ID": run_id + ":reload",
    }
    with spawned_authoring_app(port, environment) as process:
        client = QaClient("http://127.0.0.1:{}".format(port), timeout)
        client.wait_health()
        loaded = client.state()
        if loaded["project"] != expected_project:
            raise QaFailure("fresh process loaded a different Appearance Project")
        graph = assert_canonical_appearance_graph(
            loaded["project"], definition_id, output_id, expected_facade_operations
        )
        for item_id, operations, description in direct_operations:
            if appearance(loaded, item_id) != operations:
                raise QaFailure(
                    "fresh process changed direct {} Appearance".format(description)
                )
        actual_previews = [
            _preview_at(client, expected["seconds"], loaded["history"]["revision"])
            for expected in expected_previews
        ]
        if actual_previews != expected_previews:
            raise QaFailure("fresh process changed Appearance Preview pixels")

        activate_dock_tab(
            client,
            "dock.tab:timeline",
            "Timeline",
            "Fresh-process converted Appearance",
        )
        client.click_component("timeline.item:" + converted_item_id)
        section = bring_into_inspector(
            client, "inspector.appearance:" + converted_item_id
        )
        metadata = section.get("metadata") or {}
        if metadata.get("owner_model") != "module_graph":
            raise QaFailure(
                "fresh process did not project converted Appearance from its Module graph"
            )
        if metadata.get("operations") != expected_facade_operations:
            raise QaFailure(
                "fresh process changed converted Appearance IDs/order/values"
            )

        artifact_dir = pathlib.Path(
            os.environ.get(
                "RUVIE_QA_ARTIFACT_DIR",
                pathlib.Path("target") / "qa-appearance-e2e",
            )
        ).resolve()
        artifact_dir.mkdir(parents=True, exist_ok=True)
        capture = capture_viewport(client, artifact_dir / "capture.png")
        close = close_clean_native_app(
            client, process, "reloaded Appearance app", timeout
        )

    file_after_reload = project_file_evidence(
        project_file, "fresh-process Appearance Project"
    )
    if file_after_reload["sha256"] != expected_file["sha256"]:
        raise QaFailure("fresh Appearance process changed the saved Project file")
    return {
        "history": loaded["history"],
        "previews": actual_previews,
        "facade": metadata,
        "graph": graph,
        "file": file_after_reload,
        "capture": capture,
        "close": close,
        "actions": client.evidence,
    }


def exercise_appearance_persistence(
    client,
    text_item_id,
    authored_state,
    appearance,
    bring_into_inspector,
):
    """Persist direct Text/Shape, then converted Node Clip Appearance."""

    text_operations = appearance(authored_state, text_item_id)
    shape_state, shape = _author_direct_appearance(
        client,
        "rectangle",
        "Rectangle",
        "Rectangle",
        appearance,
        bring_into_inspector,
    )
    direct_text_state, direct_text = _author_direct_appearance(
        client,
        "text",
        "Text",
        "direct Text",
        appearance,
        bring_into_inspector,
    )
    shape_operations = appearance(shape_state, shape["item_id"])
    direct_text_operations = appearance(direct_text_state, direct_text["item_id"])
    direct_reopened, direct_persistence = _save_and_reopen(
        client,
        "direct Appearance",
        [3.0, shape["active_seconds"], direct_text["active_seconds"]],
    )
    if appearance(direct_reopened, text_item_id) != text_operations:
        raise QaFailure("direct Text Appearance IDs/order/values changed after reopen")
    if appearance(direct_reopened, shape["item_id"]) != shape_operations:
        raise QaFailure("direct Shape Appearance IDs/order/values changed after reopen")
    if (
        appearance(direct_reopened, direct_text["item_id"])
        != direct_text_operations
    ):
        raise QaFailure("direct Text Appearance IDs/order/values changed after reopen")

    activate_dock_tab(
        client,
        "dock.tab:timeline",
        "Timeline",
        "Appearance persistence conversion",
    )
    bring_timeline_component(client, "timeline.item:" + text_item_id, -120.0)
    client.click_component("timeline.item:" + text_item_id)
    client.wait_until(
        "Appearance conversion target selection",
        lambda: state
        if ((state := client.state())["editor"]["selection"].get("primary") or {}).get(
            "id"
        )
        == text_item_id
        else None,
    )
    converted = convert_timeline_item_to_node_clip(
        client, text_item_id, direct_reopened["history"]["revision"]
    )
    module_source = converted["project"]["items"][text_item_id]["source"]
    instance_id = module_source["value"]["instance_id"]
    definition_id = converted["project"]["module_instances"][instance_id]["definition_id"]
    activate_dock_tab(
        client,
        "dock.tab:timeline",
        "Timeline",
        "Converted Appearance Preview",
    )
    converted_preview = _preview_at(client, 3.0, converted["history"]["revision"])
    if converted_preview["pixel_hash"] != direct_persistence["previews"][0]["pixel_hash"]:
        raise QaFailure("Text Appearance conversion changed Preview pixels before save")
    converted_section = bring_into_inspector(
        client, "inspector.appearance:" + text_item_id
    )
    converted_metadata = converted_section.get("metadata") or {}
    if converted_metadata.get("owner_model") != "module_graph":
        raise QaFailure("converted Text Appearance is not projected from its Module graph")
    converted_operation_ids = [
        operation.get("id") for operation in converted_metadata.get("operations") or []
    ]
    expected_operation_ids = [operation["id"] for operation in text_operations]
    if converted_operation_ids != expected_operation_ids:
        raise QaFailure("Text Appearance stable IDs/order changed during Node Clip conversion")
    converted_graph = assert_canonical_appearance_graph(
        converted["project"],
        definition_id,
        module_source["value"]["output_id"],
        converted_metadata.get("operations") or [],
    )

    converted_reopened, converted_persistence = _save_and_reopen(
        client,
        "converted Appearance",
        [3.0, shape["active_seconds"], direct_text["active_seconds"]],
    )
    converted_source = converted_reopened["project"]["items"][text_item_id]["source"]
    if converted_source.get("kind") != "module":
        raise QaFailure("reopened converted Appearance is no longer a Node Clip")
    if converted_source["value"]["instance_id"] != instance_id:
        raise QaFailure("reopened converted Appearance changed its Instance ID")
    if (
        converted_reopened["project"]["module_instances"][instance_id][
            "definition_id"
        ]
        != definition_id
    ):
        raise QaFailure("reopened converted Appearance changed its Definition ID")
    if appearance(converted_reopened, shape["item_id"]) != shape_operations:
        raise QaFailure("second reopen changed the direct Shape Appearance")
    if (
        appearance(converted_reopened, direct_text["item_id"])
        != direct_text_operations
    ):
        raise QaFailure("second reopen changed the direct Text Appearance")
    activate_dock_tab(
        client,
        "dock.tab:timeline",
        "Timeline",
        "Reopened converted Appearance",
    )
    client.click_component("timeline.item:" + text_item_id)
    reopened_section = bring_into_inspector(
        client, "inspector.appearance:" + text_item_id
    )
    reopened_metadata = reopened_section.get("metadata") or {}
    if [
        operation.get("id") for operation in reopened_metadata.get("operations") or []
    ] != expected_operation_ids:
        raise QaFailure("reopened Node Clip Appearance facade changed stable IDs/order")
    reopened_graph = assert_canonical_appearance_graph(
        converted_reopened["project"],
        definition_id,
        module_source["value"]["output_id"],
        reopened_metadata.get("operations") or [],
    )
    before_graph_capture = client.state()
    opened_definition_id, _ = open_timeline_item_definition(
        client, text_item_id, "node_clip", "Converted Appearance graph"
    )
    if opened_definition_id != definition_id:
        raise QaFailure("Appearance graph capture opened a different Definition")
    graph_capture_state = client.state()
    if (
        graph_capture_state["history"]["revision"]
        != before_graph_capture["history"]["revision"]
        or graph_capture_state["project"] != before_graph_capture["project"]
    ):
        raise QaFailure("opening the converted Appearance graph mutated the Project")
    graph_layout = assert_node_editor_nodes_do_not_overlap(
        client,
        [
            reopened_graph["shape_source"][0],
            *reopened_graph["operation_ids"],
            *reopened_graph["merge_ids"],
            reopened_graph["output_node_id"],
        ],
        "converted Appearance graph",
    )
    artifact_dir = pathlib.Path(
        os.environ.get(
            "RUVIE_QA_ARTIFACT_DIR",
            pathlib.Path("target") / "qa-appearance-e2e",
        )
    ).resolve()
    artifact_dir.mkdir(parents=True, exist_ok=True)
    graph_capture = capture_viewport(
        client, artifact_dir / "converted-appearance-graph.png"
    )
    initial_close = request_clean_native_close(
        client, "saved Appearance authoring app", client.timeout
    )
    fresh_process = _fresh_process_reload(
        _project_file(),
        converted_reopened["project"],
        converted_persistence,
        converted_persistence["previews"],
        text_item_id,
        definition_id,
        module_source["value"]["output_id"],
        reopened_metadata.get("operations") or [],
        (
            (shape["item_id"], shape_operations, "Shape"),
            (direct_text["item_id"], direct_text_operations, "Text"),
        ),
        appearance,
        bring_into_inspector,
        client.timeout,
    )
    return {
        "direct": direct_persistence,
        "converted": converted_persistence,
        "initial_close": initial_close,
        "fresh_process": fresh_process,
        "shape": shape,
        "direct_text": direct_text,
        "text_operation_ids": expected_operation_ids,
        "instance_id": instance_id,
        "definition_id": definition_id,
        "converted_graph": converted_graph,
        "reopened_graph": reopened_graph,
        "graph_capture": graph_capture,
        "graph_layout": graph_layout,
        "reopened_facade": reopened_metadata,
    }
