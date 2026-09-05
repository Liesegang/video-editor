#!/usr/bin/env python3
"""Exercise the production authoring Curve Editor through loopback QA input."""

from __future__ import annotations

import math

from qa_support import (
    QaFailure,
    activate_dock_tab,
    component_point,
    media_seconds,
    rendered_current_revision,
    run_suite_main,
)
from qa_curve_support import exercise_curve_key_live_preview

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
                and target == {
                    "kind": "authored_property",
                    "owner": {"kind": "item", "item_id": item_id},
                    "key": POSITION_PROPERTY,
                }
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


def rational_value(value):
    return float(value["numerator"]) / float(value["denominator"])


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


def undo_exact_project(client, before, after_revision, description):
    client.key("z", True, command=True)
    client.key("z", False, command=True)
    return client.wait_until(
        description,
        lambda: state
        if (state := client.state())["project"] == before["project"]
        and state["history"]["revision"] == after_revision + 1
        else None,
    )


def quantitative_key_drag(client, item_id, keyframe_id, delta_x, delta_y, steps):
    _, key_component = wait_position_key(client, item_id, keyframe_id)
    _, sibling_component = wait_position_key(
        client, item_id, keyframe_id, component="y"
    )
    _, canvas = client.component(CANVAS_ID)
    metadata = canvas.get("metadata") or {}
    rect = canvas["rect_points"]
    zoom = metadata.get("zoom") or {}
    value_range = metadata.get("visible_value_range") or {}
    time_range = metadata.get("visible_time_range") or {}
    before = client.state()
    duration = media_seconds(
        before["project"]["items"][item_id]["interval"]["duration"]
    )
    original = keyframe_by_id(before["project"], item_id, keyframe_id)
    expected_time = max(
        0.0,
        float(original["time"])
        + delta_x / (float(rect["width"]) * float(zoom["x"])) * duration,
    )
    expected_value = float(original["value"]["x"]) - delta_y * float(
        value_range["span"]
    ) / float(rect["height"])

    start = component_point(key_component, 0.5, 0.5)
    sibling_start = component_point(sibling_component, 0.5, 0.5)
    endpoint = {"x": start["x"] + delta_x, "y": start["y"] + delta_y}
    client.inject(
        "press", {**start, "button": "primary", "coordinate_space": "points"}
    )
    for index in range(1, steps + 1):
        fraction = index / steps
        client.inject(
            "move",
            {
                "x": start["x"] + delta_x * fraction,
                "y": start["y"] + delta_y * fraction,
                "button": "primary",
                "coordinate_space": "points",
            },
        )

    held_state = client.wait_until(
        "held quantitative Curve drag with {} steps".format(steps),
        lambda: state
        if (state := client.state())["editor"]["curve_editor"]["drag_active"]
        and state["project"] == before["project"]
        and state["history"] == before["history"]
        else None,
    )
    _, held_component = wait_position_key(client, item_id, keyframe_id)
    _, held_sibling = wait_position_key(
        client, item_id, keyframe_id, component="y"
    )
    held_point = component_point(held_component, 0.5, 0.5)
    held_sibling_point = component_point(held_sibling, 0.5, 0.5)
    held_metadata = held_component.get("metadata") or {}
    held_sibling_metadata = held_sibling.get("metadata") or {}
    time_span = float(time_range["max"]) - float(time_range["min"])

    def plotted_point(time, value):
        return {
            "x": float(rect["min_x"])
            + (float(time) - float(time_range["min"]))
            / time_span
            * float(rect["width"]),
            "y": float(rect["max_y"])
            - (float(value) - float(value_range["min"]))
            / float(value_range["span"])
            * float(rect["height"]),
        }

    original_visual = plotted_point(original["time"], original["value"]["x"])
    held_visual = plotted_point(held_metadata["time"], held_metadata["value"])
    sibling_original_visual = plotted_point(
        original["time"], original["value"]["y"]
    )
    sibling_held_visual = plotted_point(
        held_sibling_metadata["time"], held_sibling_metadata["value"]
    )
    held_frame = held_state["frame"]
    stable_state = client.wait_until(
        "stationary quantitative Curve endpoint with {} steps".format(steps),
        lambda: state
        if (state := client.state())["frame"] >= held_frame + 3
        and state["editor"]["curve_editor"]["drag_active"]
        and state["project"] == before["project"]
        and state["history"] == before["history"]
        else None,
    )
    _, stable_component = wait_position_key(client, item_id, keyframe_id)
    stable_point = component_point(stable_component, 0.5, 0.5)
    stable_metadata = stable_component.get("metadata") or {}
    client.inject(
        "release",
        {**endpoint, "button": "primary", "coordinate_space": "points"},
    )

    def committed():
        state = client.state()
        value = keyframe_by_id(state["project"], item_id, keyframe_id)
        if (
            value is not None
            and state["history"]["revision"] == before["history"]["revision"] + 1
            and not state["editor"]["curve_editor"]["drag_active"]
        ):
            return state, value
        return None

    dragged, actual = client.wait_until(
        "quantitative Curve drag with {} steps".format(steps), committed
    )
    _, released_component = wait_position_key(client, item_id, keyframe_id)
    _, released_sibling = wait_position_key(
        client, item_id, keyframe_id, component="y"
    )
    released_point = component_point(released_component, 0.5, 0.5)
    released_sibling_point = component_point(released_sibling, 0.5, 0.5)
    if dragged["editor"]["curve_editor"]["pan"] != before["editor"]["curve_editor"]["pan"]:
        raise QaFailure("Curve key drag changed pan")
    if dragged["editor"]["curve_editor"]["zoom"] != before["editor"]["curve_editor"]["zoom"]:
        raise QaFailure("Curve key drag changed zoom")
    restored = undo_exact_project(
        client,
        before,
        dragged["history"]["revision"],
        "Undo quantitative Curve drag with {} steps".format(steps),
    )
    return {
        "steps": steps,
        "delta": {"x": delta_x, "y": delta_y},
        "expected": {"time": expected_time, "value": expected_value},
        "actual": {
            "time": float(actual["time"]),
            "value": float(actual["value"]["x"]),
        },
        "history": {
            "before": before["history"]["revision"],
            "committed": dragged["history"]["revision"],
            "restored": restored["history"]["revision"],
        },
        "marker": {
            "start": start,
            "expected_endpoint": endpoint,
            "original_visual": original_visual,
            "held_visual": held_visual,
            "held": held_point,
            "stationary": stable_point,
            "released": released_point,
            "sibling_start": sibling_start,
            "sibling_original_visual": sibling_original_visual,
            "sibling_held_visual": sibling_held_visual,
            "sibling_held": held_sibling_point,
            "sibling_released": released_sibling_point,
            "held_time": held_metadata.get("time"),
            "held_value": held_metadata.get("value"),
            "held_sibling_time": held_sibling_metadata.get("time"),
            "stationary_time": stable_metadata.get("time"),
            "stationary_value": stable_metadata.get("value"),
            "stationary_frame": stable_state["frame"],
        },
        "transform": {
            "rect": rect,
            "pan": metadata.get("pan"),
            "zoom": zoom,
            "visible_value_range": value_range,
            "visible_time_range": time_range,
        },
    }


