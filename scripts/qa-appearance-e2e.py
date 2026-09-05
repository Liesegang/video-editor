#!/usr/bin/env python3
"""Exercise and reopen the production Text/Shape/Node Clip Appearance stack."""

from qa_support import (
    QaFailure,
    item_by_name,
    run_suite_main,
    seek_timeline_seconds,
    settled_preview_state,
)
from qa_appearance_persistence import exercise_appearance_persistence


def _component(snapshot, component_id):
    return next(
        (value for value in snapshot["components"] if value.get("id") == component_id),
        None,
    )


def _inside_scroll_area(component, scroll_area):
    if not component or not component.get("visible") or not component.get("enabled"):
        return False
    rect = component.get("rect_points") or {}
    area = scroll_area.get("rect_points") or {}
    return (
        float(rect.get("width", 0.0)) > 0.0
        and float(rect.get("height", 0.0)) > 0.0
        and float(area.get("min_y", 0.0))
        <= float(rect.get("center_y", -1.0))
        <= float(area.get("max_y", 0.0))
    )


def _bring_into_inspector(client, component_id):
    for _ in range(14):
        snapshot = client.component_snapshot()
        component = _component(snapshot, component_id)
        scroll_area = _component(snapshot, "inspector.scroll_area")
        if _inside_scroll_area(component, scroll_area or {}):
            return client.wait_component_settled(component_id)[1]
        rect = (component or {}).get("rect_points") or {}
        area = (scroll_area or {}).get("rect_points") or {}
        delta = (
            320.0
            if float(rect.get("center_y", 0.0)) < float(area.get("min_y", 0.0))
            else -320.0
        )
        client.scroll_component("inspector.scroll_area", 0.0, delta)
    raise QaFailure("could not bring {} into the Inspector".format(component_id))


def _appearance(state, item_id):
    source = state["project"]["items"][item_id]["source"]
    if source.get("kind") == "text":
        return source["value"].get("appearance_operations") or []
    if source.get("kind") == "shape":
        return (source["value"].get("shape") or {}).get("appearance_operations") or []
    raise QaFailure("item no longer owns a direct Text/Shape Appearance")


def _constant(operation, key):
    prop = (operation.get("properties") or {}).get(key) or {}
    if prop.get("type") != "constant":
        return None
    return (prop.get("properties") or {}).get("value")


def _add_style(client, item_id, query, component_id):
    add_id = "inspector.appearance.add_menu:" + item_id
    add_component = _bring_into_inspector(client, add_id)
    available_styles = set((add_component.get("metadata") or {}).get("component_ids") or [])
    required_styles = {
        "bevel_emboss",
        "stroke",
        "inner_shadow",
        "inner_glow",
        "satin",
        "color_overlay",
        "gradient_overlay",
        "pattern_overlay",
        "outer_glow",
        "drop_shadow",
    }
    missing_styles = sorted(required_styles - available_styles)
    if missing_styles:
        raise QaFailure("Appearance catalog is missing: {}".format(", ".join(missing_styles)))
    client.click_component(add_id)
    query_id = "inspector.appearance.menu:" + item_id + ".query"
    client.wait_component_settled(query_id)
    client.click_component(query_id)
    client.key("a", True, command=True)
    client.key("a", False, command=True)
    client.inject("text", {"text": query})
    choice_id = "inspector.appearance.add." + component_id
    choice = client.wait_component_settled(choice_id)[1]
    metadata = choice.get("metadata") or {}
    if metadata.get("component_id") != component_id:
        raise QaFailure(
            "Appearance search did not expose the real {} plugin".format(component_id)
        )
    client.click_component(choice_id)
    return metadata, sorted(available_styles)


