#!/usr/bin/env python3
"""Verify the production Inspector property-authoring control in the native UI."""

import os
import pathlib

from qa_support import (
    QaFailure,
    capture_viewport,
    component_center,
    item_by_name,
    rendered_current_revision,
    run_suite_main,
    seek_timeline_seconds,
    settled_preview_state,
)
from qa_property_gesture_support import (
    begin_reserved_keyframe_scrub,
    release_property_scrub,
)


def _component(snapshot, component_id):
    return next(
        (
            component
            for component in snapshot["components"]
            if component.get("id") == component_id
        ),
        None,
    )


def _required_component(client, component_id):
    _, component = client.wait_component_settled(component_id)
    return component


def _semantic(component):
    return ((component.get("metadata") or {}).get("icon") or {}).get("semantic")


def _authored_keys(state, item_id, key):
    prop = state["project"]["items"][item_id]["authored_properties"][key]
    if prop.get("type") != "keyframe":
        raise QaFailure("{}.{} is not keyframed".format(item_id, key))
    return prop["properties"]["keyframes"]


def _key_time(keyframe):
    value = keyframe["time"]
    if isinstance(value, dict):
        return float(value["value"]) / float(value["timescale"])
    return float(value)


def _undo(client):
    client.key("z", True, command=True)
    client.key("z", False, command=True)


def _row_has_no_reservation(client, row_id):
    component = _component(client.component_snapshot(), row_id)
    metadata = (component or {}).get("metadata") or {}
    return component if metadata.get("pending_keyframe_insertion_id") is None else None


def _exercise_inserted_key_identity(client, item_id, baseline):
    control_id = "inspector.property:item:{}:position:x".format(item_id)
    row_id = "inspector.property_row:item:{}:position".format(item_id)
    before_project = baseline["project"]
    before_revision = baseline["history"]["revision"]
    before_frame = baseline["editor"]["timeline"]["current_frame"]
    before_hash = baseline["editor"]["preview"]["pixel_hash"]
    before_keys = _authored_keys(baseline, item_id, "position")
    if len(before_keys) != 2 or any(abs(_key_time(key) - 1.0) < 0.001 for key in before_keys):
        raise QaFailure("Position fixture must have two neighbors and no key at local 1s")

    reservation = begin_reserved_keyframe_scrub(
        client, control_id, row_id, 36.0, "held Position insertion"
    )
    if abs(reservation["time"] - 1.0) > 0.001:
        raise QaFailure("Position reserved key used the wrong Clip-local time")
    held = client.wait_until(
        "held Position projection",
        lambda: state
        if (state := rendered_current_revision(client, before_hash))
        and state["project"] == before_project
        and state["history"]["revision"] == before_revision
        and len(_authored_keys(state, item_id, "position")) == 2
        else None,
        30.0,
    )
    held_capture = capture_viewport(
        client,
        pathlib.Path(os.environ["RUVIE_QA_ARTIFACT_DIR"])
        / "position-keyframe-held.png",
    )
    release_property_scrub(client, reservation)

    def inserted():
        state = client.state()
        keys = _authored_keys(state, item_id, "position")
        inserted_key = next(
            (key for key in keys if abs(_key_time(key) - 1.0) < 0.001), None
        )
        if (
            len(keys) == 3
            and inserted_key is not None
            and state["history"]["revision"] == before_revision + 1
        ):
            return state, inserted_key
        return None

    committed, inserted_key = client.wait_until(
        "one Position insertion command", inserted
    )
    if inserted_key["id"] != reservation["id"]:
        raise QaFailure(
            "held Position reserved KeyframeId {} became {} on release".format(
                reservation["id"], inserted_key["id"]
            )
        )
    client.wait_until(
        "Position reservation cleared after release",
        lambda: _row_has_no_reservation(client, row_id),
    )
    rendered = client.wait_until(
        "committed Position insertion Preview",
        lambda: state
        if (state := settled_preview_state(client, committed["history"]["revision"], before_frame))
        and state["editor"]["preview"]["pixel_hash"]
        == held["editor"]["preview"]["pixel_hash"]
        else None,
        30.0,
    )

    _undo(client)
    undone = client.wait_until(
        "Position insertion Undo",
        lambda: state
        if (state := client.state())["project"] == before_project
        and state["history"]["revision"] == committed["history"]["revision"] + 1
        else None,
    )
    client.wait_until(
        "Position insertion Undo Preview",
        lambda: state
        if (state := settled_preview_state(client, undone["history"]["revision"], before_frame))
        and state["editor"]["preview"]["pixel_hash"] == before_hash
        else None,
        30.0,
    )

    second = begin_reserved_keyframe_scrub(
        client, control_id, row_id, 24.0, "second held Position insertion"
    )
    if second["id"] == reservation["id"]:
        raise QaFailure("a later Position gesture reused the prior reserved KeyframeId")
    client.key("escape", True)
    client.key("escape", False)
    release_property_scrub(client, second)
    cancelled = client.wait_until(
        "second Position insertion cancelled",
        lambda: state
        if (state := client.state())["project"] == before_project
        and state["history"]["revision"] == undone["history"]["revision"]
        and _row_has_no_reservation(client, row_id)
        else None,
    )
    client.wait_until(
        "cancelled Position insertion Preview",
        lambda: state
        if (state := settled_preview_state(client, cancelled["history"]["revision"], before_frame))
        and state["editor"]["preview"]["pixel_hash"] == before_hash
        else None,
        30.0,
    )
    return {
        "reserved_id": reservation["id"],
        "committed_id": inserted_key["id"],
        "second_reserved_id": second["id"],
        "local_time": reservation["time"],
        "held_hash": held["editor"]["preview"]["pixel_hash"],
        "held_capture": held_capture,
        "committed_hash": rendered["editor"]["preview"]["pixel_hash"],
        "restored_hash": before_hash,
    }