def assert_quantitative_drag_results(results):
    failures = []
    for result in results:
        if abs(result["actual"]["time"] - result["expected"]["time"]) > 0.002:
            failures.append(
                "steps={}: time actual {:.6f}, expected {:.6f}".format(
                    result["steps"],
                    result["actual"]["time"],
                    result["expected"]["time"],
                )
            )
        if abs(result["actual"]["value"] - result["expected"]["value"]) > 0.05:
            failures.append(
                "steps={}: value actual {:.6f}, expected {:.6f}".format(
                    result["steps"],
                    result["actual"]["value"],
                    result["expected"]["value"],
                )
            )
        marker = result["marker"]
        for axis in ("x", "y"):
            visual_delta = (
                marker["held_visual"][axis] - marker["original_visual"][axis]
            )
            if abs(visual_delta - result["delta"][axis]) > 0.75:
                failures.append(
                    "steps={}: plotted marker {} delta {:.3f}, expected {:.3f}".format(
                        result["steps"], axis, visual_delta, result["delta"][axis]
                    )
                )
            if abs(marker["held"][axis] - marker["held_visual"][axis]) > 0.75:
                failures.append(
                    "steps={}: held component {} center {:.3f}, plotted {:.3f}".format(
                        result["steps"],
                        axis,
                        marker["held"][axis],
                        marker["held_visual"][axis],
                    )
                )
            if abs(marker["stationary"][axis] - marker["held"][axis]) > 0.25:
                failures.append(
                    "steps={}: stationary marker {} drifted".format(
                        result["steps"], axis
                    )
                )
            if abs(marker["released"][axis] - marker["held"][axis]) > 0.75:
                failures.append(
                    "steps={}: marker {} jumped on release".format(
                        result["steps"], axis
                    )
                )
        sibling_visual_delta = (
            marker["sibling_held_visual"]["x"]
            - marker["sibling_original_visual"]["x"]
        )
        if abs(sibling_visual_delta - result["delta"]["x"]) > 0.75:
            failures.append(
                "steps={}: sibling plotted x delta {:.3f}, expected {:.3f}".format(
                    result["steps"], sibling_visual_delta, result["delta"]["x"]
                )
            )
        if abs(marker["sibling_held"]["y"] - marker["sibling_start"]["y"]) > 0.25:
            failures.append(
                "steps={}: sibling channel changed its value".format(result["steps"])
            )
        if abs(marker["sibling_released"]["x"] - marker["sibling_held"]["x"]) > 0.75:
            failures.append(
                "steps={}: sibling channel jumped on release".format(result["steps"])
            )
        if abs(float(marker["held_time"]) - float(marker["held_sibling_time"])) > 0.002:
            failures.append(
                "steps={}: sibling projected a different time".format(result["steps"])
            )
        if abs(float(marker["stationary_time"]) - float(marker["held_time"])) > 0.002:
            failures.append(
                "steps={}: held time drifted without pointer movement".format(result["steps"])
            )
        if abs(float(marker["stationary_value"]) - float(marker["held_value"])) > 0.05:
            failures.append(
                "steps={}: held value drifted without pointer movement".format(result["steps"])
            )
    first, second = results
    if abs(first["actual"]["time"] - second["actual"]["time"]) > 0.002:
        failures.append("same horizontal endpoint depends on step count")
    if abs(first["actual"]["value"] - second["actual"]["value"]) > 0.05:
        failures.append("same vertical endpoint depends on step count")
    if failures:
        raise QaFailure("quantitative Curve key drag failed: " + "; ".join(failures))


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

    before_quantitative_navigation = client.state()["editor"]["curve_editor"]
    client.scroll_component(CANVAS_ID, 3.0, -2.0)
    client.wait_until(
        "Curve pan before quantitative drag",
        lambda: state
        if (state := client.state())["editor"]["curve_editor"]["pan"]
        != before_quantitative_navigation["pan"]
        else None,
    )
    client.scroll_component(CANVAS_ID, 0.0, 2.0, modifiers={"command": True})
    client.scroll_component(
        CANVAS_ID, 0.0, 2.0, modifiers={"command": True, "shift": True}
    )
    client.wait_until(
        "Curve zoom before quantitative drag",
        lambda: state
        if (state := client.state())["editor"]["curve_editor"]["zoom"]["x"] != 1.0
        and state["editor"]["curve_editor"]["zoom"]["y"] != 1.0
        else None,
    )
    assert_canvas_metadata_matches_state(client, "quantitative drag transform")
    quantitative_keyframe = max(
        property_keyframes(client.state()["project"], item_id),
        key=lambda candidate: float(candidate["time"]),
    )
    quantitative_drags = [
        quantitative_key_drag(
            client,
            item_id,
            quantitative_keyframe["id"],
            36.0,
            24.0,
            steps,
        )
        for steps in (2, 16)
    ]
    assert_quantitative_drag_results(quantitative_drags)
    client.click_component("curve_editor.fit")
    client.wait_until(
        "Fit after quantitative Curve drags",
        lambda: state
        if (state := client.state())["editor"]["curve_editor"]["pan"]
        == {"x": 0.0, "y": 0.0}
        and state["editor"]["curve_editor"]["zoom"] == {"x": 1.0, "y": 1.0}
        else None,
    )

    _, ruler = client.wait_component_settled("curve_editor.ruler")
    before_scrub = client.state()
    before_scrub_frame = before_scrub["editor"]["timeline"]["current_frame"]
    _, scrub_canvas = client.component(CANVAS_ID)
    scrub_metadata = scrub_canvas.get("metadata") or {}
    visible_time = scrub_metadata.get("visible_time_range") or {}
    _, ruler_key = wait_position_key(
        client, item_id, quantitative_keyframe["id"]
    )
    ruler_rect = ruler["rect_points"]
    ruler_point = {
        "x": ruler_key["rect_points"]["center_x"],
        "y": ruler_rect["center_y"],
    }
    fraction = (ruler_point["x"] - scrub_canvas["rect_points"]["min_x"]) / float(
        scrub_canvas["rect_points"]["width"]
    )
    local_seconds = float(visible_time["min"]) + fraction * (
        float(visible_time["max"]) - float(visible_time["min"])
    )
    item = before_scrub["project"]["items"][item_id]
    duration = media_seconds(item["interval"]["duration"])
    timeline_seconds = media_seconds(item["interval"]["start"]) + (
        max(0.0, min(duration, local_seconds))
        - media_seconds(item["time_map"]["source_start"])
    ) / rational_value(item["time_map"]["playback_rate"])
    timeline_id = before_scrub["editor"]["navigation"]["active_timeline_id"]
    expected_scrub_frame = math.floor(
        timeline_seconds
        * rational_value(before_scrub["project"]["timelines"][timeline_id]["fps"])
        + 0.5
    )
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
        if (state := client.state())["editor"]["timeline"]["current_frame"]
        != before_scrub_frame
        else None,
    )
    if scrubbed["editor"]["timeline"]["current_frame"] != expected_scrub_frame:
        raise QaFailure(
            "Curve ruler sought frame {}, expected {}".format(
                scrubbed["editor"]["timeline"]["current_frame"],
                expected_scrub_frame,
            )
        )
    live_baseline = client.wait_until(
        "Position key Preview baseline",
        lambda: state
        if (state := rendered_current_revision(client))
        and state["history"]["revision"] == scrubbed["history"]["revision"]
        and state["editor"]["preview"]["rendered_frame"] == expected_scrub_frame
        else None,
        30.0,
    )
    direct_live_preview = exercise_curve_key_live_preview(
        client,
        ruler_key["id"],
        live_baseline,
        "direct Position Curve drag",
        delta_y=18.0,
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
            "after_frame": scrubbed["editor"]["timeline"]["current_frame"],
            "expected_frame": expected_scrub_frame,
            "visible_time_range": visible_time,
        },
        "quantitative_drags": quantitative_drags,
        "direct_live_preview": direct_live_preview,
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
