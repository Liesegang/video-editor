#!/usr/bin/env python3
"""Verify Inspector input policy and source-level Solid/Shape authoring."""

from qa_support import (
    QaFailure,
    activate_dock_tab,
    convert_timeline_item_to_node_clip,
    create_basic_timeline_clip,
    item_by_name,
    rendered_preview_state,
    run_suite_main,
    seek_timeline_seconds,
    settled_preview_state,
)

TIMELINE_TAB_ID = "dock.tab:timeline"

def _component(snapshot, component_id):
    return next(
        (
            component
            for component in snapshot["components"]
            if component.get("id") == component_id
        ),
        None,
    )


def _visible_in_inspector(snapshot, component_id):
    component = _component(snapshot, component_id)
    panel = _component(snapshot, "inspector.scroll_area")
    rect = (component or {}).get("rect_points") or {}
    panel_rect = (panel or {}).get("rect_points") or {}
    return (
        component
        if component is not None
        and component.get("visible") is True
        and float(rect.get("width", 0.0)) > 0.0
        and float(rect.get("height", 0.0)) > 0.0
        and float(panel_rect.get("min_y", 0.0))
        <= float(rect.get("center_y", -1.0))
        <= float(panel_rect.get("max_y", 0.0))
        else None
    )


def _bring_into_inspector(client, component_id):
    for scroll_delta in (0.0, 480.0, 480.0, -320.0, -320.0, -320.0):
        snapshot = client.component_snapshot()
        component = _visible_in_inspector(snapshot, component_id)
        if component is not None:
            return client.wait_component_settled(component_id)[1]
        if scroll_delta:
            client.scroll_component("inspector.scroll_area", 0.0, scroll_delta)
    raise QaFailure("could not bring {} into the Inspector".format(component_id))


def _inspector_scroll_metadata(client):
    component = _component(client.component_snapshot(), "inspector.scroll_area")
    if component is None:
        return None
    metadata = component.get("metadata") or {}
    required = (
        "offset_y",
        "content_height",
        "viewport_height",
        "drag_to_scroll",
        "mouse_wheel",
        "scroll_bar",
    )
    return (component, metadata) if all(key in metadata for key in required) else None


def _verify_inspector_scroll_input_policy(client):
    component, metadata = client.wait_until(
        "Inspector scroll input metadata", lambda: _inspector_scroll_metadata(client)
    )
    if metadata["drag_to_scroll"] is not False:
        raise QaFailure("Inspector content drag scrolling is still enabled")
    if metadata["mouse_wheel"] is not True or metadata["scroll_bar"] is not True:
        raise QaFailure("Inspector disabled its wheel or scrollbar together with content drag")
    if float(metadata["content_height"]) <= float(metadata["viewport_height"]):
        raise QaFailure("Inspector fixture is not scrollable")

    before_wheel = float(metadata["offset_y"])
    expected_wheel_offset = min(
        before_wheel + 180.0,
        float(metadata["content_height"]) - float(metadata["viewport_height"]),
    )
    client.scroll_component("inspector.scroll_area", 0.0, -180.0)

    def wheel_scrolled():
        current = _inspector_scroll_metadata(client)
        if current is None:
            return None
        _, current_metadata = current
        offset = float(current_metadata["offset_y"])
        # Wait for egui's smooth wheel tail, not just its first changed frame;
        # residual wheel motion must not be mistaken for the next drag.
        return current if abs(offset - expected_wheel_offset) < 0.05 else None

    wheel_component, wheel_metadata = client.wait_until(
        "Inspector mouse-wheel scroll", wheel_scrolled
    )
    wheel_offset = float(wheel_metadata["offset_y"])
    rect = wheel_component["rect_points"]
    blank_start = {
        "x": float(rect["min_x"]) + 3.0,
        "y": float(rect["center_y"]),
    }
    blank_end = {"x": blank_start["x"], "y": blank_start["y"] - 56.0}
    client.drag(blank_start, blank_end, steps=12)
    _, after_drag = client.wait_until(
        "Inspector after blank primary drag", lambda: _inspector_scroll_metadata(client)
    )
    after_drag_offset = float(after_drag["offset_y"])
    if abs(after_drag_offset - wheel_offset) > 0.1:
        raise QaFailure("Inspector blank primary drag changed its scroll offset")
    return {
        "wheel_offset_before": before_wheel,
        "wheel_offset_after": wheel_offset,
        "blank_drag_offset_after": after_drag_offset,
        "blank_drag_from": blank_start,
        "blank_drag_to": blank_end,
    }


