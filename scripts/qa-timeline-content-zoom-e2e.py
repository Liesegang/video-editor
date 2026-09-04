#!/usr/bin/env python3
"""Verify Timeline source visuals, shared waveform, and two-axis View Scale."""

from qa_support import (
    QaFailure,
    component_center,
    component_point,
    item_by_name,
    run_suite_main,
)


EXPECTED_VISUALS = {
    "QA Image": "image_thumbnail",
    "QA Overlap": "solid_color",
    "QA Text": "text_content",
    "QA Node Clip": "node_topology",
    "QA Audio": "audio_waveform",
    "QA Video": "video_frame_strip",
    "QA Rectangle": "shape_rectangle",
    "QA Composition": "composition_lanes",
}
DECODED_VISUALS = {"image_thumbnail", "video_frame_strip"}
BACKGROUND_VISUALS = DECODED_VISUALS | {"audio_waveform"}


def _component(snapshot, component_id):
    return next(
        (component for component in snapshot["components"] if component["id"] == component_id),
        None,
    )


def _near(left, right, tolerance=0.2):
    return abs(float(left) - float(right)) <= tolerance


def _scale_control(client, component_id):
    snapshot = client.component_snapshot()
    component = _component(snapshot, component_id)
    if component is None or component.get("visible") is not True:
        client.click_component("timeline.view_scale")
    return client.wait_component_settled(component_id)


def _click_scale_fraction(client, component_id, fraction):
    _, component = _scale_control(client, component_id)
    point = component_point(component, fraction, 0.5)
    client.inject(
        "click", {**point, "button": "primary", "coordinate_space": "points"}
    )
    return component, point


def _wait_scale_menu_closed(client):
    control_ids = {
        "timeline.view_scale.time",
        "timeline.view_scale.rows",
        "timeline.view_scale.reset",
    }

    def closed():
        present = {component["id"] for component in client.component_snapshot()["components"]}
        return True if control_ids.isdisjoint(present) else None

    client.wait_until("View Scale popover to close", closed, timeout=5.0)


def _ready_visuals(client, items):
    def ready():
        snapshot = client.component_snapshot()
        canvas = _component(snapshot, "timeline.canvas")
        if canvas is None:
            return None
        reports = {}
        for name, expected in EXPECTED_VISUALS.items():
            item = items[name]
            component = _component(snapshot, "timeline.content:" + item["id"])
            if component is None or component.get("visible") is not True:
                return None
            metadata = component.get("metadata") or {}
            if metadata.get("visual") != expected:
                return None
            if expected in DECODED_VISUALS and metadata.get("decoded_frames", 0) < 1:
                return None
            if expected == "audio_waveform" and (
                metadata.get("waveform_status") != "ready"
                or metadata.get("waveform_segments", 0) < 1
            ):
                return None
            if expected not in BACKGROUND_VISUALS and metadata.get("primitive_count", 0) < 1:
                return None
            reports[name] = component
        return snapshot, canvas, reports

    return client.wait_until("all eight Timeline source visuals", ready, timeout=30.0)


def _begin_reorder(client, start, target):
    client.inject(
        "press", {**start, "button": "primary", "coordinate_space": "points"}
    )
    client.inject(
        "move",
        {
            "x": start["x"],
            "y": start["y"] - 8.0,
            "button": "primary",
            "coordinate_space": "points",
        },
    )
    client.inject(
        "move", {**target, "button": "primary", "coordinate_space": "points"}
    )


def _wait_video_thumbnail(client, item_id):
    def ready():
        client.state()
        snapshot = client.component_snapshot()
        component = _component(snapshot, "timeline.content:" + item_id)
        if component is None or component.get("visible") is not True:
            return None
        metadata = component.get("metadata") or {}
        hashes = metadata.get("frame_hashes") or []
        if (
            metadata.get("visual") != "video_frame_strip"
            or metadata.get("ready") is not True
            or metadata.get("decoded_frames") != metadata.get("frame_slots")
            or metadata.get("failed_frames") != 0
            or not hashes
        ):
            return None
        return snapshot, component

    return client.wait_until("stable Video thumbnail strip", ready, timeout=30.0)


def _thumbnail_identity(component):
    metadata = component.get("metadata") or {}
    return {
        "frame_slots": metadata.get("frame_slots"),
        "frame_hashes": metadata.get("frame_hashes"),
        "requested_sizes": metadata.get("requested_sizes"),
    }


