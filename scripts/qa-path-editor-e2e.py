#!/usr/bin/env python3
"""Exercise the production canonical Path editor through native egui input."""

from __future__ import annotations

import copy
import math

from qa_support import QaFailure, component_center, item_by_name, run_suite_main


FIXTURE = "authoring_path_e2e"


def _path_value(state, item_id):
    item = state["project"]["items"][item_id]
    source = item["source"]
    if source.get("kind") != "shape":
        raise QaFailure("QA Path item is not a Shape source")
    shape = source["value"]["shape"]
    if shape.get("shape_kind") != "path":
        raise QaFailure("QA Path item is not canonical Path geometry")
    path = shape["parameters"].get("path")
    if not isinstance(path, dict) or path.get("$type") != "path_value":
        raise QaFailure("QA Path item omitted its authoritative PathValue")
    return path


def _point(point):
    return [float(point["x"]), float(point["y"])]


def _subtract(left, right):
    return [left[0] - right[0], left[1] - right[1]]


def _near(left, right, tolerance=0.08):
    return all(abs(float(a) - float(b)) <= tolerance for a, b in zip(left, right))


def _point_zero_handles(path):
    contour = path["contours"][0]
    start = _point(contour["start"])
    segments = contour["segments"]
    outgoing = [0.0, 0.0]
    incoming = [0.0, 0.0]
    if segments and segments[0]["kind"] == "cubic":
        outgoing = _subtract(_point(segments[0]["control1"]), start)
    if segments:
        closing = segments[-1]
        if closing["kind"] == "cubic" and _near(_point(closing["to"]), start):
            incoming = _subtract(_point(closing["control2"]), start)
    return incoming, outgoing


def _length(vector):
    return math.hypot(vector[0], vector[1])


def _wait_path(client, item_id, description, predicate):
    return client.wait_until(
        description,
        lambda: state
        if predicate(_path_value(state := client.state(), item_id), state)
        else None,
        timeout=20.0,
    )


def _assert_one_revision(before, after, operation):
    delta = int(after["history"]["revision"]) - int(before["history"]["revision"])
    if delta != 1:
        raise QaFailure(
            "{} created {} Project revisions; expected one transaction".format(
                operation, delta
            )
        )


def _drag_component(client, component_id, delta, modifiers=None, steps=14):
    _, component = client.wait_component_settled(component_id)
    start = component_center(component)
    end = {"x": start["x"] + delta[0], "y": start["y"] + delta[1]}
    client.inject(
        "drag",
        {
            "from": start,
            "to": end,
            "steps": steps,
            "button": "primary",
            "coordinate_space": "points",
            "modifiers": modifiers or {},
        },
    )
    return start, end


def _activate_shape_tool(client, tool):
    client.click_component("preview.tool.shape")
    client.click_component("preview.tool." + tool)
    return client.wait_until(
        "active {} tool".format(tool),
        lambda: state
        if (state := client.state())["editor"]["preview"]["active_tool"] == tool
        else None,
    )


def _new_item(before, after):
    new_ids = set(after["project"]["items"]) - set(before["project"]["items"])
    if len(new_ids) != 1:
        raise QaFailure("creation produced {} new Timeline Items".format(len(new_ids)))
    item_id = new_ids.pop()
    return item_id, after["project"]["items"][item_id]


def _wait_rendered_revision(client, revision, old_hash=None):
    return client.wait_until(
        "Path edit Preview render",
        lambda: state
        if (state := client.state())["editor"]["preview"].get("rendered_revision")
        == revision
        and (
            old_hash is None
            or state["editor"]["preview"].get("pixel_hash") != old_hash
        )
        else None,
        timeout=30.0,
    )


