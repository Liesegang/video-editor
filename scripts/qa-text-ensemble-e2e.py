#!/usr/bin/env python3
"""Exercise descriptor-backed Text Ensemble authoring and Preview evaluation."""

from qa_support import (
    QaFailure,
    component_point,
    item_by_name,
    run_suite_main,
    seek_timeline_seconds,
)


def _text_operations(state, item_id):
    try:
        source = state["project"]["items"][item_id]["source"]
        if source["kind"] != "text":
            raise QaFailure("selected QA item is no longer Text")
        return source["value"]["ensemble_operations"]
    except KeyError as error:
        raise QaFailure("Text source omitted ensemble_operations") from error


def _operation(state, item_id, operation_id):
    return next(
        (
            operation
            for operation in _text_operations(state, item_id)
            if operation["id"] == operation_id
        ),
        None,
    )


def _constant_number(operation, property_name):
    try:
        property_value = operation["properties"][property_name]
        if property_value["type"] != "constant":
            raise QaFailure(
                "{} must start as a constant for the native Step Delay QA".format(
                    property_name
                )
            )
        value = property_value["properties"]["value"]
    except KeyError as error:
        raise QaFailure("{} omitted its authored value".format(property_name)) from error
    if not isinstance(value, (int, float)):
        raise QaFailure("{} is not numeric: {!r}".format(property_name, value))
    return float(value)


def _component_in_inspector(client, component_id, attempts=14):
    _, scroll = client.wait_component("inspector.scroll_area")
    panel = scroll["rect_points"]
    for _ in range(attempts):
        snapshot = client.component_snapshot()
        component = next(
            (entry for entry in snapshot["components"] if entry["id"] == component_id),
            None,
        )
        if component is not None:
            rect = component["rect_points"]
            if (
                component.get("visible") is True
                and panel["min_y"] <= rect["center_y"] <= panel["max_y"]
            ):
                return client.wait_component_settled(component_id)
            delta = 300.0 if rect["center_y"] < panel["min_y"] else -300.0
        else:
            delta = -300.0
        client.scroll_component("inspector.scroll_area", 0.0, delta)
    raise QaFailure("could not bring {} into the Inspector".format(component_id))


def _open_and_choose(client, item_id, query, component_id):
    add_id = "inspector.text_ensemble.add_menu:" + item_id
    _component_in_inspector(client, add_id)
    client.click_component(add_id)
    query_id = "inspector.text_ensemble.menu:" + item_id + ".query"
    client.click_component(query_id)
    client.inject("text", {"text": query})
    client.click_component(component_id)


def _rendered_current_revision(client, prior_hash=None):
    state = client.state()
    preview = state["editor"]["preview"]
    revision = state["history"]["revision"]
    if (
        preview.get("rendered_revision") == revision
        and preview.get("rendered_frame")
        == state["editor"]["timeline"]["current_frame"]
        and preview.get("pixel_hash") is not None
        and state["editor"].get("error") is None
        and (prior_hash is None or preview["pixel_hash"] != prior_hash)
    ):
        return state
    return None


def _seek_rendered(client, seconds):
    sought = seek_timeline_seconds(client, seconds)
    expected_frame = sought["editor"]["timeline"]["current_frame"]
    return client.wait_until(
        "rendered Preview at {:.3f}s".format(seconds),
        lambda: state
        if (state := _rendered_current_revision(client))
        and state["editor"]["preview"]["rendered_frame"] == expected_frame
        else None,
        30.0,
    )


def _remove_ensemble_operation(client, item_id, operation_id):
    actions_id = "inspector.text_ensemble.actions:" + operation_id
    _component_in_inspector(client, actions_id)
    client.click_component(actions_id)
    client.click_component("inspector.text_ensemble.remove:" + operation_id)
    return client.wait_until(
        "Text Ensemble operation removal",
        lambda: state
        if _operation((state := client.state()), item_id, operation_id) is None
        else None,
    )


