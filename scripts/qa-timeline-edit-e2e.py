#!/usr/bin/env python3
"""Verify Timeline selection, independent edits, and atomic cross-Track group moves."""

from qa_support import (
    QaFailure,
    component_center,
    item_by_name,
    media_seconds,
    run_suite_main,
    seek_timeline_seconds,
)


TARGET_NAME = "QA Overlap"
SIBLING_NAME = "QA Image"


def _interval(item):
    return (
        media_seconds(item["interval"]["start"]),
        media_seconds(item["interval"]["duration"]),
    )


def _component(snapshot, component_id):
    return next(
        (component for component in snapshot["components"] if component["id"] == component_id),
        None,
    )


def _near(left, right, tolerance=1.0):
    return abs(float(left) - float(right)) <= tolerance


def _click_with_modifiers(client, component, modifiers):
    point = component_center(component)
    client.inject(
        "click",
        {
            **point,
            "button": "primary",
            "coordinate_space": "points",
            "modifiers": modifiers,
        },
    )


def _selected_item_ids(client):
    return set(client.state()["editor"]["selection"]["item_ids"])


def _verify_track_headers(client):
    before = client.state()
    project = before["project"]
    timeline_id = project["root_timeline_id"]
    order = project["timelines"][timeline_id]["track_order"]
    source_id, target_id = order[-1], order[-2]
    _, source = client.wait_component_settled("timeline.track_header:" + source_id)
    _, target = client.wait_component_settled("timeline.track_header:" + target_id)
    start, end = component_center(source), component_center(target)
    client.inject("press", {**start, "button": "primary", "coordinate_space": "points"})
    client.inject("move", {**end, "button": "primary", "coordinate_space": "points"})
    expected = list(order)
    expected.remove(source_id)
    expected.insert(order.index(target_id), source_id)

    def projected():
        snapshot = client.component_snapshot()
        preview = _component(snapshot, "timeline.track_reorder_preview")
        if preview and preview.get("metadata", {}).get("displayed_order") == expected[::-1]:
            return snapshot, preview
        return None

    held_snapshot, held = client.wait_until("Track blocks reflow while header drag is held", projected)
    if client.state()["project"] != project or client.state()["history"] != before["history"]:
        raise QaFailure("Header drag mutated the Project before release")
    original_target_y = target["rect_points"]["center_y"]
    moved_target = _component(held_snapshot, "timeline.track_header:" + target_id)
    if moved_target is None or _near(moved_target["rect_points"]["center_y"], original_target_y):
        raise QaFailure("Header drag showed an overlay but did not reflow the actual rows")
    client.inject("release", {**end, "button": "primary", "coordinate_space": "points"})
    committed = client.wait_until(
        "Header release commits Track order",
        lambda: state if (state := client.state())["project"]["timelines"][timeline_id]["track_order"] == expected else None,
    )
    if committed["history"]["revision"] != before["history"]["revision"] + 1:
        raise QaFailure("Track reorder did not commit one undoable edit")
    if committed["project"]["items"] != project["items"]:
        raise QaFailure("Track header reorder changed clip placement")
    client.key("z", True, command=True)
    client.key("z", False, command=True)
    client.wait_until("Undo restores Track block order", lambda: client.state()["project"] == project)

    image_track_id = item_by_name(project, SIBLING_NAME)["track_id"]
    seek_timeline_seconds(client, 0.5)
    baseline = client.state()
    def rendered(revision):
        state = client.state()
        preview = state["editor"]["preview"]
        if (state["history"]["revision"] == revision
                and preview.get("rendered_revision") == revision
                and preview.get("rendered_frame") == state["editor"]["timeline"]["current_frame"]
                and preview.get("pixel_hash") is not None
                and state["editor"].get("error") is None):
            return state
        return None

    visible = client.wait_until("Visible Track Preview", lambda: rendered(baseline["history"]["revision"]))
    client.click_component("timeline.track_visibility:" + image_track_id)
    hidden = client.wait_until(
        "Hidden Track Preview",
        lambda: rendered(baseline["history"]["revision"] + 1),
    )
    if hidden["editor"]["preview"]["pixel_hash"] == visible["editor"]["preview"]["pixel_hash"]:
        raise QaFailure("Track Eye changed state but not rendered pixels")
    if hidden["project"]["items"] != project["items"]:
        raise QaFailure("Track Eye changed clip placement")
    client.key("z", True, command=True)
    client.key("z", False, command=True)
    restored = client.wait_until(
        "Undo Track visibility restores Preview",
        lambda: rendered(baseline["history"]["revision"] + 2),
    )
    if restored["project"] != project or restored["editor"]["preview"]["pixel_hash"] != visible["editor"]["preview"]["pixel_hash"]:
        raise QaFailure("Track visibility Undo did not restore model and pixels")
    return {"held_preview": held, "committed_order": expected, "visibility_pixel_change": True}


