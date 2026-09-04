#!/usr/bin/env python3
"""Exercise descriptor-backed Text Ensemble authoring and Preview evaluation."""

from qa_support import QaFailure, component_point, item_by_name, run_suite_main


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
