#!/usr/bin/env python3
"""Exercise Boolean Point fields through Compare and typed Select Nodes."""

import copy
import os
import pathlib

from qa_automation_support import history_shortcut
from qa_node_module_support import (
    component,
    connect_nodes,
    create_node_from_menu,
    enter_exact_numeric,
    node_content_type,
    open_timeline_item_definition,
    place_created_node,
    place_private_node_clip_source,
    reload_node_clip_project,
    sample_rendered_preview,
)
from qa_support import (
    QaClient,
    QaFailure,
    capture_viewport,
    media_seconds,
    run_suite_main,
    save_project_to_disk,
    write_json,
)


def _catalog_id(node):
    if node_content_type(node) != "nativeoperation":
        return None
    data = (node.get("content") or {}).get("data")
    return data.get("catalog_id") if isinstance(data, dict) else None


def _constant(node, key):
    prop = (node.get("properties") or {}).get(key) or {}
    if prop.get("type") != "constant":
        return None
    return (prop.get("properties") or {}).get("value")


def _edit_float(client, definition_id, node_id, key, value, description):
    control_id = "node_editor.property.node:{}:{}".format(node_id, key)
    _, control = client.wait_component_settled(control_id)
    metadata = control.get("metadata") or {}
    if metadata.get("editor_kind") != "float" or metadata.get("connected") is not False:
        raise QaFailure(description + " bypassed the shared Number editor")
    before = client.state()
    enter_exact_numeric(client, control_id, value)

    def edited():
        state = client.state()
        node = state["project"]["module_definitions"][definition_id]["graph"][
            "nodes"
        ][node_id]
        return (
            state
            if state["history"]["revision"] == before["history"]["revision"] + 1
            and _constant(node, key) == value
            else None
        )

    return client.wait_until(description + " exact edit", edited)


def _node_name(state, definition_id, node_id):
    return state["project"]["module_definitions"][definition_id]["graph"]["nodes"][
        node_id
    ]["name"]


def _name_editor_id(node_id):
    return "node_editor.node_menu:{}:name".format(node_id)


def _open_name_editor(client, node_id, expected_source):
    client.click_component("node_editor.node_header:" + node_id, button="secondary")
    editor_id = _name_editor_id(node_id)
    _, editor = client.wait_component_settled(editor_id)
    metadata = editor.get("metadata") or {}
    if (
        editor.get("type") != "node_name_editor"
        or metadata.get("node_id") != node_id
        or metadata.get("source") != expected_source
        or metadata.get("draft") != expected_source
        or metadata.get("validation_error") is not None
    ):
        raise QaFailure("Node Name editor lost its source/draft identity")
    return editor_id


def _wait_name_draft(client, editor_id, expected, require_error, description):
    def drafted():
        snapshot = client.component_snapshot()
        editor = component(snapshot, editor_id)
        metadata = (editor or {}).get("metadata") or {}
        error = metadata.get("validation_error")
        valid_error = bool(error) if require_error else error is None
        return (
            (snapshot, editor)
            if editor
            and editor.get("visible") is True
            and metadata.get("draft") == expected
            and valid_error
            else None
        )

    return client.wait_until(description, drafted)


def _assert_draft_is_transient(client, before, description):
    current = client.state()
    if (
        current["history"]["revision"] != before["history"]["revision"]
        or current["project"] != before["project"]
    ):
        raise QaFailure(description + " mutated the Project before commit")


def _wait_editor_closed(client, editor_id, description):
    def closed():
        snapshot = client.component_snapshot()
        editor = component(snapshot, editor_id)
        return snapshot if editor is None or editor.get("visible") is not True else None

    return client.wait_until(description, closed)


def _wait_node_name(client, definition_id, node_id, expected, before_revision, description):
    return client.wait_until(
        description,
        lambda: state
        if (
            (state := client.state())["history"]["revision"] == before_revision + 1
            and _node_name(state, definition_id, node_id) == expected
        )
        else None,
    )


def _replace_name_draft(client, editor_id, text):
    client.click_component(editor_id)
    client.key("a", True, command=True)
    client.key("a", False, command=True)
    client.inject("text", {"text": text})


