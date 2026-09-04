#!/usr/bin/env python3
"""Verify Preview render, shared camera/grid, source gizmo, and playback."""

from qa_support import (
    QaFailure,
    component_center,
    component_point,
    item_by_name,
    run_suite_main,
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
    # projection; leaving the editor commits exactly one authoritative change.
    before_text_edit = client.state()
    original_text = _text_value(before_text_edit, text_item["id"])
    edited_text = "Preview direct text"
    client.click_component("preview.tool.text")
    client.wait_component("preview.text.editor")
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
    client.click_component("preview.tool.select")
    edited = client.wait_until(
        "one accepted Preview Text transaction",
        lambda: state
        if _text_value((state := client.state()), text_item["id"]) == edited_text
        else None,
    )
    if edited["history"]["revision"] != before_text_edit["history"]["revision"] + 1:
        raise QaFailure("Preview Text session was not exactly one transaction")
    client.key("z", True, command=True)
    client.key("z", False, command=True)
    client.wait_until(
        "one-step Preview Text Undo",
        lambda: state
        if _text_value((state := client.state()), text_item["id"]) == original_text
        else None,
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
        "text_edit": {"before": original_text, "accepted": edited_text},
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