def _constant_value(item, key):
    authored = (item.get("authored_properties") or {}).get(key)
    if authored is None or authored.get("type") != "constant":
        return None
    return (authored.get("properties") or {}).get("value")


def _appearance_fill(item):
    source = item.get("source") or {}
    if source.get("kind") not in ("shape", "text"):
        return None
    value = source.get("value") or {}
    if source.get("kind") == "shape":
        value = value.get("shape") or value
    fills = [
        operation
        for operation in value.get("appearance_operations") or []
        if (operation.get("operation") or {}).get("component_id") == "fill"
    ]
    if len(fills) != 1:
        raise QaFailure("Shape/Text source expected one authoritative Fill Appearance")
    return fills[0]


def _appearance_constant(operation, key):
    authored = (operation.get("properties") or {}).get(key)
    if authored is None or authored.get("type") != "constant":
        return None
    return (authored.get("properties") or {}).get("value")


def _color_control_and_value(state, item_id):
    item = state["project"]["items"][item_id]
    fill = _appearance_fill(item)
    if fill is None:
        return (
            "inspector.property:item:{}:color".format(item_id),
            _constant_value(item, "color"),
        )
    return (
        "inspector.property:appearance:{}:{}:color".format(item_id, fill["id"]),
        _appearance_constant(fill, "color"),
    )


def _wait_preview(client, revision, frame, different_from=None, expected_hash=None):
    def ready():
        state = rendered_preview_state(client, revision)
        if state is None:
            return None
        preview = state["editor"]["preview"]
        if preview.get("rendered_frame") != frame:
            return None
        pixel_hash = preview.get("pixel_hash")
        if different_from is not None and pixel_hash == different_from:
            return None
        if expected_hash is not None and pixel_hash != expected_hash:
            return None
        return state

    return client.wait_until("source property Preview render", ready, timeout=30.0)


def _select_item(client, item):
    client.click_component("timeline.item:" + item["id"])
    return client.wait_until(
        "Inspector source controls for " + item["name"],
        lambda: snapshot
        if _component(
            (snapshot := client.component_snapshot()),
            "inspector.property:item:{}:color".format(item["id"]),
        )
        else None,
    )


def _edit_numeric(client, item_id, key, delta):
    control_id = "inspector.property:item:{}:{}".format(item_id, key)
    control = _bring_into_inspector(client, control_id)
    metadata = control.get("metadata") or {}
    if not isinstance(metadata.get("value"), (int, float)):
        raise QaFailure("{} is not a typed numeric source control".format(control_id))
    before = client.state()
    before_item = before["project"]["items"][item_id]
    before_value = _constant_value(before_item, key)
    client.drag_component_by(control_id, delta, 0.0, steps=10)

    def edited():
        state = client.state()
        value = _constant_value(state["project"]["items"][item_id], key)
        return (
            state
            if state["history"]["revision"] == before["history"]["revision"] + 1
            and value is not None
            and value != before_value
            else None
        )

    state = client.wait_until("authored {} edit".format(key), edited)
    return state, _constant_value(state["project"]["items"][item_id], key)


def _edit_color(client, item_id, fraction_x, fraction_y):
    before = client.state()
    control_id, before_value = _color_control_and_value(before, item_id)
    _bring_into_inspector(client, control_id)
    client.click_component(control_id)
    _, field = client.wait_component_settled("color_picker.saturation_value")
    metadata = field.get("metadata") or {}
    if not metadata.get("authored_color_space"):
        raise QaFailure("shared color picker did not expose its authored color space")
    rect = field["rect_points"]
    client.inject(
        "click",
        {
            "x": float(rect["min_x"]) + float(rect["width"]) * fraction_x,
            "y": float(rect["min_y"]) + float(rect["height"]) * fraction_y,
            "button": "primary",
            "coordinate_space": "points",
        },
    )
    client.click_component(control_id)

    def edited():
        state = client.state()
        _, value = _color_control_and_value(state, item_id)
        return (
            state
            if state["history"]["revision"] == before["history"]["revision"] + 1
            and value is not None
            and value != before_value
            else None
        )

    state = client.wait_until("authored source color edit", edited)
    return state, _color_control_and_value(state, item_id)[1]