def _assert_semantic(client, component_id, expected):
    def matches():
        component = _component(client.component_snapshot(), component_id)
        return component if component is not None and _semantic(component) == expected else None

    return client.wait_until("{} showing {}".format(component_id, expected), matches)


def _assert_row_order(client, item_id, key):
    control_id = "item:{}:{}".format(item_id, key)
    label = _required_component(
        client, "inspector.property_label:" + control_id
    )
    mode = _required_component(
        client, "inspector.property_mode:" + control_id
    )
    value = _required_component(client, "inspector.property:" + control_id)
    label_rect = label["rect_points"]
    mode_rect = mode["rect_points"]
    value_rect = value["rect_points"]
    if float(label_rect["max_x"]) > float(mode_rect["min_x"]):
        raise QaFailure("property mode is not after the label")
    if float(mode_rect["max_x"]) > float(value_rect["min_x"]):
        raise QaFailure("typed value is not after the property mode")
    metadata = label.get("metadata") or {}
    if metadata.get("horizontal_alignment") != "left":
        raise QaFailure("Inspector label is not explicitly left-aligned")
    if metadata.get("text_anchor") != "left_center":
        raise QaFailure("Inspector label does not paint from its left edge")
    if abs(float(metadata.get("text_anchor_x", -1.0)) - float(label_rect["min_x"])) > 0.01:
        raise QaFailure("Inspector label anchor does not match its visible left edge")
    return {"label": label_rect, "mode": mode_rect, "value": value_rect}


def run_suite(client):
    client.wait_health()
    initial = client.state()
    item = item_by_name(initial["project"], "QA Text")
    item_id = item["id"]
    client.click_component("timeline.item:" + item_id)

    position_mode = "inspector.property_mode:item:{}:position".format(item_id)
    opacity_mode = "inspector.property_mode:item:{}:opacity".format(item_id)
    anchor_mode = "inspector.property_mode:item:{}:anchor".format(item_id)

    # QA Text starts at 1 second and has position keys at local 0 and 2.
    seek_timeline_seconds(client, 1.0)
    key_here = _assert_semantic(
        client, position_mode, "diamond_filled_keyframe"
    )
    if (key_here.get("metadata") or {}).get("key_at_current_time") is not True:
        raise QaFailure("filled keyframe icon does not report a key at the playhead")

    seek_timeline_seconds(client, 2.0)
    key_away = _assert_semantic(
        client, position_mode, "diamond_outline_keyframe"
    )
    if (key_away.get("metadata") or {}).get("key_at_current_time") is not False:
        raise QaFailure("outline keyframe icon incorrectly reports a key at the playhead")

    baseline = client.wait_until(
        "Position insertion baseline",
        lambda: rendered_current_revision(client),
        30.0,
    )
    inserted_key_identity = _exercise_inserted_key_identity(client, item_id, baseline)

    # Both an authored constant and an implicit default must remain timers.
    opacity = _assert_semantic(client, opacity_mode, "timer_constant")
    anchor = _assert_semantic(client, anchor_mode, "timer_constant")
    if (anchor.get("metadata") or {}).get("keyframe_count") != 0:
        raise QaFailure("implicit transform default manufactured a keyframe")

    row_geometry = _assert_row_order(client, item_id, "opacity")

    before_expression = client.state()
    client.click_component(opacity_mode)
    expression_option = opacity_mode + ".option:expression"
    client.click_component(expression_option)
    expression = _assert_semantic(client, opacity_mode, "function_expression")
    expression_editor = _required_component(
        client, "inspector.expression_source:item:{}:opacity".format(item_id)
    )
    after_expression = client.state()
    if after_expression["history"]["revision"] != before_expression["history"]["revision"] + 1:
        raise QaFailure("changing authoring mode was not one undoable transaction")
    authored = after_expression["project"]["items"][item_id]["authored_properties"][
        "opacity"
    ]
    if authored.get("type") != "expression":
        raise QaFailure("function icon is not backed by an Expression property")

    return {
        "suite": "inspector-property-mode",
        "item_id": item_id,
        "states": {
            "key_here": key_here.get("metadata"),
            "key_away": key_away.get("metadata"),
            "constant": opacity.get("metadata"),
            "implicit_default": anchor.get("metadata"),
            "expression": expression.get("metadata"),
        },
        "row_geometry": row_geometry,
        "inserted_key_identity": inserted_key_identity,
        "expression_editor": expression_editor,
        "history": after_expression["history"],
        "actions": client.evidence,
    }


if __name__ == "__main__":
    raise SystemExit(
        run_suite_main(
            "qa-inspector-property-mode-e2e",
            run_suite,
            "target/qa-inspector-property-mode-e2e-evidence.json",
        )
    )
