#!/usr/bin/env python3
"""Verify Preview render, shared camera/grid, source gizmo, and playback."""

import os
import pathlib

from qa_support import (
    QaFailure,
    capture_viewport,
    component_center,
    component_point,
    item_by_name,
    run_suite_main,
    settled_preview_state,
)


def _preview_ready(client):
    state = client.state()
    preview = state["editor"]["preview"]
    if (
        preview.get("nontransparent_pixels", 0) > 0
        and preview.get("pixel_hash") is not None
        and preview.get("rendered_frame") is not None
        and state["editor"].get("error") is None
    ):
        return state
    return None


def _metadata(client, component_id):
    return client.wait_component(component_id)[1].get("metadata") or {}


def _text_value(state, item_id):
    source = state["project"]["items"][item_id]["source"]
    if source.get("kind") != "text":
        raise QaFailure("Preview Text tool target is no longer a Text clip")
    return source["value"]["text"]


def _settled_preview(client, revision, frame, description):
    return client.wait_until(
        description,
        lambda: settled_preview_state(client, revision, frame),
        timeout=20.0,
    )


def _constant_vec2(item, key):
    prop = item.get("authored_properties", {}).get(key) or {}
    if prop.get("type") != "constant":
        raise QaFailure("new Text {} is not an authored constant".format(key))
    value = (prop.get("properties") or {}).get("value")
    if not isinstance(value, dict) or "x" not in value or "y" not in value:
        raise QaFailure("new Text {} omitted its Vec2 value".format(key))
    return {"x": float(value["x"]), "y": float(value["y"])}


def _canvas_margin_point(canvas_component, content_component):
    canvas = canvas_component.get("rect_points") or {}
    content = content_component.get("rect_points") or {}
    candidates = (
        (
            float(content.get("min_x", 0.0)) - float(canvas.get("min_x", 0.0)),
            {
                "x": (
                    float(canvas.get("min_x", 0.0))
                    + float(content.get("min_x", 0.0))
                )
                * 0.5,
                "y": float(content.get("center_y", 0.0)),
            },
        ),
        (
            float(canvas.get("max_x", 0.0)) - float(content.get("max_x", 0.0)),
            {
                "x": (
                    float(content.get("max_x", 0.0))
                    + float(canvas.get("max_x", 0.0))
                )
                * 0.5,
                "y": float(content.get("center_y", 0.0)),
            },
        ),
        (
            float(content.get("min_y", 0.0)) - float(canvas.get("min_y", 0.0)),
            {
                "x": float(content.get("center_x", 0.0)),
                "y": (
                    float(canvas.get("min_y", 0.0))
                    + float(content.get("min_y", 0.0))
                )
                * 0.5,
            },
        ),
        (
            float(canvas.get("max_y", 0.0)) - float(content.get("max_y", 0.0)),
            {
                "x": float(content.get("center_x", 0.0)),
                "y": (
                    float(content.get("max_y", 0.0))
                    + float(canvas.get("max_y", 0.0))
                )
                * 0.5,
            },
        ),
    )
    gap, point = max(candidates, key=lambda candidate: candidate[0])
    if gap <= 4.0:
        raise QaFailure("Preview has no grid margin for an unambiguous deselect click")
    return point


