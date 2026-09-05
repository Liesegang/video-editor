#!/usr/bin/env python3
"""Exercise the production authoring Curve Editor through loopback QA input."""

from __future__ import annotations

import math

from qa_support import QaFailure, activate_dock_tab, run_suite_main

CANVAS_ID = "curve_editor.canvas"
CURVE_TAB_ID = "dock.tab:curve_editor"
TIMELINE_TAB_ID = "dock.tab:timeline"
FIXTURE = "authoring_e2e"
TEXT_ITEM_NAME = "QA Text"
POSITION_PROPERTY = "position"


def item_by_name(project, name):
    matches = [item for item in project["items"].values() if item["name"] == name]
    if len(matches) != 1:
        raise QaFailure("expected one {!r} item, got {}".format(name, len(matches)))
    return matches[0]


def property_keyframes(project, item_id, property_name=POSITION_PROPERTY):
    prop = project["items"][item_id]["authored_properties"][property_name]
    if prop.get("type") != "keyframe":
        raise QaFailure("{}.{} is not keyframed".format(item_id, property_name))
    return prop["properties"]["keyframes"]


def keyframe_by_id(project, item_id, keyframe_id):
    return next(
        (
            keyframe
            for keyframe in property_keyframes(project, item_id)
            if keyframe["id"] == keyframe_id
        ),
        None,
    )


def open_curve_editor(client):
    component_ids = {
        component["id"] for component in client.component_snapshot()["components"]
    }
    if CURVE_TAB_ID not in component_ids:
        client.key("p", True, command=True, shift=True)
        client.key("p", False, command=True, shift=True)
        client.inject("text", {"text": "Curve Editor"})
        client.key("enter", True)
        client.key("enter", False)
    return activate_dock_tab(
        client, CURVE_TAB_ID, "Curve Editor", "Curve Editor production surface"
    )


def wait_position_key(client, item_id, keyframe_id=None, component="x"):
    def find():
        client.state()
        snapshot = client.component_snapshot()
        for candidate in snapshot["components"]:
            metadata = candidate.get("metadata") or {}
            target = metadata.get("target") or {}
            if not (
                candidate.get("type") == "curve_editor_keyframe"
                and metadata.get("item_id") == item_id
                and target == {"kind": "authored_property", "key": POSITION_PROPERTY}
                and metadata.get("component") == component
                and (keyframe_id is None or metadata.get("keyframe_id") == keyframe_id)
            ):
                continue
            rect = candidate["rect_points"]
            if candidate.get("visible") and rect["width"] > 0.0 and rect["height"] > 0.0:
                return snapshot, candidate
        return None

    suffix = "" if keyframe_id is None else " {}".format(keyframe_id)
    return client.wait_until("position keyframe{}".format(suffix), find)


def assert_close(actual, expected, description, tolerance=0.05):
    if not math.isfinite(float(actual)) or abs(float(actual) - float(expected)) > tolerance:
        raise QaFailure("{}: {} != {}".format(description, actual, expected))


def assertable_close(actual, expected, tolerance=1.0e-5):
    return math.isfinite(float(actual)) and abs(float(actual) - float(expected)) <= tolerance


def assert_canvas_metadata_matches_state(client, description):
    state = client.state()["editor"]["curve_editor"]
    _, canvas = client.component(CANVAS_ID)
    metadata = canvas.get("metadata") or {}
    for axis in ("x", "y"):
        assert_close(
            metadata["pan"][axis], state["pan"][axis], description + " pan." + axis
        )
        assert_close(
            metadata["zoom"][axis],
            state["zoom"][axis],
            description + " zoom." + axis,
            1.0e-5,
        )
    return state, metadata


def wait_keyframe(client, item_id, keyframe_id, predicate, description):
    def changed():
        state = client.state()
        keyframe = keyframe_by_id(state["project"], item_id, keyframe_id)
        return state if keyframe is not None and predicate(keyframe) else None

    return client.wait_until(description, changed)