def run_suite(client):
    client.wait_health()
    initial = client.state()
    target = item_by_name(initial["project"], TARGET_NAME)
    sibling = item_by_name(initial["project"], SIBLING_NAME)
    item_id = target["id"]
    sibling_id = sibling["id"]
    component_id = "timeline.item:" + item_id
    source_component_id = "timeline.item:" + sibling_id
    source_row_id = "timeline.row:" + sibling_id
    target_row_id = "timeline.row:" + item_id
    client.wait_component_settled(component_id)
    _, source_component = client.wait_component_settled(source_component_id)
    _, source_row = client.wait_component_settled(source_row_id)
    _, target_row = client.wait_component_settled(target_row_id)
    before_items = dict(initial["project"]["items"])
    before_target = _interval(target)
    before_sibling = _interval(sibling)
    before_project = initial["project"]
    before_history = initial["history"]

    _click_with_modifiers(client, source_component, {})
    _click_with_modifiers(client, target_row, {"shift": True})
    selected_by_shift = _selected_item_ids(client)
    if selected_by_shift != {item_id, sibling_id}:
        raise QaFailure("Shift-click did not add the second Timeline clip")
    _click_with_modifiers(client, source_component, {"ctrl": True})
    selected_after_toggle = _selected_item_ids(client)
    if selected_after_toggle != {item_id}:
        raise QaFailure("Ctrl-click did not toggle only the clicked Timeline clip")

    _, timeline_canvas = client.wait_component_settled("timeline.canvas")
    canvas = timeline_canvas["rect_points"]
    marquee_start = {
        "x": canvas["max_x"] - 8.0,
        "y": canvas["max_y"] - 8.0,
    }
    marquee_end = {
        "x": min(source_component["rect_points"]["min_x"], target_row["rect_points"]["min_x"])
        + 2.0,
        "y": min(source_row["rect_points"]["min_y"], target_row["rect_points"]["min_y"])
        + 2.0,
    }
    client.drag(marquee_start, marquee_end, steps=12)
    selected_by_marquee = _selected_item_ids(client)
    if not {item_id, sibling_id}.issubset(selected_by_marquee):
        raise QaFailure("Timeline marquee did not select intersecting clips")

    _click_with_modifiers(client, target_row, {})
    if _selected_item_ids(client) != {item_id}:
        raise QaFailure("plain Timeline click did not replace the multi-selection")

    reorder_start = component_center(source_component)
    target_rect = target_row["rect_points"]
    reorder_end = {
        "x": reorder_start["x"],
        "y": target_rect["min_y"] + target_rect["height"] * 0.2,
    }

    def begin_reorder():
        client.inject(
            "press",
            {**reorder_start, "button": "primary", "coordinate_space": "points"},
        )
        client.inject(
            "move",
            {
                "x": reorder_start["x"],
                "y": reorder_start["y"] - 8.0,
                "button": "primary",
                "coordinate_space": "points",
            },
        )
        client.inject(
            "move",
            {**reorder_end, "button": "primary", "coordinate_space": "points"},
        )

    begin_reorder()

    def live_reflow():
        snapshot = client.component_snapshot()
        preview = _component(snapshot, "timeline.reorder_preview")
        source_live = _component(snapshot, source_row_id)
        target_live = _component(snapshot, target_row_id)
        if preview is None or source_live is None or target_live is None:
            return None
        preview_metadata = preview.get("metadata") or {}
        if preview_metadata.get("projected_layer") != 1:
            return None
        if not _near(
            source_live["rect_points"]["center_y"],
            target_row["rect_points"]["center_y"],
        ):
            return None
        if not _near(
            target_live["rect_points"]["center_y"],
            source_row["rect_points"]["center_y"],
        ):
            return None
        return snapshot, preview, source_live, target_live

    held_snapshot, held_preview, held_source_row, held_target_row = client.wait_until(
        "live Timeline row reflow before release", live_reflow
    )
    held_state = client.state()
    if held_state["project"] != before_project:
        raise QaFailure("live row reflow mutated the Project before release")
    if held_state["history"] != before_history:
        raise QaFailure("live row reflow changed history before release")
    if held_state["editor"]["timeline"]["item_gesture_active"] is not True:
        raise QaFailure("live row reflow did not retain the active gesture")

    client.key("escape", True)
    client.key("escape", False)

    def cancelled():
        state = client.state()
        snapshot = client.component_snapshot()
        restored_source = _component(snapshot, source_row_id)
        restored_target = _component(snapshot, target_row_id)
        if restored_source is None or restored_target is None:
            return None
        if state["editor"]["timeline"]["item_gesture_active"]:
            return None
        if not _near(
            restored_source["rect_points"]["center_y"],
            source_row["rect_points"]["center_y"],
        ):
            return None
        if not _near(
            restored_target["rect_points"]["center_y"],
            target_row["rect_points"]["center_y"],
        ):
            return None
        return state

    after_cancel = client.wait_until("Escape restoring the original row order", cancelled)
    if after_cancel["project"] != before_project or after_cancel["history"] != before_history:
        raise QaFailure("Escape changed the Project or history")
    client.inject(
        "release",
        {**reorder_end, "button": "primary", "coordinate_space": "points"},
    )

    begin_reorder()
    client.wait_until("committable Timeline row reflow", live_reflow)
    client.inject(
        "release",
        {**reorder_end, "button": "primary", "coordinate_space": "points"},
    )

    def reordered():
        state = client.state()
        moved_source = state["project"]["items"][sibling_id]
        displaced_target = state["project"]["items"][item_id]
        if (
            moved_source["layer"] == 1
            and displaced_target["layer"] == 0
            and state["history"]["revision"] == before_history["revision"] + 1
            and state["editor"]["timeline"]["item_gesture_active"] is False
        ):
            return state
        return None

    after_reorder = client.wait_until("one committed Timeline row reorder", reordered)
    client.key("z", True, command=True)
    client.key("z", False, command=True)

    def undo_restored():
        state = client.state()
        return state if state["project"] == before_project else None

    after_reorder_undo = client.wait_until(
        "one Undo restoring the whole row reorder", undo_restored
    )
    if after_reorder_undo["history"]["can_redo"] is not True:
        raise QaFailure("row reorder was not represented by one undoable history entry")

    client.drag_component_by(component_id, 47.0, 0.0, steps=14)

    def moved():
        state = client.state()
        candidate = state["project"]["items"][item_id]
        return state if _interval(candidate)[0] != before_target[0] else None

    after_move = client.wait_until("one independent clip move", moved)
    moved_target = after_move["project"]["items"][item_id]
    if _interval(moved_target)[1] != before_target[1]:
        raise QaFailure("moving a clip changed its duration")
    if _interval(after_move["project"]["items"][sibling_id]) != before_sibling:
        raise QaFailure("moving the lower clip also moved the upper clip")
    for candidate_id, before in before_items.items():
        if candidate_id != item_id and after_move["project"]["items"][candidate_id] != before:
            raise QaFailure("moving one clip mutated sibling {}".format(candidate_id))

    _, moved_component = client.wait_component_settled(component_id)
    _, end_edge = client.wait_component_settled(
        "timeline.item.trim_end:" + item_id
    )
    moved_rect = moved_component["rect_points"]
    end_drag_start = component_center(end_edge)
    end_drag_end = {"x": end_drag_start["x"] + 36.0, "y": end_drag_start["y"]}
    before_trim = after_move["project"]["items"]
    client.drag(end_drag_start, end_drag_end, steps=12)

    def end_trimmed():
        state = client.state()
        current = state["project"]["items"][item_id]
        return state if _interval(current)[1] > _interval(moved_target)[1] else None

    after_end_trim = client.wait_until("right edge extending one clip", end_trimmed)
    end_trimmed_target = after_end_trim["project"]["items"][item_id]
    _, end_trimmed_component = client.wait_component_settled(component_id)
    end_trimmed_rect = end_trimmed_component["rect_points"]
    if _interval(end_trimmed_target)[0] != _interval(moved_target)[0]:
        raise QaFailure("right-edge trim changed the clip start")
    if not _near(end_trimmed_rect["min_x"], moved_rect["min_x"]):
        raise QaFailure("right-edge trim moved the rendered clip start")
    if end_trimmed_rect["max_x"] <= moved_rect["max_x"] + 10.0:
        raise QaFailure("right-edge trim did not extend the rendered clip")
    for candidate_id, before in before_trim.items():
        if (
            candidate_id != item_id
            and after_end_trim["project"]["items"][candidate_id] != before
        ):
            raise QaFailure(
                "right-edge trim mutated sibling {}".format(candidate_id)
            )

    _, start_edge = client.wait_component_settled(
        "timeline.item.trim_start:" + item_id
    )
    start_drag_start = component_center(start_edge)
    start_drag_end = {
        "x": start_drag_start["x"] + 36.0,
        "y": start_drag_start["y"],
    }
    client.drag(start_drag_start, start_drag_end, steps=12)

    def start_trimmed():
        state = client.state()
        current = state["project"]["items"][item_id]
        return state if _interval(current)[0] > _interval(end_trimmed_target)[0] else None

    after_start_trim = client.wait_until("left edge shortening one clip", start_trimmed)
    start_trimmed_target = after_start_trim["project"]["items"][item_id]
    _, start_trimmed_component = client.wait_component_settled(component_id)
    start_trimmed_rect = start_trimmed_component["rect_points"]
    end_before_start_trim = sum(_interval(end_trimmed_target))
    end_after_start_trim = sum(_interval(start_trimmed_target))
    if abs(end_before_start_trim - end_after_start_trim) > 1.0e-6:
        raise QaFailure("left-edge trim changed the clip end")
    if start_trimmed_rect["min_x"] <= end_trimmed_rect["min_x"] + 10.0:
        raise QaFailure("left-edge trim did not shorten the rendered clip")
    if not _near(start_trimmed_rect["max_x"], end_trimmed_rect["max_x"]):
        raise QaFailure("left-edge trim moved the rendered clip end")
    if after_start_trim["editor"]["timeline"]["item_gesture_active"]:
        raise QaFailure("Timeline trim gesture remained active after release")

    client.key("z", True, command=True)
    client.key("z", False, command=True)
    after_start_undo = client.wait_until(
        "one Undo restoring the left-edge trim",
        lambda: state
        if (state := client.state())["project"] == after_end_trim["project"]
        else None,
    )
    _, restored_end_component = client.wait_component_settled(component_id)
    if not _near(
        restored_end_component["rect_points"]["min_x"],
        end_trimmed_rect["min_x"],
    ):
        raise QaFailure("left-edge trim Undo did not restore rendered geometry")

    client.key("z", True, command=True)
    client.key("z", False, command=True)
    after_trim_undo = client.wait_until(
        "one Undo restoring the right-edge trim",
        lambda: state
        if (state := client.state())["project"] == after_move["project"]
        else None,
    )
    _, restored_move_component = client.wait_component_settled(component_id)
    restored_move_rect = restored_move_component["rect_points"]
    if not _near(restored_move_rect["min_x"], moved_rect["min_x"]) or not _near(
        restored_move_rect["max_x"], moved_rect["max_x"]
    ):
        raise QaFailure("right-edge trim Undo did not restore rendered geometry")
    if after_trim_undo["history"]["can_redo"] is not True:
        raise QaFailure("clip trims were not represented by undoable history entries")

    before_group = after_trim_undo
    timeline_id = before_group["project"]["root_timeline_id"]
    before_track_ids = set(
        before_group["project"]["timelines"][timeline_id]["track_order"]
    )
    _, timeline_canvas = client.wait_component_settled("timeline.canvas")
    canvas_rect = timeline_canvas["rect_points"]
    client.inject(
        "click",
        {
            "x": canvas_rect["max_x"] - 6.0,
            "y": canvas_rect["max_y"] - 6.0,
            "button": "secondary",
            "coordinate_space": "points",
        },
    )
    client.wait_component("timeline.menu.add_track")
    client.click_component("timeline.menu.add_track")

    def added_track():
        state = client.state()
        track_ids = set(state["project"]["timelines"][timeline_id]["track_order"])
        created = track_ids - before_track_ids
        return (state, next(iter(created))) if len(created) == 1 else None

    after_add_track, target_track_id = client.wait_until(
        "Timeline context menu Add Track", added_track
    )
    _, destination_track = client.wait_component_settled(
        "timeline.track:" + target_track_id
    )

    _, group_target_component = client.wait_component_settled(component_id)
    _, group_sibling_component = client.wait_component_settled(source_component_id)
    _click_with_modifiers(client, group_target_component, {})
    _click_with_modifiers(client, group_sibling_component, {"shift": True})
    if _selected_item_ids(client) != {item_id, sibling_id}:
        raise QaFailure("cross-Track group drag could not select both clips")
    before_group_move = client.state()
    primary_before = before_group_move["project"]["items"][sibling_id]
    target_before = before_group_move["project"]["items"][item_id]
    group_start = component_center(group_sibling_component)
    destination_rect = destination_track["rect_points"]
    group_end = {
        "x": group_start["x"] + 24.0,
        "y": destination_rect["center_y"],
    }
    client.drag(group_start, group_end, steps=14)

    def group_moved():
        state = client.state()
        primary = state["project"]["items"][sibling_id]
        target = state["project"]["items"][item_id]
        if (
            primary["track_id"] == target_track_id
            and target["track_id"] == target_track_id
            and state["history"]["revision"]
            == before_group_move["history"]["revision"] + 1
            and not state["editor"]["timeline"]["item_gesture_active"]
        ):
            return state
        return None

    after_group_move = client.wait_until(
        "atomic multi-clip cross-Track drag", group_moved
    )
    primary_after = after_group_move["project"]["items"][sibling_id]
    target_after = after_group_move["project"]["items"][item_id]
    primary_delta = _interval(primary_after)[0] - _interval(primary_before)[0]
    target_delta = _interval(target_after)[0] - _interval(target_before)[0]
    if abs(primary_delta - target_delta) > 1.0e-6:
        raise QaFailure("group drag did not preserve relative clip times")
    before_layer_order = sorted(
        (primary_before, target_before), key=lambda item: (item["layer"], item["id"])
    )
    after_layer_order = sorted(
        (primary_after, target_after), key=lambda item: (item["layer"], item["id"])
    )
    if [item["id"] for item in before_layer_order] != [
        item["id"] for item in after_layer_order
    ]:
        raise QaFailure("group drag did not preserve relative layer order")

    client.key("z", True, command=True)
    client.key("z", False, command=True)
    after_group_undo = client.wait_until(
        "one Undo restoring multi-clip cross-Track drag",
        lambda: state
        if (state := client.state())["project"] == before_group_move["project"]
        else None,
    )
    track_headers = _verify_track_headers(client)
    return {
        "suite": "timeline-independent-edit",
        "track_headers": track_headers,
        "item_id": item_id,
        "sibling_id": sibling_id,
        "intervals": {
            "before": before_target,
            "moved": _interval(moved_target),
            "end_trimmed": _interval(end_trimmed_target),
            "start_trimmed": _interval(start_trimmed_target),
            "sibling": before_sibling,
        },
        "trim": {
            "end_from": end_drag_start,
            "end_to": end_drag_end,
            "start_from": start_drag_start,
            "start_to": start_drag_end,
            "end_undo_history": after_start_undo["history"],
            "all_trim_undo_history": after_trim_undo["history"],
        },
        "reorder": {
            "from": reorder_start,
            "to": reorder_end,
            "held_preview": held_preview,
            "held_source_row": held_source_row,
            "held_target_row": held_target_row,
            "committed_revision": after_reorder["history"]["revision"],
            "undo_history": after_reorder_undo["history"],
        },
        "multi_selection": {
            "shift": sorted(selected_by_shift),
            "ctrl_toggle": sorted(selected_after_toggle),
            "marquee": sorted(selected_by_marquee),
            "marquee_from": marquee_start,
            "marquee_to": marquee_end,
        },
        "cross_track_group_move": {
            "target_track_id": target_track_id,
            "from": group_start,
            "to": group_end,
            "time_delta": primary_delta,
            "committed_revision": after_group_move["history"]["revision"],
            "undo_history": after_group_undo["history"],
            "track_creation_revision": after_add_track["history"]["revision"],
        },
        "history": after_trim_undo["history"],
        "actions": client.evidence,
    }


if __name__ == "__main__":
    raise SystemExit(
        run_suite_main(
            "qa-timeline-edit-e2e",
            run_suite,
            "target/qa-timeline-edit-e2e-evidence.json",
        )
    )