def _source_controls(client, state, item_id):
    controls = {
        key: _bring_into_inspector(
            client, "inspector.property:item:{}:{}".format(item_id, key)
        )
        for key in ("width", "height")
    }
    color_id, _ = _color_control_and_value(state, item_id)
    controls["color"] = _bring_into_inspector(client, color_id)
    return controls


def _history_shortcut(client, redo=False):
    client.key("z", True, command=True, shift=redo)
    client.key("z", False, command=True, shift=redo)


def _convert_and_undo(client, before, item_id, frame, preview_hash):
    project_before = before["project"]
    converted = convert_timeline_item_to_node_clip(
        client, item_id, before["history"]["revision"]
    )
    if converted["project"]["items"][item_id]["source"].get("kind") != "module":
        raise QaFailure("source conversion did not create a bounded Node Clip")
    converted_render = client.wait_until(
        "source-authored Node Clip pixel parity",
        lambda: state
        if (
            (state := settled_preview_state(
                client, converted["history"]["revision"], frame
            ))
            and state["editor"]["preview"].get("pixel_hash") == preview_hash
        )
        else None,
        timeout=30.0,
    )
    _history_shortcut(client)

    def undone():
        state = client.state()
        return (
            state
            if state["history"]["revision"] == converted["history"]["revision"] + 1
            and state["project"] == project_before
            and state["history"].get("can_redo") is True
            else None
        )

    undo_state = client.wait_until("one-step source conversion Undo", undone)
    undo_render = client.wait_until(
        "source Preview after conversion Undo",
        lambda: state
        if (
            (state := settled_preview_state(
                client, undo_state["history"]["revision"], frame
            ))
            and state["editor"]["preview"].get("pixel_hash") == preview_hash
        )
        else None,
        timeout=30.0,
    )
    activate_dock_tab(client, TIMELINE_TAB_ID, "Timeline", "Inspector Source conversion")
    client.wait_component_settled("timeline.canvas")
    return {
        "converted_revision": converted["history"]["revision"],
        "converted_preview": converted_render["editor"]["preview"]["pixel_hash"],
        "undo_revision": undo_state["history"]["revision"],
        "undo_preview": undo_render["editor"]["preview"]["pixel_hash"],
    }