def _assert_thumbnail_identity(component, expected, operation):
    metadata = component.get("metadata") or {}
    if metadata.get("ready") is not True or metadata.get("decoded_frames", 0) < 1:
        raise QaFailure("Video thumbnail disappeared during {}".format(operation))
    if _thumbnail_identity(component) != expected:
        raise QaFailure("Video thumbnail identity changed during {}".format(operation))


def run_suite(client):
    client.wait_health()
    initial = client.state()
    project = initial["project"]
    if project.get("name") != "Authoring QA":
        raise QaFailure("Timeline content QA requires authoring_audio_e2e")
    asset_kinds = [str(asset.get("kind", "")).lower() for asset in project["assets"]]
    if len(project["assets"]) != 3 or sorted(asset_kinds) != ["audio", "image", "video"]:
        raise QaFailure("authoring_audio_e2e must contain one Audio, Image, and Video Asset")
    items = {name: item_by_name(project, name) for name in EXPECTED_VISUALS}
    track_id = items["QA Audio"]["track_id"]
    if any(item["track_id"] != track_id for item in items.values()):
        raise QaFailure("content fixture clips must share one expanded Track")
    before_project = project
    before_history = initial["history"]

    initial_snapshot, initial_canvas = client.wait_component_settled("timeline.canvas")
    initial_canvas_metadata = initial_canvas.get("metadata") or {}
    initial_zoom = initial_canvas_metadata.get("zoom") or {}
    initial_pan = initial_canvas_metadata.get("pan") or {}
    initial_origin = initial_canvas_metadata.get("screen_origin") or {}
    initial_track = _component(initial_snapshot, "timeline.track:" + track_id)
    if initial_track is None or (initial_track.get("metadata") or {}).get("expanded") is not True:
        raise QaFailure("fixture Track is not expanded")

    _, _, gesture_point = client.scroll_component(
        "timeline.canvas", 0.0, 4.0, modifiers={"ctrl": True, "shift": True}
    )

    def gesture_zoomed():
        state = client.state()
        snapshot = client.component_snapshot()
        canvas = _component(snapshot, "timeline.canvas")
        composition_row = _component(
            snapshot, "timeline.row:" + items["QA Composition"]["id"]
        )
        shape_row = _component(snapshot, "timeline.row:" + items["QA Rectangle"]["id"])
        track = _component(snapshot, "timeline.track:" + track_id)
        if canvas is None or composition_row is None or shape_row is None or track is None:
            return None
        metadata = canvas.get("metadata") or {}
        zoom = metadata.get("zoom") or {}
        if float(zoom.get("y", 0.0)) <= float(initial_zoom.get("y", 0.0)):
            return None
        return state, snapshot, canvas, track, composition_row, shape_row

    (
        gesture_state,
        gesture_snapshot,
        gesture_canvas,
        gesture_track,
        gesture_composition_row,
        gesture_shape_row,
    ) = client.wait_until("Ctrl/Cmd+Shift wheel row zoom", gesture_zoomed)
    gesture_metadata = gesture_canvas.get("metadata") or {}
    gesture_zoom = gesture_metadata["zoom"]
    gesture_pan = gesture_metadata["pan"]
    if not _near(gesture_zoom["x"], initial_zoom["x"], 0.01):
        raise QaFailure("row zoom changed the Timeline time scale")
    local_anchor_y = gesture_point["y"] - float(initial_origin["y"])
    world_before = (local_anchor_y - float(initial_pan["y"])) / float(initial_zoom["y"])
    world_after = (local_anchor_y - float(gesture_pan["y"])) / float(gesture_zoom["y"])
    if not _near(world_before, world_after, 0.05):
        raise QaFailure("row zoom did not preserve the pointer world anchor")
    metrics = gesture_metadata["row_metrics"]
    for component in (gesture_track, gesture_composition_row, gesture_shape_row):
        if not _near((component.get("metadata") or {})["row_height"], metrics["track_height"]):
            raise QaFailure("painted/hit-tested row height diverged from TimelineRowMetrics")
    center_delta = abs(
        gesture_shape_row["rect_points"]["center_y"]
        - gesture_composition_row["rect_points"]["center_y"]
    )
    if not _near(center_delta, metrics["stride"]):
        raise QaFailure("expanded Clip rows do not use the authoritative scaled stride")
    if not _near(
        gesture_state["editor"]["timeline"]["vertical_zoom"], gesture_zoom["y"], 0.001
    ):
        raise QaFailure("state and shared CanvasState disagree on vertical zoom")

    _, scale_menu = client.wait_component("timeline.view_scale")
    menu_metadata = scale_menu.get("metadata") or {}
    if "Ctrl/Cmd+Shift+wheel" not in menu_metadata.get("wheel_shortcut", ""):
        raise QaFailure("View Scale tooltip metadata omits the row-height wheel shortcut")
    row_slider, row_slider_point = _click_scale_fraction(
        client, "timeline.view_scale.rows", 0.0
    )
    row_slider_metadata = row_slider.get("metadata") or {}
    if row_slider_metadata.get("axis") != "y" or not _near(
        row_slider_metadata.get("minimum"), 17.6, 0.01
    ):
        raise QaFailure("Row height slider does not expose authoritative Y bounds")

    def rows_compacted():
        state = client.state()
        return (
            state
            if float(state["editor"]["timeline"]["vertical_zoom"])
            <= 0.56
            else None
        )

    first_compact_state = client.wait_until("View Scale Row height slider", rows_compacted)
    before_time_scale = float(
        first_compact_state["editor"]["timeline"]["pixels_per_second"]
    )
    before_row_scale = float(first_compact_state["editor"]["timeline"]["vertical_zoom"])
    _wait_scale_menu_closed(client)
    time_slider, time_slider_point = _click_scale_fraction(
        client, "timeline.view_scale.time", 0.20
    )
    time_metadata = time_slider.get("metadata") or {}
    if (
        time_metadata.get("axis") != "x"
        or time_metadata.get("minimum") != 8.0
        or time_metadata.get("maximum") != 2000.0
    ):
        raise QaFailure("Time scale slider does not expose navigation bounds")

    def time_scaled():
        state = client.state()
        timeline = state["editor"]["timeline"]
        if abs(float(timeline["pixels_per_second"]) - before_time_scale) < 1.0:
            return None
        if not _near(timeline["vertical_zoom"], before_row_scale, 0.01):
            raise QaFailure("Time scale slider changed Row height")
        return state

    scaled_time_state = client.wait_until("View Scale Time scale slider", time_scaled)
    _wait_scale_menu_closed(client)
    _scale_control(client, "timeline.view_scale.reset")
    client.click_component("timeline.view_scale.reset")

    def reset_complete():
        state = client.state()
        timeline = state["editor"]["timeline"]
        if _near(timeline["pixels_per_second"], 80.0, 0.01) and _near(
            timeline["vertical_zoom"], 1.0, 0.001
        ):
            return state
        return None

    reset_state = client.wait_until("View Scale Reset", reset_complete)
    _wait_scale_menu_closed(client)
    _, final_row_slider_point = _click_scale_fraction(
        client, "timeline.view_scale.rows", 0.0
    )
    compact_state = client.wait_until(
        "compact rows after View Scale Reset", rows_compacted
    )
    compact_snapshot, compact_canvas, visual_components = _ready_visuals(client, items)
    compact_canvas_rect = compact_canvas["rect_points"]
    for name, component in visual_components.items():
        metadata = component.get("metadata") or {}
        expected = EXPECTED_VISUALS[name]
        rect = component["rect_points"]
        if rect["min_y"] < compact_canvas_rect["min_y"] - 0.2 or rect["max_y"] > compact_canvas_rect["max_y"] + 0.2:
            raise QaFailure("{} content paints outside the Timeline viewport".format(name))
        if expected in BACKGROUND_VISUALS:
            if metadata.get("decode_thread") != "background":
                raise QaFailure("{} media decode is not marked as background".format(name))
            if metadata.get("uses_shared_media_cache") is not True:
                raise QaFailure("{} bypassed the shared media cache".format(name))
        elif metadata.get("decode_thread") is not None:
            raise QaFailure("{} incorrectly claims a background decode".format(name))

    video_item_id = items["QA Video"]["id"]
    stable_video_snapshot, stable_video = _wait_video_thumbnail(client, video_item_id)
    stable_video_identity = _thumbnail_identity(stable_video)
    consecutive_video_frames = []
    for sample in range(4):
        client.inject(
            "move",
            {
                "x": compact_canvas["rect_points"]["center_x"] + sample,
                "y": compact_canvas["rect_points"]["center_y"],
                "coordinate_space": "points",
            },
        )
        snapshot, component = _wait_video_thumbnail(client, video_item_id)
        _assert_thumbnail_identity(component, stable_video_identity, "consecutive repaint")
        consecutive_video_frames.append(snapshot["frame"])

    client.click_component("timeline.item_display:" + video_item_id)

    def video_keyframes_mode():
        snapshot = client.component_snapshot()
        clip = _component(snapshot, "timeline.item:" + video_item_id)
        summary = _component(snapshot, "timeline.keyframes:" + video_item_id)
        content = _component(snapshot, "timeline.content:" + video_item_id)
        if (
            clip is None
            or (clip.get("metadata") or {}).get("display_mode") != "keyframes"
            or summary is None
            or content is not None
        ):
            return None
        return snapshot, summary

    video_keyframe_snapshot, video_keyframe_summary = client.wait_until(
        "Video Keyframes display mode", video_keyframes_mode
    )
    client.click_component("timeline.item_display:" + video_item_id)
    video_content_snapshot, video_after_mode = _wait_video_thumbnail(client, video_item_id)
    _assert_thumbnail_identity(video_after_mode, stable_video_identity, "display-mode toggle")

    _, video_item_component = client.wait_component_settled(
        "timeline.item:" + video_item_id
    )
    video_drag_start = component_center(video_item_component)
    video_drag_hold = {"x": video_drag_start["x"] + 18.0, "y": video_drag_start["y"]}
    client.inject(
        "press",
        {**video_drag_start, "button": "primary", "coordinate_space": "points"},
    )
    client.inject(
        "move",
        {**video_drag_hold, "button": "primary", "coordinate_space": "points"},
    )
    video_drag_snapshot, video_during_drag = _wait_video_thumbnail(client, video_item_id)
    _assert_thumbnail_identity(video_during_drag, stable_video_identity, "Clip drag")
    client.key("escape", True)
    client.key("escape", False)
    client.inject(
        "release",
        {**video_drag_hold, "button": "primary", "coordinate_space": "points"},
    )
    video_cancel_snapshot, video_after_cancel = _wait_video_thumbnail(client, video_item_id)
    _assert_thumbnail_identity(video_after_cancel, stable_video_identity, "Clip drag cancel")

    client.scroll_component("timeline.canvas", 0.0, -1.0)
    _, video_after_scroll = _wait_video_thumbnail(client, video_item_id)
    _assert_thumbnail_identity(video_after_scroll, stable_video_identity, "Timeline scroll")
    client.scroll_component("timeline.canvas", 0.0, 1.0)
    video_scroll_return_snapshot, video_after_scroll_return = _wait_video_thumbnail(
        client, video_item_id
    )
    _assert_thumbnail_identity(
        video_after_scroll_return, stable_video_identity, "Timeline scroll return"
    )

    audio_item_id = items["QA Audio"]["id"]
    timeline_waveform = _component(compact_snapshot, "timeline.waveform:" + audio_item_id)
    timeline_waveform_metadata = (timeline_waveform or {}).get("metadata") or {}
    if (
        timeline_waveform is None
        or timeline_waveform_metadata.get("status") != "ready"
        or timeline_waveform_metadata.get("segment_count", 0) < 1
        or timeline_waveform_metadata.get("failed_chunks") != 0
        or timeline_waveform_metadata.get("shared_painter") != "authoring_waveform"
        or timeline_waveform_metadata.get("cache_owner")
        != "CacheManager.audio_waveform"
    ):
        raise QaFailure("Timeline Audio clip did not paint its shared real waveform")

    audio_asset = next(
        asset for asset in project["assets"] if str(asset.get("kind", "")).lower() == "audio"
    )
    client.click_component("assets.asset:" + audio_asset["id"])

    def inspector_waveform_ready():
        snapshot = client.component_snapshot()
        preview = _component(snapshot, "inspector.asset_preview:" + audio_asset["id"])
        if preview is None:
            return None
        metadata = preview.get("metadata") or {}
        if metadata.get("waveform_status") != "ready" or metadata.get(
            "waveform_segments", 0
        ) < 1:
            return None
        return snapshot, preview

    inspector_snapshot, inspector_preview = client.wait_until(
        "real Audio waveform in Inspector", inspector_waveform_ready, timeout=30.0
    )
    inspector_metadata = inspector_preview.get("metadata") or {}
    if inspector_metadata.get("visual") != "audio_waveform":
        raise QaFailure("Inspector Audio still uses a placeholder representation")
    if inspector_metadata.get("waveform_shared_painter") != timeline_waveform_metadata.get(
        "shared_painter"
    ):
        raise QaFailure("Inspector and Timeline do not use the same waveform painter")
    if inspector_metadata.get("waveform_cache_owner") != timeline_waveform_metadata.get(
        "cache_owner"
    ):
        raise QaFailure("Inspector and Timeline do not use the same waveform cache")
    if inspector_metadata.get("waveform_sample_rate") != timeline_waveform_metadata.get(
        "sample_rate"
    ):
        raise QaFailure("Inspector and Timeline waveform formats differ")
    if (
        inspector_metadata.get("waveform_ready_chunks", 0) < 1
        or inspector_metadata.get("waveform_failed_chunks") != 0
        or inspector_metadata.get("waveform_decode_thread") != "background"
        or inspector_metadata.get("uses_shared_media_cache") is not True
    ):
        raise QaFailure("Inspector Audio waveform did not resolve shared cached samples")

    before_reorder = client.state()
    _, source_item = client.wait_component_settled(
        "timeline.item:" + items["QA Rectangle"]["id"]
    )
    _, source_row = client.wait_component_settled(
        "timeline.row:" + items["QA Rectangle"]["id"]
    )
    _, target_row = client.wait_component_settled(
        "timeline.row:" + items["QA Composition"]["id"]
    )
    reorder_start = component_center(source_item)
    reorder_target = {
        "x": reorder_start["x"],
        "y": target_row["rect_points"]["min_y"]
        + target_row["rect_points"]["height"] * 0.2,
    }
    _begin_reorder(client, reorder_start, reorder_target)

    def live_reorder():
        snapshot = client.component_snapshot()
        preview = _component(snapshot, "timeline.reorder_preview")
        moved_source = _component(snapshot, "timeline.row:" + items["QA Rectangle"]["id"])
        moved_target = _component(snapshot, "timeline.row:" + items["QA Composition"]["id"])
        if preview is None or moved_source is None or moved_target is None:
            return None
        metadata = preview.get("metadata") or {}
        if metadata.get("projected_layer") != items["QA Composition"]["layer"]:
            return None
        if not _near(
            moved_source["rect_points"]["center_y"], target_row["rect_points"]["center_y"]
        ) or not _near(
            moved_target["rect_points"]["center_y"], source_row["rect_points"]["center_y"]
        ):
            return None
        return snapshot, preview

    reorder_snapshot, reorder_preview = client.wait_until(
        "scaled live row reorder projection", live_reorder
    )
    held = client.state()
    if held["project"] != before_reorder["project"] or held["history"] != before_reorder["history"]:
        raise QaFailure("live scaled reorder mutated Project/history before release")
    client.key("escape", True)
    client.key("escape", False)

    def reorder_cancelled():
        state = client.state()
        snapshot = client.component_snapshot()
        restored_source = _component(
            snapshot, "timeline.row:" + items["QA Rectangle"]["id"]
        )
        restored_target = _component(
            snapshot, "timeline.row:" + items["QA Composition"]["id"]
        )
        if restored_source is None or restored_target is None:
            return None
        if state["editor"]["timeline"]["item_gesture_active"]:
            return None
        if not _near(
            restored_source["rect_points"]["center_y"], source_row["rect_points"]["center_y"]
        ) or not _near(
            restored_target["rect_points"]["center_y"], target_row["rect_points"]["center_y"]
        ):
            return None
        return state

    after_cancel = client.wait_until("Escape restoring scaled row order", reorder_cancelled)
    client.inject(
        "release",
        {**reorder_target, "button": "primary", "coordinate_space": "points"},
    )
    if after_cancel["project"] != before_reorder["project"] or after_cancel[
        "history"
    ] != before_reorder["history"]:
        raise QaFailure("Escape changed Project/history")

    _, image_asset_row = client.wait_component_settled("assets.asset:" + next(
        asset["id"]
        for asset in project["assets"]
        if str(asset.get("kind", "")).lower() == "image"
    ))
    _, drop_canvas = client.wait_component_settled("timeline.canvas")
    _, drop_row = client.wait_component_settled(
        "timeline.row:" + items["QA Rectangle"]["id"]
    )
    drop_start = component_center(image_asset_row)
    drop_target = {
        "x": drop_canvas["rect_points"]["min_x"] + 320.0,
        "y": drop_row["rect_points"]["center_y"],
    }
    before_drop = client.state()
    before_item_ids = set(before_drop["project"]["items"])
    client.drag(drop_start, drop_target, steps=18)

    def dropped_once():
        state = client.state()
        added = set(state["project"]["items"]) - before_item_ids
        if (
            len(added) == 1
            and state["history"]["revision"] == before_drop["history"]["revision"] + 1
            and state["editor"]["timeline"]["library_drag_active"] is False
        ):
            return state, next(iter(added))
        return None

    after_drop, dropped_item_id = client.wait_until(
        "one scaled Timeline library drop", dropped_once
    )
    dropped_item = after_drop["project"]["items"][dropped_item_id]
    if dropped_item["track_id"] != track_id or dropped_item["layer"] != items[
        "QA Rectangle"
    ]["layer"] + 1:
        raise QaFailure("scaled drop target disagreed with the painted Clip row")
    _, dropped_component = client.wait_component_settled("timeline.item:" + dropped_item_id)
    if (dropped_component.get("metadata") or {}).get("display_row_index") != (
        drop_row.get("metadata") or {}
    ).get("display_row_index"):
        raise QaFailure("drop result did not occupy the targeted canonical row slot")
    client.key("z", True, command=True)
    client.key("z", False, command=True)

    def drop_undone():
        state = client.state()
        return state if state["project"] == before_drop["project"] else None

    after_drop_undo = client.wait_until("one Undo restoring library drop", drop_undone)
    if after_drop_undo["history"]["can_redo"] is not True:
        raise QaFailure("library drop was not one undoable history entry")

    final = client.state()
    if final["project"] != before_drop["project"]:
        raise QaFailure("view controls mutated the authoritative Project")
    if final["editor"].get("error") is not None:
        raise QaFailure("Timeline content QA reported an editor error: {}".format(final["editor"]["error"]))

    return {
        "suite": "timeline-content-zoom",
        "fixture_sources": EXPECTED_VISUALS,
        "gesture": {
            "point": gesture_point,
            "before_canvas": initial_canvas,
            "after_canvas": gesture_canvas,
            "row_stride": metrics["stride"],
        },
        "view_scale": {
            "menu": scale_menu,
            "row_slider_point": row_slider_point,
            "row_slider_after_reset_point": final_row_slider_point,
            "compact_timeline": compact_state["editor"]["timeline"],
            "time_slider_point": time_slider_point,
            "scaled_time": scaled_time_state["editor"]["timeline"],
            "reset": reset_state["editor"]["timeline"],
        },
        "visuals": visual_components,
        "video_thumbnail_stability": {
            "identity": stable_video_identity,
            "initial_frame": stable_video_snapshot["frame"],
            "consecutive_frames": consecutive_video_frames,
            "keyframes_frame": video_keyframe_snapshot["frame"],
            "keyframes_summary": video_keyframe_summary,
            "content_return_frame": video_content_snapshot["frame"],
            "drag_frame": video_drag_snapshot["frame"],
            "drag_cancel_frame": video_cancel_snapshot["frame"],
            "scroll_return_frame": video_scroll_return_snapshot["frame"],
        },
        "timeline_waveform": timeline_waveform,
        "inspector_waveform": inspector_preview,
        "inspector_frame": inspector_snapshot["frame"],
        "reorder": {
            "preview": reorder_preview,
            "frame": reorder_snapshot["frame"],
            "from": reorder_start,
            "to": reorder_target,
        },
        "drop": {
            "from": drop_start,
            "to": drop_target,
            "item": dropped_item,
            "undo_history": after_drop_undo["history"],
        },
        "history": final["history"],
        "actions": client.evidence,
    }


if __name__ == "__main__":
    raise SystemExit(
        run_suite_main(
            "qa-timeline-content-zoom-e2e",
            run_suite,
            "target/qa-timeline-content-zoom-e2e-evidence.json",
        )
    )