def run_suite(client):
    client.wait_health()
    initial = client.wait_until("a rendered authoring Preview", lambda: _preview_ready(client), 30)
    text_item = item_by_name(initial["project"], "QA Text")
    client.click_component("timeline.item:" + text_item["id"])
    client.wait_until(
        "text selection",
        lambda: state
        if (state := client.state())["editor"]["selection"]["primary"]
        == {"kind": "timeline_item", "id": text_item["id"]}
        else None,
    )
    # Selection is independent from playback time. Seek inside the selected
    # clip before asserting its evaluated gizmo, just as a user would move the
    # playhead to inspect an otherwise off-time item.
    _, ruler = client.wait_component("timeline.ruler")
    seek_point = component_point(ruler, 0.15, 0.5)
    client.inject(
        "click",
        {**seek_point, "button": "primary", "coordinate_space": "points"},
    )
    client.wait_until(
        "playhead inside the selected Text clip",
        lambda: state
        if 30 <= (state := client.state())["editor"]["timeline"]["current_frame"] < 240
        else None,
    )
    client.wait_component("preview.position_gizmo")
    content = client.wait_component("preview.content")[1]
    gizmo = client.wait_component("preview.position_gizmo")[1]
    gizmo_metadata = gizmo.get("metadata") or {}
    if gizmo_metadata.get("source_kind") != "text":
        raise QaFailure("Preview gizmo did not preserve Text source provenance")
    content_rect = content["rect_points"]
    bounds = gizmo_metadata.get("screen_bounds") or {}
    try:
        gizmo_width = bounds["max"]["x"] - bounds["min"]["x"]
        gizmo_height = bounds["max"]["y"] - bounds["min"]["y"]
    except (KeyError, TypeError) as error:
        raise QaFailure("Preview gizmo omitted evaluated screen bounds") from error
    if gizmo_width >= content_rect["width"] and gizmo_height >= content_rect["height"]:
        raise QaFailure("Text gizmo incorrectly uses the whole composition bounds")

    # Restore the production Preview Text tool. Typing is a transient render
    # projection; Escape restores the exact Project and pixels, while leaving
    # the later accepted session commits one authoritative change.
    before_text_edit = client.state()
    original_text = _text_value(before_text_edit, text_item["id"])
    text_frame = before_text_edit["editor"]["timeline"]["current_frame"]
    baseline_text_preview = _settled_preview(
        client,
        before_text_edit["history"]["revision"],
        text_frame,
        "settled original Text Preview",
    )
    baseline_text_hash = baseline_text_preview["editor"]["preview"]["pixel_hash"]
    edited_text = "Preview direct text"
    client.click_component("preview.tool.text")
    text_click = {
        "x": (bounds["min"]["x"] + bounds["max"]["x"]) * 0.5,
        "y": (bounds["min"]["y"] + bounds["max"]["y"]) * 0.5,
    }
    client.inject(
        "click", {**text_click, "button": "primary", "coordinate_space": "points"}
    )
    existing_editor = _metadata(client, "preview.text.editor")
    if existing_editor.get("item_id") != text_item["id"]:
        raise QaFailure("Text tool did not edit the top-most Text under the click")
    after_existing_hit = client.state()
    if (
        after_existing_hit["project"] != before_text_edit["project"]
        or after_existing_hit["history"] != before_text_edit["history"]
    ):
        raise QaFailure("clicking an existing Text created or mutated an Item")

    # Empty content remains a live editor state rather than ending the edit or
    # falling through to blank-canvas creation. Typing can resume immediately.
    client.key("a", True, command=True)
    client.key("a", False, command=True)
    client.key("backspace", True)
    client.key("backspace", False)
    client.wait_until(
        "empty Preview Text buffer remains editable",
        lambda: metadata
        if (metadata := _metadata(client, "preview.text.editor")).get("buffer") == ""
        and metadata.get("item_id") == text_item["id"]
        else None,
    )
    empty_preview = _settled_preview(
        client,
        before_text_edit["history"]["revision"],
        text_frame,
        "settled empty Text Preview",
    )
    if empty_preview["editor"]["preview"]["pixel_hash"] == baseline_text_hash:
        raise QaFailure("empty transient Text buffer did not change Preview pixels")

    cancelled_value = "Cancelled Preview text"
    client.inject("text", {"text": cancelled_value})
    client.wait_until(
        "resumed Preview Text typing",
        lambda: metadata
        if (metadata := _metadata(client, "preview.text.editor")).get("buffer")
        == cancelled_value
        else None,
    )
    cancelled_preview = _settled_preview(
        client,
        before_text_edit["history"]["revision"],
        text_frame,
        "settled cancelled Text draft Preview",
    )
    if cancelled_preview["editor"]["preview"]["pixel_hash"] == baseline_text_hash:
        raise QaFailure("resumed transient Text typing did not change Preview pixels")
    client.key("escape", True)
    client.key("escape", False)
    escaped = client.wait_until(
        "Preview Text Escape",
        lambda: state
        if not (state := client.state())["editor"]["preview"]["text_editor"]["editing"]
        and state["editor"]["preview"]["active_tool"] == "select"
        else None,
    )
    if (
        escaped["project"] != before_text_edit["project"]
        or escaped["history"] != before_text_edit["history"]
    ):
        raise QaFailure("Escape committed or recorded transient Text typing")
    escaped_preview = _settled_preview(
        client,
        before_text_edit["history"]["revision"],
        text_frame,
        "settled Text Preview after Escape",
    )
    if escaped_preview["editor"]["preview"]["pixel_hash"] != baseline_text_hash:
        raise QaFailure("Escape did not restore the original Text Preview pixels")

    client.click_component("preview.tool.text")
    client.inject(
        "click", {**text_click, "button": "primary", "coordinate_space": "points"}
    )
    accepted_editor = _metadata(client, "preview.text.editor")
    if accepted_editor.get("item_id") != text_item["id"]:
        raise QaFailure("reopening existing Text targeted a different Item")
    client.key("a", True, command=True)
    client.key("a", False, command=True)
    client.inject("text", {"text": edited_text})
    client.wait_until(
        "transient Preview Text buffer",
        lambda: metadata
        if (metadata := _metadata(client, "preview.text.editor")).get("buffer")
        == edited_text
        else None,
    )
    if _text_value(client.state(), text_item["id"]) != original_text:
        raise QaFailure("Preview Text typing mutated Project before acceptance")
    edited_preview = _settled_preview(
        client,
        before_text_edit["history"]["revision"],
        text_frame,
        "settled accepted Text draft Preview",
    )
    if edited_preview["editor"]["preview"]["pixel_hash"] == baseline_text_hash:
        raise QaFailure("Preview Text draft did not affect rendered pixels")
    edited_draft_hash = edited_preview["editor"]["preview"]["pixel_hash"]
    client.click_component("preview.tool.select")
    edited = client.wait_until(
        "one accepted Preview Text transaction",
        lambda: state
        if _text_value((state := client.state()), text_item["id"]) == edited_text
        else None,
    )
    if edited["history"]["revision"] != before_text_edit["history"]["revision"] + 1:
        raise QaFailure("Preview Text session was not exactly one transaction")
    accepted_text_preview = _settled_preview(
        client,
        edited["history"]["revision"],
        text_frame,
        "settled accepted Text Preview",
    )
    if accepted_text_preview["editor"]["preview"]["pixel_hash"] != edited_draft_hash:
        raise QaFailure("accepted Text pixels differ from its transient draft")
    client.key("z", True, command=True)
    client.key("z", False, command=True)
    reverted_text = client.wait_until(
        "one-step Preview Text Undo",
        lambda: state
        if _text_value((state := client.state()), text_item["id"]) == original_text
        else None,
    )
    reverted_text_preview = _settled_preview(
        client,
        reverted_text["history"]["revision"],
        text_frame,
        "settled Preview Text Undo",
    )
    if reverted_text_preview["editor"]["preview"]["pixel_hash"] != baseline_text_hash:
        raise QaFailure("Text Undo did not restore the original Preview pixels")

    # The Text tool is armed independently of selection. Clicking blank
    # canvas creates one Text Item at that point and enters the same editor;
    # the edit and creation remain two explicit, individually undoable acts.
    _, settled_canvas = client.wait_component_settled("preview.canvas")
    _, settled_content = client.wait_component_settled("preview.content")
    _, settled_select_tool = client.wait_component_settled("preview.tool.select")
    precreate_canvas_rect = settled_canvas.get("rect_points")
    precreate_select_rect = settled_select_tool.get("rect_points")
    blank = component_point(settled_content, 0.88, 0.84)
    clear_selection_point = _canvas_margin_point(settled_canvas, settled_content)
    canvas = settled_content.get("metadata") or {}
    origin = canvas.get("screen_origin") or {}
    pan = canvas.get("pan") or {}
    zoom = float(canvas.get("zoom", 0.0))
    if zoom <= 0.0:
        raise QaFailure("Preview content omitted a valid canvas transform")
    expected_position = {
        "x": (blank["x"] - float(origin["x"]) - float(pan["x"])) / zoom,
        "y": (blank["y"] - float(origin["y"]) - float(pan["y"])) / zoom,
    }
    # Explicitly clear the existing Text selection with the Select tool. This
    # proves the Text tool itself is usable without any selected Text Item.
    client.click_component("preview.tool.select")
    before_clear = client.state()
    client.inject(
        "click",
        {
            **clear_selection_point,
            "button": "primary",
            "coordinate_space": "points",
        },
    )
    before_text_create = client.wait_until(
        "blank Preview selection clear",
        lambda: state
        if (state := client.state())["editor"]["selection"]["primary"] is None
        else None,
    )
    if (
        before_text_create["project"] != before_clear["project"]
        or before_text_create["history"] != before_clear["history"]
    ):
        raise QaFailure("blank Select click mutated the Project")
    before_create_preview = _settled_preview(
        client,
        before_text_create["history"]["revision"],
        text_frame,
        "settled unselected Preview before Text creation",
    )
    before_create_hash = before_create_preview["editor"]["preview"]["pixel_hash"]
    original_module_counts = (
        len(before_text_create["project"].get("module_definitions") or {}),
        len(before_text_create["project"].get("module_instances") or {}),
    )
    client.click_component("preview.tool.text")
    client.inject(
        "click", {**blank, "button": "primary", "coordinate_space": "points"}
    )
    text_created = client.wait_until(
        "blank-canvas Text creation",
        lambda: state
        if len((state := client.state())["project"]["items"])
        == len(before_text_create["project"]["items"]) + 1
        else None,
    )
    created_ids = set(text_created["project"]["items"]) - set(
        before_text_create["project"]["items"]
    )
    if len(created_ids) != 1:
        raise QaFailure("Text tool did not create exactly one Timeline Item")
    created_text_id = created_ids.pop()
    if _text_value(text_created, created_text_id) != "Text":
        raise QaFailure("Text tool did not create the canonical Text source")
    authored_position = _constant_vec2(
        text_created["project"]["items"][created_text_id], "position"
    )
    if any(
        abs(authored_position[axis] - expected_position[axis]) > 1.0e-3
        for axis in ("x", "y")
    ):
        raise QaFailure(
            "new Text position does not match the clicked canvas point: {} != {}".format(
                authored_position, expected_position
            )
        )
    if text_created["history"]["revision"] != before_text_create["history"]["revision"] + 1:
        raise QaFailure("Text creation was not one atomic Project transaction")
    if (
        len(text_created["project"].get("module_definitions") or {}),
        len(text_created["project"].get("module_instances") or {}),
    ) != original_module_counts:
        raise QaFailure("direct Text creation expanded the Item into a Node Module")
    client.wait_until(
        "new Text editing session",
        lambda: metadata
        if (metadata := _metadata(client, "preview.text.editor")).get("item_id")
        == created_text_id
        else None,
    )
    default_text_preview = _settled_preview(
        client,
        text_created["history"]["revision"],
        text_frame,
        "settled default new Text Preview",
    )
    default_text_hash = default_text_preview["editor"]["preview"]["pixel_hash"]
    created_value = "Canvas-created Text"
    client.key("a", True, command=True)
    client.key("a", False, command=True)
    client.key("backspace", True)
    client.key("backspace", False)
    client.wait_until(
        "new Text empty buffer remains targeted",
        lambda: metadata
        if (metadata := _metadata(client, "preview.text.editor")).get("buffer") == ""
        and metadata.get("item_id") == created_text_id
        else None,
    )
    client.inject("text", {"text": created_value})
    client.wait_until(
        "new Text transient buffer",
        lambda: metadata
        if (metadata := _metadata(client, "preview.text.editor")).get("buffer")
        == created_value
        else None,
    )
    new_text_draft = _settled_preview(
        client,
        text_created["history"]["revision"],
        text_frame,
        "settled new Text draft Preview",
    )
    if (
        new_text_draft["project"] != text_created["project"]
        or new_text_draft["history"] != text_created["history"]
    ):
        raise QaFailure("new Text typing mutated Project before acceptance")
    new_text_draft_hash = new_text_draft["editor"]["preview"]["pixel_hash"]
    if new_text_draft_hash == default_text_hash:
        raise QaFailure("new Text draft did not affect rendered pixels")
    _, draft_canvas = client.wait_component_settled("preview.canvas")
    _, draft_select_tool = client.wait_component_settled("preview.tool.select")
    if draft_canvas.get("rect_points") != precreate_canvas_rect:
        raise QaFailure("Text editing scrolled the Preview canvas outside its dock")
    if draft_select_tool.get("rect_points") != precreate_select_rect:
        raise QaFailure("Text editing moved the Preview toolbar outside its dock")
    text_draft_capture = capture_viewport(
        client,
        pathlib.Path(
            os.environ.get("RUVIE_QA_ARTIFACT_DIR", "target/qa-preview-authoring")
        )
        / "text-tool-draft.png",
    )
    client.click_component("preview.tool.select")
    accepted_creation = client.wait_until(
        "new Text accepted",
        lambda: state
        if _text_value((state := client.state()), created_text_id) == created_value
        else None,
    )
    if accepted_creation["history"]["revision"] != text_created["history"]["revision"] + 1:
        raise QaFailure("new Text edit was not one atomic Project transaction")
    accepted_creation_preview = _settled_preview(
        client,
        accepted_creation["history"]["revision"],
        text_frame,
        "settled accepted new Text Preview",
    )
    if accepted_creation_preview["editor"]["preview"]["pixel_hash"] != new_text_draft_hash:
        raise QaFailure("accepted new Text pixels differ from its transient draft")
    client.key("z", True, command=True)
    client.key("z", False, command=True)
    reverted_creation_edit = client.wait_until(
        "new Text edit Undo",
        lambda: state
        if _text_value((state := client.state()), created_text_id) == "Text"
        else None,
    )
    reverted_creation_preview = _settled_preview(
        client,
        reverted_creation_edit["history"]["revision"],
        text_frame,
        "settled new Text edit Undo",
    )
    if reverted_creation_preview["editor"]["preview"]["pixel_hash"] != default_text_hash:
        raise QaFailure("new Text edit Undo did not restore default pixels")
    client.key("z", True, command=True)
    client.key("z", False, command=True)
    reverted_creation = client.wait_until(
        "new Text creation Undo",
        lambda: state
        if created_text_id not in (state := client.state())["project"]["items"]
        else None,
    )
    reverted_creation_preview = _settled_preview(
        client,
        reverted_creation["history"]["revision"],
        text_frame,
        "settled new Text creation Undo",
    )
    if reverted_creation_preview["editor"]["preview"]["pixel_hash"] != before_create_hash:
        raise QaFailure("new Text creation Undo did not restore baseline pixels")

    # Undo removes the selected newly-created Item, so that selection must be
    # reconciled away. Select the original Text for the separate camera/gizmo
    # scenario instead of expecting an outline for a now nonexistent Item.
    client.click_component("timeline.item:" + text_item["id"])
    client.wait_until(
        "original Text selection for camera/gizmo checks",
        lambda: client.state()["editor"]["selection"]["primary"]
        == {"kind": "timeline_item", "id": text_item["id"]},
    )

    preview_before = initial["editor"]["preview"]
    if not preview_before["show_grid"]:
        client.click_component("preview.grid")
    client.click_component("preview.tool.pan")
    _, canvas = client.wait_component_settled("preview.canvas")
    start = component_center(canvas)
    end = {"x": start["x"] + 36.0, "y": start["y"] + 24.0}
    before_pan = client.state()["editor"]["preview"]
    client.drag(start, end, steps=12)

    def panned():
        state = client.state()
        preview = state["editor"]["preview"]
        return state if preview["pan"] != before_pan["pan"] else None

    after_pan = client.wait_until("Preview pan", panned)
    pan = after_pan["editor"]["preview"]
    if pan["show_grid"] is not True or pan["auto_fit"] is not False:
        raise QaFailure("Preview pan did not preserve the visible grid and leave auto-fit")
    canvas_metadata = _metadata(client, "preview.canvas")
    if canvas_metadata.get("pan") != pan["pan"]:
        raise QaFailure("Preview canvas metadata diverged from camera pan")

    # The position gizmo is a direct-edit affordance and is intentionally only
    # painted by the Select tool. Restore Select after exercising the Pan tool,
    # then verify that the real source outline follows the shared camera state.
    client.click_component("preview.tool.select")
    gizmo_after_pan = _metadata(client, "preview.position_gizmo")
    if gizmo_after_pan.get("canvas_transform", {}).get("pan") != pan["pan"]:
        raise QaFailure("Preview gizmo did not use the same pan as the grid/canvas")

    zoom_before = pan["zoom"]
    client.pinch_component("preview.canvas", 1.2, 0.62, 0.42)
    zoomed = client.wait_until(
        "Preview cursor zoom",
        lambda: state
        if (state := client.state())["editor"]["preview"]["zoom"] != zoom_before
        else None,
    )
    if zoomed["editor"]["preview"]["show_grid"] is not True:
        raise QaFailure("Preview zoom hid the grid")

    frame_before = zoomed["editor"]["timeline"]["current_frame"]
    client.click_component("timeline.play")
    playing = client.wait_until(
        "Timeline playback in Preview",
        lambda: state
        if (state := client.state())["editor"]["timeline"]["current_frame"] > frame_before
        else None,
        timeout=8.0,
    )
    client.click_component("timeline.play")
    stopped = client.wait_until(
        "Timeline playback stop",
        lambda: state
        if (state := client.state())["editor"]["timeline"]["is_playing"] is False
        else None,
    )
    client.wait_until(
        "Preview publication after playback",
        lambda: state
        if (state := _preview_ready(client))["editor"]["preview"]["rendered_frame"]
        is not None
        else None,
        timeout=20.0,
    )
    return {
        "suite": "preview-authoring",
        "item_id": text_item["id"],
        "gizmo": gizmo_metadata,
        "gizmo_after_pan": gizmo_after_pan,
        "text_edit": {
            "before": original_text,
            "accepted": edited_text,
            "empty_buffer_hash": empty_preview["editor"]["preview"]["pixel_hash"],
            "cancelled_draft_hash": cancelled_preview["editor"]["preview"][
                "pixel_hash"
            ],
            "escaped_hash": escaped_preview["editor"]["preview"]["pixel_hash"],
        },
        "canvas_text_creation": {
            "item_id": created_text_id,
            "accepted": created_value,
            "expected_position": expected_position,
            "authored_position": authored_position,
            "deselect_point": clear_selection_point,
            "draft_capture": text_draft_capture,
            "canvas_rect": precreate_canvas_rect,
            "select_tool_rect": precreate_select_rect,
        },
        "camera_before": preview_before,
        "camera_after_pan": pan,
        "camera_after_zoom": zoomed["editor"]["preview"],
        "playback_frames": {
            "before": frame_before,
            "playing": playing["editor"]["timeline"]["current_frame"],
            "stopped": stopped["editor"]["timeline"]["current_frame"],
        },
        "actions": client.evidence,
    }


if __name__ == "__main__":
    raise SystemExit(
        run_suite_main(
            "qa-preview-authoring-e2e",
            run_suite,
            "target/qa-preview-authoring-e2e-evidence.json",
        )
    )
