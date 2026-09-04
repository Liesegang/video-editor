#!/usr/bin/env python3
"""Verify the Inspector exposes every Effect and its shared value controls."""

from qa_support import QaFailure, component_center, item_by_name, run_suite_main


def _effect_components(client):
    snapshot = client.component_snapshot()
    return snapshot, [
        component
        for component in snapshot["components"]
        if component.get("type") == "effect_stack_entry"
    ]


def _attachment(project, attachment_id):
    try:
        return project["attachments"][attachment_id]
    except KeyError as error:
        raise QaFailure("missing fixture Effect {}".format(attachment_id)) from error


def _component(snapshot, component_id):
    return next(
        (value for value in snapshot["components"] if value.get("id") == component_id),
        None,
    )


def _visible(component):
    rect = (component or {}).get("rect_points") or {}
    return (
        (component or {}).get("visible") is True
        and float(rect.get("width", 0.0)) > 0.0
        and float(rect.get("height", 0.0)) > 0.0
    )


def _bring_into_inspector(client, component_id, scroll_delta):
    for _ in range(12):
        snapshot = client.component_snapshot()
        component = _component(snapshot, component_id)
        panel = _component(snapshot, "inspector.scroll_area")
        rect = (component or {}).get("rect_points") or {}
        panel_rect = (panel or {}).get("rect_points") or {}
        inside_panel = (
            float(panel_rect.get("min_y", 0.0))
            <= float(rect.get("center_y", -1.0))
            <= float(panel_rect.get("max_y", 0.0))
        )
        if _visible(component) and component.get("enabled") is True and inside_panel:
            return client.wait_component_settled(component_id)[1]
        client.scroll_component("inspector.scroll_area", 0.0, scroll_delta)
    raise QaFailure("could not bring {} into the Inspector".format(component_id))


def _drag_effect_to_slot(
    client,
    attachment_id,
    slot_id,
    source_scroll_delta,
    target_scroll_delta,
    expected_stage,
    index,
):
    handle_id = "inspector.effect_drag_handle:" + attachment_id
    handle = _bring_into_inspector(client, handle_id, source_scroll_delta)
    start = component_center(handle)
    pointer = {**start, "button": "primary", "coordinate_space": "points"}
    client.inject("press", pointer)
    client.inject(
        "move",
        {
            "x": start["x"] + 8.0,
            "y": start["y"],
            "button": "primary",
            "coordinate_space": "points",
        },
    )

    def drag_started():
        component = _component(
            client.component_snapshot(), "inspector.effects.drag_state"
        )
        metadata = (component or {}).get("metadata") or {}
        return component if metadata.get("dragging") is True else None

    client.wait_until("Effect drag payload", drag_started)
    slot = _bring_into_inspector(client, slot_id, target_scroll_delta)
    target = component_center(slot)
    for _ in range(2):
        client.inject(
            "move",
            {
                **target,
                "button": "primary",
                "coordinate_space": "points",
            },
        )
        slot = _bring_into_inspector(client, slot_id, target_scroll_delta)
        target = component_center(slot)

    drag_state = _component(
        client.component_snapshot(), "inspector.effects.drag_state"
    )
    drag_metadata = (drag_state or {}).get("metadata") or {}
    if drag_metadata.get("insertion_stage") != expected_stage:
        raise QaFailure(
            "drag preview stage mismatch: {}".format(drag_metadata)
        )
    if drag_metadata.get("insertion_index") != index:
        raise QaFailure(
            "drag preview index mismatch: {}".format(drag_metadata)
        )
    slot_metadata = slot.get("metadata") or {}
    if slot_metadata.get("hovered") is not True:
        raise QaFailure("insertion slot did not expose its hovered preview state")
    if float(slot["rect_points"]["height"]) < 24.0:
        raise QaFailure("insertion preview did not expand into a visible Effect placeholder")

    client.inject(
        "release",
        {
            **target,
            "button": "primary",
            "coordinate_space": "points",
        },
    )
    return drag_metadata


