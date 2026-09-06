#!/usr/bin/env python3
"""Exercise multilingual Text Content through the production Inspector UI."""

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
    component_in_inspector,
    convert_timeline_item_to_node_clip,
    free_port,
    item_by_name,
    rendered_current_revision,
    request_clean_native_close,
    run_suite_main,
    save_project_to_disk,
    spawned_authoring_app,
)
from qa_text_content_support import (
    direct_text_content,
    edit_direct_text,
    edit_module_text,
    module_text_content,
)
from qa_text_ensemble_support import seek_rendered


OPEN_EXISTING_PROJECT_ENV = "RUVIE_QA_OPEN_EXISTING_PROJECT"
TIMELINE_TAB = "dock.tab:timeline"
SAMPLES = (
    ("japanese", "日本語 かな カナ"),
    ("greek-cyrillic", "Ελληνικά Привет"),
    ("hebrew", "אבגד"),
    ("arabic", "سلام"),
    ("emoji", "🙂"),
)
SAVED_MATRIX = "\n".join(text for _, text in SAMPLES)


def _artifact_dir():
    value = pathlib.Path(
        os.environ.get(
            "RUVIE_QA_ARTIFACT_DIR",
            pathlib.Path("target") / "qa-text-font-fallback-e2e",
        )
    ).resolve()
    value.mkdir(parents=True, exist_ok=True)
    return value


def _undo(client):
    client.key("z", True, command=True)
    client.key("z", False, command=True)


def _redo(client):
    client.key("z", True, command=True, shift=True)
    client.key("z", False, command=True, shift=True)


def _rendered_revision(client, revision, frame, description):
    return client.wait_until(
        description,
        lambda: state
        if (state := rendered_current_revision(client))
        and state["history"]["revision"] == revision
        and state["editor"]["preview"]["rendered_frame"] == frame
        else None,
        30.0,
    )


def _direct_control(item_id):
    return "inspector.property:item:{}:text".format(item_id)


def _module_control(project, item_id):
    context = module_text_content(project, item_id)
    return "inspector.property:module_instance:{}:{}".format(
        context["instance"]["id"], context["parameter"]["id"]
    )


def _assert_visible_content(client, control_id, text, description):
    _, component = component_in_inspector(client, control_id)
    metadata = component.get("metadata") or {}
    if metadata.get("editor_kind") != "multiline_text":
        raise QaFailure(description + " did not use the multiline Text editor")
    if metadata.get("value") != text:
        raise QaFailure(description + " Inspector metadata changed authored text")
    return component


def _edit_undo_redo(
    client,
    item_id,
    text,
    description,
    edit,
    resolve,
    control,
    capture_name=None,
):
    before = seek_rendered(client, 1.5)
    before_project = before["project"]
    before_hash = before["editor"]["preview"]["pixel_hash"]
    edited = edit(text, description + " edit")
    if resolve(edited["project"]) != text:
        raise QaFailure(description + " did not persist exact Unicode Content")
    frame = edited["editor"]["timeline"]["current_frame"]
    rendered = _rendered_revision(
        client, edited["history"]["revision"], frame, description + " Preview"
    )
    control_id = control(rendered["project"])
    _assert_visible_content(client, control_id, text, description)
    capture = None
    if capture_name is not None:
        capture = capture_viewport(client, _artifact_dir() / capture_name)

    # Move focus out of TextEdit so application history owns Undo/Redo.
    client.click_component("timeline.item:" + item_id)
    _undo(client)
    undone = client.wait_until(
        description + " Undo",
        lambda: state
        if (state := client.state())["project"] == before_project
        and state["history"]["revision"] == edited["history"]["revision"] + 1
        else None,
    )
    undo_render = _rendered_revision(
        client,
        undone["history"]["revision"],
        frame,
        description + " Undo Preview",
    )
    if undo_render["editor"]["preview"]["pixel_hash"] != before_hash:
        raise QaFailure(description + " Undo did not restore Preview pixels")

    _redo(client)
    redone = client.wait_until(
        description + " Redo",
        lambda: state
        if (state := client.state())["project"] == edited["project"]
        and state["history"]["revision"] == undone["history"]["revision"] + 1
        else None,
    )
    redo_render = _rendered_revision(
        client,
        redone["history"]["revision"],
        frame,
        description + " Redo Preview",
    )
    if (
        redo_render["editor"]["preview"]["pixel_hash"]
        != rendered["editor"]["preview"]["pixel_hash"]
    ):
        raise QaFailure(description + " Redo changed Preview pixels")
    if resolve(redo_render["project"]) != text:
        raise QaFailure(description + " Redo changed authored Unicode Content")
    return {
        "text": text,
        "revision": redo_render["history"]["revision"],
        "frame": frame,
        "pixel_hash": redo_render["editor"]["preview"]["pixel_hash"],
        "capture": capture,
    }


def _exercise_direct(client, item_id, label, text, capture=True):
    return _edit_undo_redo(
        client,
        item_id,
        text,
        "direct " + label,
        lambda value, description: edit_direct_text(
            client, item_id, value, description
        ),
        lambda project: direct_text_content(project, item_id),
        lambda _project: _direct_control(item_id),
        "direct-{}.png".format(label) if capture else None,
    )


def _exercise_promoted(client, item_id, label, text, capture=True):
    return _edit_undo_redo(
        client,
        item_id,
        text,
        "promoted " + label,
        lambda value, description: edit_module_text(
            client, item_id, value, description
        ),
        lambda project: module_text_content(project, item_id)["value"],
        lambda project: _module_control(project, item_id),
        "promoted-{}.png".format(label) if capture else None,
    )


