"""Shared native-UI helpers for direct and promoted Text Ensemble authoring."""

from qa_support import QaFailure, rendered_current_revision, seek_timeline_seconds


def text_operations(state, item_id):
    try:
        source = state["project"]["items"][item_id]["source"]
        if source["kind"] != "text":
            raise QaFailure("selected QA item is no longer Text")
        return source["value"]["ensemble_operations"]
    except KeyError as error:
        raise QaFailure("Text source omitted ensemble_operations") from error


def operation(state, item_id, operation_id):
    return next(
        (
            candidate
            for candidate in text_operations(state, item_id)
            if candidate["id"] == operation_id
        ),
        None,
    )


def constant_value(authored_property, description):
    if authored_property.get("type") != "constant":
        raise QaFailure(description + " must be a constant")
    try:
        return authored_property["properties"]["value"]
    except KeyError as error:
        raise QaFailure(description + " omitted its authored value") from error


def constant_number(operation_value, property_name):
    try:
        value = constant_value(
            operation_value["properties"][property_name], property_name
        )
    except KeyError as error:
        raise QaFailure("{} omitted its authored value".format(property_name)) from error
    if not isinstance(value, (int, float)):
        raise QaFailure("{} is not numeric: {!r}".format(property_name, value))
    return float(value)


def component_in_inspector(client, component_id, attempts=14):
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


def open_and_choose(client, item_id, query, component_id):
    add_id = "inspector.text_ensemble.add_menu:" + item_id
    component_in_inspector(client, add_id)
    client.click_component(add_id)
    query_id = "inspector.text_ensemble.menu:" + item_id + ".query"
    client.wait_component_settled(query_id)
    client.click_component(query_id)
    client.inject("text", {"text": query})
    client.wait_component_settled(component_id)
    client.click_component(component_id)


def seek_rendered(client, seconds):
    sought = seek_timeline_seconds(client, seconds)
    expected_frame = sought["editor"]["timeline"]["current_frame"]
    return client.wait_until(
        "rendered Preview at {:.3f}s".format(seconds),
        lambda: state
        if (state := rendered_current_revision(client))
        and state["editor"]["preview"]["rendered_frame"] == expected_frame
        else None,
        30.0,
    )


def remove_ensemble_operation(client, item_id, operation_id):
    actions_id = "inspector.text_ensemble.actions:" + operation_id
    component_in_inspector(client, actions_id)
    client.click_component(actions_id)
    client.wait_component_settled("inspector.text_ensemble.remove:" + operation_id)
    client.click_component("inspector.text_ensemble.remove:" + operation_id)
    return client.wait_until(
        "Text Ensemble operation removal",
        lambda: state
        if operation((state := client.state()), item_id, operation_id) is None
        else None,
    )