def run_suite(client):
    health = client.wait_health()
    initial = client.state()
    if initial["project"].get("name") != "Authoring QA":
        raise QaFailure("Curve QA requires the production authoring fixture")
    item = item_by_name(initial["project"], TEXT_ITEM_NAME)
    item_id = item["id"]

    activate_dock_tab(client, TIMELINE_TAB_ID, "Timeline", "Curve target selection")
    client.wait_component_settled("timeline.item:" + item_id)
    client.click_component("timeline.item:" + item_id)
    client.wait_until(
        "typed Curve target selection",
        lambda: state
        if (state := client.state())["editor"]["selection"]["primary"]
        == {"kind": "timeline_item", "id": item_id}
        else None,
    )
    open_curve_editor(client)
    client.wait_component_settled(CANVAS_ID)

    _, ruler = client.wait_component_settled("curve_editor.ruler")
    before_scrub_frame = client.state()["frame"]
    ruler_rect = ruler["rect_points"]
    ruler_point = {
        "x": ruler_rect["min_x"] + ruler_rect["width"] * 0.37,
        "y": ruler_rect["center_y"],
    }
    client.inject(
        "click",
        {
            **ruler_point,
            "button": "primary",
            "coordinate_space": "points",
        },
    )
    scrubbed = client.wait_until(
        "Curve ruler scrub updates shared Timeline frame",
        lambda: state
        if (state := client.state())["frame"] != before_scrub_frame
        else None,
    )

    keys = property_keyframes(client.state()["project"], item_id)
    keyframe = max(keys, key=lambda candidate: float(candidate["time"]))
    keyframe_id = keyframe["id"]
    _, key_component = wait_position_key(client, item_id, keyframe_id)
    key_component_id = key_component["id"]
    before_drag = client.state()
    original_key = keyframe_by_id(before_drag["project"], item_id, keyframe_id)
    client.drag_component_by(key_component_id, 14.0, -9.0, steps=10)
    dragged = wait_keyframe(
        client,
        item_id,
        keyframe_id,
        lambda value: value["time"] != original_key["time"]
        and value["value"]["x"] != original_key["value"]["x"],
        "atomic authoring keyframe drag",
    )
    if dragged["editor"]["curve_editor"]["drag_active"]:
        raise QaFailure("Curve keyframe drag remained active after release")
    if dragged["history"]["revision"] != before_drag["history"]["revision"] + 1:
        raise QaFailure("Curve keyframe drag did not commit exactly one authoring command")

    wait_position_key(client, item_id, keyframe_id)
    client.click_component(key_component_id, button="secondary")
    family_id = "curve_editor.keyframe_menu.easing.family.cubic:" + keyframe_id
    client.wait_component(family_id)
    client.click_component(family_id)
    easing_id = "curve_editor.keyframe_menu.easing.ease_in_out_cubic:" + keyframe_id
    client.wait_component(easing_id)
    before_easing = client.state()
    client.click_component(easing_id)
    eased = wait_keyframe(
        client,
        item_id,
        keyframe_id,
        lambda value: value["easing"] == "EaseInOutCubic",
        "shared Curve easing menu mutation",
    )
    if eased["history"]["revision"] != before_easing["history"]["revision"] + 1:
        raise QaFailure("Curve easing update did not commit exactly one authoring command")

    _, editable_key = wait_position_key(client, item_id, keyframe_id)
    before_dialog = client.state()
    before_dialog_key = keyframe_by_id(before_dialog["project"], item_id, keyframe_id)
    client.click_component(editable_key["id"], button="secondary")
    edit_key_id = "curve_editor.keyframe_menu.edit:{}:x".format(keyframe_id)
    client.wait_component(edit_key_id)
    client.click_component(edit_key_id)
    client.wait_component("curve_editor.keyframe_dialog.value")
    client.wait_until(
        "Curve Edit Keyframe dialog open",
        lambda: state
        if (state := client.state())["editor"]["curve_editor"][
            "keyframe_editor_active"
        ]
        else None,
    )
    client.drag_component_by(
        "curve_editor.keyframe_dialog.value", 24.0, 0.0, steps=8
    )
    client.click_component("curve_editor.keyframe_dialog.apply")
    dialog_applied = wait_keyframe(
        client,
        item_id,
        keyframe_id,
        lambda value: value["value"]["x"] != before_dialog_key["value"]["x"],
        "Edit Keyframe dialog value mutation",
    )
    if dialog_applied["history"]["revision"] != before_dialog["history"]["revision"] + 1:
        raise QaFailure("Edit Keyframe dialog did not commit exactly one authoring command")
    if dialog_applied["editor"]["curve_editor"]["keyframe_editor_active"]:
        raise QaFailure("Edit Keyframe dialog remained active after Apply")

    before_navigation, _ = assert_canvas_metadata_matches_state(
        client, "pre-navigation canonical transform"
    )
    client.scroll_component(CANVAS_ID, 7.0, -5.0)
    panned = client.wait_until(
        "two-axis Curve pan",
        lambda: state
        if (state := client.state())["editor"]["curve_editor"]["pan"]
        != before_navigation["pan"]
        else None,
    )
    assert_close(
        panned["editor"]["curve_editor"]["zoom"]["x"],
        before_navigation["zoom"]["x"],
        "plain scroll changed Curve x zoom",
        1.0e-5,
    )
    assert_close(
        panned["editor"]["curve_editor"]["zoom"]["y"],
        before_navigation["zoom"]["y"],
        "plain scroll changed Curve y zoom",
        1.0e-5,
    )
    client.scroll_component(CANVAS_ID, 0.0, 4.0, modifiers={"command": True})
    zoomed_x = client.wait_until(
        "independent Curve x zoom",
        lambda: state
        if (state := client.state())["editor"]["curve_editor"]["zoom"]["x"]
        != panned["editor"]["curve_editor"]["zoom"]["x"]
        else None,
    )
    assert_close(
        zoomed_x["editor"]["curve_editor"]["zoom"]["y"],
        panned["editor"]["curve_editor"]["zoom"]["y"],
        "x zoom changed Curve y zoom",
        1.0e-5,
    )
    client.scroll_component(
        CANVAS_ID, 0.0, 4.0, modifiers={"command": True, "shift": True}
    )
    zoomed_y = client.wait_until(
        "independent Curve y zoom",
        lambda: state
        if (state := client.state())["editor"]["curve_editor"]["zoom"]["y"]
        != zoomed_x["editor"]["curve_editor"]["zoom"]["y"]
        else None,
    )
    before_extreme_x = float(zoomed_y["editor"]["curve_editor"]["zoom"]["x"])
    extreme_metadata = None
    for _ in range(80):
        client.scroll_component(
            CANVAS_ID,
            0.0,
            -4.0,
            modifiers={"command": True, "shift": True},
        )
        _, candidate = client.component(CANVAS_ID)
        metadata = candidate.get("metadata") or {}
        value_range = metadata.get("visible_value_range") or {}
        if float(value_range.get("span", 0.0)) >= 200_000.0:
            extreme_metadata = metadata
            break
    if extreme_metadata is None:
        raise QaFailure("Curve Y zoom could not expose a 200,000-unit value range")
    if extreme_metadata.get("finite") is not True:
        raise QaFailure("Curve transform became non-finite at extreme Y zoom")
    extreme_state, checked_extreme_metadata = assert_canvas_metadata_matches_state(
        client, "extreme vertical zoom canonical transform"
    )
    assert_close(
        extreme_state["zoom"]["x"],
        before_extreme_x,
        "extreme y zoom changed Curve x zoom",
        1.0e-5,
    )
    extreme_health = client.wait_health()

    client.click_component("curve_editor.fit")

    def fitted():
        state = client.state()["editor"]["curve_editor"]
        return (
            state
            if assertable_close(state["zoom"]["x"], 1.0)
            and assertable_close(state["zoom"]["y"], 1.0)
            else None
        )

    client.wait_until("Fit visible Curve channels", fitted)
    _, post_extreme_key = wait_position_key(client, item_id, keyframe_id)
    post_extreme_before = client.state()
    post_extreme_original = keyframe_by_id(
        post_extreme_before["project"], item_id, keyframe_id
    )
    client.drag_component_by(post_extreme_key["id"], 0.0, -7.0, steps=8)
    post_extreme_drag = wait_keyframe(
        client,
        item_id,
        keyframe_id,
        lambda value: value["value"]["x"] != post_extreme_original["value"]["x"],
        "keyframe drag after extreme vertical zoom",
    )
    if (
        post_extreme_drag["history"]["revision"]
        != post_extreme_before["history"]["revision"] + 1
    ):
        raise QaFailure("post-extreme key drag was not one authoring command")
    client.key("z", True, command=True)
    client.key("z", False, command=True)
    client.wait_until(
        "Undo post-extreme Curve drag",
        lambda: state
        if (state := client.state())["project"] == post_extreme_before["project"]
        else None,
    )

    final_curve, final_metadata = assert_canvas_metadata_matches_state(
        client, "post-extreme Fit canonical transform"
    )
    final = client.state()
    if final["editor"]["error"] is not None:
        raise QaFailure("Curve interaction reported an error: {}".format(final["editor"]["error"]))

    return {
        "ok": True,
        "fixture": FIXTURE,
        "initial_frame": initial["frame"],
        "final_frame": final["frame"],
        "item_id": item_id,
        "keyframe_id": keyframe_id,
        "ruler_scrub": {
            "point": ruler_point,
            "before_frame": before_scrub_frame,
            "after_frame": scrubbed["frame"],
        },
        "keyframe_dialog_history": dialog_applied["history"],
        "curve_editor": final_curve,
        "canvas_metadata": final_metadata,
        "extreme_vertical_zoom": {
            "state": extreme_state,
            "metadata": checked_extreme_metadata,
            "health_frame": extreme_health["frame"],
            "post_zoom_drag": post_extreme_drag["history"],
        },
        "final_keyframe": keyframe_by_id(final["project"], item_id, keyframe_id),
        "history": final["history"],
        "actions": client.evidence,
        "health_frame": health["frame"],
    }


if __name__ == "__main__":
    raise SystemExit(
        run_suite_main(
            "qa-curve-editor-e2e",
            run_suite,
            "target/qa-curve-editor-e2e-evidence.json",
        )
    )