def _exercise_attribute_names(
    client,
    item_id,
    definition_id,
    number_id,
    boolean_id,
    start,
    baseline_pixels,
    artifact_dir,
):
    open_timeline_item_definition(
        client, item_id, "node_clip", "Point attribute names"
    )
    number_before = client.state()
    number_editor = _open_name_editor(
        client, number_id, "Store Number Attribute"
    )
    write_json(
        artifact_dir / "point-name-open-components.json",
        client.component_snapshot(),
    )
    capture_viewport(client, artifact_dir / "point-name-open.png")
    client.click_component(number_editor)
    write_json(
        artifact_dir / "point-name-focused-components.json",
        client.component_snapshot(),
    )
    capture_viewport(client, artifact_dir / "point-name-focused.png")
    client.key("a", True, command=True)
    client.key("a", False, command=True)
    client.inject("text", {"text": "he"})
    write_json(
        artifact_dir / "point-name-typed-components.json",
        client.component_snapshot(),
    )
    capture_viewport(client, artifact_dir / "point-name-typed.png")
    _wait_name_draft(client, number_editor, "he", False, "Number name first draft")
    _assert_draft_is_transient(client, number_before, "Number name first draft")
    client.inject("text", {"text": "at"})
    _wait_name_draft(client, number_editor, "heat", False, "Number name second draft")
    _assert_draft_is_transient(client, number_before, "Number name second draft")
    client.key("enter", True)
    client.key("enter", False)
    number_named = _wait_node_name(
        client,
        definition_id,
        number_id,
        "heat",
        number_before["history"]["revision"],
        "Enter committing Number attribute name",
    )
    expected_number_project = copy.deepcopy(number_before["project"])
    expected_number_project["module_definitions"][definition_id]["graph"]["nodes"][
        number_id
    ]["name"] = "heat"
    expected_number_project["module_definitions"][definition_id][
        "topology_revision"
    ] += 1
    if number_named["project"] != expected_number_project:
        raise QaFailure("Number name commit changed more than its exact Node name")
    _wait_editor_closed(client, number_editor, "Number Name editor closed")
    number_pixels = sample_rendered_preview(
        client, start + 0.5, number_named["history"]["revision"], "named Number field"
    )
    if number_pixels["pixel_hash"] != baseline_pixels["pixel_hash"]:
        raise QaFailure("Number attribute rename changed rendered pixels")

    open_timeline_item_definition(
        client, item_id, "node_clip", "Boolean attribute name"
    )
    boolean_before = client.state()
    boolean_editor = _open_name_editor(
        client, boolean_id, "Store Boolean Attribute"
    )
    _replace_name_draft(client, boolean_editor, "heat")
    _wait_name_draft(
        client, boolean_editor, "heat", True, "duplicate Boolean name validation"
    )
    _assert_draft_is_transient(client, boolean_before, "duplicate Boolean name")
    client.key("enter", True)
    client.key("enter", False)
    _wait_name_draft(
        client, boolean_editor, "heat", True, "blocked duplicate Boolean commit"
    )
    _assert_draft_is_transient(client, boolean_before, "blocked duplicate Boolean commit")
    duplicate_capture = capture_viewport(
        client, artifact_dir / "point-name-duplicate.png"
    )
    _assert_draft_is_transient(client, boolean_before, "duplicate-name capture")

    _replace_name_draft(client, boolean_editor, "temporary")
    _wait_name_draft(
        client, boolean_editor, "temporary", False, "Boolean Escape draft"
    )
    _assert_draft_is_transient(client, boolean_before, "Boolean Escape draft")
    client.key("escape", True)
    client.key("escape", False)
    _wait_editor_closed(client, boolean_editor, "Escape closing Boolean Name editor")
    _assert_draft_is_transient(client, boolean_before, "Boolean Escape cancellation")
    if _node_name(client.state(), definition_id, boolean_id) != "Store Boolean Attribute":
        raise QaFailure("Escape changed the Boolean attribute name")

    boolean_editor = _open_name_editor(
        client, boolean_id, "Store Boolean Attribute"
    )
    _replace_name_draft(client, boolean_editor, "hot")
    _wait_name_draft(client, boolean_editor, "hot", False, "Boolean lost-focus draft")
    _assert_draft_is_transient(client, boolean_before, "Boolean lost-focus draft")
    client.click_component("node_editor.node_header:" + number_id)
    boolean_named = _wait_node_name(
        client,
        definition_id,
        boolean_id,
        "hot",
        boolean_before["history"]["revision"],
        "outside-click committing Boolean attribute name",
    )
    expected_boolean_project = copy.deepcopy(boolean_before["project"])
    expected_boolean_project["module_definitions"][definition_id]["graph"]["nodes"][
        boolean_id
    ]["name"] = "hot"
    expected_boolean_project["module_definitions"][definition_id][
        "topology_revision"
    ] += 1
    if boolean_named["project"] != expected_boolean_project:
        raise QaFailure("Boolean name commit changed more than its exact Node name")
    _wait_editor_closed(client, boolean_editor, "Boolean Name editor closed")
    named_pixels = sample_rendered_preview(
        client, start + 0.5, boolean_named["history"]["revision"], "named Boolean field"
    )
    if named_pixels["pixel_hash"] != baseline_pixels["pixel_hash"]:
        raise QaFailure("Boolean attribute rename changed rendered pixels")

    history_scenario = [
        (
            False,
            "Undo Boolean attribute name",
            number_named["project"],
            "undone Boolean name",
        ),
        (
            False,
            "Undo Number attribute name",
            number_before["project"],
            "undone Number name",
        ),
        (
            True,
            "Redo Number attribute name",
            number_named["project"],
            "redone Number name",
        ),
        (
            True,
            "Redo Boolean attribute name",
            boolean_named["project"],
            "redone Boolean name",
        ),
    ]
    current = boolean_named
    restored_pixels = named_pixels
    for redo, action, expected_project, sample_description in history_scenario:
        history_shortcut(client, redo=redo)
        expected_revision = current["history"]["revision"] + 1

        def history_applied():
            state = client.state()
            return (
                state
                if state["history"]["revision"] == expected_revision
                and state["project"] == expected_project
                else None
            )

        current = client.wait_until(action, history_applied)
        restored_pixels = sample_rendered_preview(
            client,
            start + 0.5,
            current["history"]["revision"],
            sample_description,
        )
        if restored_pixels["pixel_hash"] != baseline_pixels["pixel_hash"]:
            raise QaFailure(action + " changed rendered pixels")
    return (
        current,
        restored_pixels,
        {
            "number": "heat",
            "boolean": "hot",
            "duplicate_capture": duplicate_capture,
        },
    )


