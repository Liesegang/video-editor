#!/usr/bin/env python3
"""Verify the production Inspector property-authoring control in the native UI."""

from qa_support import (
    QaFailure,
    component_center,
    item_by_name,
    run_suite_main,
    seek_timeline_seconds,
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
