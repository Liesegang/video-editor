"""Native Preview geometry assertions shared by Text Tracking lifecycle QA."""

import math

from qa_support import QaFailure
from qa_text_ensemble_support import seek_rendered


def _screen_bounds(metadata, description):
    try:
        bounds = metadata["screen_bounds"]
        result = {
            "min_x": float(bounds["min"]["x"]),
            "min_y": float(bounds["min"]["y"]),
            "max_x": float(bounds["max"]["x"]),
            "max_y": float(bounds["max"]["y"]),
        }
    except (KeyError, TypeError, ValueError) as error:
        raise QaFailure(description + " omitted finite Gizmo screen bounds") from error
    result["width"] = result["max_x"] - result["min_x"]
    result["height"] = result["max_y"] - result["min_y"]
    if not all(math.isfinite(value) for value in result.values()):
        raise QaFailure(description + " has non-finite Gizmo bounds")
    if result["width"] <= 0.0 or result["height"] <= 0.0:
        raise QaFailure(description + " has empty Gizmo bounds")
    return result


def selected_gizmo(client, item_id, description):
    _, component = client.wait_component("preview.position_gizmo")
    metadata = component.get("metadata") or {}
    if metadata.get("item_id") != item_id:
        raise QaFailure(description + " belongs to a different Timeline Item")
    return {
        "bounds": _screen_bounds(metadata, description),
        "canvas_transform": metadata.get("canvas_transform"),
        "outline_count": metadata.get("outline_count"),
        "source_kind": metadata.get("source_kind"),
    }


def observe_gizmos(client, item_id, seconds, revision, description):
    observed = []
    for value in seconds:
        rendered = seek_rendered(client, value)
        if rendered["history"]["revision"] != revision:
            raise QaFailure(description + " changed Project revision while seeking")
        observed.append(
            {
                "seconds": value,
                "frame": rendered["editor"]["preview"]["rendered_frame"],
                "gizmo": selected_gizmo(
                    client, item_id, "{} at {:.3f}s".format(description, value)
                ),
            }
        )
    return observed


def assert_expanded_from_neutral(neutral, tracked, description):
    if len(neutral) != len(tracked):
        raise QaFailure(description + " compared different time samples")
    for plain, evaluated in zip(neutral, tracked):
        if plain["seconds"] != evaluated["seconds"]:
            raise QaFailure(description + " compared mismatched seek times")
        plain_bounds = plain["gizmo"]["bounds"]
        bounds = evaluated["gizmo"]["bounds"]
        if bounds["width"] <= plain_bounds["width"] + 4.0:
            raise QaFailure(
                "{} at {:.3f}s did not include Tracking expansion: {:.3f} <= {:.3f}"
                .format(
                    description,
                    evaluated["seconds"],
                    bounds["width"],
                    plain_bounds["width"],
                )
            )


def assert_gizmo_parity(expected, actual, description, tolerance=0.75):
    if len(expected) != len(actual):
        raise QaFailure(description + " has a different sample count")
    for left, right in zip(expected, actual):
        if left["seconds"] != right["seconds"] or left["frame"] != right["frame"]:
            raise QaFailure(description + " changed sampled time/frame")
        if left["gizmo"]["canvas_transform"] != right["gizmo"]["canvas_transform"]:
            raise QaFailure(description + " changed the shared Canvas transform")
        for key in ("min_x", "min_y", "max_x", "max_y"):
            difference = abs(
                left["gizmo"]["bounds"][key] - right["gizmo"]["bounds"][key]
            )
            if difference > tolerance:
                raise QaFailure(
                    "{} changed {} by {:.3f}px at {:.3f}s".format(
                        description, key, difference, left["seconds"]
                    )
                )


def exercise_moved_letter_selection(client, item_id, neutral, tracked, description):
    before = client.state()
    before_project = before["project"]
    before_history = before["history"]
    plain = neutral["gizmo"]["bounds"]
    bounds = tracked["gizmo"]["bounds"]
    moved_point = {
        "x": bounds["max_x"] - plain["width"] * 0.25,
        "y": bounds["min_y"] + bounds["height"] * 0.75,
    }
    if moved_point["x"] <= plain["max_x"] + 2.0:
        raise QaFailure(description + " has no moved-letter point outside neutral Text")

    client.click_component("preview.tool.select")
    components = {
        component["id"]: component
        for component in client.component_snapshot()["components"]
    }
    try:
        canvas = components["preview.canvas"]["rect_points"]
        content = components["preview.content"]["rect_points"]
    except KeyError as error:
        raise QaFailure(description + " omitted Preview Canvas geometry") from error
    clear_point = {
        "x": (float(canvas["min_x"]) + float(content["min_x"])) * 0.5,
        "y": float(canvas["center_y"]),
    }
    if clear_point["x"] >= float(content["min_x"]):
        raise QaFailure(description + " has no blank Canvas margin for deselection")
    client.inject(
        "click",
        {**clear_point, "button": "primary", "coordinate_space": "points"},
    )
    client.wait_until(
        description + " Canvas deselection",
        lambda: state
        if (state := client.state())["editor"]["selection"]["primary"] is None
        and state["project"] == before_project
        and state["history"] == before_history
        else None,
    )
    client.inject(
        "click",
        {**moved_point, "button": "primary", "coordinate_space": "points"},
    )
    client.wait_until(
        description + " moved-letter Canvas selection",
        lambda: state
        if (state := client.state())["editor"]["selection"]["primary"]
        == {"kind": "timeline_item", "id": item_id}
        and state["project"] == before_project
        and state["history"] == before_history
        else None,
    )
    return {"clear_point": clear_point, "moved_letter_point": moved_point}
