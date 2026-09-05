"""Save/reopen/restart checks shared by the native Appearance QA suite."""

import os
import pathlib

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
    initial_close = request_clean_native_close(
        client, "saved Appearance authoring app", client.timeout
    )
    fresh_process = _fresh_process_reload(
        _project_file(),
        converted_reopened["project"],
        converted_persistence,
        converted_persistence["previews"],
        text_item_id,
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
        "reopened_facade": reopened_metadata,
    }
