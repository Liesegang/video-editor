"""Shared native-UI assertions for Curve key drag Preview gestures."""

from qa_support import QaFailure, component_point, settled_preview_state


def _press_and_move(client, component_id, delta_x, delta_y, steps):
    _, component = client.wait_component_settled(component_id)
    start = component_point(component, 0.5, 0.5)
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
    return start, endpoint


def exercise_curve_key_live_preview(
    client,
    component_id,
    baseline,
    description,
    delta_x=0.0,
    delta_y=18.0,
    steps=8,
    timeout=10.0,
):
    """Verify one Curve gesture's transient, commit, Undo, and Escape lifecycle."""

    before_project = baseline["project"]
    before_history = baseline["history"]
    before_preview = baseline["editor"]["preview"]
    before_hash = before_preview["pixel_hash"]
    frame = baseline["editor"]["timeline"]["current_frame"]
    start, endpoint = _press_and_move(
        client, component_id, delta_x, delta_y, steps
    )
    held = client.wait_until(
        description + " held Preview",
        lambda: state
        if (state := settled_preview_state(
            client, before_history["revision"], frame
        ))
        and state["editor"]["preview"]["pixel_hash"] != before_hash
        and state["project"] == before_project
        and state["history"] == before_history
        and state["editor"]["curve_editor"]["drag_active"]
        and state["editor"]["preview"]["rendered_frame"] == frame
        else None,
        timeout,
    )
    held_hash = held["editor"]["preview"]["pixel_hash"]
    client.inject(
        "release",
        {**endpoint, "button": "primary", "coordinate_space": "points"},
    )
    committed = client.wait_until(
        description + " one release command",
        lambda: state
        if (state := client.state())["history"]["revision"]
        == before_history["revision"] + 1
        and state["project"] != before_project
        and not state["editor"]["curve_editor"]["drag_active"]
        else None,
    )
    observed_committed_hashes = set()

    def matching_committed_preview():
        state = settled_preview_state(
            client, committed["history"]["revision"], frame
        )
        if not state:
            return None
        observed_hash = state["editor"]["preview"]["pixel_hash"]
        observed_committed_hashes.add(observed_hash)
        return state if observed_hash == held_hash else None

    try:
        committed_render = client.wait_until(
            description + " committed Preview",
            matching_committed_preview,
            timeout,
        )
    except QaFailure as error:
        raise QaFailure(
            f"{error}; held hash {held_hash}, committed hashes "
            f"{sorted(observed_committed_hashes)}"
        ) from error
    client.key("z", True, command=True)
    client.key("z", False, command=True)
    undone = client.wait_until(
        description + " Undo",
        lambda: state
        if (state := client.state())["project"] == before_project
        and state["history"]["revision"] == committed["history"]["revision"] + 1
        else None,
    )
    restored = client.wait_until(
        description + " Undo Preview",
        lambda: state
        if (state := settled_preview_state(
            client, undone["history"]["revision"], frame
        ))
        and state["editor"]["preview"]["pixel_hash"] == before_hash
        else None,
        timeout,
    )

    escape_start, escape_endpoint = _press_and_move(
        client, component_id, -delta_x, -delta_y, steps
    )
    escaped_held = client.wait_until(
        description + " Escape held Preview",
        lambda: state
        if (state := settled_preview_state(
            client, restored["history"]["revision"], frame
        ))
        and state["editor"]["preview"]["pixel_hash"] != before_hash
        and state["project"] == before_project
        and state["history"] == restored["history"]
        and state["editor"]["curve_editor"]["drag_active"]
        else None,
        timeout,
    )
    client.key("escape", True)
    client.key("escape", False)
    escaped = client.wait_until(
        description + " Escape cancellation",
        lambda: state
        if not (state := client.state())["editor"]["curve_editor"]["drag_active"]
        and state["project"] == before_project
        and state["history"] == restored["history"]
        else None,
    )
    client.inject(
        "release",
        {**escape_endpoint, "button": "primary", "coordinate_space": "points"},
    )
    escaped_restored = client.wait_until(
        description + " Escape stable Preview",
        lambda: state
        if (state := settled_preview_state(
            client, restored["history"]["revision"], frame
        ))
        and state["project"] == before_project
        and state["history"] == restored["history"]
        and state["editor"]["preview"]["pixel_hash"] == before_hash
        else None,
        timeout,
    )
    if escaped_restored["history"] != escaped["history"]:
        raise QaFailure(description + " release after Escape changed history")
    return {
        "component_id": component_id,
        "frame": frame,
        "start": start,
        "endpoint": endpoint,
        "baseline_hash": before_hash,
        "held_hash": held_hash,
        "committed_hash": committed_render["editor"]["preview"]["pixel_hash"],
        "undo_hash": restored["editor"]["preview"]["pixel_hash"],
        "escape_start": escape_start,
        "escape_held_hash": escaped_held["editor"]["preview"]["pixel_hash"],
        "escape_restored_hash": escaped_restored["editor"]["preview"]["pixel_hash"],
        "history": {
            "before": before_history["revision"],
            "committed": committed["history"]["revision"],
            "undone": undone["history"]["revision"],
            "escaped": escaped_restored["history"]["revision"],
        },
    }