def _assert_routes(definition, expected):
    actual = {
        (
            route["from"]["node_id"],
            route["from"]["port"],
            route["to"]["node_id"],
            route["to"]["port"],
        )
        for route in definition["graph"]["connections"]
    }
    if actual != expected:
        raise QaFailure(
            "Point conditional routes differ: missing={!r}, extra={!r}".format(
                sorted(expected - actual), sorted(actual - expected)
            )
        )


def _assert_definition(definition, context, threshold, expected_names):
    nodes = definition["graph"]["nodes"]
    expected_native = {
        context["grid_id"]: "native.point.grid",
        context["info_id"]: "native.point.info",
        context["number_id"]: "native.point.store-number-attribute",
        context["greater_id"]: "native.logic.greater",
        context["boolean_id"]: "native.point.store-boolean-attribute",
        context["select_id"]: "native.logic.select-number",
        context["sprite_id"]: "native.particle.sprite-renderer",
    }
    for node_id, catalog_id in expected_native.items():
        if node_id not in nodes or _catalog_id(nodes[node_id]) != catalog_id:
            raise QaFailure("Point conditional graph lost " + catalog_id)
    ramp = nodes.get(context["ramp_id"])
    ramp_data = str((ramp or {}).get("content", {}).get("data", ""))
    if (
        node_content_type(ramp or {}) != "color"
        or ramp_data.replace("_", "").lower() != "colorramp"
    ):
        raise QaFailure("Point conditional graph lost Color Ramp")
    if node_content_type(nodes.get(context["output_id"], {})) != "moduleoutput":
        raise QaFailure("Point conditional graph lost its Output boundary")
    if nodes[context["number_id"]].get("name") != expected_names[0]:
        raise QaFailure("Number Point attribute lost its authoritative Node name")
    if nodes[context["boolean_id"]].get("name") != expected_names[1]:
        raise QaFailure("Boolean Point attribute lost its authoritative Node name")
    if _constant(nodes[context["greater_id"]], "b") != threshold:
        raise QaFailure("Greater Than threshold changed")
    if _constant(nodes[context["select_id"]], "if_true") != 1.0:
        raise QaFailure("Select Number true branch changed")
    if _constant(nodes[context["select_id"]], "if_false") != 0.0:
        raise QaFailure("Select Number false branch changed")
    _assert_routes(definition, context["routes"])


