#!/usr/bin/env python3
"""Exercise Timeline property lanes and shared Curve automation through HTTP QA."""

from __future__ import annotations

from qa_support import QaFailure, activate_dock_tab, bring_timeline_component, run_suite_main


FIXTURE = "authoring_e2e"
TIMELINE_TAB_ID = "dock.tab:timeline"
CURVE_TAB_ID = "dock.tab:curve_editor"
INSPECTOR_TAB_ID = "dock.tab:inspector"
TEXT_NAME = "QA Text"
NODE_CLIP_NAME = "QA Node Clip"
IMAGE_NAME = "QA Image"


def item_by_name(project, name):
    matches = [item for item in project["items"].values() if item["name"] == name]
    if len(matches) != 1:
        raise QaFailure("expected one {!r} item, got {}".format(name, len(matches)))
    return matches[0]


def authored_keys(project, item_id, key="position"):
    prop = project["items"][item_id]["authored_properties"][key]
    if prop.get("type") != "keyframe":
        raise QaFailure("{}.{} is not keyframed".format(item_id, key))
    return prop["properties"]["keyframes"]


def key_by_id(project, item_id, keyframe_id):
    return next(
        (key for key in authored_keys(project, item_id) if key["id"] == keyframe_id),
        None,
    )


def builtin_effect(project, item_id, component_id):
    matches = []
    for attachment in project["attachments"].values():
        owner = attachment.get("owner") or {}
        processor = attachment.get("processor") or {}
        value = processor.get("value") or {}
        operation = value.get("operation") or {}
        if (
            owner == {"kind": "item", "item_id": item_id}
            and processor.get("kind") == "builtin_effect"
            and operation.get("component_id") == component_id
        ):
            matches.append((attachment, value))
    if len(matches) != 1:
        raise QaFailure(
            "expected one {!r} Effect on {}, got {}".format(
                component_id, item_id, len(matches)
            )
        )
    return matches[0]


def effect_keys(project, item_id, component_id, key):
    _, effect = builtin_effect(project, item_id, component_id)
    parameter = effect["parameters"][key]
    automation = parameter.get("automation")
    return [] if automation is None else automation["keyframes"]


def media_seconds(value):
    return float(value["value"]) / float(value["timescale"])


def find_component(snapshot, component_type, predicate):
    return next(
        (
            component
            for component in snapshot["components"]
            if component.get("type") == component_type
            and component.get("visible")
            and predicate(component.get("metadata") or {})
        ),
        None,
    )


def component_by_id(snapshot, component_id):
    return next(
        (
            component
            for component in snapshot["components"]
            if component.get("id") == component_id and component.get("visible")
        ),
        None,
    )


def wait_clip_display(client, item_id, mode, visual=None):
    def displayed():
        client.state()
        snapshot = client.component_snapshot()
        clip = component_by_id(snapshot, "timeline.item:" + item_id)
        content = component_by_id(snapshot, "timeline.content:" + item_id)
        keyframes = component_by_id(snapshot, "timeline.keyframes:" + item_id)
        if clip is None or (clip.get("metadata") or {}).get("display_mode") != mode:
            return None
        if mode == "content":
            if content is None or keyframes is not None:
                return None
            metadata = content.get("metadata") or {}
            if visual is not None and metadata.get("visual") != visual:
                return None
            if visual == "image_thumbnail" and metadata.get("decoded_frames", 0) < 1:
                return None
            return clip, content
        if keyframes is None or content is not None:
            return None
        return clip, keyframes

    return client.wait_until("{} Clip display for {}".format(mode, item_id), displayed)


def pulse_timeline(client):
    _, canvas = client.wait_component("timeline.canvas")
    client.inject(
        "move",
        {
            "x": canvas["rect_points"]["center_x"],
            "y": canvas["rect_points"]["center_y"],
            "coordinate_space": "points",
        },
    )


def wait_lane(client, item_id, target):
    def current_lane():
        # State queries are serviced by the native UI frame and therefore also
        # advance the Project snapshot consumed by component registration.
        client.state()
        return find_component(
            client.component_snapshot(),
            "timeline_property_label",
            lambda metadata: metadata.get("item_id") == item_id
            and metadata.get("target") == target,
        )

    return client.wait_until(
        "Timeline property lane {}".format(target),
        current_lane,
    )