def run_suite(client):
    client.wait_health()
    initial = client.state()
    text_item = item_by_name(initial["project"], "QA Text")
    client.click_component("timeline.item:" + text_item["id"])

    def effects_ready():
        snapshot, effects = _effect_components(client)
        return (snapshot, effects) if len(effects) == 2 else None

    snapshot, effects = client.wait_until("both fixture Effects in Inspector", effects_ready)
    metadata = [effect.get("metadata") or {} for effect in effects]
    attachment_ids = [value.get("attachment_id") for value in metadata]
    if None in attachment_ids or len(set(attachment_ids)) != 2:
        raise QaFailure("Effect Stack did not expose two stable Effect identities")
    if sorted(value.get("order") for value in metadata) != [0, 1]:
        raise QaFailure("Effect Stack did not preserve both Effect orders")
    if any(value.get("kind") != "builtin" for value in metadata):
        raise QaFailure("fixture Effects unexpectedly changed processor kind")
    tile = next(
        (value for value in metadata if value.get("component_id") == "tile"), None
    )
    if tile is None:
        raise QaFailure("fixture Tile Effect is missing from the Effect Stack")

    visible_actions = {
        component.get("type")
        for component in snapshot["components"]
        if (component.get("metadata") or {}).get("attachment_id") in attachment_ids
    }
    for required in ("effect_drag_handle", "effect_remove", "effect_enabled"):
        if required not in visible_actions:
            raise QaFailure("Effect card is missing visible {} controls".format(required))
    expected_stage_adds = {
        "inspector.effects.add:item_pre_transform",
        "inspector.effects.add:item_post_transform",
        "inspector.effects.add:audio_pre_fader",
        "inspector.effects.add:audio_post_fader",
    }
    actual_stage_adds = {
        component.get("id")
        for component in snapshot["components"]
        if component.get("type") == "effect_stack_add"
    }
    if not expected_stage_adds.issubset(actual_stage_adds):
        raise QaFailure("each Effect stage must own its Add affordance")

    controls = [
        component
        for component in snapshot["components"]
        if component["id"].startswith("inspector.property:attachment:")
        and component.get("type") == "inspector_property_control"
    ]
    if not controls:
        raise QaFailure("Effect Stack rendered no shared property controls")
    if not any((control.get("metadata") or {}).get("has_definition") for control in controls):
        raise QaFailure("Effect parameters bypassed their typed property definitions")
    tile_prefix = "inspector.property:attachment:{}:".format(tile["attachment_id"])
    tile_controls = {
        control["id"][len(tile_prefix) :]
        for control in controls
        if control["id"].startswith(tile_prefix)
    }
    if tile_controls != {"offset_x", "offset_y", "width", "height"}:
        raise QaFailure(
            "Tile must expose centred Offset X/Y plus Width/Height; got {}".format(
                sorted(tile_controls)
            )
        )

    labels = {
        component["id"]: component.get("metadata") or {}
        for component in snapshot["components"]
        if component.get("type") == "inspector_property_label"
    }
    for control in controls:
        label_id = control["id"].replace(
            "inspector.property:", "inspector.property_label:", 1
        )
        if labels.get(label_id, {}).get("horizontal_alignment") != "left":
            raise QaFailure("Inspector property label is not explicitly left-aligned")

    blur = next(
        value for value in metadata if value.get("component_id") == "blur"
    )
    before_reorder = client.state()
    content_before = {
        attachment_id: {
            key: value
            for key, value in _attachment(
                before_reorder["project"], attachment_id
            ).items()
            if key not in ("stage", "order")
        }
        for attachment_id in attachment_ids
    }
    reorder_preview = _drag_effect_to_slot(
        client,
        tile["attachment_id"],
        "inspector.effect_drop:item_post_transform:0",
        -320.0,
        320.0,
        "ItemPostTransform",
        0,
    )

    def reordered():
        state = client.state()
        tile_attachment = _attachment(state["project"], tile["attachment_id"])
        blur_attachment = _attachment(state["project"], blur["attachment_id"])
        return (
            state
            if tile_attachment["order"] == 0 and blur_attachment["order"] == 1
            else None
        )

    after_reorder = client.wait_until("same-stage Effect reorder", reordered)
    if after_reorder["history"]["revision"] != before_reorder["history"]["revision"] + 1:
        raise QaFailure("same-stage Effect drag was not exactly one revision")

    before_cross_stage = after_reorder
    cross_stage_preview = _drag_effect_to_slot(
        client,
        tile["attachment_id"],
        "inspector.effect_drop:item_pre_transform:0",
        -320.0,
        320.0,
        "ItemPreTransform",
        0,
    )

    def moved_cross_stage():
        state = client.state()
        moved = _attachment(state["project"], tile["attachment_id"])
        sibling = _attachment(state["project"], blur["attachment_id"])
        return (
            state
            if moved["stage"] == "item_pre_transform"
            and moved["order"] == 0
            and sibling["stage"] == "item_post_transform"
            and sibling["order"] == 0
            else None
        )

    after_cross_stage = client.wait_until("cross-stage Effect move", moved_cross_stage)
    if (
        after_cross_stage["history"]["revision"]
        != before_cross_stage["history"]["revision"] + 1
    ):
        raise QaFailure("cross-stage Effect drag was not exactly one revision")
    for attachment_id in attachment_ids:
        content_after = {
            key: value
            for key, value in _attachment(
                after_cross_stage["project"], attachment_id
            ).items()
            if key not in ("stage", "order")
        }
        if content_after != content_before[attachment_id]:
            raise QaFailure("Effect drag mutated sibling processor or parameter content")

    # Scroll inside the actual Inspector until a numeric Effect control is
    # physically inside the panel, then edit it through the shared drag value.
    _, scroll_area = client.wait_component("inspector.scroll_area")
    panel = scroll_area["rect_points"]

    def visible_control():
        current = client.component_snapshot()
        candidates = [
            component
            for component in current["components"]
            if component["id"].startswith("inspector.property:attachment:")
            and component.get("type") == "inspector_property_control"
        ]
        for control in candidates:
            rect = control["rect_points"]
            if panel["min_y"] <= rect["center_y"] <= panel["max_y"]:
                return control
        return None

    control = visible_control()
    for _ in range(6):
        if control is not None:
            break
        client.scroll_component("inspector.scroll_area", 0.0, -360.0)
        control = visible_control()
    if control is None:
        raise QaFailure("could not bring an Effect property control into the Inspector viewport")
    parts = control["id"].split(":")
    if len(parts) < 4:
        raise QaFailure("Effect property control has an unstable ID")
    attachment_id = parts[2]
    before_state = client.state()
    before_attachment = _attachment(before_state["project"], attachment_id)
    client.drag_component_by(control["id"], 14.0, 0.0, steps=8)

    def edited():
        state = client.state()
        candidate = _attachment(state["project"], attachment_id)
        return state if candidate != before_attachment else None

    final = client.wait_until("a shared numeric Effect edit", edited)
    for other_id in attachment_ids:
        if other_id == attachment_id:
            continue
        if _attachment(final["project"], other_id) != _attachment(
            before_state["project"], other_id
        ):
            raise QaFailure("editing one Effect mutated sibling {}".format(other_id))
    return {
        "suite": "inspector-effects",
        "item_id": text_item["id"],
        "effect_entries": metadata,
        "tile_controls": sorted(tile_controls),
        "property_control_ids": [candidate["id"] for candidate in controls],
        "edited_control": control["id"],
        "same_stage_drag_preview": reorder_preview,
        "cross_stage_drag_preview": cross_stage_preview,
        "history": final["history"],
        "actions": client.evidence,
    }


if __name__ == "__main__":
    raise SystemExit(
        run_suite_main(
            "qa-inspector-effects-e2e",
            run_suite,
            "target/qa-inspector-effects-e2e-evidence.json",
        )
    )