def run_suite(client):
    client.wait_health()
    source_snapshot, source = client.wait_component_settled("assets.node_clip_source")
    metadata = source.get("metadata") or {}
    if metadata.get("kind") != "node_clip" or metadata.get("draggable_to_timeline") is not True:
        raise QaFailure("New Node Clip Assets source lost its production drag contract")
    created = place_private_node_clip_source(
        client, source_snapshot, source, 6.5, "Point conditional Node Clip"
    )
    item_id = created["item_id"]
    definition_id = created["definition_id"]
    start = media_seconds(created["item"]["interval"]["start"])
    opened_id, before = open_timeline_item_definition(
        client, item_id, "node_clip", "Point conditional fields"
    )
    if opened_id != definition_id:
        raise QaFailure("Node Editor opened a different Point conditional Definition")
    output_ids = [
        node_id
        for node_id, node in before["graph"]["nodes"].items()
        if node_content_type(node) == "moduleoutput"
    ]
    if len(output_ids) != 1 or before["graph"]["connections"]:
        raise QaFailure("New Node Clip is not the clean Output-only production fixture")
    output_id = output_ids[0]

    _, canvas_before = client.wait_component_settled("node_editor.canvas")
    old_scale = float(canvas_before["metadata"]["scale"])
    client.scroll_component(
        "node_editor.canvas", 0.0, -100.0, modifiers={"command": True}
    )
    _, canvas = client.wait_component_settled("node_editor.canvas")
    scale = float(canvas["metadata"]["scale"])
    if scale >= old_scale or not 0.4 <= scale <= 0.75:
        raise QaFailure("Node Editor did not reach Point conditional authoring overview")
    place_created_node(client, output_id, 0.95, vertical_offset=140.0)

    specs = [
        (
            "sprite",
            "Sprite Renderer",
            "node_editor.menu.create.particle_sprite_renderer",
            0.92,
            26.0,
        ),
        ("grid", "Point Grid", "node_editor.menu.create.point_grid", 0.06, 26.0),
        ("info", "Point Info", "node_editor.menu.create.point_info", 0.20, 26.0),
        (
            "number",
            "Store Number Attribute",
            "node_editor.menu.create.point_store_number_attribute",
            0.35,
            26.0,
        ),
        (
            "greater",
            "Greater Than",
            "node_editor.menu.create.logic:greater",
            0.50,
            26.0,
        ),
        (
            "boolean",
            "Store Boolean Attribute",
            "node_editor.menu.create.point_store_boolean_attribute",
            0.65,
            26.0,
        ),
        (
            "select",
            "Select Number",
            "node_editor.menu.create.logic:select-number",
            0.46,
            140.0,
        ),
        ("ramp", "Color Ramp", "node_editor.menu.create.color:ramp", 0.70, 140.0),
    ]
    nodes = {}
    for name, query, choice_id, horizontal, vertical in specs:
        _, node_id = create_node_from_menu(client, "node_clip", query, choice_id)
        place_created_node(client, node_id, horizontal, vertical_offset=vertical)
        nodes[name] = node_id

    _edit_float(client, definition_id, nodes["greater"], "b", 0.5, "threshold")
    _edit_float(client, definition_id, nodes["select"], "if_true", 1.0, "true branch")
    route_specs = [
        (nodes["grid"], "points", nodes["info"], "points", "Grid to Point Info"),
        (nodes["grid"], "points", nodes["number"], "points", "Grid to Number Store"),
        (nodes["info"], "random", nodes["number"], "value", "Random to Number Store"),
        (
            nodes["number"],
            "points",
            nodes["boolean"],
            "points",
            "Number Store to Boolean Store",
        ),
        (
            nodes["number"],
            "attribute",
            nodes["greater"],
            "a",
            "stored Random to Greater Than",
        ),
        (
            nodes["greater"],
            "result",
            nodes["boolean"],
            "value",
            "comparison to Boolean Store",
        ),
        (
            nodes["boolean"],
            "attribute",
            nodes["select"],
            "condition",
            "Boolean attribute to Select",
        ),
        (
            nodes["boolean"],
            "points",
            nodes["sprite"],
            "particles",
            "Boolean Store to Sprite",
        ),
        (
            nodes["select"],
            "result",
            nodes["ramp"],
            "factor",
            "selected mask to Color Ramp",
        ),
        (
            nodes["ramp"],
            "color",
            nodes["sprite"],
            "color",
            "Color Ramp to Sprite",
        ),
        (nodes["sprite"], "image", output_id, "image_in", "Sprite to Output"),
    ]
    connections = [connect_nodes(client, "node_clip", *route) for route in route_specs]
    routed = client.state()
    context = {
        "item_id": item_id,
        "definition_id": definition_id,
        "grid_id": nodes["grid"],
        "info_id": nodes["info"],
        "number_id": nodes["number"],
        "greater_id": nodes["greater"],
        "boolean_id": nodes["boolean"],
        "select_id": nodes["select"],
        "ramp_id": nodes["ramp"],
        "sprite_id": nodes["sprite"],
        "output_id": output_id,
        "routes": {route[:4] for route in route_specs},
    }
    _assert_definition(
        routed["project"]["module_definitions"][definition_id],
        context,
        0.5,
        ("Store Number Attribute", "Store Boolean Attribute"),
    )
    unnamed_pixels = sample_rendered_preview(
        client, start + 0.5, routed["history"]["revision"], "midpoint mask"
    )
    artifact_dir = pathlib.Path(os.environ["RUVIE_QA_ARTIFACT_DIR"])
    named, midpoint, rename_evidence = _exercise_attribute_names(
        client,
        item_id,
        definition_id,
        nodes["number"],
        nodes["boolean"],
        start,
        unnamed_pixels,
        artifact_dir,
    )
    _assert_definition(
        named["project"]["module_definitions"][definition_id],
        context,
        0.5,
        ("heat", "hot"),
    )
    open_timeline_item_definition(
        client, item_id, "node_clip", "Point conditional threshold"
    )
    changed = _edit_float(
        client, definition_id, nodes["greater"], "b", 0.85, "threshold"
    )
    high = sample_rendered_preview(
        client, start + 0.5, changed["history"]["revision"], "high threshold mask"
    )
    if high["pixel_hash"] == midpoint["pixel_hash"]:
        raise QaFailure("Greater Than threshold did not change real masked Grid pixels")

    history_shortcut(client)

    def threshold_undone():
        state = client.state()
        node = state["project"]["module_definitions"][definition_id]["graph"][
            "nodes"
        ][nodes["greater"]]
        return (
            state
            if state["history"]["revision"] == changed["history"]["revision"] + 1
            and _constant(node, "b") == 0.5
            else None
        )

    undone = client.wait_until("Undo conditional threshold", threshold_undone)
    undone_pixels = sample_rendered_preview(
        client, start + 0.5, undone["history"]["revision"], "undone threshold"
    )
    if undone_pixels["pixel_hash"] != midpoint["pixel_hash"]:
        raise QaFailure("Undo did not restore exact midpoint mask pixels")
    history_shortcut(client, redo=True)

    def threshold_redone():
        state = client.state()
        node = state["project"]["module_definitions"][definition_id]["graph"][
            "nodes"
        ][nodes["greater"]]
        return (
            state
            if state["history"]["revision"] == undone["history"]["revision"] + 1
            and _constant(node, "b") == 0.85
            else None
        )

    redone = client.wait_until("Redo conditional threshold", threshold_redone)
    restored = sample_rendered_preview(
        client, start + 0.5, redone["history"]["revision"], "redone threshold"
    )
    if restored["pixel_hash"] != high["pixel_hash"]:
        raise QaFailure("Redo did not restore exact high-threshold mask pixels")

    samples = [
        restored,
        sample_rendered_preview(
            client, start + 1.0, redone["history"]["revision"], "Point conditional"
        ),
    ]
    open_timeline_item_definition(
        client, item_id, "node_clip", "Point conditional capture"
    )
    capture = capture_viewport(client, artifact_dir / "capture.png")
    project_path = os.environ.get("RUVIE_QA_PROJECT_PATH")
    if not project_path:
        raise QaFailure("Point conditional persistence QA requires a Project file")
    project_file = pathlib.Path(project_path)
    saved, saved_file = save_project_to_disk(client, project_file, "Point conditional")
    reloaded = reload_node_clip_project(
        client,
        project_file,
        saved,
        item_id,
        definition_id,
        samples,
        artifact_dir,
        "Point conditional",
        "point-conditional",
        "point-conditional-reloaded.png",
        lambda definition: _assert_definition(
            definition, context, 0.85, ("heat", "hot")
        ),
    )
    return {
        "suite": "point-conditional-fields",
        "item_id": item_id,
        "definition_id": definition_id,
        "nodes": nodes,
        "connections": connections,
        "renames": rename_evidence,
        "samples": samples,
        "saved": saved_file,
        "capture": capture,
        "reloaded": reloaded,
        "actions": client.evidence,
    }


if __name__ == "__main__":
    raise SystemExit(
        run_suite_main(
            "qa-point-conditional-fields-e2e",
            run_suite,
            "target/qa-point-conditional-fields-e2e-evidence.json",
        )
    )
