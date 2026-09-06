"""Shared native-UI observation for held property keyframe insertions."""

from qa_support import QaFailure, component_in_inspector, component_point, finite_number


def begin_reserved_keyframe_scrub(
    client,
    control_id,
    row_id,
    delta_x,
    description,
    control_locator=None,
):
    """Hold a multi-frame scrub and prove its reserved identity remains stable."""

    control = (
        component_in_inspector(client, control_id)[1]
        if control_locator is None
        else control_locator(client, control_id)
    )
    if not isinstance(control, dict):
        raise QaFailure(description + " control locator returned no component")
    start = component_point(control, 0.5, 0.5)
    midpoint = {"x": start["x"] + delta_x * 0.5, "y": start["y"]}
    end = {"x": start["x"] + delta_x, "y": start["y"]}
    client.inject(
        "press", {**start, "button": "primary", "coordinate_space": "points"}
    )
    client.inject(
        "move", {**midpoint, "button": "primary", "coordinate_space": "points"}
    )

    def reservation():
        snapshot = client.component_snapshot()
        row = next(
            (
                item
                for item in snapshot["components"]
                if item.get("id") == row_id
            ),
            None,
        )
        value_control = next(
            (item for item in snapshot["components"] if item.get("id") == control_id),
            None,
        )
        metadata = (row or {}).get("metadata") or {}
        insertion_id = metadata.get("pending_keyframe_insertion_id")
        insertion_time = metadata.get("pending_keyframe_time")
        value = ((value_control or {}).get("metadata") or {}).get("value")
        if not insertion_id or insertion_time is None or value is None:
            return None
        return {
            "id": insertion_id,
            "time": float(insertion_time),
            "row_metadata": metadata,
            "value": finite_number(value, description + " property value"),
            "frame": snapshot["frame"],
        }

    first = client.wait_until(description + " first reserved KeyframeId", reservation)
    client.inject(
        "move", {**end, "button": "primary", "coordinate_space": "points"}
    )

    def continued_reservation():
        current = reservation()
        if current is None or current["frame"] <= first["frame"]:
            return None
        if current["id"] != first["id"] or current["time"] != first["time"]:
            raise QaFailure(description + " changed its reserved KeyframeId during one scrub")
        return current if current["value"] != first["value"] else None

    current = client.wait_until(
        description + " continued reserved KeyframeId", continued_reservation
    )
    return {
        **current,
        "first_value": first["value"],
        "first_frame": first["frame"],
        "endpoint": end,
    }


def release_property_scrub(client, reservation):
    endpoint = reservation.get("endpoint")
    if not isinstance(endpoint, dict):
        raise QaFailure("reserved keyframe scrub omitted its pointer endpoint")
    client.inject(
        "release", {**endpoint, "button": "primary", "coordinate_space": "points"}
    )