def wait_timeline_key(client, item_id, keyframe_id):
    def current_key():
        client.state()
        return find_component(
            client.component_snapshot(),
            "timeline_property_keyframe",
            lambda metadata: metadata.get("item_id") == item_id
            and metadata.get("keyframe_id") == keyframe_id,
        )

    return client.wait_until(
        "Timeline keyframe {}".format(keyframe_id),
        current_key,
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
    activate_dock_tab(client, CURVE_TAB_ID, "Curve Editor", "shared automation Curve panel")


def visible_inspector_control(client, component_id):
    activate_dock_tab(client, INSPECTOR_TAB_ID, "Inspector", "Effect automation Inspector")
    _, scroll_area = client.wait_component("inspector.scroll_area")
    panel = scroll_area["rect_points"]

    def inside_panel():
        component = next(
            (
                candidate
                for candidate in client.component_snapshot()["components"]
                if candidate.get("id") == component_id and candidate.get("visible")
            ),
            None,
        )
        if component is None:
            return None
        rect = component["rect_points"]
        return (
            component
            if panel["min_y"] <= rect["center_y"] <= panel["max_y"]
            else None
        )

    component = inside_panel()
    for _ in range(8):
        if component is not None:
            return client.wait_component_settled(component_id)[1]
        client.scroll_component("inspector.scroll_area", 0.0, -360.0)
        component = inside_panel()
    raise QaFailure("could not bring {} into the Inspector viewport".format(component_id))


def run_suite(client):
    health = client.wait_health()
    initial = client.state()
    if initial["project"].get("name") != "Authoring QA":
        raise QaFailure("Dope Sheet QA requires the production authoring fixture")
    text = item_by_name(initial["project"], TEXT_NAME)
    node_clip = item_by_name(initial["project"], NODE_CLIP_NAME)
    image_item = item_by_name(initial["project"], IMAGE_NAME)
    text_id = text["id"]
    node_id = node_clip["id"]
    image_id = image_item["id"]
    track_id = image_item["track_id"]
    opacity = text["authored_properties"].get("opacity") or {}
    if opacity.get("type") != "constant":
        raise QaFailure("fixture must include one constant-only authored property")

    activate_dock_tab(client, TIMELINE_TAB_ID, "Timeline", "Timeline Dope Sheet")
    bring_timeline_component(client, "timeline.item_display:" + image_id, -140.0)
    default_image_clip, default_image_content = wait_clip_display(
        client, image_id, "content", "image_thumbnail"
    )
    client.click_component("timeline.item_display:" + image_id)
    image_keyframe_clip, image_keyframe_summary = wait_clip_display(
        client, image_id, "keyframes"
    )
    if (image_keyframe_summary.get("metadata") or {}).get("lane_count") != 0:
        raise QaFailure("Image keyframe display invented a non-keyframed property lane")
    client.click_component("timeline.item_display:" + image_id)
    image_content_clip, image_content = wait_clip_display(
        client, image_id, "content", "image_thumbnail"
    )

    bring_timeline_component(client, "timeline.track_display:" + track_id, 800.0)
    client.click_component("timeline.track_display:" + track_id)
    client.wait_until(
        "Track Keyframes display mode",
        lambda: state
        if (state := client.state())["editor"]["timeline"]["track_display_modes"].get(
            track_id
        )
        == "keyframes"
        else None,
    )
    bring_timeline_component(client, "timeline.item:" + image_id, -180.0)
    track_keyframe_clip, track_keyframe_summary = wait_clip_display(
        client, image_id, "keyframes"
    )
    bring_timeline_component(client, "timeline.track_display:" + track_id, 800.0)
    client.click_component("timeline.track_display:" + track_id)
    client.wait_until(
        "Track Content display mode",
        lambda: state
        if track_id
        not in (state := client.state())["editor"]["timeline"]["track_display_modes"]
        else None,
    )
    bring_timeline_component(client, "timeline.item:" + image_id, -180.0)
    track_content_clip, track_content = wait_clip_display(
        client, image_id, "content", "image_thumbnail"
    )
    bring_timeline_component(client, "timeline.item_expand:" + text_id, 800.0)
    client.click_component("timeline.item_expand:" + text_id)
    client.wait_until(
        "expanded Text Clip state",
        lambda: state
        if text_id in (state := client.state())["editor"]["timeline"]["expanded_items"]
        else None,
    )
    position_target = {"kind": "authored_property", "key": "position"}
    try:
        position_lane = client.wait_until(
            "visible Text property lane",
            lambda: value
            if (
                value := find_component(
                    client.component_snapshot(),
                    "timeline_property_label",
                    lambda metadata: metadata.get("item_id") == text_id
                    and metadata.get("target") == position_target,
                )
            )
            else None,
            timeout=2.0,
        )
    except QaFailure:
        client.scroll_component("timeline.canvas", 0.0, -100.0)
        position_lane = wait_lane(client, text_id, position_target)
    if (position_lane.get("metadata") or {}).get("keyframe_count") != 2:
        raise QaFailure("Position lane did not expose both authoritative keyframes")
    opacity_target = {"kind": "authored_property", "key": "opacity"}
    if find_component(
        client.component_snapshot(),
        "timeline_property_label",
        lambda metadata: metadata.get("item_id") == text_id
        and metadata.get("target") == opacity_target,
    ):
        raise QaFailure("constant-only Opacity leaked into Timeline property rows")
    _, text_row = client.wait_component("timeline.row:" + text_id)
    if (position_lane.get("metadata") or {}).get("display_row_index") != (
        (text_row.get("metadata") or {}).get("display_row_index", -2) + 1
    ):
        raise QaFailure("Position property row is not directly below its Clip")

    if "timeline.item_expand:" + node_id not in {
        component["id"] for component in client.component_snapshot()["components"]
    }:
        client.scroll_component("timeline.canvas", 0.0, 100.0)
    client.click_component("timeline.item_expand:" + node_id)
    module_source = client.state()["project"]["items"][node_id]["source"]["value"]
    parameter_ids = list(module_source["automation_tracks"])
    if len(parameter_ids) != 1:
        raise QaFailure("fixture Node Clip should expose one published parameter")
    parameter_target = {"kind": "module_parameter", "id": parameter_ids[0]}
    parameter_lane = wait_lane(client, node_id, parameter_target)
    visible_lanes = [position_lane, parameter_lane]

    keys = authored_keys(client.state()["project"], text_id)
    key = max(keys, key=lambda candidate: float(candidate["time"]))
    keyframe_id = key["id"]
    timeline_key = wait_timeline_key(client, text_id, keyframe_id)
    client.click_component(timeline_key["id"])
    sought = client.wait_until(
        "key click selecting the Clip and seeking its host time",
        lambda: state
        if (state := client.state())["editor"]["selection"]["primary"]
        == {"kind": "timeline_item", "id": text_id}
        and state["editor"]["timeline"]["current_frame"] == 90
        else None,
    )

    blur_attachment, _ = builtin_effect(sought["project"], text_id, "blur")
    blur_attachment_id = blur_attachment["id"]
    effect_target = {
        "kind": "attachment_parameter",
        "attachment_id": blur_attachment_id,
        "key": "sigma_x",
    }
    before_effect = client.state()
    effect_mode_id = "inspector.property_mode:attachment:{}:sigma_x".format(
        blur_attachment_id
    )
    effect_mode_control = visible_inspector_control(client, effect_mode_id)
    if (effect_mode_control.get("metadata") or {}).get("mode") != "constant":
        raise QaFailure("fixture Blur sigma_x must begin in Constant mode")
    client.click_component(effect_mode_control["id"])
    client.click_component(effect_mode_id + ".option:keyframe")

    def effect_key_created():
        state = client.state()
        keys = effect_keys(state["project"], text_id, "blur", "sigma_x")
        return (
            (state, keys[0])
            if len(keys) == 1
            and state["history"]["revision"]
            == before_effect["history"]["revision"] + 1
            else None
        )

    effect_state, effect_key = client.wait_until(
        "one Inspector Effect automation command", effect_key_created
    )
    effect_keyframe_id = effect_key["id"]
    if abs(media_seconds(effect_key["time"]) - 2.0) > 0.001:
        raise QaFailure("Effect keyframe did not use the selected Clip's local time")
    activate_dock_tab(client, TIMELINE_TAB_ID, "Timeline", "Timeline Effect lane")
    effect_lane = wait_lane(client, text_id, effect_target)
    visible_lanes.append(effect_lane)
    if effect_keyframe_id not in (effect_lane.get("metadata") or {}).get(
        "keyframe_ids", []
    ):
        raise QaFailure("Timeline Effect lane changed the Inspector KeyframeId")
    pulse_timeline(client)
    bring_timeline_component(
        client,
        "timeline.keyframe:{}:{}".format(text_id, effect_keyframe_id),
        -140.0,
    )
    effect_timeline_key = wait_timeline_key(client, text_id, effect_keyframe_id)
    if (effect_timeline_key.get("metadata") or {}).get("target") != effect_target:
        raise QaFailure("Timeline Effect keyframe changed its shared AutomationTarget")

    before_cancel = client.state()
    _, cancel_key = client.wait_component(timeline_key["id"])
    center = {
        "x": cancel_key["rect_points"]["center_x"],
        "y": cancel_key["rect_points"]["center_y"],
    }
    client.inject(
        "press", {**center, "button": "primary", "coordinate_space": "points"}
    )
    client.inject(
        "move",
        {
            "x": center["x"] + 20.0,
            "y": center["y"],
            "button": "primary",
            "coordinate_space": "points",
        },
    )
    client.wait_until(
        "projected key drag before Escape",
        lambda: state
        if (state := client.state())["editor"]["timeline"]["keyframe_gesture_active"]
        else None,
    )
    client.key("escape", True)
    client.key("escape", False)
    cancelled = client.wait_until(
        "Escape cancelling Timeline key drag",
        lambda: state
        if not (state := client.state())["editor"]["timeline"]["keyframe_gesture_active"]
        else None,
    )
    client.inject(
        "release",
        {
            "x": center["x"] + 20.0,
            "y": center["y"],
            "button": "primary",
            "coordinate_space": "points",
        },
    )
    if cancelled["project"] != before_cancel["project"] or cancelled["history"] != before_cancel["history"]:
        raise QaFailure("Escape changed Project/history during Timeline key drag")

    timeline_key = wait_timeline_key(client, text_id, keyframe_id)
    before_drag = client.state()
    original_key = key_by_id(before_drag["project"], text_id, keyframe_id)
    client.drag_component_by(timeline_key["id"], 40.0, 0.0, steps=12)

    def retimed():
        state = client.state()
        current = key_by_id(state["project"], text_id, keyframe_id)
        if (
            current is not None
            and current["time"] != original_key["time"]
            and state["history"]["revision"] == before_drag["history"]["revision"] + 1
            and not state["editor"]["timeline"]["keyframe_gesture_active"]
        ):
            return state, current
        return None

    retimed_state, retimed_key = client.wait_until(
        "one frame-snapped Timeline keyframe command", retimed
    )
    expected_time = 2.5
    if abs(float(retimed_key["time"]) - expected_time) > 0.001:
        raise QaFailure(
            "Timeline keyframe was not frame-snapped to {:.3f}s: {}".format(
                expected_time, retimed_key["time"]
            )
        )

    open_curve_editor(client)

    def shared_curve_key():
        component = find_component(
            client.component_snapshot(),
            "curve_editor_keyframe",
            lambda metadata: metadata.get("item_id") == text_id
            and metadata.get("keyframe_id") == keyframe_id
            and metadata.get("target") == position_target,
        )
        if component is None:
            return None
        metadata = component.get("metadata") or {}
        return component if abs(float(metadata.get("time", -1.0)) - expected_time) < 0.001 else None

    curve_key = client.wait_until("same key ID/time in Curve Editor", shared_curve_key)

    def shared_effect_curve_key():
        component = find_component(
            client.component_snapshot(),
            "curve_editor_keyframe",
            lambda metadata: metadata.get("item_id") == text_id
            and metadata.get("keyframe_id") == effect_keyframe_id
            and metadata.get("target") == effect_target,
        )
        if component is None:
            return None
        metadata = component.get("metadata") or {}
        return component if abs(float(metadata.get("time", -1.0)) - 2.0) < 0.001 else None

    effect_curve_key = client.wait_until(
        "same Effect key ID/time in Curve Editor", shared_effect_curve_key
    )
    client.key("z", True, command=True)
    client.key("z", False, command=True)
    undone = client.wait_until(
        "one Undo restoring Timeline keyframe time",
        lambda: state
        if (state := client.state())["project"] == before_drag["project"]
        else None,
    )
    if undone["history"]["can_redo"] is not True:
        raise QaFailure("Timeline key drag was not one undoable command")
    if undone["editor"].get("error") is not None:
        raise QaFailure("Dope Sheet interaction reported an error")
    client.key("z", True, command=True)
    client.key("z", False, command=True)
    effect_undone = client.wait_until(
        "one Undo removing the Inspector Effect key",
        lambda: state
        if (state := client.state())["project"] == before_effect["project"]
        else None,
    )

    return {
        "ok": True,
        "fixture": FIXTURE,
        "health_frame": health["frame"],
        "item_id": text_id,
        "module_item_id": node_id,
        "image_item_id": image_id,
        "image_display_modes": {
            "default_content_clip": default_image_clip,
            "default_content": default_image_content,
            "item_keyframes_clip": image_keyframe_clip,
            "item_keyframes": image_keyframe_summary,
            "item_content_clip": image_content_clip,
            "item_content": image_content,
            "track_keyframes_clip": track_keyframe_clip,
            "track_keyframes": track_keyframe_summary,
            "track_content_clip": track_content_clip,
            "track_content": track_content,
        },
        "lanes": visible_lanes,
        "seek_frame": sought["editor"]["timeline"]["current_frame"],
        "keyframe_id": keyframe_id,
        "retimed_keyframe": retimed_key,
        "retimed_history": retimed_state["history"],
        "curve_key": curve_key,
        "effect_keyframe_id": effect_keyframe_id,
        "effect_timeline_key": effect_timeline_key,
        "effect_curve_key": effect_curve_key,
        "effect_history": effect_state["history"],
        "undo_history": undone["history"],
        "effect_undo_history": effect_undone["history"],
        "actions": client.evidence,
    }


if __name__ == "__main__":
    raise SystemExit(
        run_suite_main(
            "qa-timeline-dopesheet-e2e",
            run_suite,
            "target/qa-timeline-dopesheet-e2e-evidence.json",
        )
    )