def run_suite(client):
    client.wait_health()
    initial = client.state()
    solid = item_by_name(initial["project"], "QA Overlap")
    solid_source = solid["source"]
    seek_timeline_seconds(client, 3.0)
    # The Text fixture includes multiple Effects and actually overflows the
    # Inspector at the native 1920x1080 QA window size; the Solid alone does not.
    text_item = item_by_name(initial["project"], "QA Text")
    client.click_component("timeline.item:" + text_item["id"])
    scroll_input = _verify_inspector_scroll_input_policy(client)
    _select_item(client, solid)
    solid_before_render = _wait_preview(
        client, initial["history"]["revision"], 90
    )["editor"]["preview"]
    solid_state, solid_color = _edit_color(client, solid["id"], 0.18, 0.18)
    if solid_state["project"]["items"][solid["id"]]["source"] != solid_source:
        raise QaFailure("Solid Inspector override mutated its source identity")
    solid_render = _wait_preview(
        client,
        solid_state["history"]["revision"],
        90,
        different_from=solid_before_render["pixel_hash"],
    )["editor"]["preview"]
    solid_conversion = _convert_and_undo(
        client, solid_state, solid["id"], 90, solid_render["pixel_hash"]
    )

    seek_timeline_seconds(client, 9.0)
    rectangle_created, rectangle = create_basic_timeline_clip(client, "rectangle", "Rectangle")
    _source_controls(client, rectangle_created, rectangle["id"])
    rectangle_baseline = _wait_preview(
        client, rectangle_created["history"]["revision"], 270
    )["editor"]["preview"]
    rectangle_width_state, rectangle_width = _edit_numeric(
        client, rectangle["id"], "width", 36.0
    )
    rectangle_width_render = _wait_preview(
        client,
        rectangle_width_state["history"]["revision"],
        270,
        different_from=rectangle_baseline["pixel_hash"],
    )["editor"]["preview"]
    rectangle_color_state, rectangle_color = _edit_color(
        client, rectangle["id"], 0.82, 0.18
    )
    rectangle_color_render = _wait_preview(
        client,
        rectangle_color_state["history"]["revision"],
        270,
        different_from=rectangle_width_render["pixel_hash"],
    )["editor"]["preview"]
    rectangle_conversion = _convert_and_undo(
        client,
        rectangle_color_state,
        rectangle["id"],
        270,
        rectangle_color_render["pixel_hash"],
    )

    ellipse_created, ellipse = create_basic_timeline_clip(client, "ellipse", "Ellipse")
    _source_controls(client, ellipse_created, ellipse["id"])
    ellipse_baseline = _wait_preview(
        client, ellipse_created["history"]["revision"], 270
    )["editor"]["preview"]
    ellipse_height_state, ellipse_height = _edit_numeric(
        client, ellipse["id"], "height", 30.0
    )
    ellipse_height_render = _wait_preview(
        client,
        ellipse_height_state["history"]["revision"],
        270,
        different_from=ellipse_baseline["pixel_hash"],
    )["editor"]["preview"]
    before_ellipse_color = ellipse_height_state
    ellipse_color_state, ellipse_color = _edit_color(
        client, ellipse["id"], 0.72, 0.36
    )
    ellipse_color_render = _wait_preview(
        client,
        ellipse_color_state["history"]["revision"],
        270,
        different_from=ellipse_height_render["pixel_hash"],
    )["editor"]["preview"]

    _history_shortcut(client)

    def undone():
        state = client.state()
        item = state["project"]["items"][ellipse["id"]]
        return (
            state
            if state["history"]["revision"]
            == ellipse_color_state["history"]["revision"] + 1
            and item == before_ellipse_color["project"]["items"][ellipse["id"]]
            else None
        )

    undo_state = client.wait_until("Undo Ellipse Fill edit", undone)
    undo_render = _wait_preview(
        client,
        undo_state["history"]["revision"],
        270,
        expected_hash=ellipse_height_render["pixel_hash"],
    )["editor"]["preview"]
    _history_shortcut(client, redo=True)

    def redone():
        state = client.state()
        item = state["project"]["items"][ellipse["id"]]
        return (
            state
            if state["history"]["revision"] == undo_state["history"]["revision"] + 1
            and item == ellipse_color_state["project"]["items"][ellipse["id"]]
            else None
        )

    redo_state = client.wait_until("Redo Ellipse Fill edit", redone)
    redo_render = _wait_preview(
        client,
        redo_state["history"]["revision"],
        270,
        expected_hash=ellipse_color_render["pixel_hash"],
    )["editor"]["preview"]

    return {
        "suite": "inspector-source",
        "scroll_input": scroll_input,
        "solid": {
            "item_id": solid["id"],
            "authored_color": solid_color,
            "preview_before": solid_before_render["pixel_hash"],
            "preview_after": solid_render["pixel_hash"],
            "node_clip_conversion": solid_conversion,
        },
        "rectangle": {
            "item_id": rectangle["id"],
            "authored_width": rectangle_width,
            "authored_color": rectangle_color,
            "preview_after_width": rectangle_width_render["pixel_hash"],
            "preview_after_color": rectangle_color_render["pixel_hash"],
            "node_clip_conversion": rectangle_conversion,
        },
        "ellipse": {
            "item_id": ellipse["id"],
            "authored_height": ellipse_height,
            "authored_color": ellipse_color,
            "preview_after_height": ellipse_height_render["pixel_hash"],
            "preview_after_color": ellipse_color_render["pixel_hash"],
        },
        "history": redo_state["history"],
        "undo_preview": undo_render["pixel_hash"],
        "redo_preview": redo_render["pixel_hash"],
        "actions": client.evidence,
    }


if __name__ == "__main__":
    raise SystemExit(
        run_suite_main(
            "qa-inspector-source-e2e",
            run_suite,
            "target/qa-inspector-source-e2e-evidence.json",
        )
    )