def run_suite(client):
    client.wait_health()
    initial = client.wait_until(
        "initial rendered Preview", lambda: _rendered_current_revision(client), 30.0
    )
    if initial["project"].get("name") != "Authoring QA":
        raise QaFailure("Text Ensemble QA requires authoring_e2e")
    text_item = item_by_name(initial["project"], "QA Text")
    item_id = text_item["id"]
    if _text_operations(initial, item_id):
        raise QaFailure("QA Text must start with an empty Ensemble stack")

    client.click_component("timeline.item:" + item_id)
    _, ruler = client.wait_component("timeline.ruler")
    seek_point = component_point(ruler, 0.15, 0.5)
    client.inject(
        "click",
        {
            **seek_point,
            "button": "primary",
            "coordinate_space": "points",
        },
    )
    rough_seek = client.wait_until(
        "initial Timeline seek",
        lambda: state
        if (state := client.state())["editor"]["timeline"]["current_frame"] > 0
        else None,
    )
    # Resolve the exact visible ruler mapping from the real seek result, then
    # move to 1.5s where QA Text is visible above the later overlap clip.
    current_frame = rough_seek["editor"]["timeline"]["current_frame"]
    pixels_per_second = rough_seek["editor"]["timeline"]["pixels_per_second"]
    target_x = seek_point["x"] + (1.5 - current_frame / 30.0) * pixels_per_second
    if not ruler["rect_points"]["min_x"] <= target_x <= ruler["rect_points"]["max_x"]:
        raise QaFailure("1.5s Text frame is outside the visible Timeline ruler")
    client.inject(
        "click",
        {
            "x": target_x,
            "y": seek_point["y"],
            "button": "primary",
            "coordinate_space": "points",
        },
    )
    baseline = client.wait_until(
        "Text clip Preview at current revision",
        lambda: state
        if (state := _rendered_current_revision(client))
        and 44 <= state["editor"]["timeline"]["current_frame"] <= 46
        else None,
        30.0,
    )
    baseline_hash = baseline["editor"]["preview"]["pixel_hash"]

    add_id = "inspector.text_ensemble.add_menu:" + item_id
    _, add_button = _component_in_inspector(client, add_id)
    add_metadata = add_button.get("metadata") or {}
    if add_metadata.get("descriptor_driven") is not True:
        raise QaFailure("Text Ensemble add catalog is not descriptor-driven")
    if add_metadata.get("node_graph_decorator_count", 0) < 1:
        raise QaFailure("media-input Decorators were not kept in the Node Editor")

    # Reproduce the production Step Delay path before testing stack mechanics.
    # The check compares enabled/removed pixels at the same clip-local times;
    # a descriptor-only or time-insensitive implementation cannot pass it.
    _open_and_choose(
        client,
        item_id,
        "step delay",
        "inspector.text_ensemble.add.effector:step_delay",
    )

    def one_step_delay():
        state = client.state()
        operations = _text_operations(state, item_id)
        return state if len(operations) == 1 else None

    step_added = client.wait_until("Step Delay addition", one_step_delay)
    step_delay = _text_operations(step_added, item_id)[0]
    step_delay_id = step_delay["id"]
    if step_delay["operation"] != {
        "category": "effector",
        "component_id": "step_delay",
        "operation": "effector.apply.v1",
        "version": "0.1.0",
    }:
        raise QaFailure("Step Delay did not persist its production operation identity")
    required_properties = {"delay", "duration", "from_opacity", "to_opacity", "target"}
    if not step_delay.get("declared_ports") or not required_properties.issubset(
        step_delay.get("properties", {})
    ):
        raise QaFailure("Step Delay omitted its descriptor contract or properties")

    delay = _constant_number(step_delay, "delay")
    duration_control = "inspector.property:text_ensemble:{}:{}:duration".format(
        item_id, step_delay_id
    )
    _, duration_component = _component_in_inspector(client, duration_control)
    if (duration_component.get("metadata") or {}).get("has_definition") is not True:
        raise QaFailure("Step Delay Duration bypassed its typed plugin descriptor")
    duration_before = _constant_number(step_delay, "duration")
    revision_before_duration = step_added["history"]["revision"]
    client.drag_component_by(duration_control, 32.0, 0.0, steps=12)

    def duration_edited():
        state = client.state()
        operation = _operation(state, item_id, step_delay_id)
        if operation is None:
            return None
        duration = _constant_number(operation, "duration")
        if duration == duration_before:
            return None
        return state, duration

    duration_state, duration = client.wait_until(
        "typed Step Delay Duration edit", duration_edited
    )
    if duration_state["history"]["revision"] <= revision_before_duration:
        raise QaFailure("Step Delay Duration edit did not commit a Project revision")
    if not 0.0 < duration <= 5.0 or not 0.0 < delay <= 5.0:
        raise QaFailure(
            "Step Delay typed values left their descriptor range: delay={}, duration={}".format(
                delay, duration
            )
        )

    text_length = len(text_item["source"]["value"]["text"])
    clip_start = 1.0
    early_local = duration / 2.0
    late_local = duration + delay * max(0, text_length - 1) + 0.1
    if late_local >= 7.0:
        raise QaFailure(
            "Step Delay completion time {:.3f}s exceeds the QA Text clip".format(
                late_local
            )
        )

    # The native Timeline is frame-addressed. Quantize computed clip-local
    # observation times before using the ruler so the click and assertion
    # cannot disagree at a half-frame boundary.
    early_frame = int((clip_start + early_local) * 30.0 + 0.5)
    late_frame = int((clip_start + late_local) * 30.0 + 0.5)
    early_seconds = early_frame / 30.0
    late_seconds = late_frame / 30.0
    early_local = early_seconds - clip_start
    late_local = late_seconds - clip_start

    early_with_step = _seek_rendered(client, early_seconds)
    early_step_hash = early_with_step["editor"]["preview"]["pixel_hash"]
    step_removed_early = _remove_ensemble_operation(client, item_id, step_delay_id)
    early_without_step = client.wait_until(
        "early Preview without Step Delay",
        lambda: state
        if (state := _rendered_current_revision(client, early_step_hash))
        and state["editor"]["preview"]["rendered_frame"]
        == step_removed_early["editor"]["timeline"]["current_frame"]
        else None,
        30.0,
    )
    early_plain_hash = early_without_step["editor"]["preview"]["pixel_hash"]

    client.key("z", True, command=True)
    client.key("z", False, command=True)
    restored_step = client.wait_until(
        "Step Delay restoration through Undo",
        lambda: state
        if _operation((state := client.state()), item_id, step_delay_id) is not None
        else None,
    )
    client.wait_until(
        "deterministic early Step Delay Preview after Undo",
        lambda: state
        if (state := _rendered_current_revision(client))
        and state["editor"]["preview"]["rendered_frame"]
        == restored_step["editor"]["timeline"]["current_frame"]
        and state["editor"]["preview"]["pixel_hash"] == early_step_hash
        else None,
        30.0,
    )

    late_with_step = _seek_rendered(client, late_seconds)
    late_step_hash = late_with_step["editor"]["preview"]["pixel_hash"]
    step_removed_late = _remove_ensemble_operation(client, item_id, step_delay_id)
    late_without_step = client.wait_until(
        "completed Preview without Step Delay",
        lambda: state
        if (state := _rendered_current_revision(client))
        and state["editor"]["preview"]["rendered_frame"]
        == step_removed_late["editor"]["timeline"]["current_frame"]
        else None,
        30.0,
    )
    late_plain_hash = late_without_step["editor"]["preview"]["pixel_hash"]

    # Any enabled Ensemble currently uses the production per-grapheme draw
    # path, while an empty stack uses SkParagraph. Compare the completed Step
    # Delay with a neutral Ensemble operation at this exact frame so shaping
    # differences cannot masquerade as a Step Delay failure.
    _open_and_choose(
        client,
        item_id,
        "transform",
        "inspector.text_ensemble.add.effector:transform",
    )
    neutral_added = client.wait_until(
        "neutral Ensemble comparison operation",
        lambda: state
        if len(_text_operations((state := client.state()), item_id)) == 1
        and _text_operations(state, item_id)[0]["operation"]["component_id"]
        == "transform"
        else None,
    )
    neutral_id = _text_operations(neutral_added, item_id)[0]["id"]
    neutral_rendered = client.wait_until(
        "neutral Ensemble Preview at Step Delay completion time",
        lambda: state
        if (state := _rendered_current_revision(client))
        and state["editor"]["preview"]["rendered_frame"]
        == neutral_added["editor"]["timeline"]["current_frame"]
        else None,
        30.0,
    )
    late_neutral_hash = neutral_rendered["editor"]["preview"]["pixel_hash"]
    if late_step_hash != late_neutral_hash:
        raise QaFailure(
            "Step Delay did not converge to a neutral Ensemble after completion: "
            "step={}, neutral={}, no_ensemble={}".format(
                late_step_hash, late_neutral_hash, late_plain_hash
            )
        )
    _remove_ensemble_operation(client, item_id, neutral_id)
    _seek_rendered(client, 1.5)

    before_add_revision = baseline["history"]["revision"]
    _open_and_choose(
        client,
        item_id,
        "transform",
        "inspector.text_ensemble.add.effector:transform",
    )

    def one_operation():
        state = client.state()
        operations = _text_operations(state, item_id)
        return state if len(operations) == 1 else None

    after_transform = client.wait_until("Transform Effector addition", one_operation)
    if after_transform["history"]["revision"] <= before_add_revision:
        raise QaFailure("adding an Effector did not commit a Project revision")
    transform = _text_operations(after_transform, item_id)[0]
    transform_id = transform["id"]
    if transform["operation"] != {
        "category": "effector",
        "component_id": "transform",
        "operation": "effector.apply.v1",
        "version": "0.1.0",
    }:
        raise QaFailure("Transform did not persist the production operation identity")
    if not transform.get("declared_ports") or "tx" not in transform.get("properties", {}):
        raise QaFailure("Transform did not persist its descriptor contract and properties")

    _open_and_choose(
        client,
        item_id,
        "opacity",
        "inspector.text_ensemble.add.effector:opacity",
    )

    def two_operations():
        state = client.state()
        operations = _text_operations(state, item_id)
        return state if len(operations) == 2 else None

    after_opacity = client.wait_until("Opacity Effector addition", two_operations)
    opacity = next(
        operation
        for operation in _text_operations(after_opacity, item_id)
        if operation["operation"]["component_id"] == "opacity"
    )
    opacity_id = opacity["id"]

    move_id = "inspector.text_ensemble.move_up:" + opacity_id
    _component_in_inspector(client, move_id)
    client.click_component(move_id)

    def reordered():
        state = client.state()
        operations = _text_operations(state, item_id)
        return state if [entry["id"] for entry in operations] == [opacity_id, transform_id] else None

    try:
        after_reorder = client.wait_until("Text Ensemble reorder", reordered)
    except QaFailure as error:
        failed = client.state()
        raise QaFailure(
            "{}; operation order={!r}; editor error={!r}".format(
                error,
                [entry["id"] for entry in _text_operations(failed, item_id)],
                {
                    "transform": transform_id,
                    "opacity": opacity_id,
                    "editor_error": failed["editor"].get("error"),
                    "last_action": client.evidence[-1] if client.evidence else None,
                },
            )
        ) from error

    control_id = "inspector.property:text_ensemble:{}:{}:opacity".format(
        item_id, opacity_id
    )
    _, control = _component_in_inspector(client, control_id)
    metadata = control.get("metadata") or {}
    if metadata.get("has_definition") is not True:
        raise QaFailure("Opacity property bypassed its plugin descriptor")
    opacity_before = _operation(after_reorder, item_id, opacity_id)["properties"][
        "opacity"
    ]
    revision_before_edit = after_reorder["history"]["revision"]
    client.drag_component_by(control_id, 64.0, 0.0, steps=12)

    def property_edited():
        state = client.state()
        operation = _operation(state, item_id, opacity_id)
        if operation is None or operation["properties"]["opacity"] == opacity_before:
            return None
        return state

    edited = client.wait_until("descriptor-backed Opacity property edit", property_edited)
    if edited["history"]["revision"] <= revision_before_edit:
        raise QaFailure("property edit did not commit a Project revision")
    try:
        rendered = client.wait_until(
            "edited Ensemble reflected in Preview",
            lambda: _rendered_current_revision(client, baseline_hash),
            30.0,
        )
    except QaFailure as error:
        failed = client.state()
        raise QaFailure(
            "{}; baseline_hash={!r}; preview={!r}; revision={!r}; opacity={!r}; error={!r}".format(
                error,
                baseline_hash,
                failed["editor"]["preview"],
                failed["history"]["revision"],
                _operation(failed, item_id, opacity_id)["properties"]["opacity"],
                failed["editor"].get("error"),
            )
        ) from error

    actions_id = "inspector.text_ensemble.actions:" + opacity_id
    _component_in_inspector(client, actions_id)
    client.click_component(actions_id)
    client.click_component("inspector.text_ensemble.remove:" + opacity_id)

    def removed():
        state = client.state()
        operations = _text_operations(state, item_id)
        return state if [entry["id"] for entry in operations] == [transform_id] else None

    final = client.wait_until("Opacity removal through overflow", removed)
    if final["editor"].get("error") is not None:
        raise QaFailure("Text Ensemble left an editor error: {}".format(final["editor"]["error"]))
    if final["history"].get("can_undo") is not True:
        raise QaFailure("Text Ensemble edits did not reach shared history")
    return {
        "suite": "text-ensemble",
        "item_id": item_id,
        "catalog": add_metadata,
        "transform_operation_id": transform_id,
        "opacity_operation_id": opacity_id,
        "step_delay": {
            "operation_id": step_delay_id,
            "delay": delay,
            "duration_before": duration_before,
            "duration_after": duration,
            "early_local_seconds": early_local,
            "late_local_seconds": late_local,
            "early_enabled_pixel_hash": early_step_hash,
            "early_removed_pixel_hash": early_plain_hash,
            "late_enabled_pixel_hash": late_step_hash,
            "late_removed_pixel_hash": late_plain_hash,
            "late_neutral_ensemble_pixel_hash": late_neutral_hash,
        },
        "baseline_pixel_hash": baseline_hash,
        "edited_pixel_hash": rendered["editor"]["preview"]["pixel_hash"],
        "edited_revision": rendered["history"]["revision"],
        "final_revision": final["history"]["revision"],
        "final_operations": _text_operations(final, item_id),
        "actions": client.evidence,
    }


if __name__ == "__main__":
    raise SystemExit(
        run_suite_main(
            "qa-text-ensemble-e2e",
            run_suite,
            "target/qa-text-ensemble-e2e-evidence.json",
        )
    )
