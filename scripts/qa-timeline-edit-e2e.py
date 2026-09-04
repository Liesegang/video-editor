#!/usr/bin/env python3
"""Verify one Timeline clip moves and trims without dragging siblings."""

from qa_support import (
    QaFailure,
    component_center,
    item_by_name,
    media_seconds,
    run_suite_main,
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
    rect = moved_component["rect_points"]
    trim_start = {"x": rect["max_x"] - 2.0, "y": rect["center_y"]}
    trim_end = {"x": trim_start["x"] + 36.0, "y": trim_start["y"]}
    before_trim = after_move["project"]["items"]
    client.drag(trim_start, trim_end, steps=12)

    def trimmed():
        state = client.state()
        current = state["project"]["items"][item_id]
        return state if _interval(current)[1] != _interval(moved_target)[1] else None

    final = client.wait_until("one independent clip trim", trimmed)
    final_target = final["project"]["items"][item_id]
    if _interval(final_target)[0] != _interval(moved_target)[0]:
        raise QaFailure("end trim changed the clip start")
    for candidate_id, before in before_trim.items():
        if candidate_id != item_id and final["project"]["items"][candidate_id] != before:
            raise QaFailure("trimming one clip mutated sibling {}".format(candidate_id))
    if final["editor"]["timeline"]["item_gesture_active"]:
        raise QaFailure("Timeline gesture remained active after release")
    return {
        "suite": "timeline-independent-edit",
        "item_id": item_id,
        "sibling_id": sibling_id,
        "intervals": {
            "before": before_target,
            "moved": _interval(moved_target),
            "trimmed": _interval(final_target),
            "sibling": before_sibling,
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
        "history": final["history"],
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