def run_suite(client):
    client.wait_health()

    def fixture_ready():
        state = client.state()
        return (
            state
            if any(
                item.get("name") == "QA Path"
                for item in state["project"]["items"].values()
            )
            else None
        )

    initial = client.wait_until(
        "canonical QA Path fixture",
        fixture_ready,
        timeout=30.0,
    )
    path_item = item_by_name(initial["project"], "QA Path")
    item_id = path_item["id"]
    original = copy.deepcopy(_path_value(initial, item_id))
    if len(original["contours"]) != 1:
        raise QaFailure("Path fixture must contain exactly one editable contour")

    client.click_component("timeline.item:" + item_id)
    selected = client.wait_until(
        "QA Path Timeline selection",
        lambda: state
        if (state := client.state())["editor"]["selection"]["primary"]
        == {"kind": "timeline_item", "id": item_id}
        else None,
    )
    _wait_rendered_revision(client, selected["history"]["revision"])
    initial_hash = client.state()["editor"]["preview"]["pixel_hash"]
    _, gizmo = client.wait_component("preview.position_gizmo")
    gizmo_bounds = (gizmo.get("metadata") or {}).get("screen_bounds")
    if not gizmo_bounds:
        raise QaFailure("Path selection gizmo omitted its rendered bounds")

    client.click_component("preview.tool.shape")
    _, path_tool = client.wait_component("preview.tool.path")
    if path_tool.get("enabled") is not True:
        raise QaFailure("Path tool did not enable for a selected canonical Path clip")
    client.click_component("preview.tool.path")
    client.wait_until(
        "active Path tool",
        lambda: state
        if (state := client.state())["editor"]["preview"]["active_tool"] == "path"
        else None,
    )
    client.wait_component("preview.vector.point:0")
    client.wait_component("preview.vector.point:3")
    point_components = [
        component
        for component in client.component_snapshot()["components"]
        if component["id"].startswith("preview.vector.point:")
    ]
    for point in point_components:
        center = component_center(point)
        if not (
            gizmo_bounds["min"]["x"] - 2.0
            <= center["x"]
            <= gizmo_bounds["max"]["x"] + 2.0
            and gizmo_bounds["min"]["y"] - 2.0
            <= center["y"]
            <= gizmo_bounds["max"]["y"] + 2.0
        ):
            raise QaFailure(
                "Path control point is offset from the normal Shape gizmo: {!r}".format(
                    center
                )
            )

    # A multi-frame native drag remains ephemeral until release and commits
    # exactly one Project transaction.
    before_move = client.state()
    _drag_component(client, "preview.vector.point:0", (24.0, 14.0), steps=18)
    moved = _wait_path(
        client,
        item_id,
        "committed Path vertex move",
        lambda path, _state: path != original,
    )
    _assert_one_revision(before_move, moved, "Path vertex drag")
    moved_path = copy.deepcopy(_path_value(moved, item_id))
    original_segments = original["contours"][0]["segments"]
    moved_segments = moved_path["contours"][0]["segments"]
    if original_segments != moved_segments:
        raise QaFailure("moving vertex zero changed unrelated Path endpoints")
    rendered_move = _wait_rendered_revision(
        client, moved["history"]["revision"], initial_hash
    )

    # One Undo restores the authoritative path and a fresh renderer result.
    client.key("z", True, command=True)
    client.key("z", False, command=True)
    undone = _wait_path(
        client,
        item_id,
        "one-step Path Undo",
        lambda path, state: path == original and state["history"]["can_redo"],
    )
    _wait_rendered_revision(client, undone["history"]["revision"])

    # Double-clicking a visible segment inserts one canonical point in one
    # Project transaction; one Undo restores the exact source path.
    before_insert = client.state()
    _, point_zero = client.wait_component_settled("preview.vector.point:0")
    _, point_one = client.wait_component_settled("preview.vector.point:1")
    zero_center = component_center(point_zero)
    one_center = component_center(point_one)
    insertion = {
        "x": (zero_center["x"] + one_center["x"]) * 0.5,
        "y": (zero_center["y"] + one_center["y"]) * 0.5,
    }
    client.inject(
        "double-click",
        {**insertion, "button": "primary", "coordinate_space": "points"},
    )
    inserted = _wait_path(
        client,
        item_id,
        "inserted Path vertex",
        lambda path, _state: len(path["contours"][0]["segments"])
        == len(original["contours"][0]["segments"]) + 1,
    )
    _assert_one_revision(before_insert, inserted, "Path vertex insertion")
    _wait_rendered_revision(client, inserted["history"]["revision"])
    client.key("z", True, command=True)
    client.key("z", False, command=True)
    insert_undone = _wait_path(
        client,
        item_id,
        "one-step Path insertion Undo",
        lambda path, _state: path == original,
    )
    _wait_rendered_revision(client, insert_undone["history"]["revision"])

    # Alt-dragging a vertex creates a symmetric Bezier pair, reusing the old
    # editor gesture rather than exposing raw PathValue text in Inspector.
    before_handles = client.state()
    _drag_component(
        client,
        "preview.vector.point:0",
        (34.0, -18.0),
        modifiers={"alt": True},
    )
    with_handles = _wait_path(
        client,
        item_id,
        "Alt-created Bezier handles",
        lambda path, _state: _length(_point_zero_handles(path)[1]) > 1.0,
    )
    _assert_one_revision(before_handles, with_handles, "Alt handle creation")
    incoming, outgoing = _point_zero_handles(_path_value(with_handles, item_id))
    if not _near(incoming, [-outgoing[0], -outgoing[1]]):
        raise QaFailure("Alt-created handles were not symmetric")

    # Normal handle drag keeps symmetric coupling; Alt handle drag breaks only
    # that coupling and leaves the opposite handle untouched.
    before_coupled = client.state()
    _drag_component(client, "preview.vector.handle_out:0", (12.0, 8.0))
    coupled = _wait_path(
        client,
        item_id,
        "coupled Bezier handle drag",
        lambda path, state: state["history"]["revision"]
        == before_coupled["history"]["revision"] + 1,
    )
    _assert_one_revision(before_coupled, coupled, "coupled handle drag")
    coupled_in, coupled_out = _point_zero_handles(_path_value(coupled, item_id))
    if not _near(coupled_in, [-coupled_out[0], -coupled_out[1]]):
        raise QaFailure("symmetric handle drag did not move its opposite")

    before_break = client.state()
    _drag_component(
        client,
        "preview.vector.handle_out:0",
        (10.0, -11.0),
        modifiers={"alt": True},
    )
    broken = _wait_path(
        client,
        item_id,
        "Alt-broken Bezier handle",
        lambda path, state: state["history"]["revision"]
        == before_break["history"]["revision"] + 1,
    )
    _assert_one_revision(before_break, broken, "Alt handle break")
    broken_in, broken_out = _point_zero_handles(_path_value(broken, item_id))
    if not _near(broken_in, coupled_in) or _near(broken_out, coupled_out):
        raise QaFailure("Alt handle drag did not preserve only the opposite handle")

    # Point modes live in the vertex context menu, not in a permanently
    # crowded toolbar.
    client.click_component("preview.vector.point:0", button="secondary")
    for component_id in (
        "preview.vector.mode.corner",
        "preview.vector.mode.smooth",
        "preview.vector.mode.symmetric",
    ):
        client.wait_component(component_id)
    before_symmetric = client.state()
    client.click_component("preview.vector.mode.symmetric")
    symmetric = _wait_path(
        client,
        item_id,
        "Symmetric point mode",
        lambda path, state: state["history"]["revision"]
        == before_symmetric["history"]["revision"] + 1,
    )
    _assert_one_revision(before_symmetric, symmetric, "point mode change")
    symmetric_in, symmetric_out = _point_zero_handles(_path_value(symmetric, item_id))
    if not _near(symmetric_in, [-symmetric_out[0], -symmetric_out[1]]):
        raise QaFailure("Symmetric mode did not restore mirrored handles")

    # Escape cancels the projected drag while the pointer is still held. It
    # must not mutate Project data or allocate an Undo entry.
    before_cancel = client.state()
    before_cancel_path = copy.deepcopy(_path_value(before_cancel, item_id))
    _, point = client.wait_component_settled("preview.vector.point:0")
    start = component_center(point)
    projected = {"x": start["x"] - 28.0, "y": start["y"] + 17.0}
    client.inject(
        "press",
        {**start, "button": "primary", "coordinate_space": "points"},
    )
    client.inject("move", {**projected, "coordinate_space": "points"})
    client.wait_until(
        "ephemeral Path drag",
        lambda: metadata
        if (metadata := (client.component("preview.path.editor")[1].get("metadata") or {}))
        .get("dragging")
        else None,
    )
    client.key("escape", True)
    client.key("escape", False)
    client.inject(
        "release",
        {**projected, "button": "primary", "coordinate_space": "points"},
    )
    canceled = client.wait_until(
        "canceled Path drag",
        lambda: state
        if not (state := client.state())["editor"]["preview"]["path_editor"][
            "drag_active"
        ]
        else None,
    )
    if _path_value(canceled, item_id) != before_cancel_path:
        raise QaFailure("Escape committed an ephemeral Path drag")
    if canceled["history"]["revision"] != before_cancel["history"]["revision"]:
        raise QaFailure("Escape created a Path history transaction")
    if canceled["editor"].get("error") is not None:
        raise QaFailure(
            "Path editor left an application error: {}".format(
                canceled["editor"]["error"]
            )
        )

    # Rectangle/Ellipse/Pen are canvas gestures backed by the same basic clip
    # constructor as the Timeline menu. Each gesture creates exactly one Item
    # and one Undo transaction.
    created_items = {}
    _, content = client.wait_component_settled("preview.content")
    center = component_center(content)
    for tool, expected_kind, offset in (
        ("rectangle", "rectangle", (-145.0, -90.0)),
        ("ellipse", "ellipse", (70.0, -90.0)),
    ):
        _activate_shape_tool(client, tool)
        before_create = client.state()
        start = {"x": center["x"] + offset[0], "y": center["y"] + offset[1]}
        end = {"x": start["x"] + 84.0, "y": start["y"] + 52.0}
        client.drag(start, end, steps=12)
        created = client.wait_until(
            "{} canvas creation".format(tool),
            lambda: state
            if len((state := client.state())["project"]["items"])
            == len(before_create["project"]["items"]) + 1
            else None,
        )
        created_id, created_item = _new_item(before_create, created)
        if created_item["source"].get("kind") != "shape":
            raise QaFailure("{} tool created the wrong source".format(tool))
        shape = created_item["source"]["value"]["shape"]
        if shape.get("shape_kind") != expected_kind:
            raise QaFailure("{} tool created the wrong source".format(tool))
        _assert_one_revision(before_create, created, tool + " creation")
        _wait_rendered_revision(client, created["history"]["revision"])
        client.key("z", True, command=True)
        client.key("z", False, command=True)
        client.wait_until(
            "one-step {} creation Undo".format(tool),
            lambda: state
            if created_id not in (state := client.state())["project"]["items"]
            else None,
        )
        created_items[tool] = created_id

    _activate_shape_tool(client, "pen")
    before_pen = client.state()
    pen_points = [
        {"x": center["x"] - 110.0, "y": center["y"] + 70.0},
        {"x": center["x"] - 30.0, "y": center["y"] + 35.0},
        {"x": center["x"] + 45.0, "y": center["y"] + 85.0},
    ]
    for point in pen_points:
        client.inject(
            "click", {**point, "button": "primary", "coordinate_space": "points"}
        )
    client.wait_until(
        "three authored Pen points",
        lambda: metadata
        if (metadata := (client.wait_component("preview.pen.creation")[1].get("metadata") or {})).get(
            "point_count"
        )
        == 3
        else None,
    )
    client.inject(
        "click",
        {**pen_points[0], "button": "primary", "coordinate_space": "points"},
    )
    pen_created = client.wait_until(
        "Pen Path creation",
        lambda: state
        if len((state := client.state())["project"]["items"])
        == len(before_pen["project"]["items"]) + 1
        else None,
    )
    pen_id, pen_item = _new_item(before_pen, pen_created)
    if pen_item["source"].get("kind") != "shape":
        raise QaFailure("Pen tool did not create canonical Path geometry")
    pen_shape = pen_item["source"]["value"]["shape"]
    if pen_shape.get("shape_kind") != "path":
        raise QaFailure("Pen tool did not create canonical Path geometry")
    _assert_one_revision(before_pen, pen_created, "Pen Path creation")
    _wait_rendered_revision(client, pen_created["history"]["revision"])
    client.key("z", True, command=True)
    client.key("z", False, command=True)
    client.wait_until(
        "one-step Pen creation Undo",
        lambda: state
        if pen_id not in (state := client.state())["project"]["items"]
        else None,
    )
    created_items["pen"] = pen_id

    return {
        "suite": "path-editor",
        "item_id": item_id,
        "initial_hash": initial_hash,
        "moved_hash": rendered_move["editor"]["preview"]["pixel_hash"],
        "canvas_created_items": created_items,
        "revisions": {
            "initial": before_move["history"]["revision"],
            "moved": moved["history"]["revision"],
            "undone": undone["history"]["revision"],
            "inserted": inserted["history"]["revision"],
            "insert_undone": insert_undone["history"]["revision"],
            "handles": with_handles["history"]["revision"],
            "coupled": coupled["history"]["revision"],
            "broken": broken["history"]["revision"],
            "symmetric": symmetric["history"]["revision"],
            "canceled": canceled["history"]["revision"],
        },
        "actions": client.evidence,
    }


if __name__ == "__main__":
    raise SystemExit(
        run_suite_main(
            "qa-path-editor-e2e",
            run_suite,
            "target/qa-path-editor-e2e-evidence.json",
        )
    )