def _fresh_process_parity(
    project_file, expected_project, item_id, expected_text, expected_hash, timeout
):
    port = free_port()
    run_id = os.environ.get("RUVIE_QA_RUN_ID", "text-font-fallback")
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
            raise QaFailure("fresh process loaded a different multilingual Project")
        if module_text_content(loaded["project"], item_id)["value"] != expected_text:
            raise QaFailure("fresh process changed published multilingual Content")
        activate_dock_tab(
            client, TIMELINE_TAB, "Timeline", "reloaded multilingual Text selection"
        )
        bring_timeline_component(client, "timeline.item:" + item_id, -120.0)
        client.click_component("timeline.item:" + item_id)
        rendered = seek_rendered(client, 1.5)
        if rendered["editor"]["preview"]["pixel_hash"] != expected_hash:
            raise QaFailure("fresh process changed multilingual Preview pixels")
        control_id = _module_control(rendered["project"], item_id)
        _assert_visible_content(
            client, control_id, expected_text, "reloaded multilingual Content"
        )
        capture = capture_viewport(client, _artifact_dir() / "capture.png")
        close = close_clean_native_app(
            client, process, "reloaded multilingual Text app", timeout
        )
    return {
        "revision": rendered["history"]["revision"],
        "frame": rendered["editor"]["preview"]["rendered_frame"],
        "pixel_hash": rendered["editor"]["preview"]["pixel_hash"],
        "capture": capture,
        "close": close,
        "actions": client.evidence,
    }


def run_suite(client):
    client.wait_health()
    initial = client.wait_until(
        "initial multilingual Text Preview",
        lambda: rendered_current_revision(client),
        30.0,
    )
    item = item_by_name(initial["project"], "QA Text")
    item_id = item["id"]
    client.click_component("timeline.item:" + item_id)
    seek_rendered(client, 1.5)

    direct = {
        label: _exercise_direct(client, item_id, label, text)
        for label, text in SAMPLES
    }
    direct_matrix = _exercise_direct(
        client, item_id, "matrix", SAVED_MATRIX, capture=False
    )

    activate_dock_tab(client, TIMELINE_TAB, "Timeline", "multilingual promotion")
    bring_timeline_component(client, "timeline.item:" + item_id, -120.0)
    client.click_component("timeline.item:" + item_id)
    converted = convert_timeline_item_to_node_clip(
        client, item_id, direct_matrix["revision"]
    )
    converted_context = module_text_content(converted["project"], item_id)
    if converted_context["value"] != SAVED_MATRIX:
        raise QaFailure("Node Clip conversion changed multilingual Content")
    converted_render = _rendered_revision(
        client,
        converted["history"]["revision"],
        direct_matrix["frame"],
        "converted multilingual Preview",
    )
    if converted_render["editor"]["preview"]["pixel_hash"] != direct_matrix["pixel_hash"]:
        raise QaFailure("Node Clip conversion changed multilingual Preview pixels")

    promoted = {
        label: _exercise_promoted(client, item_id, label, text)
        for label, text in SAMPLES
    }
    for label, direct_sample in direct.items():
        if promoted[label]["pixel_hash"] != direct_sample["pixel_hash"]:
            raise QaFailure(
                "promoted {} Content changed Preview pixels".format(label)
            )
    promoted_matrix = _exercise_promoted(
        client, item_id, "matrix", SAVED_MATRIX, capture=False
    )
    if promoted_matrix["pixel_hash"] != direct_matrix["pixel_hash"]:
        raise QaFailure("promoted multilingual matrix changed Preview pixels")

    project_file_value = os.environ.get("RUVIE_QA_PROJECT_PATH")
    if not project_file_value:
        raise QaFailure("multilingual persistence requires RUVIE_QA_PROJECT_PATH")
    project_file = pathlib.Path(project_file_value).resolve()
    expected_project = client.state()["project"]
    saved, file_evidence = save_project_to_disk(
        client, project_file, "multilingual Text Node Clip"
    )
    initial_close = request_clean_native_close(
        client, "saved multilingual Text app", client.timeout
    )
    fresh = _fresh_process_parity(
        project_file,
        expected_project,
        item_id,
        SAVED_MATRIX,
        promoted_matrix["pixel_hash"],
        client.timeout,
    )
    return {
        "suite": "text-font-fallback",
        "native_scope": (
            "production Inspector Content lifecycle and Skia Preview parity; "
            "egui glyph availability is enforced by the application unit test"
        ),
        "item_id": item_id,
        "direct": direct,
        "promoted": promoted,
        "matrix": {
            "text": SAVED_MATRIX,
            "direct": direct_matrix,
            "promoted": promoted_matrix,
        },
        "instance_id": converted_context["instance"]["id"],
        "definition_id": converted_context["definition"]["id"],
        "content_parameter_id": converted_context["parameter"]["id"],
        "saved": {**file_evidence, "revision": saved["history"]["revision"]},
        "initial_close": initial_close,
        "fresh_process": fresh,
        "capture": fresh["capture"],
        "actions": client.evidence,
    }


if __name__ == "__main__":
    raise SystemExit(
        run_suite_main(
            "qa-text-font-fallback-e2e",
            run_suite,
            "target/qa-text-font-fallback-e2e-evidence.json",
        )
    )