def run_suite(client):
    client.wait_health()
    initial = client.state()
    item = item_by_name(initial["project"], "QA Text")
    item_id = item["id"]
    client.click_component("timeline.item:" + item_id)
    seeked = seek_timeline_seconds(client, 3.0)
    frame = seeked["editor"]["timeline"]["current_frame"]
    before = client.state()
    original_operations = _appearance(before, item_id)
    if len(original_operations) != 1:
        raise QaFailure("QA Text expected one explicit initial Fill Appearance")
    if original_operations[0]["operation"].get("component_id") != "fill":
        raise QaFailure("QA Text initial Appearance is not Fill")
    original_module_counts = (
        len(before["project"].get("module_definitions") or {}),
        len(before["project"].get("module_instances") or {}),
    )
    baseline = client.wait_until(
        "Appearance baseline Preview",
        lambda: settled_preview_state(client, before["history"]["revision"], frame),
        timeout=30.0,
    )

    choice_metadata, available_styles = _add_style(
        client, item_id, "drop shadow", "drop_shadow"
    )

    def added():
        state = client.state()
        operations = _appearance(state, item_id)
        return (
            (state, operations[-1])
            if state["history"]["revision"] == before["history"]["revision"] + 1
            and len(operations) == 2
            and operations[-1]["operation"].get("component_id") == "drop_shadow"
            else None
        )

    added_state, shadow = client.wait_until("Drop Shadow Appearance add", added)
    if (
        len(added_state["project"].get("module_definitions") or {}),
        len(added_state["project"].get("module_instances") or {}),
    ) != original_module_counts:
        raise QaFailure("direct Appearance edit expanded the Timeline item into Nodes")
    shadow_id = shadow["id"]
    added_render = client.wait_until(
        "Drop Shadow Preview",
        lambda: state
        if (
            (state := settled_preview_state(
                client, added_state["history"]["revision"], frame
            ))
            and state["editor"]["preview"].get("pixel_hash")
            != baseline["editor"]["preview"].get("pixel_hash")
        )
        else None,
        timeout=30.0,
    )

    expected_editors = {
        "color": "managed_color",
        "opacity": "float",
        "blend_mode": "dropdown",
        "distance": "float",
    }
    editor_metadata = {}
    for key, expected in expected_editors.items():
        control_id = "inspector.property:appearance:{}:{}:{}".format(
            item_id, shadow_id, key
        )
        control = _bring_into_inspector(client, control_id)
        metadata = control.get("metadata") or {}
        if metadata.get("editor_kind") != expected:
            raise QaFailure(
                "Drop Shadow {} did not use the typed {} editor".format(key, expected)
            )
        editor_metadata[key] = metadata

    distance_id = "inspector.property:appearance:{}:{}:distance".format(
        item_id, shadow_id
    )
    before_distance = _constant(shadow, "distance")
    # Release beyond the compact field to verify pointer capture commits the
    # user's scrub instead of leaving only an unpublished transient preview.
    client.drag_component_by(distance_id, 28.0, 0.0, steps=8)

    def distance_edited():
        state = client.state()
        operation = next(
            (candidate for candidate in _appearance(state, item_id) if candidate["id"] == shadow_id),
            None,
        )
        return (
            state
            if operation is not None
            and state["history"]["revision"]
            == added_state["history"]["revision"] + 1
            and _constant(operation, "distance") != before_distance
            else None
        )

    edited_state = client.wait_until("Drop Shadow distance edit", distance_edited)
    edited_render = client.wait_until(
        "edited Drop Shadow Preview",
        lambda: settled_preview_state(
            client, edited_state["history"]["revision"], frame
        ),
        timeout=30.0,
    )

    move_id = "inspector.appearance.move_up:" + shadow_id
    _bring_into_inspector(client, move_id)
    client.click_component(move_id)

    def reordered():
        state = client.state()
        ids = [operation["id"] for operation in _appearance(state, item_id)]
        return (
            state
            if state["history"]["revision"] == edited_state["history"]["revision"] + 1
            and ids == [shadow_id, original_operations[0]["id"]]
            else None
        )

    reordered_state = client.wait_until("Appearance reorder", reordered)
    reordered_render = client.wait_until(
        "reordered Appearance Preview",
        lambda: settled_preview_state(
            client, reordered_state["history"]["revision"], frame
        ),
        timeout=30.0,
    )

    actions_id = "inspector.appearance.actions:" + shadow_id
    _bring_into_inspector(client, actions_id)
    client.click_component(actions_id)
    remove_id = "inspector.appearance.remove:" + shadow_id
    client.wait_component_settled(remove_id)
    client.click_component(remove_id)

    def removed():
        state = client.state()
        operations = _appearance(state, item_id)
        return (
            state
            if state["history"]["revision"] == reordered_state["history"]["revision"] + 1
            and [operation["id"] for operation in operations]
            == [original_operations[0]["id"]]
            else None
        )

    removed_state = client.wait_until("Appearance remove", removed)
    removed_render = client.wait_until(
        "Appearance removal Preview",
        lambda: state
        if (
            (state := settled_preview_state(
                client, removed_state["history"]["revision"], frame
            ))
            and state["editor"]["preview"].get("pixel_hash")
            == baseline["editor"]["preview"].get("pixel_hash")
        )
        else None,
        timeout=30.0,
    )

    client.key("z", True, command=True)
    client.key("z", False, command=True)
    restored = client.wait_until(
        "Appearance remove Undo",
        lambda: state
        if (
            (state := client.state())["history"]["revision"]
            == removed_state["history"]["revision"] + 1
            and [operation["id"] for operation in _appearance(state, item_id)]
            == [shadow_id, original_operations[0]["id"]]
        )
        else None,
    )
    client.wait_until(
        "Appearance Undo Preview",
        lambda: state
        if (
            (state := settled_preview_state(
                client, restored["history"]["revision"], frame
            ))
            and state["editor"]["preview"].get("pixel_hash")
            == reordered_render["editor"]["preview"].get("pixel_hash")
        )
        else None,
        timeout=30.0,
    )

    gradient_choice, _ = _add_style(
        client, item_id, "gradient overlay", "gradient_overlay"
    )

    def gradient_added():
        state = client.state()
        operations = _appearance(state, item_id)
        operation = operations[-1] if operations else None
        return (
            (state, operation)
            if operation is not None
            and operation["operation"].get("component_id") == "gradient_overlay"
            and state["history"]["revision"] == restored["history"]["revision"] + 1
            else None
        )

    gradient_state, gradient = client.wait_until("Gradient Overlay add", gradient_added)
    gradient_id = gradient["id"]
    gradient_control = "inspector.property:appearance:{}:{}:gradient".format(
        item_id, gradient_id
    )
    gradient_metadata = (
        _bring_into_inspector(client, gradient_control).get("metadata") or {}
    )
    if gradient_metadata.get("editor_kind") != "gradient":
        raise QaFailure("Gradient Overlay did not use the typed Gradient editor")
    gradient_render = client.wait_until(
        "Gradient Overlay Preview",
        lambda: state
        if (
            (state := settled_preview_state(
                client, gradient_state["history"]["revision"], frame
            ))
            and state["editor"]["preview"].get("pixel_hash")
            != reordered_render["editor"]["preview"].get("pixel_hash")
        )
        else None,
        timeout=30.0,
    )

    client.click_component(gradient_control)
    gradient_offset = gradient_control + ".gradient.stop.0.offset"
    client.wait_component_settled(gradient_offset)
    before_gradient = _constant(gradient, "gradient")
    client.drag_component_by(gradient_offset, 12.0, 0.0, steps=8)

    def gradient_edited():
        state = client.state()
        operation = next(
            (
                candidate
                for candidate in _appearance(state, item_id)
                if candidate["id"] == gradient_id
            ),
            None,
        )
        return (
            state
            if operation is not None
            and state["history"]["revision"]
            == gradient_state["history"]["revision"] + 1
            and _constant(operation, "gradient") != before_gradient
            else None
        )

    gradient_edited_state = client.wait_until("Gradient stop edit", gradient_edited)
    client.click_component(gradient_control)
    client.key("z", True, command=True)
    client.key("z", False, command=True)
    gradient_undo = client.wait_until(
        "Gradient stop Undo",
        lambda: state
        if (
            (state := client.state())["history"]["revision"]
            == gradient_edited_state["history"]["revision"] + 1
            and _constant(
                next(
                    operation
                    for operation in _appearance(state, item_id)
                    if operation["id"] == gradient_id
                ),
                "gradient",
            )
            == before_gradient
        )
        else None,
    )

    pattern_choice, _ = _add_style(
        client, item_id, "pattern overlay", "pattern_overlay"
    )

    def pattern_added():
        state = client.state()
        operations = _appearance(state, item_id)
        operation = operations[-1] if operations else None
        return (
            (state, operation)
            if operation is not None
            and operation["operation"].get("component_id") == "pattern_overlay"
            and state["history"]["revision"] == gradient_undo["history"]["revision"] + 1
            else None
        )

    pattern_state, pattern = client.wait_until("Pattern Overlay add", pattern_added)
    pattern_id = pattern["id"]
    pattern_control = "inspector.property:appearance:{}:{}:pattern".format(
        item_id, pattern_id
    )
    pattern_metadata = _bring_into_inspector(client, pattern_control).get("metadata") or {}
    if pattern_metadata.get("editor_kind") != "pattern":
        raise QaFailure("Pattern Overlay did not use the typed Pattern editor")
    pattern_render = client.wait_until(
        "Pattern Overlay Preview",
        lambda: state
        if (
            (state := settled_preview_state(
                client, pattern_state["history"]["revision"], frame
            ))
            and state["editor"]["preview"].get("pixel_hash")
            != gradient_render["editor"]["preview"].get("pixel_hash")
        )
        else None,
        timeout=30.0,
    )
    client.click_component(pattern_control)
    pattern_angle = pattern_control + ".pattern.angle"
    client.wait_component_settled(pattern_angle)
    before_pattern = _constant(pattern, "pattern")
    client.drag_component_by(pattern_angle, 12.0, 0.0, steps=8)

    def pattern_edited():
        state = client.state()
        operation = next(
            (
                candidate
                for candidate in _appearance(state, item_id)
                if candidate["id"] == pattern_id
            ),
            None,
        )
        return (
            state
            if operation is not None
            and state["history"]["revision"]
            == pattern_state["history"]["revision"] + 1
            and _constant(operation, "pattern") != before_pattern
            else None
        )

    pattern_edited_state = client.wait_until("Pattern angle edit", pattern_edited)
    pattern_edited_render = client.wait_until(
        "edited Pattern Overlay Preview",
        lambda: state
        if (
            (state := settled_preview_state(
                client, pattern_edited_state["history"]["revision"], frame
            ))
            and state["editor"]["preview"].get("pixel_hash")
            != pattern_render["editor"]["preview"].get("pixel_hash")
        )
        else None,
        timeout=30.0,
    )
    client.click_component(pattern_control)
    client.key("z", True, command=True)
    client.key("z", False, command=True)
    pattern_undo = client.wait_until(
        "Pattern angle Undo",
        lambda: state
        if (
            (state := client.state())["history"]["revision"]
            == pattern_edited_state["history"]["revision"] + 1
            and _constant(
                next(
                    operation
                    for operation in _appearance(state, item_id)
                    if operation["id"] == pattern_id
                ),
                "pattern",
            )
            == before_pattern
        )
        else None,
    )

    palette_count = len(pattern_undo["project"]["palette"]["definitions"])
    client.click_component(pattern_control)
    client.click_component(pattern_control + ".paint.add_current")
    palette_state = client.wait_until(
        "Pattern Project Palette add",
        lambda: state
        if (
            (state := client.state())["history"]["revision"]
            == pattern_undo["history"]["revision"] + 1
            and len(state["project"]["palette"]["definitions"]) == palette_count + 1
            and any(
                definition.get("paint", {}).get("kind") == "pattern"
                for definition in state["project"]["palette"]["definitions"].values()
            )
        )
        else None,
    )
    client.click_component(pattern_control)
    persistence = exercise_appearance_persistence(
        client,
        item_id,
        palette_state,
        _appearance,
        _bring_into_inspector,
    )
    return {
        "suite": "appearance",
        "item_id": item_id,
        "shadow_id": shadow_id,
        "choice": choice_metadata,
        "available_styles": sorted(available_styles),
        "typed_editors": editor_metadata,
        "paint_overlays": {
            "gradient_choice": gradient_choice,
            "gradient_editor": gradient_metadata,
            "pattern_choice": pattern_choice,
            "pattern_editor": pattern_metadata,
            "gradient_hash": gradient_render["editor"]["preview"]["pixel_hash"],
            "pattern_hash": pattern_render["editor"]["preview"]["pixel_hash"],
            "pattern_edited_hash": pattern_edited_render["editor"]["preview"][
                "pixel_hash"
            ],
            "palette_definition_count": len(
                palette_state["project"]["palette"]["definitions"]
            ),
        },
        "preview_hashes": {
            "baseline": baseline["editor"]["preview"]["pixel_hash"],
            "added": added_render["editor"]["preview"]["pixel_hash"],
            "edited": edited_render["editor"]["preview"]["pixel_hash"],
            "reordered": reordered_render["editor"]["preview"]["pixel_hash"],
            "removed": removed_render["editor"]["preview"]["pixel_hash"],
        },
        "history": persistence["fresh_process"]["history"],
        "persistence": persistence,
        "capture": persistence["fresh_process"]["capture"],
        "actions": client.evidence,
    }


if __name__ == "__main__":
    raise SystemExit(
        run_suite_main(
            "qa-appearance-e2e",
            run_suite,
            "target/qa-appearance-e2e-evidence.json",
        )
    )
