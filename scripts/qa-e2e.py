#!/usr/bin/env python3
"""Drive RuViE through the loopback QA bridge using real egui input events.

The suite deliberately resolves every component rectangle immediately before
the click or drag that uses it. It asserts the authoritative Project returned
by /v1/state after each UI operation; no test-only model mutation endpoint is
used.
"""

import argparse
import hashlib
import json
import math
import os
import signal
import socket
import subprocess
import sys
import time
import urllib.error
import urllib.parse
import urllib.request


COMPOSITION = "00000000-0000-0000-0000-000000000100"
TRACK_A = "00000000-0000-0000-0000-000000000201"
TRACK_B = "00000000-0000-0000-0000-000000000202"
CLIP_A1 = "00000000-0000-0000-0000-000000000301"
CLIP_A2 = "00000000-0000-0000-0000-000000000302"
CLIP_B1 = "00000000-0000-0000-0000-000000000303"
SOLID = "00000000-0000-0000-0000-000000000401"
MERGE = "00000000-0000-0000-0000-000000000402"
TEXT = "00000000-0000-0000-0000-000000000403"
SHAPE = "00000000-0000-0000-0000-000000000404"
TRANSFORM_EFFECTOR = "00000000-0000-0000-0000-000000000501"
OPACITY_EFFECTOR = "00000000-0000-0000-0000-000000000502"
BACKPLATE_DECORATOR = "00000000-0000-0000-0000-000000000503"
BLUR_EFFECT = "00000000-0000-0000-0000-000000000504"
TEXT_FILL = "00000000-0000-0000-0000-000000000601"
SHAPE_FILL = "00000000-0000-0000-0000-000000000602"
SHAPE_STROKE = "00000000-0000-0000-0000-000000000603"
SHAPE_MERGE = "00000000-0000-0000-0000-000000000604"

EXPECTED_FIXTURE_NODES = frozenset(
    {
        SOLID,
        MERGE,
        TEXT,
        SHAPE,
        TRANSFORM_EFFECTOR,
        OPACITY_EFFECTOR,
        BACKPLATE_DECORATOR,
        BLUR_EFFECT,
        TEXT_FILL,
        SHAPE_FILL,
        SHAPE_STROKE,
        SHAPE_MERGE,
    }
)
EXPECTED_CLIP_NODES = {
    CLIP_A1: [SOLID, MERGE],
    CLIP_A2: [
        TEXT,
        TRANSFORM_EFFECTOR,
        OPACITY_EFFECTOR,
        BACKPLATE_DECORATOR,
        TEXT_FILL,
        BLUR_EFFECT,
    ],
    CLIP_B1: [SHAPE, SHAPE_FILL, SHAPE_STROKE, SHAPE_MERGE],
}
EXPECTED_CLIP_OUTPUTS = {
    CLIP_A1: MERGE,
    CLIP_A2: BLUR_EFFECT,
    CLIP_B1: SHAPE_MERGE,
}
EXPECTED_OPERATIONS = {
    TRANSFORM_EFFECTOR: ("effector", "transform", "effector.apply.v1"),
    OPACITY_EFFECTOR: ("effector", "opacity", "effector.apply.v1"),
    BACKPLATE_DECORATOR: ("decorator", "backplate", "decorator.apply.v1"),
    BLUR_EFFECT: ("effect", "blur", "effect.apply.v1"),
    TEXT_FILL: ("style", "fill", "style.apply.v1"),
    SHAPE_FILL: ("style", "fill", "style.apply.v1"),
    SHAPE_STROKE: ("style", "stroke", "style.apply.v1"),
}


class QaFailure(RuntimeError):
    pass


def expected_pointer_frames(endpoint, payload):
    """Describe the per-frame pointer sequence emitted by the QA bridge."""
    if endpoint == "click":
        point = {"x": payload["x"], "y": payload["y"]}
        return [
            {"kind": "settle", "point": point},
            {"kind": "press", "point": point},
            {"kind": "release", "point": point},
        ]
    if endpoint == "double-click":
        point = {"x": payload["x"], "y": payload["y"]}
        return [
            {"kind": "settle", "point": point},
            {
                "kind": "double_click",
                "point": point,
                "events": ["press", "release", "press", "release"],
            },
        ]
    if endpoint != "drag":
        return None
    start = payload["from"]
    end = payload["to"]
    step_count = payload.get("steps", 8)
    frames = [
        {"kind": "settle", "point": dict(start)},
        {"kind": "press", "point": dict(start)},
    ]
    for index in range(1, step_count + 1):
        factor = index / step_count
        frames.append(
            {
                "kind": "move",
                "step": index,
                "point": {
                    "x": start["x"] + (end["x"] - start["x"]) * factor,
                    "y": start["y"] + (end["y"] - start["y"]) * factor,
                },
            }
        )
    frames.append({"kind": "release", "point": dict(end)})
    return frames


class QaClient:
    def __init__(self, base_url, timeout=8.0):
        self.base_url = base_url.rstrip("/")
        self.timeout = timeout
        self.evidence = []

    def request(self, path, data=None, method=None):
        body = None if data is None else json.dumps(data).encode("utf-8")
        request = urllib.request.Request(
            self.base_url + path,
            data=body,
            headers={"Content-Type": "application/json"},
            method=method,
        )
        try:
            with urllib.request.urlopen(request, timeout=3.0) as response:
                return json.load(response)
        except urllib.error.HTTPError as error:
            payload = error.read().decode("utf-8", errors="replace")
            raise QaFailure(
                "HTTP {} for {}: {}".format(error.code, path, payload)
            ) from error
        except OSError as error:
            raise QaFailure("request {} failed: {}".format(path, error)) from error

    def wait_health(self):
        deadline = time.monotonic() + self.timeout
        last_error = None
        while time.monotonic() < deadline:
            try:
                health = self.request("/health")
                if health.get("ok"):
                    return health
            except QaFailure as error:
                last_error = error
            time.sleep(0.05)
        raise QaFailure("QA bridge did not become healthy: {}".format(last_error))

    def state(self):
        return self.request("/v1/state")

    def capture(self, output_path):
        # The capture endpoint deliberately has an empty-body POST contract.
        # Passing `{}` here is not equivalent: it serializes two body bytes and
        # the loopback bridge rejects it as a malformed capture request.
        queued = self.request("/v1/captures", method="POST")
        capture_id = queued["capture_id"]

        def ready():
            status = self.request("/v1/captures/{}".format(capture_id))
            if status.get("phase") == "failed":
                raise QaFailure(
                    "capture {} failed: {}".format(capture_id, status.get("error"))
                )
            return status if status.get("phase") == "ready" else None

        status = self.wait_until("capture {} readiness".format(capture_id), ready)
        request = urllib.request.Request(
            self.base_url + "/v1/captures/{}.png".format(capture_id)
        )
        try:
            with urllib.request.urlopen(request, timeout=5.0) as response:
                png = response.read()
        except (urllib.error.HTTPError, OSError) as error:
            raise QaFailure("capture {} PNG read failed: {}".format(capture_id, error)) from error
        actual_sha = hashlib.sha256(png).hexdigest()
        if actual_sha != status.get("sha256"):
            raise QaFailure(
                "capture {} SHA-256 mismatch: {} != {}".format(
                    capture_id, actual_sha, status.get("sha256")
                )
            )
        absolute_path = os.path.abspath(output_path)
        os.makedirs(os.path.dirname(absolute_path), exist_ok=True)
        with open(absolute_path, "wb") as output:
            output.write(png)
        result = dict(status)
        result["path"] = absolute_path
        return result

    def component_snapshot(self):
        return self.request("/v1/components")

    def component(self, component_id, require_visible=True):
        snapshot = self.component_snapshot()
        component = next(
            (item for item in snapshot["components"] if item["id"] == component_id),
            None,
        )
        if component is None:
            raise QaFailure(
                "component {!r} is absent in frame {}".format(
                    component_id, snapshot["frame"]
                )
            )
        rect = component["rect_points"]
        if not component.get("enabled", False):
            raise QaFailure("component {!r} is disabled".format(component_id))
        if require_visible and (
            not component.get("visible", False)
            or rect["width"] <= 0.0
            or rect["height"] <= 0.0
        ):
            raise QaFailure("component {!r} has no visible rectangle".format(component_id))
        return snapshot, component

    def wait_until(self, description, predicate, timeout=None):
        deadline = time.monotonic() + (self.timeout if timeout is None else timeout)
        last_value = None
        while time.monotonic() < deadline:
            last_value = predicate()
            if last_value:
                return last_value
            time.sleep(0.04)
        raise QaFailure(
            "timed out waiting for {}; last value={!r}".format(description, last_value)
        )

    def wait_component(self, component_id):
        def find():
            try:
                return self.component(component_id)
            except QaFailure:
                return None

        return self.wait_until("component {}".format(component_id), find)

    def wait_component_settled(self, component_id, consecutive_reads=2, tolerance=0.05):
        observed = {"frame": None, "rect": None, "stable": 0}

        def settled():
            try:
                # `/v1/state` schedules a UI pass. Geometry must be stable over
                # distinct completed frames; repeated reads of one stale
                # registry snapshot are not evidence that layout settled.
                self.state()
                snapshot, component = self.component(component_id)
            except QaFailure:
                return None
            if snapshot["frame"] == observed["frame"]:
                return None
            rect = component["rect_points"]
            current = (
                float(rect["min_x"]),
                float(rect["min_y"]),
                float(rect["max_x"]),
                float(rect["max_y"]),
            )
            previous = observed["rect"]
            observed["frame"] = snapshot["frame"]
            observed["rect"] = current
            if previous is not None and all(
                abs(before - after) <= tolerance
                for before, after in zip(previous, current)
            ):
                observed["stable"] += 1
            else:
                observed["stable"] = 0
            return (snapshot, component) if observed["stable"] >= consecutive_reads else None

        return self.wait_until("settled component {}".format(component_id), settled)

    def inject(self, endpoint, payload, evidence=None):
        component_frame_before = self.component_snapshot()["frame"]
        queued = self.request("/v1/input/" + endpoint, payload)
        action_id = queued["action_id"]

        def injected():
            status = self.request("/v1/actions/{}".format(action_id))
            return status if status.get("phase") == "injected" else None

        status = self.wait_until("action {} injection".format(action_id), injected)

        def completed_frame():
            state = self.state()
            return state if state.get("frame", 0) > component_frame_before else None

        completed = self.wait_until(
            "action {} completed UI frame".format(action_id), completed_frame
        )
        record = {
            "action_id": action_id,
            "endpoint": endpoint,
            "payload": payload,
            "phase": status["phase"],
            "completed_frame": completed["frame"],
        }
        pointer_frames = expected_pointer_frames(endpoint, payload)
        if pointer_frames is not None:
            record["pointer_frames"] = pointer_frames
        if evidence:
            record.update(evidence)
        self.evidence.append(record)
        return action_id

    @staticmethod
    def point(rect, x_fraction=0.5, y_fraction=0.5):
        return {
            "x": rect["min_x"] + rect["width"] * x_fraction,
            "y": rect["min_y"] + rect["height"] * y_fraction,
        }

    def click_component(self, component_id, button="primary", x_fraction=0.5, y_fraction=0.5):
        # This query is intentionally adjacent to input injection. Stale
        # rectangles from an earlier layout frame must never drive E2E input.
        snapshot, component = self.component(component_id)
        point = self.point(component["rect_points"], x_fraction, y_fraction)
        self.inject(
            "click",
            {
                "x": point["x"],
                "y": point["y"],
                "coordinate_space": "points",
                "button": button,
            },
            {
                "component_id": component_id,
                "component_frame": snapshot["frame"],
                "component_rect_points": component["rect_points"],
            },
        )
        return point

    def double_click_component(
        self, component_id, button="primary", x_fraction=0.5, y_fraction=0.5
    ):
        snapshot, component = self.component(component_id)
        point = self.point(component["rect_points"], x_fraction, y_fraction)
        self.inject(
            "double-click",
            {
                "x": point["x"],
                "y": point["y"],
                "coordinate_space": "points",
                "button": button,
            },
            {
                "component_id": component_id,
                "component_frame": snapshot["frame"],
                "component_rect_points": component["rect_points"],
            },
        )
        return point

    def drag_components(
        self,
        source_id,
        target_id,
        steps=12,
        source_fraction=(0.5, 0.5),
        target_fraction=(0.5, 0.5),
    ):
        # Resolve both endpoints from one latest complete frame.
        snapshot = self.component_snapshot()
        components = {item["id"]: item for item in snapshot["components"]}
        try:
            source = components[source_id]
            target = components[target_id]
        except KeyError as error:
            raise QaFailure(
                "drag component {} absent in frame {}".format(error, snapshot["frame"])
            ) from error
        for role, component_id, component in (
            ("source", source_id, source),
            ("target", target_id, target),
        ):
            rect = component["rect_points"]
            if (
                not component.get("enabled", False)
                or not component.get("visible", False)
                or rect["width"] <= 0.0
                or rect["height"] <= 0.0
            ):
                raise QaFailure(
                    "drag {} component {!r} is not visible in frame {}".format(
                        role, component_id, snapshot["frame"]
                    )
                )
        start = self.point(source["rect_points"], *source_fraction)
        end = self.point(target["rect_points"], *target_fraction)
        self.inject(
            "drag",
            {
                "from": start,
                "to": end,
                "coordinate_space": "points",
                "steps": steps,
                "button": "primary",
            },
            {
                "source_component_id": source_id,
                "target_component_id": target_id,
                "component_frame": snapshot["frame"],
                "source_rect_points": source["rect_points"],
                "target_rect_points": target["rect_points"],
            },
        )
        return start, end

    def drag_component_by(self, component_id, dx, dy, steps=12):
        snapshot, component = self.component(component_id)
        start = self.point(component["rect_points"])
        end = {"x": start["x"] + dx, "y": start["y"] + dy}
        self.inject(
            "drag",
            {
                "from": start,
                "to": end,
                "coordinate_space": "points",
                "steps": steps,
                "button": "primary",
            },
            {
                "source_component_id": component_id,
                "component_frame": snapshot["frame"],
                "source_rect_points": component["rect_points"],
            },
        )
        return start, end

    def drag_timeline_by_seconds(self, clip_id, target_id, seconds, steps=12):
        """Drag a Clip body/edge by an exact time delta from fresh geometry."""
        snapshot = self.component_snapshot()
        components = {item["id"]: item for item in snapshot["components"]}
        clip_component_id = "timeline.clip:" + clip_id
        try:
            clip_component = components[clip_component_id]
            target = components[target_id]
        except KeyError as error:
            raise QaFailure(
                "timeline drag component {} absent in frame {}".format(
                    error, snapshot["frame"]
                )
            ) from error
        for component_id, component in (
            (clip_component_id, clip_component),
            (target_id, target),
        ):
            rect = component["rect_points"]
            if (
                not component.get("enabled", False)
                or not component.get("visible", False)
                or rect["width"] <= 0.0
                or rect["height"] <= 0.0
            ):
                raise QaFailure(
                    "timeline drag component {!r} is not interactive".format(
                        component_id
                    )
                )
        clip_metadata = clip_component.get("metadata") or {}
        pixels_per_second = float(clip_metadata.get("pixels_per_second", 0.0))
        if not math.isfinite(pixels_per_second) or pixels_per_second <= 0.0:
            raise QaFailure("Timeline Clip omitted usable time geometry")
        dx = float(seconds) * pixels_per_second
        start = self.point(target["rect_points"])
        end = {"x": start["x"] + dx, "y": start["y"]}
        self.inject(
            "drag",
            {
                "from": start,
                "to": end,
                "coordinate_space": "points",
                "steps": steps,
                "button": "primary",
            },
            {
                "source_component_id": target_id,
                "geometry_component_id": clip_component_id,
                "component_frame": snapshot["frame"],
                "source_rect_points": target["rect_points"],
                "geometry_rect_points": clip_component["rect_points"],
                "expected_delta_seconds": float(seconds),
                "pixels_per_second": pixels_per_second,
                "coordinate_reason": "authoritative Timeline pixels_per_second",
            },
        )
        return {
            "start": start,
            "end": end,
            "pixels_per_second": pixels_per_second,
            "delta_seconds": float(seconds),
        }

    def drag_component_to_row(self, source_id, target_id, steps=12):
        """Drag to a target row while preserving the source's screen x.

        Timeline insertion slots span their whole pane. Keeping x fixed makes
        an ordering gesture independent from the Clip time axis.
        """
        snapshot = self.component_snapshot()
        components = {item["id"]: item for item in snapshot["components"]}
        try:
            source = components[source_id]
            target = components[target_id]
        except KeyError as error:
            raise QaFailure(
                "drag component {} absent in frame {}".format(error, snapshot["frame"])
            ) from error
        for component_id, component in ((source_id, source), (target_id, target)):
            rect = component["rect_points"]
            if not component.get("enabled", False):
                raise QaFailure("component {!r} is disabled".format(component_id))
            if (
                not component.get("visible", False)
                or rect["width"] <= 0.0
                or rect["height"] <= 0.0
            ):
                raise QaFailure(
                    "component {!r} has no visible rectangle".format(component_id)
                )
        start = self.point(source["rect_points"])
        target_rect = target["rect_points"]
        if not target_rect["min_x"] <= start["x"] <= target_rect["max_x"]:
            raise QaFailure(
                "source x is outside target row for {} -> {}".format(source_id, target_id)
            )
        end = {"x": start["x"], "y": target_rect["center_y"]}
        self.inject(
            "drag",
            {
                "from": start,
                "to": end,
                "coordinate_space": "points",
                "steps": steps,
                "button": "primary",
            },
            {
                "source_component_id": source_id,
                "target_component_id": target_id,
                "component_frame": snapshot["frame"],
                "source_rect_points": source["rect_points"],
                "target_rect_points": target_rect,
                "coordinate_reason": "preserve source x for an ordering-only gesture",
            },
        )
        return start, end

    def key(self, key, pressed, command=False, shift=False):
        self.inject(
            "key",
            {
                "key": key,
                "pressed": pressed,
                "modifiers": {"command": command, "shift": shift},
            },
        )

    def replace_text(self, component_id, value):
        self.ensure_in_scroll_area(component_id)
        self.replace_component_text(component_id, value)

    def replace_component_text(self, component_id, value):
        """Replace a real text control that is not inside the Inspector scroll area."""
        self.click_component(component_id)
        self.key("a", True, command=True)
        self.key("a", False, command=True)
        self.inject("text", {"text": value})

    def ensure_in_scroll_area(self, component_id, max_attempts=16):
        for _ in range(max_attempts):
            snapshot = self.component_snapshot()
            items = {item["id"]: item for item in snapshot["components"]}
            target = items.get(component_id)
            area = items.get("inspector.scroll_area")
            if target is None or area is None:
                time.sleep(0.04)
                continue
            target_rect = target["rect_points"]
            area_rect = area["rect_points"]
            margin = 4.0
            if (
                target_rect["min_y"] >= area_rect["min_y"] + margin
                and target_rect["max_y"] <= area_rect["max_y"] - margin
            ):
                return target
            delta_y = -260.0 if target_rect["center_y"] > area_rect["center_y"] else 260.0
            point = self.point(area_rect)
            self.inject(
                "scroll",
                {
                    "x": point["x"],
                    "y": point["y"],
                    "delta_x": 0.0,
                    "delta_y": delta_y,
                },
                {
                    "component_id": "inspector.scroll_area",
                    "target_component_id": component_id,
                    "component_frame": snapshot["frame"],
                    "component_rect_points": area_rect,
                },
            )
        raise QaFailure("could not scroll {} into the Inspector viewport".format(component_id))

    def wait_project(self, description, predicate):
        def check():
            state = self.state()
            return state if predicate(state["project"]) else None

        return self.wait_until(description, check)

    def wait_preview_change(self, previous_hash, previous_revision):
        def changed():
            state = self.state()
            preview = state["editor"]["preview"]
            if (
                preview["render_revision"] > previous_revision
                and preview["pixel_hash"] is not None
                and preview["pixel_hash"] != previous_hash
                and preview["nontransparent_pixels"] is not None
                and preview["nontransparent_pixels"] > 0
                and preview["modal_error"] is None
            ):
                return state
            return None

        return self.wait_until("a non-transparent changed Preview image", changed)

    def wait_preview_render_after(self, project_state, operation):
        """Wait for a valid Preview result completed after a Project snapshot."""
        previous_revision = project_state["editor"]["preview"]["render_revision"]

        def rendered():
            state = self.state()
            preview = state["editor"]["preview"]
            if (
                preview["render_revision"] > previous_revision
                and preview["pixel_hash"] is not None
                and preview["nontransparent_pixels"] is not None
                and preview["nontransparent_pixels"] > 0
                and preview["modal_error"] is None
            ):
                return state
            return None

        return self.wait_until(
            "a valid Preview render after {}".format(operation), rendered
        )

    def wait_preview_settled(self, operation, consecutive_frames=2):
        observed = {"frame": None, "signature": None, "stable": 0}

        def settled():
            state = self.state()
            if state["frame"] == observed["frame"]:
                return None
            preview = state["editor"]["preview"]
            signature = (
                preview["pixel_hash"],
                preview["nontransparent_pixels"],
                preview["modal_error"],
            )
            if signature == observed["signature"]:
                observed["stable"] += 1
            else:
                observed["stable"] = 0
            observed["frame"] = state["frame"]
            observed["signature"] = signature
            if observed["stable"] < consecutive_frames:
                return None
            assert_valid_preview(state, operation)
            return state

        return self.wait_until("settled Preview after {}".format(operation), settled)


def property_value(node, name):
    return node["properties"][name]["properties"]["value"]


def history_depth(state):
    return state["history"]["undo_depth"]


def node_editor_layout_positions(project):
    """Capture positions that local creation must never globally reflow."""
    return {
        "compositions": {
            item["id"]: item["ui_position"] for item in project["compositions"]
        },
        "tracks": {
            item_id: item["ui_position"] for item_id, item in project["tracks"].items()
        },
        "clips": {
            item_id: item["ui_position"] for item_id, item in project["clips"].items()
        },
        "nodes": {
            item_id: item["ui_position"] for item_id, item in project["nodes"].items()
        },
    }


def assert_history_delta(before, after, expected, operation):
    actual = history_depth(after) - history_depth(before)
    if actual != expected:
        raise QaFailure(
            "{} history delta was {}, expected {}".format(operation, actual, expected)
        )


def assert_close(actual, expected, description, tolerance=1.0e-5):
    if abs(actual - expected) > tolerance:
        raise QaFailure(
            "{} was {}, expected {}".format(description, actual, expected)
        )


def selection_target(kind, entity_id):
    return {"kind": kind, "id": entity_id}


def selection_matches(state, kind, entity_id):
    return state["editor"]["selection"]["primary"] == selection_target(
        kind, entity_id
    )


def assert_exact_selection(state, kind, entity_id, operation):
    expected = selection_target(kind, entity_id)
    selection = state["editor"]["selection"]
    if selection["primary"] != expected or selection["targets"] != [expected]:
        raise QaFailure(
            "{} produced non-exact typed selection {!r}".format(operation, selection)
        )


def assert_selection(state, entity_id, track_id, operation):
    selection = state["editor"]["selection"]
    expected = selection_target("clip", entity_id)
    if selection["primary"] != expected:
        raise QaFailure("{} selected the wrong entity".format(operation))
    if expected not in selection["targets"]:
        raise QaFailure("{} omitted the selected entity from selection".format(operation))
    owners = [
        candidate_id
        for candidate_id, track in state["project"]["tracks"].items()
        if entity_id in track.get("clip_ids", [])
    ]
    if owners != [track_id]:
        raise QaFailure(
            "{} has canonical Track owners {}, expected {}".format(
                operation, owners, track_id
            )
        )


def assert_valid_preview(state, operation):
    preview = state["editor"]["preview"]
    if (
        preview["render_revision"] <= 0
        or preview["pixel_hash"] is None
        or preview["nontransparent_pixels"] is None
        or preview["nontransparent_pixels"] <= 0
        or preview["modal_error"] is not None
    ):
        raise QaFailure("{} did not leave a valid Preview".format(operation))
    return preview


def activate_dock_tab(client, tab_id, tab_name, operation):
    client.wait_component_settled(tab_id)
    client.click_component(tab_id)
    return client.wait_until(
        "{} dock activation".format(operation),
        lambda: state
        if tab_name in (state := client.state())["dock"]["active_tabs"]
        else None,
    )


def assert_inspector_clip_timing(client, clip_id, track_id, expected_clip):
    """Read Clip timing/owner UI derived from the authoritative Project."""
    owner_id = "inspector.owner.clip:" + clip_id
    _, owner = client.wait_component(owner_id)
    owner_metadata = owner.get("metadata") or {}
    if not (
        owner_metadata.get("owner") == "clip"
        and owner_metadata.get("id") == clip_id
        and owner_metadata.get("track_id") == track_id
    ):
        raise QaFailure("Inspector Clip owner metadata is stale")

    fps = None
    for property_name in ("start_time", "duration", "trim_in", "time_stretch"):
        component_id = "inspector.property.clip:{}:{}".format(
            clip_id, property_name
        )
        _, component = client.wait_component(component_id)
        metadata = component.get("metadata") or {}
        if metadata.get("scope") != "clip:" + clip_id:
            raise QaFailure("Inspector timing control has stale scope")
        if metadata.get("property") != property_name:
            raise QaFailure("Inspector timing control names the wrong property")
        assert_close(
            float(metadata.get("value")),
            float(expected_clip[property_name]),
            "Inspector {} value".format(property_name),
        )
        definition = metadata.get("definition") or {}
        if definition.get("name") != property_name:
            raise QaFailure("Inspector timing control omitted authoritative metadata")
        component_fps = float(metadata.get("fps", 0.0))
        if component_fps <= 0.0:
            raise QaFailure("Inspector timing control has invalid FPS metadata")
        if fps is None:
            fps = component_fps
        else:
            assert_close(component_fps, fps, "Inspector timing FPS")

        if property_name == "duration":
            expected_display = (
                expected_clip["start_time"] + expected_clip["duration"]
            ) * component_fps
            expected_semantics = "out_frame"
        elif property_name in ("start_time", "trim_in"):
            expected_display = expected_clip[property_name] * component_fps
            expected_semantics = "frame"
        else:
            expected_display = expected_clip[property_name]
            expected_semantics = "ratio"
        assert_close(
            float(metadata.get("display_value")),
            float(expected_display),
            "Inspector {} display value".format(property_name),
        )
        if metadata.get("display_semantics") != expected_semantics:
            raise QaFailure("Inspector timing display semantics are stale")
    return fps


def composition_map(project):
    return {item["id"]: item for item in project["compositions"]}


def validate_canonical_ownership(project):
    """Validate that the fixture remains one authoritative Project graph."""
    compositions = composition_map(project)
    track_owners = {track_id: [] for track_id in project["tracks"]}
    clip_owners = {clip_id: [] for clip_id in project["clips"]}
    node_owners = {node_id: [] for node_id in project["nodes"]}

    for composition_id, composition in compositions.items():
        if len(composition["track_ids"]) != len(set(composition["track_ids"])):
            raise QaFailure("Composition {} contains a duplicate Track".format(composition_id))
        for track_id in composition["track_ids"]:
            if track_id not in track_owners:
                raise QaFailure("Composition references missing Track {}".format(track_id))
            track_owners[track_id].append(composition_id)
        for node_id in composition["node_ids"]:
            if node_id not in node_owners:
                raise QaFailure("Composition references missing Node {}".format(node_id))
            node_owners[node_id].append("composition:" + composition_id)
        output = composition.get("output_node_id")
        if output is not None and output not in composition["node_ids"]:
            raise QaFailure("Composition output is not a directly contained Node")

    for track_id, track in project["tracks"].items():
        if len(track["clip_ids"]) != len(set(track["clip_ids"])):
            raise QaFailure("Track {} contains a duplicate Clip".format(track_id))
        for clip_id in track["clip_ids"]:
            if clip_id not in clip_owners:
                raise QaFailure("Track references missing Clip {}".format(clip_id))
            clip_owners[clip_id].append(track_id)
        for node_id in track["node_ids"]:
            if node_id not in node_owners:
                raise QaFailure("Track references missing Node {}".format(node_id))
            node_owners[node_id].append("track:" + track_id)
        output = track.get("output_node_id")
        if output is not None and output not in track["node_ids"]:
            raise QaFailure("Track output is not a directly contained Node")

    for clip_id, clip in project["clips"].items():
        if len(clip["node_ids"]) != len(set(clip["node_ids"])):
            raise QaFailure("Clip {} contains a duplicate Node".format(clip_id))
        if clip["start_time"] < 0.0 or clip["duration"] <= 0.0:
            raise QaFailure("Clip {} has an invalid time range".format(clip_id))
        if clip["trim_in"] < 0.0 or clip["time_stretch"] < 0.0:
            raise QaFailure("Clip {} has invalid source timing metadata".format(clip_id))
        for node_id in clip["node_ids"]:
            if node_id not in node_owners:
                raise QaFailure("Clip references missing Node {}".format(node_id))
            node_owners[node_id].append("clip:" + clip_id)
        output = clip.get("output_node_id")
        if output is not None and output not in clip["node_ids"]:
            raise QaFailure("Clip output is not a directly contained Node")

    def require_single_owner(owners, kind):
        for item_id, item_owners in owners.items():
            if len(item_owners) != 1:
                raise QaFailure(
                    "{} {} has {} canonical owners: {}".format(
                        kind, item_id, len(item_owners), item_owners
                    )
                )

    require_single_owner(track_owners, "Track")
    require_single_owner(clip_owners, "Clip")
    require_single_owner(node_owners, "Node")
    return {
        "track_owners": {key: value[0] for key, value in track_owners.items()},
        "clip_owners": {key: value[0] for key, value in clip_owners.items()},
        "node_owners": {key: value[0] for key, value in node_owners.items()},
    }


def timeline_invariant_signature(project):
    owners = validate_canonical_ownership(project)
    project_static = {
        key: value
        for key, value in project.items()
        if key not in {"compositions", "tracks", "clips", "nodes", "connections"}
    }
    composition_static = {}
    for item_id, item in composition_map(project).items():
        composition_static[item_id] = {
            key: value for key, value in item.items() if key != "track_ids"
        }
    track_static = {}
    for item_id, item in project["tracks"].items():
        track_static[item_id] = {
            key: value for key, value in item.items() if key != "clip_ids"
        }
    clip_static = {}
    for item_id, item in project["clips"].items():
        clip_static[item_id] = {
            key: value
            for key, value in item.items()
            if key not in {"start_time", "duration", "trim_in"}
        }
    return {
        "project_static": project_static,
        "composition_static": composition_static,
        "track_static": track_static,
        "clip_static": clip_static,
        "nodes": project["nodes"],
        "connections": project["connections"],
        **owners,
    }


def assert_timeline_integrity(
    before_project, after_project, operation, clip_owner_changes=None
):
    before = timeline_invariant_signature(before_project)
    after = timeline_invariant_signature(after_project)
    expected_clip_owners = dict(before["clip_owners"])
    expected_clip_owners.update(clip_owner_changes or {})
    before["clip_owners"] = expected_clip_owners
    for category in before:
        if before[category] != after[category]:
            raise QaFailure(
                "{} unexpectedly changed {}".format(operation, category)
            )


def wait_timeline_edit(client, before, description, predicate):
    before_depth = history_depth(before)

    def changed_once():
        state = client.state()
        if not predicate(state["project"]):
            return None
        delta = history_depth(state) - before_depth
        if delta > 1:
            raise QaFailure(
                "{} produced {} history entries".format(description, delta)
            )
        return state if delta == 1 else None

    return client.wait_until(
        "{} and exactly one history entry".format(description), changed_once
    )


def point_near_node_wire(point, component, radius=12.0):
    metadata = component.get("metadata") or {}
    source = metadata.get("from") or {}
    target = metadata.get("to") or {}
    if not all(key in source and key in target for key in ("x", "y")):
        return False
    start = (float(source["x"]), float(source["y"]))
    end = (float(target["x"]), float(target["y"]))
    minimum_frame = 2.0 if metadata.get("overview_painted") else 36.0
    frame = min(110.0, max(minimum_frame, abs(end[0] - start[0]) * 0.45))
    control_a = (start[0] + frame, start[1])
    control_b = (end[0] - frame, end[1])

    def bezier(factor):
        inverse = 1.0 - factor
        weights = (
            inverse**3,
            3.0 * inverse**2 * factor,
            3.0 * inverse * factor**2,
            factor**3,
        )
        return (
            start[0] * weights[0]
            + control_a[0] * weights[1]
            + control_b[0] * weights[2]
            + end[0] * weights[3],
            start[1] * weights[0]
            + control_a[1] * weights[1]
            + control_b[1] * weights[2]
            + end[1] * weights[3],
        )

    def segment_distance(left, right):
        dx = right[0] - left[0]
        dy = right[1] - left[1]
        length_squared = dx * dx + dy * dy
        if length_squared <= 1.0e-12:
            return ((point["x"] - left[0]) ** 2 + (point["y"] - left[1]) ** 2) ** 0.5
        factor = min(
            1.0,
            max(
                0.0,
                ((point["x"] - left[0]) * dx + (point["y"] - left[1]) * dy)
                / length_squared,
            ),
        )
        nearest = (left[0] + factor * dx, left[1] + factor * dy)
        return (
            (point["x"] - nearest[0]) ** 2 + (point["y"] - nearest[1]) ** 2
        ) ** 0.5

    previous = start
    for sample in range(1, 33):
        current = bezier(sample / 32.0)
        if segment_distance(previous, current) <= radius:
            return True
        previous = current
    return False


def find_free_canvas_point(client, scope_component_id=None):
    snapshot = client.component_snapshot()
    components = snapshot["components"]
    canvas_component = next(
        item for item in components if item["id"] == "node_editor.canvas"
    )
    canvas = canvas_component["rect_points"]
    scope_component_id = scope_component_id or "node_editor.container.composition:{}".format(
        COMPOSITION
    )
    containers = [item for item in components if item["id"] == scope_component_id]
    search = containers[0]["rect_points"] if containers else canvas
    obstacles = [
        item["rect_points"]
        for item in components
        if item["id"].startswith("node_editor.node:")
        or item["id"].startswith("node_editor.container_header.")
        or (
            item["id"].startswith(("node_editor.edge:", "node_editor.edge."))
            and (item.get("metadata") or {}).get("editable") is True
        )
    ]
    wires = [
        item
        for item in components
        if (item.get("metadata") or {}).get("kind")
        in ("explicit", "output_binding", "derived_output")
    ]
    for y_step in range(1, 10):
        for x_step in range(1, 14):
            point = {
                "x": search["min_x"] + search["width"] * x_step / 14.0,
                "y": search["min_y"] + search["height"] * y_step / 10.0,
            }
            if not (
                canvas["min_x"] + 4.0 <= point["x"] <= canvas["max_x"] - 4.0
                and canvas["min_y"] + 4.0 <= point["y"] <= canvas["max_y"] - 4.0
            ):
                continue
            if all(
                not (
                    rect["min_x"] - 5.0 <= point["x"] <= rect["max_x"] + 5.0
                    and rect["min_y"] - 5.0 <= point["y"] <= rect["max_y"] + 5.0
                )
                for rect in obstacles
            ) and all(not point_near_node_wire(point, wire) for wire in wires):
                return snapshot, point
    raise QaFailure("no unobstructed point was found in the Node Editor canvas")


def open_create_menu(client, scope_component_id=None, operation="Node Editor create menu"):
    snapshot, point = find_free_canvas_point(client, scope_component_id)
    client.inject(
        "click",
        {
            "x": point["x"],
            "y": point["y"],
            "coordinate_space": "points",
            "button": "secondary",
        },
        {
            "component_id": "node_editor.canvas",
            "component_frame": snapshot["frame"],
            "coordinate_reason": "unobstructed composition-container interior",
        },
    )
    client.wait_until(
        operation,
        lambda: client.state()
        if client.state()["editor"]["node_editor"]["context_menu_open"]
        else None,
    )
    return point


def project_connection(project, connection_id):
    return next(
        (
            connection
            for connection in project.get("connections", ())
            if connection.get("id") == connection_id
        ),
        None,
    )


def find_project_connection(
    project,
    from_owner_type,
    from_owner_id,
    from_port,
    to_owner_type,
    to_owner_id,
    to_port,
):
    def address_matches(address, owner_type, owner_id, port):
        owner = address.get("owner", {})
        return (
            owner.get("owner_type") == owner_type
            and owner.get("owner_id") == owner_id
            and address.get("port") == port
        )

    matches = [
        connection
        for connection in project.get("connections", ())
        if address_matches(
            connection.get("from", {}), from_owner_type, from_owner_id, from_port
        )
        and address_matches(
            connection.get("to", {}), to_owner_type, to_owner_id, to_port
        )
    ]
    if len(matches) != 1:
        raise QaFailure(
            "expected one {}:{} -> {}:{} connection, found {}".format(
                from_owner_id, from_port, to_owner_id, to_port, len(matches)
            )
        )
    return matches[0]


def undo_project_edit(client, description, predicate):
    client.key("z", True, command=True)
    client.key("z", False, command=True)
    return client.wait_project(description + " undo", predicate)


def redo_project_edit(client, description, predicate):
    client.key("z", True, command=True, shift=True)
    client.key("z", False, command=True, shift=True)
    return client.wait_project(description + " redo", predicate)


def wait_preview_hash_after(client, expected_hash, previous_revision, operation):
    def rendered():
        state = client.state()
        preview = state["editor"]["preview"]
        if (
            preview["render_revision"] > previous_revision
            and preview["pixel_hash"] == expected_hash
            and preview["nontransparent_pixels"] is not None
            and preview["nontransparent_pixels"] > 0
            and preview["modal_error"] is None
        ):
            return state
        return None

    return client.wait_until(
        "Preview hash restored after {}".format(operation), rendered
    )


def click_disabled_component(client, component_id):
    """Inject a real coordinate click into a disabled QA component."""
    snapshot = client.component_snapshot()
    component = next(
        (item for item in snapshot["components"] if item["id"] == component_id),
        None,
    )
    if component is None:
        raise QaFailure("disabled component {!r} is absent".format(component_id))
    if component.get("enabled", True):
        raise QaFailure("component {!r} should be disabled".format(component_id))
    rect = component["rect_points"]
    if not component.get("visible", False) or rect["width"] <= 0 or rect["height"] <= 0:
        raise QaFailure("disabled component {!r} has no hit rectangle".format(component_id))
    point = client.point(rect)
    client.inject(
        "click",
        {
            "x": point["x"],
            "y": point["y"],
            "coordinate_space": "points",
            "button": "primary",
        },
        {
            "component_id": component_id,
            "component_frame": snapshot["frame"],
            "component_rect_points": rect,
            "expected_disabled_no_op": True,
        },
    )
    client.wait_until(
        "disabled {} click input frames".format(component_id),
        lambda: (
            current
            if (current := client.component_snapshot())["frame"] > snapshot["frame"]
            else None
        ),
    )


def create_node_from_add_search(client, query, item_id, scope_component_id=None):
    before = client.state()
    node_ids_before = set(before["project"]["nodes"])
    open_create_menu(
        client,
        scope_component_id,
        "Node Editor create menu for {}".format(query),
    )
    client.wait_component("node_editor.menu.search")
    client.replace_component_text("node_editor.menu.search", query)
    _, item = client.wait_component_settled(item_id)
    client.click_component(item_id)
    created = client.wait_project(
        "{} creation through searchable Add menu".format(query),
        lambda project: len(project["nodes"]) == len(node_ids_before) + 1,
    )
    node_id = (set(created["project"]["nodes"]) - node_ids_before).pop()
    return node_id, created, item.get("metadata") or {}


def delete_node_through_context_menu(client, node_id):
    header_id = "node_editor.node_header:" + node_id
    reveal_node_editor_component(client, header_id)
    client.click_component(header_id, button="secondary")
    delete_id = "node_editor.menu.delete.node:" + node_id
    client.wait_component(delete_id)
    client.click_component(delete_id)
    return client.wait_project(
        "temporary Node {} deletion".format(node_id),
        lambda project: node_id not in project["nodes"],
    )


def point_in_component_rect(point, rect, padding=0.0):
    return (
        rect["min_x"] - padding <= point["x"] <= rect["max_x"] + padding
        and rect["min_y"] - padding <= point["y"] <= rect["max_y"] + padding
    )


def click_node_wire_hit_point(
    client, component_id, button="secondary", require_display_only=False
):
    """Click the latest rendered Bezier midpoint, not its bounding-box center."""
    snapshot, component = client.component(component_id)
    metadata = component.get("metadata") or {}
    hit_point = metadata.get("hit_point")
    if hit_point is None:
        raise QaFailure("wire {!r} omitted its rendered hit point".format(component_id))
    rect = component["rect_points"]
    if not point_in_component_rect(hit_point, rect):
        raise QaFailure(
            "wire {!r} hit point is outside its fresh component rect".format(
                component_id
            )
        )
    if require_display_only:
        if not (
            metadata.get("kind") == "derived_output"
            and metadata.get("editable") is False
        ):
            raise QaFailure("wire {!r} is not display-only".format(component_id))
        graph_item_prefixes = (
            "node_editor.node:",
            "node_editor.node_header:",
            "node_editor.container_header.",
            "node_editor.port.",
            "node_editor.container_port.",
            "node_editor.resize_edge.",
        )
        overlapping_graph_items = [
            item["id"]
            for item in snapshot["components"]
            if item.get("visible", False)
            and item["id"].startswith(graph_item_prefixes)
            and point_in_component_rect(hit_point, item["rect_points"])
        ]
        if overlapping_graph_items:
            raise QaFailure(
                "display-only wire hit point is owned by a graph item: {}".format(
                    overlapping_graph_items
                )
            )
        overlapping_editable_wires = [
            item["id"]
            for item in snapshot["components"]
            if (item.get("metadata") or {}).get("kind")
            in ("explicit", "output_binding")
            and point_near_node_wire(hit_point, item, radius=10.0)
        ]
        if overlapping_editable_wires:
            raise QaFailure(
                "display-only wire hit point is shadowed by an editable wire: {}".format(
                    overlapping_editable_wires
                )
            )
    client.inject(
        "click",
        {
            "x": hit_point["x"],
            "y": hit_point["y"],
            "coordinate_space": "points",
            "button": button,
        },
        {
            "component_id": component_id,
            "component_frame": snapshot["frame"],
            "component_rect_points": rect,
            "component_hit_point": hit_point,
            "coordinate_reason": "fresh rendered Bezier hit point",
        },
    )
    return snapshot, component, hit_point


def drag_component_to_node_wire_hit_point(client, source_id, wire_id, steps=12):
    """Drag a component to the latest rendered Bezier hit point."""
    snapshot = client.component_snapshot()
    components = {item["id"]: item for item in snapshot["components"]}
    try:
        source = components[source_id]
        wire = components[wire_id]
    except KeyError as error:
        raise QaFailure(
            "wire drop component {} absent in frame {}".format(
                error, snapshot["frame"]
            )
        ) from error
    for component_id, component in ((source_id, source), (wire_id, wire)):
        rect = component["rect_points"]
        if (
            not component.get("enabled", False)
            or not component.get("visible", False)
            or rect["width"] <= 0.0
            or rect["height"] <= 0.0
        ):
            raise QaFailure(
                "wire drop component {!r} is not interactive".format(component_id)
            )
    hit_point = (wire.get("metadata") or {}).get("hit_point")
    if hit_point is None or not point_in_component_rect(
        hit_point, wire["rect_points"]
    ):
        raise QaFailure("wire {!r} omitted a usable hit point".format(wire_id))
    start = client.point(source["rect_points"])
    client.inject(
        "drag",
        {
            "from": start,
            "to": hit_point,
            "coordinate_space": "points",
            "steps": steps,
            "button": "primary",
        },
        {
            "source_component_id": source_id,
            "target_component_id": wire_id,
            "component_frame": snapshot["frame"],
            "source_rect_points": source["rect_points"],
            "target_rect_points": wire["rect_points"],
            "target_hit_point": hit_point,
            "coordinate_reason": "fresh rendered Bezier hit point",
        },
    )
    return start, hit_point


def line_span_inside_rect(first, second, rect, margin=10.0):
    """Return the two boundary points where an infinite line crosses a rect."""
    dx = second["x"] - first["x"]
    dy = second["y"] - first["y"]
    if dx * dx + dy * dy < 16.0:
        return None
    bounds = {
        "min_x": rect["min_x"] + margin,
        "max_x": rect["max_x"] - margin,
        "min_y": rect["min_y"] + margin,
        "max_y": rect["max_y"] - margin,
    }
    candidates = []
    if abs(dx) > 1.0e-6:
        for x in (bounds["min_x"], bounds["max_x"]):
            factor = (x - first["x"]) / dx
            y = first["y"] + dy * factor
            if bounds["min_y"] <= y <= bounds["max_y"]:
                candidates.append((factor, {"x": x, "y": y}))
    if abs(dy) > 1.0e-6:
        for y in (bounds["min_y"], bounds["max_y"]):
            factor = (y - first["y"]) / dy
            x = first["x"] + dx * factor
            if bounds["min_x"] <= x <= bounds["max_x"]:
                candidates.append((factor, {"x": x, "y": y}))
    candidates.sort(key=lambda item: item[0])
    if len(candidates) < 2:
        return None
    return candidates[0][1], candidates[-1][1]


def find_wire_knife_gesture(snapshot):
    components = snapshot["components"]
    canvas = next(
        (item for item in components if item["id"] == "node_editor.canvas"), None
    )
    if canvas is None:
        raise QaFailure("Node Editor canvas is absent while planning a knife gesture")
    canvas_rect = canvas["rect_points"]
    hit_margin = 18.0

    def hit_point_is_inside_stroke(point):
        return (
            canvas_rect["min_x"] + hit_margin <= point["x"] <= canvas_rect["max_x"] - hit_margin
            and canvas_rect["min_y"] + hit_margin
            <= point["y"]
            <= canvas_rect["max_y"] - hit_margin
        )

    edges = [
        item
        for item in components
        if item["id"].startswith("node_editor.edge:")
        and item.get("visible", False)
        and (item.get("metadata") or {}).get("kind") == "explicit"
        and (item.get("metadata") or {}).get("hit_point") is not None
        # A hit rect may be partially visible while its Bezier midpoint sits
        # at the canvas edge. The knife span deliberately stops inside the
        # canvas, so only promise IDs whose actual hit point lies on that span.
        and hit_point_is_inside_stroke(item["metadata"]["hit_point"])
    ]
    obstacle_prefixes = (
        "node_editor.node:",
        "node_editor.node_header:",
        "node_editor.port.",
        "node_editor.container_header.",
        "node_editor.container_port.",
        "node_editor.resize_edge.",
        "node_editor.edge:",
        "node_editor.edge.",
    )
    obstacles = [
        item["rect_points"]
        for item in components
        if item.get("visible", False)
        and item["id"].startswith(obstacle_prefixes)
        and item["rect_points"]["width"] > 0.0
        and item["rect_points"]["height"] > 0.0
    ]
    for left_index, left in enumerate(edges):
        for right in edges[left_index + 1 :]:
            left_point = left["metadata"]["hit_point"]
            right_point = right["metadata"]["hit_point"]
            span = line_span_inside_rect(
                left_point, right_point, canvas_rect, margin=14.0
            )
            if span is None:
                continue
            for start, end in (span, tuple(reversed(span))):
                if all(
                    not point_in_component_rect(start, obstacle, 5.0)
                    for obstacle in obstacles
                ):
                    return start, end, [
                        left["metadata"]["connection_id"],
                        right["metadata"]["connection_id"],
                    ]
    raise QaFailure("no two visible explicit wires admit a blank-origin knife gesture")


def find_mixed_wire_knife_gesture(snapshot, binding_edge_id):
    """Plan one blank-origin stroke through an output binding and explicit wire."""
    components = snapshot["components"]
    canvas = next(
        (item for item in components if item["id"] == "node_editor.canvas"), None
    )
    if canvas is None:
        raise QaFailure("Node Editor canvas is absent while planning a mixed knife")
    binding = next(
        (item for item in components if item["id"] == binding_edge_id), None
    )
    if binding is None:
        raise QaFailure("output binding edge {!r} is absent".format(binding_edge_id))
    binding_metadata = binding.get("metadata") or {}
    if not (
        binding.get("visible", False)
        and binding_metadata.get("kind") == "output_binding"
        and binding_metadata.get("editable") is True
        and binding_metadata.get("action") == "delete_output_binding"
        and binding_metadata.get("hit_point") is not None
    ):
        raise QaFailure("output binding edge is not a stable editable QA target")

    canvas_rect = canvas["rect_points"]
    hit_margin = 18.0

    def hit_point_is_inside_stroke(point):
        return (
            canvas_rect["min_x"] + hit_margin
            <= point["x"]
            <= canvas_rect["max_x"] - hit_margin
            and canvas_rect["min_y"] + hit_margin
            <= point["y"]
            <= canvas_rect["max_y"] - hit_margin
        )

    binding_point = binding_metadata["hit_point"]
    if not hit_point_is_inside_stroke(binding_point):
        raise QaFailure("output binding midpoint is clipped at the Node Editor boundary")
    explicit_edges = [
        item
        for item in components
        if item["id"].startswith("node_editor.edge:")
        and item.get("visible", False)
        and (item.get("metadata") or {}).get("kind") == "explicit"
        and (item.get("metadata") or {}).get("hit_point") is not None
        and hit_point_is_inside_stroke(item["metadata"]["hit_point"])
    ]
    obstacle_prefixes = (
        "node_editor.node:",
        "node_editor.node_header:",
        "node_editor.port.",
        "node_editor.container_header.",
        "node_editor.container_port.",
        "node_editor.resize_edge.",
        "node_editor.edge:",
        "node_editor.edge.",
    )
    obstacles = [
        item["rect_points"]
        for item in components
        if item.get("visible", False)
        and item["id"].startswith(obstacle_prefixes)
        and item["rect_points"]["width"] > 0.0
        and item["rect_points"]["height"] > 0.0
    ]
    for explicit in explicit_edges:
        explicit_point = explicit["metadata"]["hit_point"]
        if (
            abs(binding_point["x"] - explicit_point["x"]) < 1.0
            and abs(binding_point["y"] - explicit_point["y"]) < 1.0
        ):
            continue
        span = line_span_inside_rect(
            binding_point, explicit_point, canvas_rect, margin=14.0
        )
        if span is None:
            continue
        for start, end in (span, tuple(reversed(span))):
            if all(
                not point_in_component_rect(start, obstacle, 5.0)
                for obstacle in obstacles
            ):
                return start, end, {
                    "binding_edge_id": binding_edge_id,
                    "binding_owner": binding_metadata.get("binding_owner"),
                    "binding_node_id": binding_metadata.get("binding_node_id"),
                    "connection_id": explicit["metadata"]["connection_id"],
                }
    raise QaFailure(
        "no visible explicit wire and output binding admit a blank-origin knife gesture"
    )


def explicit_wire_connection_ids(snapshot):
    return {
        item["metadata"]["connection_id"]
        for item in snapshot["components"]
        if item["id"].startswith("node_editor.edge:")
        and (item.get("metadata") or {}).get("kind") == "explicit"
        and (item.get("metadata") or {}).get("connection_id") is not None
    }


def wait_wire_snapshot_for_project(client, project_connection_ids):
    """Wait until QA edge geometry no longer contains prior-Project wires."""
    expected = set(project_connection_ids)

    def fresh_snapshot():
        # State reads schedule a UI pass. Require both a completed frame and
        # the absence of transient edge UUIDs from the previous Project.
        state_frame = client.state()["frame"]
        snapshot = client.component_snapshot()
        explicit_ids = explicit_wire_connection_ids(snapshot)
        if snapshot["frame"] < state_frame or not explicit_ids.issubset(expected):
            return None
        return snapshot

    return client.wait_until("Node Editor wire geometry for current Project", fresh_snapshot)


def reveal_node_editor_component(client, component_id, max_drags=20):
    """Pan the real Snarl canvas until an offscreen component is clickable."""
    snapshot, components = reveal_node_editor_components(
        client, [component_id], max_drags=max_drags
    )
    return snapshot, components[0]


def node_editor_pan_delta(canvas_rect, unclipped_rects, margin=12.0):
    """Return one bounded two-axis pan that centers the target union."""
    if not unclipped_rects:
        raise QaFailure("Node Editor reveal requires at least one target rectangle")
    inner_width = canvas_rect["width"] - margin * 2.0
    inner_height = canvas_rect["height"] - margin * 2.0
    if inner_width <= 0.0 or inner_height <= 0.0:
        raise QaFailure("Node Editor canvas is too small for the reveal margin")
    union_min_x = min(rect["min_x"] for rect in unclipped_rects)
    union_max_x = max(rect["max_x"] for rect in unclipped_rects)
    union_min_y = min(rect["min_y"] for rect in unclipped_rects)
    union_max_y = max(rect["max_y"] for rect in unclipped_rects)
    if (
        union_max_x - union_min_x > inner_width
        or union_max_y - union_min_y > inner_height
    ):
        raise QaFailure("Node Editor target union cannot fit in the visible canvas")
    needed_x = canvas_rect["center_x"] - (union_min_x + union_max_x) * 0.5
    needed_y = canvas_rect["center_y"] - (union_min_y + union_max_y) * 0.5
    return (
        max(-inner_width, min(inner_width, needed_x)),
        max(-inner_height, min(inner_height, needed_y)),
    )


def node_editor_reveal_rect(component):
    """Use a wire's real hit point instead of requiring its whole curve bbox."""
    rect = component["rect_points"]
    metadata = component.get("metadata") or {}
    if (
        component["id"].startswith("node_editor.edge")
        and metadata.get("kind") in ("explicit", "output_binding", "derived_output")
    ):
        hit_point = metadata.get("hit_point")
        if hit_point is not None and point_in_component_rect(hit_point, rect):
            return {
                "min_x": hit_point["x"],
                "max_x": hit_point["x"],
                "min_y": hit_point["y"],
                "max_y": hit_point["y"],
            }
    return rect


def reveal_node_editor_components(client, component_ids, max_drags=20, margin=12.0):
    """Pan until every endpoint is fully inside one fresh canvas frame."""
    for _ in range(max_drags):
        snapshot = client.component_snapshot()
        components = {item["id"]: item for item in snapshot["components"]}
        canvas = components.get("node_editor.canvas")
        targets = [components.get(component_id) for component_id in component_ids]
        if canvas is None or any(target is None for target in targets):
            time.sleep(0.04)
            continue
        canvas_rect = canvas["rect_points"]
        inner_min_x = canvas_rect["min_x"] + margin
        inner_max_x = canvas_rect["max_x"] - margin
        inner_min_y = canvas_rect["min_y"] + margin
        inner_max_y = canvas_rect["max_y"] - margin
        if all(
            target.get("visible", False)
            and target["rect_points"]["width"] > 0.0
            and target["rect_points"]["height"] > 0.0
            and node_editor_reveal_rect(target)["min_x"] >= inner_min_x
            and node_editor_reveal_rect(target)["max_x"] <= inner_max_x
            and node_editor_reveal_rect(target)["min_y"] >= inner_min_y
            and node_editor_reveal_rect(target)["max_y"] <= inner_max_y
            for target in targets
        ):
            return snapshot, targets

        unclipped = [
            target.get("metadata", {}).get("unclipped_rect") for target in targets
        ]
        if any(rect is None for rect in unclipped):
            time.sleep(0.04)
            continue
        try:
            horizontal_step, vertical_step = node_editor_pan_delta(
                canvas_rect, unclipped, margin
            )
        except QaFailure as error:
            previous_scale = float((canvas.get("metadata") or {}).get("scale", 0.0))
            client.inject(
                "scroll",
                {
                    "x": canvas_rect["center_x"],
                    "y": canvas_rect["center_y"],
                    "delta_x": 0.0,
                    "delta_y": -90.0,
                    "coordinate_space": "points",
                    "modifiers": {"command": True},
                },
                {
                    "component_id": "node_editor.canvas",
                    "target_component_ids": list(component_ids),
                    "component_frame": snapshot["frame"],
                    "component_rect_points": canvas_rect,
                    "coordinate_reason": "zoom out until all drag endpoints fit",
                },
            )

            def scale_decreased():
                current = client.component_snapshot()
                current_canvas = next(
                    (
                        item
                        for item in current["components"]
                        if item["id"] == "node_editor.canvas"
                    ),
                    None,
                )
                if current_canvas is None:
                    return None
                scale = float(
                    (current_canvas.get("metadata") or {}).get("scale", 0.0)
                )
                return current if scale < previous_scale - 1.0e-4 else None

            try:
                client.wait_until("Node Editor endpoint-fit zoom", scale_decreased)
            except QaFailure as zoom_error:
                raise QaFailure(
                    "Node Editor components cannot fit together in the visible canvas: {} ({})".format(
                        ", ".join(component_ids), error
                    )
                ) from zoom_error
            continue
        if abs(horizontal_step) < 1.0 and abs(vertical_step) < 1.0:
            raise QaFailure(
                "Node Editor components did not become fully visible: {}".format(
                    ", ".join(component_ids)
                )
            )
        start_x = (
            canvas_rect["max_x"] - margin
            if horizontal_step < 0.0
            else canvas_rect["min_x"] + margin
        )
        start_y = (
            canvas_rect["max_y"] - margin
            if vertical_step < 0.0
            else canvas_rect["min_y"] + margin
        )
        client.inject(
            "drag",
            {
                "from": {"x": start_x, "y": start_y},
                "to": {
                    "x": start_x + horizontal_step,
                    "y": start_y + vertical_step,
                },
                "coordinate_space": "points",
                "steps": 6,
                "button": "middle",
            },
            {
                "component_id": "node_editor.canvas",
                "target_component_ids": list(component_ids),
                "component_frame": snapshot["frame"],
                "component_rect_points": canvas_rect,
                "coordinate_reason": "pan all drag endpoints fully inside canvas",
            },
        )
    raise QaFailure(
        "Node Editor components did not become visible after {} canvas drags: {}".format(
            max_drags, ", ".join(component_ids)
        )
    )


def ensure_node_editor_ports_interactive(client, component_ids, max_zooms=6):
    """Zoom through real Cmd/Ctrl-wheel input until normal pin drags are enabled."""
    snapshot, targets = reveal_node_editor_components(client, component_ids)
    for _ in range(max_zooms):
        components = {item["id"]: item for item in snapshot["components"]}
        canvas = components["node_editor.canvas"]
        if (canvas.get("metadata") or {}).get("port_interaction_enabled"):
            return reveal_node_editor_components(client, component_ids)
        canvas_rect = canvas["rect_points"]
        focus = {
            "x": sum(target["rect_points"]["center_x"] for target in targets)
            / len(targets),
            "y": sum(target["rect_points"]["center_y"] for target in targets)
            / len(targets),
        }
        focus["x"] = max(canvas_rect["min_x"] + 8.0, min(canvas_rect["max_x"] - 8.0, focus["x"]))
        focus["y"] = max(canvas_rect["min_y"] + 8.0, min(canvas_rect["max_y"] - 8.0, focus["y"]))
        previous_scale = float((canvas.get("metadata") or {}).get("scale", 0.0))
        client.inject(
            "scroll",
            {
                "x": focus["x"],
                "y": focus["y"],
                "delta_x": 0.0,
                "delta_y": 90.0,
                "coordinate_space": "points",
                "modifiers": {"command": True},
            },
            {
                "component_id": "node_editor.canvas",
                "target_component_ids": list(component_ids),
                "component_frame": snapshot["frame"],
                "coordinate_reason": "enable real Node port interactions",
            },
        )

        def scale_increased():
            current = client.component_snapshot()
            current_canvas = next(
                (
                    item
                    for item in current["components"]
                    if item["id"] == "node_editor.canvas"
                ),
                None,
            )
            if current_canvas is None:
                return None
            scale = float((current_canvas.get("metadata") or {}).get("scale", 0.0))
            return current if scale > previous_scale + 1.0e-4 else None

        snapshot = client.wait_until("Node Editor port-interaction zoom", scale_increased)
        current_components = {item["id"]: item for item in snapshot["components"]}
        targets = [current_components[item_id] for item_id in component_ids]
    raise QaFailure("Node Editor did not enable normal port interactions after zoom")


def validate_explicit_operation_fixture(project):
    """Reject the retired four-Node/embedded-operation QA fixture."""
    actual_nodes = set(project["nodes"])
    if actual_nodes != EXPECTED_FIXTURE_NODES:
        missing = sorted(EXPECTED_FIXTURE_NODES - actual_nodes)
        extra = sorted(actual_nodes - EXPECTED_FIXTURE_NODES)
        raise QaFailure(
            "fixture must contain the 12 explicit Nodes; missing={}, extra={}".format(
                missing, extra
            )
        )

    for clip_id, expected_nodes in EXPECTED_CLIP_NODES.items():
        clip = project["clips"].get(clip_id)
        if clip is None:
            raise QaFailure("fixture is missing Clip {}".format(clip_id))
        if clip.get("node_ids") != expected_nodes:
            raise QaFailure(
                "Clip {} Node order is {}, expected {}".format(
                    clip_id, clip.get("node_ids"), expected_nodes
                )
            )
        expected_output = EXPECTED_CLIP_OUTPUTS[clip_id]
        if clip.get("output_node_id") != expected_output:
            raise QaFailure(
                "Clip {} output is {}, expected {}".format(
                    clip_id, clip.get("output_node_id"), expected_output
                )
            )

    for node_id, expected_identity in EXPECTED_OPERATIONS.items():
        content = project["nodes"][node_id].get("content", {})
        if content.get("type") != "PluginOperation":
            raise QaFailure("{} is not an explicit PluginOperation Node".format(node_id))
        operation = content.get("data", {})
        actual_identity = (
            operation.get("category"),
            operation.get("component_id"),
            operation.get("operation"),
        )
        if actual_identity != expected_identity:
            raise QaFailure(
                "operation {} identity is {}, expected {}".format(
                    node_id, actual_identity, expected_identity
                )
            )
        if not operation.get("declared_ports"):
            raise QaFailure("operation {} has no persisted port contract".format(node_id))

    for node_id, expected_generator in (
        (SOLID, "Solid"),
        (TEXT, "Text"),
        (SHAPE, "Shape"),
    ):
        content = project["nodes"][node_id].get("content", {})
        if (content.get("type"), content.get("data")) != (
            "Generator",
            expected_generator,
        ):
            raise QaFailure(
                "{} is not the expected {} source Node".format(
                    node_id, expected_generator
                )
            )
    for node_id in (MERGE, SHAPE_MERGE):
        if project["nodes"][node_id].get("content", {}).get("type") != "Merge":
            raise QaFailure("{} is not an explicit Merge Node".format(node_id))

    for node_id, node in project["nodes"].items():
        for collection in ("styles", "effects", "effectors", "decorators"):
            if node.get(collection):
                raise QaFailure(
                    "{} still contains embedded {}".format(node_id, collection)
                )
    if len(project.get("connections", ())) != 24:
        raise QaFailure(
            "explicit fixture has {} connections, expected 24".format(
                len(project.get("connections", ()))
            )
        )


def assert_fresh_fixture(initial):
    project = initial["project"]
    if project.get("name") != "RuViE QA E2E":
        raise QaFailure(
            "the deterministic fixture is required; start with "
            "RUVIE_QA_FIXTURE=node_editor_e2e"
        )
    validate_explicit_operation_fixture(project)
    validate_canonical_ownership(project)
    preview = initial["editor"]["preview"]
    if not (
        preview["texture_width"] == 640
        and preview["texture_height"] == 360
        and preview["nontransparent_pixels"]
        and preview["pixel_hash"] is not None
        and preview["modal_error"] is None
    ):
        raise QaFailure("initial Preview image is missing, transparent, or errored")


def wait_fresh_fixture(client):
    def ready():
        state = client.state()
        project = state["project"]
        if project.get("name") != "RuViE QA E2E":
            raise QaFailure(
                "the deterministic fixture is required; start with "
                "RUVIE_QA_FIXTURE=node_editor_e2e"
            )
        preview = state["editor"]["preview"]
        if preview["modal_error"] is not None:
            raise QaFailure("initial Preview failed: {}".format(preview["modal_error"]))
        return state if preview["render_revision"] > 0 else None

    initial = client.wait_until("the initial fixture Preview render", ready)
    assert_fresh_fixture(initial)
    return initial


def ensure_operation_property(client, operation_id, property_name):
    """Expose one descriptor-driven direct Node property in the Clip facade."""
    header_id = "inspector.operation:" + operation_id
    control_id = "inspector.property.node:{}:{}".format(operation_id, property_name)
    client.ensure_in_scroll_area(header_id)
    _, header = client.component(header_id)
    header_metadata = header.get("metadata") or {}
    expected = EXPECTED_OPERATIONS.get(operation_id)
    if expected is not None and (
        header_metadata.get("category"), header_metadata.get("component_id")
    ) != expected[:2]:
        raise QaFailure("Inspector operation header has stale plugin identity")
    if header_metadata.get("available") is not True:
        raise QaFailure("fixture operation is unavailable to the Inspector")
    snapshot = client.component_snapshot()
    if control_id not in {item["id"] for item in snapshot["components"]}:
        # The operation section is collapsed. Resolve the header again after
        # scrolling and use that fresh logical-point rectangle for the click.
        client.click_component(header_id)
        def registered():
            current = client.component_snapshot()
            return (
                current
                if control_id in {item["id"] for item in current["components"]}
                else None
            )

        client.wait_until(
            "{} operation property registration".format(property_name), registered
        )
    client.ensure_in_scroll_area(control_id)
    snapshot, component = client.component(control_id)
    metadata = component.get("metadata") or {}
    if metadata.get("scope") != "node:" + operation_id:
        raise QaFailure("Inspector property is not scoped to its operation Node")
    if metadata.get("property") != property_name:
        raise QaFailure("Inspector property metadata names the wrong property")
    if not metadata.get("definition"):
        raise QaFailure("Inspector property omitted descriptor metadata")
    return snapshot, component


def assert_only_node_enabled_changed(before_project, after_project, node_id, expected):
    before = json.loads(json.dumps(before_project))
    after = json.loads(json.dumps(after_project))
    before_enabled = before["nodes"][node_id].pop("enabled")
    after_enabled = after["nodes"][node_id].pop("enabled")
    if before_enabled == expected or after_enabled != expected:
        raise QaFailure("Node enabled transition has the wrong direction")
    if before != after:
        raise QaFailure("Node enabled command changed unrelated Project data")
    validate_canonical_ownership(after_project)


def assert_timeline_semantic_source(client, clip_id, output_node_id, source_node_id):
    _, component = client.wait_component("timeline.clip:" + clip_id)
    metadata = component.get("metadata") or {}
    if metadata.get("output_node_id") != output_node_id:
        raise QaFailure("Timeline lost the Clip's explicit output binding")
    if metadata.get("semantic_source_node_id") != source_node_id:
        raise QaFailure("Timeline semantic source did not reflect Node enabled state")
    return metadata


def run_node_toggle_cross_view_scenario(client):
    """Prove Node authored state reaches Timeline, Inspector, and Preview."""
    activate_dock_tab(
        client, "dock.tab:node_editor", "Node Editor", "Node toggle cross-view"
    )
    text_header = "node_editor.node_header:" + TEXT
    reveal_node_editor_component(client, text_header)
    _, enabled_component = client.component("node_editor.node:" + TEXT)
    enabled_metadata = enabled_component.get("metadata") or {}
    if enabled_metadata.get("inactive") or enabled_metadata.get("inactive_reason") is not None:
        raise QaFailure("active Text Node is rendered inactive before Disable")

    disable_before = client.state()
    selection_before = dict(disable_before["editor"]["selection"])
    preview_before = assert_valid_preview(disable_before, "Node Disable baseline")
    client.click_component(text_header, button="secondary")
    toggle_id = "node_editor.menu.toggle_enabled.node:" + TEXT
    client.wait_component(toggle_id)
    client.click_component(toggle_id)
    disabled = client.wait_project(
        "Text Node Disable", lambda project: not project["nodes"][TEXT]["enabled"]
    )
    assert_history_delta(disable_before, disabled, 1, "Text Node Disable")
    assert_only_node_enabled_changed(
        disable_before["project"], disabled["project"], TEXT, False
    )
    if disabled["editor"]["selection"] != selection_before:
        raise QaFailure("Node Disable unexpectedly changed selection")
    disabled_rendered = client.wait_preview_change(
        preview_before["pixel_hash"], preview_before["render_revision"]
    )
    disabled_preview = assert_valid_preview(disabled_rendered, "Text Node Disable")

    activate_dock_tab(
        client, "dock.tab:timeline", "Timeline", "disabled Node Timeline reflection"
    )
    assert_timeline_semantic_source(client, CLIP_A2, BLUR_EFFECT, None)
    client.click_component("timeline.clip:" + CLIP_A2)
    selected_clip = client.wait_until(
        "disabled Clip coordinate selection",
        lambda: state
        if selection_matches((state := client.state()), "clip", CLIP_A2)
        else None,
    )
    assert_selection(selected_clip, CLIP_A2, TRACK_A, "disabled Clip selection")
    assert_inspector_clip_timing(
        client, CLIP_A2, TRACK_A, selected_clip["project"]["clips"][CLIP_A2]
    )

    activate_dock_tab(
        client, "dock.tab:node_editor", "Node Editor", "Node Enable cross-view"
    )
    reveal_node_editor_component(client, text_header)
    _, disabled_component = client.component("node_editor.node:" + TEXT)
    disabled_metadata = disabled_component.get("metadata") or {}
    if not (
        disabled_metadata.get("inactive") is True
        and disabled_metadata.get("inactive_reason") == "disabled"
    ):
        raise QaFailure("Node Editor did not expose the authored disabled state")

    enable_before = client.state()
    client.click_component(text_header, button="secondary")
    client.wait_component(toggle_id)
    client.click_component(toggle_id)
    enabled = client.wait_project(
        "Text Node Enable", lambda project: project["nodes"][TEXT]["enabled"]
    )
    assert_history_delta(enable_before, enabled, 1, "Text Node Enable")
    assert_only_node_enabled_changed(
        enable_before["project"], enabled["project"], TEXT, True
    )
    if enabled["editor"]["selection"] != enable_before["editor"]["selection"]:
        raise QaFailure("Node Enable unexpectedly changed selection")
    restored = wait_preview_hash_after(
        client,
        preview_before["pixel_hash"],
        disabled_preview["render_revision"],
        "Text Node Enable",
    )
    assert_valid_preview(restored, "Text Node Enable")

    activate_dock_tab(
        client, "dock.tab:timeline", "Timeline", "enabled Node Timeline reflection"
    )
    source = assert_timeline_semantic_source(client, CLIP_A2, BLUR_EFFECT, TEXT)
    if source.get("semantic_source_kind") != "Text":
        raise QaFailure("Timeline restored the wrong semantic source kind")
    assert_selection(restored, CLIP_A2, TRACK_A, "Node Enable")
    print("[qa-e2e] Node enabled state -> Timeline/Inspector/Preview passed")
    return restored


def run_timeline_scenario(client):
    """Exercise Timeline mutations solely through fresh screen coordinates."""
    activate_dock_tab(client, "dock.tab:timeline", "Timeline", "Timeline edit")

    move_before = client.state()
    clip_before = move_before["project"]["clips"][CLIP_A1]
    order_before = list(move_before["project"]["tracks"][TRACK_A]["clip_ids"])
    move_delta = 1.25
    client.drag_timeline_by_seconds(
        CLIP_A1, "timeline.clip:" + CLIP_A1, move_delta
    )
    move_after = wait_timeline_edit(
        client,
        move_before,
        "Timeline Clip time move",
        lambda current: abs(
            current["clips"][CLIP_A1]["start_time"]
            - (clip_before["start_time"] + move_delta)
        )
        < 1.0e-4,
    )
    moved = move_after["project"]["clips"][CLIP_A1]
    assert_close(
        moved["start_time"],
        clip_before["start_time"] + move_delta,
        "moved Clip start_time",
        tolerance=1.0e-4,
    )
    if move_after["project"]["tracks"][TRACK_A]["clip_ids"] != order_before:
        raise QaFailure("horizontal Clip move changed canonical Clip order")
    assert_close(moved["duration"], clip_before["duration"], "moved Clip duration")
    assert_close(moved["trim_in"], clip_before["trim_in"], "moved Clip trim_in")
    assert_close(
        moved["time_stretch"], clip_before["time_stretch"], "moved Clip time_stretch"
    )
    assert_timeline_integrity(
        move_before["project"], move_after["project"], "Clip time move"
    )
    assert_history_delta(move_before, move_after, 1, "Clip time move")
    assert_selection(move_after, CLIP_A1, TRACK_A, "Clip time move")
    assert_inspector_clip_timing(client, CLIP_A1, TRACK_A, moved)
    move_rendered = client.wait_preview_render_after(move_before, "Clip time move")
    if (
        move_rendered["editor"]["preview"]["pixel_hash"]
        == move_before["editor"]["preview"]["pixel_hash"]
    ):
        raise QaFailure("moving Clip outside the playhead did not change Preview hash")

    left_before = client.state()
    old = left_before["project"]["clips"][CLIP_A1]
    left_delta = 0.25
    client.drag_timeline_by_seconds(
        CLIP_A1, "timeline.clip_edge.left:" + CLIP_A1, left_delta
    )
    left_after = wait_timeline_edit(
        client,
        left_before,
        "left-edge trim",
        lambda current: abs(
            current["clips"][CLIP_A1]["start_time"]
            - (old["start_time"] + left_delta)
        )
        < 1.0e-4,
    )
    new = left_after["project"]["clips"][CLIP_A1]
    delta = new["start_time"] - old["start_time"]
    assert_close(delta, left_delta, "left trim delta", tolerance=1.0e-4)
    assert_close(
        new["duration"], old["duration"] - delta, "left-trimmed Clip duration"
    )
    assert_close(
        new["start_time"] + new["duration"],
        old["start_time"] + old["duration"],
        "left trim end time",
    )
    assert_close(
        new["trim_in"],
        old["trim_in"] + delta * old["time_stretch"],
        "left trim source boundary",
    )
    assert_timeline_integrity(
        left_before["project"], left_after["project"], "left-edge trim"
    )
    assert_history_delta(left_before, left_after, 1, "left-edge trim")
    assert_selection(left_after, CLIP_A1, TRACK_A, "left-edge trim")
    assert_inspector_clip_timing(client, CLIP_A1, TRACK_A, new)
    client.wait_preview_render_after(left_before, "left-edge trim")

    right_before = client.state()
    old = right_before["project"]["clips"][CLIP_A1]
    right_delta = -0.25
    client.drag_timeline_by_seconds(
        CLIP_A1, "timeline.clip_edge.right:" + CLIP_A1, right_delta
    )
    right_after = wait_timeline_edit(
        client,
        right_before,
        "right-edge trim",
        lambda current: abs(
            current["clips"][CLIP_A1]["duration"]
            - (old["duration"] + right_delta)
        )
        < 1.0e-4,
    )
    new = right_after["project"]["clips"][CLIP_A1]
    assert_close(new["start_time"], old["start_time"], "right trim start time")
    assert_close(new["trim_in"], old["trim_in"], "right trim trim_in")
    assert_close(
        new["duration"],
        old["duration"] + right_delta,
        "right trim duration",
        tolerance=1.0e-4,
    )
    assert_timeline_integrity(
        right_before["project"], right_after["project"], "right-edge trim"
    )
    assert_history_delta(right_before, right_after, 1, "right-edge trim")
    assert_selection(right_after, CLIP_A1, TRACK_A, "right-edge trim")
    assert_inspector_clip_timing(client, CLIP_A1, TRACK_A, new)
    client.wait_preview_render_after(right_before, "right-edge trim")

    reorder_before = client.state()
    reordered_timing = dict(reorder_before["project"]["clips"][CLIP_A1])
    client.drag_component_to_row(
        "timeline.clip:" + CLIP_A1,
        "timeline.clip_insertion_slot.{}:2".format(TRACK_A),
    )
    reorder_after = wait_timeline_edit(
        client,
        reorder_before,
        "same-Track Clip reorder",
        lambda current: current["tracks"][TRACK_A]["clip_ids"]
        == [CLIP_A2, CLIP_A1],
    )
    reordered = reorder_after["project"]["clips"][CLIP_A1]
    for field in ("start_time", "duration", "trim_in", "time_stretch"):
        assert_close(
            reordered[field],
            reordered_timing[field],
            "same-Track reorder {}".format(field),
        )
    assert_timeline_integrity(
        reorder_before["project"], reorder_after["project"], "same-Track Clip reorder"
    )
    assert_history_delta(reorder_before, reorder_after, 1, "same-Track Clip reorder")
    assert_selection(reorder_after, CLIP_A1, TRACK_A, "same-Track Clip reorder")
    assert_inspector_clip_timing(client, CLIP_A1, TRACK_A, reordered)
    client.wait_preview_render_after(reorder_before, "same-Track Clip reorder")

    cross_before = client.state()
    cross_timing = dict(cross_before["project"]["clips"][CLIP_A2])
    client.drag_component_to_row(
        "timeline.clip:" + CLIP_A2,
        "timeline.clip_insertion_slot.{}:1".format(TRACK_B),
    )
    cross_after = wait_timeline_edit(
        client,
        cross_before,
        "cross-Track Clip move",
        lambda current: CLIP_A2 not in current["tracks"][TRACK_A]["clip_ids"]
        and current["tracks"][TRACK_B]["clip_ids"] == [CLIP_B1, CLIP_A2],
    )
    moved_across = cross_after["project"]["clips"][CLIP_A2]
    for field in ("start_time", "duration", "trim_in", "time_stretch"):
        assert_close(
            moved_across[field],
            cross_timing[field],
            "cross-Track move {}".format(field),
        )
    assert_timeline_integrity(
        cross_before["project"],
        cross_after["project"],
        "cross-Track Clip move",
        {CLIP_A2: TRACK_B},
    )
    assert_history_delta(cross_before, cross_after, 1, "cross-Track Clip move")
    assert_selection(cross_after, CLIP_A2, TRACK_B, "cross-Track Clip move")
    assert_inspector_clip_timing(client, CLIP_A2, TRACK_B, moved_across)
    client.wait_preview_render_after(cross_before, "cross-Track Clip move")

    track_before = client.state()
    client.drag_component_to_row(
        "timeline.track:" + TRACK_A,
        "timeline.track_insertion_slot:2",
    )
    track_after = wait_timeline_edit(
        client,
        track_before,
        "Track reorder",
        lambda current: composition_map(current)[COMPOSITION]["track_ids"]
        == [TRACK_B, TRACK_A],
    )
    assert_timeline_integrity(
        track_before["project"], track_after["project"], "Track reorder"
    )
    assert_history_delta(track_before, track_after, 1, "Track reorder")
    assert_selection(track_after, CLIP_A2, TRACK_B, "Track reorder")
    assert_inspector_clip_timing(
        client, CLIP_A2, TRACK_B, track_after["project"]["clips"][CLIP_A2]
    )
    track_rendered = client.wait_preview_render_after(track_before, "Track reorder")
    print("[qa-e2e] Timeline move/trim/Clip reorder/Track reorder passed")
    return track_rendered


def assert_node_editor_reflection(client, timeline_state):
    """Assert the Node Editor derives its graph from the edited Project."""
    client.wait_component_settled("dock.tab:node_editor")
    client.click_component("dock.tab:node_editor")
    client.wait_until(
        "Node Editor dock activation after Timeline edits",
        lambda: client.state()
        if "Node Editor" in client.state()["dock"]["active_tabs"]
        else None,
    )
    required = {
        "node_editor.container.clip:" + CLIP_A1,
        "node_editor.container.clip:" + CLIP_A2,
        "node_editor.container.clip:" + CLIP_B1,
        "node_editor.edge.derived:track:{}:clip:{}".format(TRACK_B, CLIP_A2),
    }
    required.update("node_editor.node:" + node_id for node_id in EXPECTED_FIXTURE_NODES)

    def reflected_snapshot():
        snapshot = client.component_snapshot()
        ids = {item["id"] for item in snapshot["components"]}
        return snapshot if required <= ids else None

    snapshot = client.wait_until(
        "Node Editor components derived from Timeline ownership", reflected_snapshot
    )
    components = {item["id"]: item for item in snapshot["components"]}
    stale_edge = "node_editor.edge.derived:track:{}:clip:{}".format(TRACK_A, CLIP_A2)
    if stale_edge in components:
        raise QaFailure("Node Editor retained the old cross-Track derived edge")
    derived_edge_id = "node_editor.edge.derived:track:{}:clip:{}".format(
        TRACK_B, CLIP_A2
    )
    derived_metadata = components[derived_edge_id].get("metadata") or {}
    if not (
        derived_metadata.get("kind") == "derived_output"
        and (derived_metadata.get("from") or {}).get("owner") == "clip:" + CLIP_A2
        and (derived_metadata.get("to") or {}).get("owner") == "track:" + TRACK_B
    ):
        raise QaFailure("Node Editor cross-Track derived wire has stale endpoints")

    owners = validate_canonical_ownership(timeline_state["project"])
    if owners["clip_owners"].get(CLIP_A2) != TRACK_B:
        raise QaFailure("Node Editor reflection read a non-canonical Clip owner")

    expected_outputs = EXPECTED_CLIP_OUTPUTS
    for clip_id, output_node_id in expected_outputs.items():
        component = components["node_editor.container.clip:" + clip_id]
        metadata = component.get("metadata") or {}
        if metadata.get("owner") != "clip:" + clip_id:
            raise QaFailure("Node Editor Clip container has stale owner metadata")
        if metadata.get("output_node_id") != output_node_id:
            raise QaFailure("Node Editor Clip container has stale output binding")

    current_time = timeline_state["editor"]["timeline"]["current_time"]
    clip = timeline_state["project"]["clips"][CLIP_A1]
    expected_inactive = not (
        clip["start_time"] <= current_time < clip["start_time"] + clip["duration"]
    )
    if not expected_inactive:
        raise QaFailure("Timeline scenario did not move Clip A1 outside the playhead")
    for node_id in (SOLID, MERGE):
        metadata = components["node_editor.node:" + node_id].get("metadata") or {}
        if not (
            metadata.get("inactive") == expected_inactive
            and metadata.get("inactive_reason") == "outside_clip_range"
        ):
            raise QaFailure(
                "Node Editor Node activity did not reflect Timeline Clip timing"
            )

    # Prove the reflected Node is still hit-testable through a real coordinate
    # click, not merely present in registry metadata.
    header_id = None
    for node_id in EXPECTED_FIXTURE_NODES:
        candidate = components.get("node_editor.node_header:" + node_id)
        if candidate and candidate.get("visible") and candidate.get("enabled"):
            header_id = candidate["id"]
            break
    if header_id is None:
        raise QaFailure("no reflected Node header is visible for coordinate verification")
    selected_node = header_id.rsplit(":", 1)[-1]
    selection_before = client.state()
    client.click_component(header_id)
    selected = client.wait_until(
        "reflected Node coordinate selection",
        lambda: state
        if selection_matches((state := client.state()), "node", selected_node)
        else None,
    )
    validate_canonical_ownership(selected["project"])
    if selected["project"] != timeline_state["project"]:
        raise QaFailure("Node coordinate selection mutated the authoritative Project")
    if history_depth(selected) != history_depth(selection_before):
        raise QaFailure("Node coordinate selection unexpectedly changed history")
    assert_valid_preview(selected, "Timeline -> Node Editor reflection")
    print("[qa-e2e] Timeline -> Node Editor graph/activity reflection passed")
    return selected


def verify_final_preview_drag(client):
    # This final gesture intentionally traverses normal egui arbitration.
    pan_before = client.wait_preview_settled("cross-view edits")
    client.key("space", True)
    client.drag_component_by("preview.canvas", 54.0, 32.0)
    client.key("space", False)
    client.wait_until(
        "Preview hand-tool pan",
        lambda: state
        if (state := client.state())["editor"]["preview"]["pan"]
        != pan_before["editor"]["preview"]["pan"]
        else None,
    )
    pan_after = client.wait_preview_render_after(pan_before, "Preview hand-tool pan")
    if pan_after["editor"]["preview"]["primary_gesture"] != "Idle":
        raise QaFailure("Preview gesture owner did not return to Idle")
    if pan_after["project"] != pan_before["project"]:
        raise QaFailure("Preview hand-tool pan mutated the authoritative Project")
    if pan_after["editor"]["selection"] != pan_before["editor"]["selection"]:
        raise QaFailure("Preview hand-tool pan changed selection")
    if pan_after["history"] != pan_before["history"]:
        raise QaFailure("Preview hand-tool pan changed Project history")
    assert_valid_preview(pan_after, "Preview hand-tool pan")
    print("[qa-e2e] final real coordinate Preview drag passed")
    return pan_after


def run_timeline_suite(client):
    health = client.wait_health()
    initial = wait_fresh_fixture(client)
    print("[qa-e2e] bridge healthy at frame {}".format(health["frame"]))
    run_node_toggle_cross_view_scenario(client)
    timeline_state = run_timeline_scenario(client)
    assert_node_editor_reflection(client, timeline_state)
    final = verify_final_preview_drag(client)
    return {
        "ok": True,
        "suite": "timeline",
        "initial_frame": initial["frame"],
        "final_frame": final["frame"],
        "final_history": final["history"],
        "final_preview": final["editor"]["preview"],
        "actions": client.evidence,
    }


def run_selection_suite(client):
    """Verify cross-view selection through fresh real screen coordinates."""
    health = client.wait_health()
    initial = wait_fresh_fixture(client)

    activate_dock_tab(
        client, "dock.tab:timeline", "Timeline", "typed Timeline selection"
    )
    clip_component = "timeline.clip:" + CLIP_A2
    client.wait_component_settled(clip_component)
    clip_before = client.state()
    client.click_component(clip_component)
    clip_state = client.wait_until(
        "typed Timeline Clip selection",
        lambda: state
        if selection_matches((state := client.state()), "clip", CLIP_A2)
        else None,
    )
    assert_exact_selection(clip_state, "clip", CLIP_A2, "Timeline Clip click")
    assert_selection(clip_state, CLIP_A2, TRACK_A, "Timeline Clip click")
    client.wait_component("inspector.owner.clip:" + CLIP_A2)
    if clip_state["project"] != clip_before["project"]:
        raise QaFailure("Timeline Clip selection mutated the authoritative Project")
    if clip_state["history"] != clip_before["history"]:
        raise QaFailure("Timeline Clip selection changed undo/redo history")

    activate_dock_tab(
        client, "dock.tab:node_editor", "Node Editor", "typed Node selection"
    )
    node_header = "node_editor.node_header:" + TEXT
    reveal_node_editor_component(client, node_header)
    node_before = client.state()
    client.click_component(node_header)
    node_state = client.wait_until(
        "typed Node Editor Node selection",
        lambda: state
        if selection_matches((state := client.state()), "node", TEXT)
        else None,
    )
    assert_exact_selection(node_state, "node", TEXT, "Node Editor Node click")
    client.wait_component("inspector.owner.node:" + TEXT)

    if node_state["project"] != node_before["project"]:
        raise QaFailure("Node Editor selection mutated the authoritative Project")
    if node_state["history"] != node_before["history"]:
        raise QaFailure("Node Editor selection changed undo/redo history")
    return {
        "ok": True,
        "suite": "selection",
        "health": health,
        "initial_frame": initial["frame"],
        "final_frame": node_state["frame"],
        "clip_selection": clip_state["editor"]["selection"],
        "node_selection": node_state["editor"]["selection"],
        "actions": client.evidence,
    }


def run_smoke_suite(client, capture_path):
    """Edit a descriptor-driven operation through fresh screen coordinates."""
    health = client.wait_health()
    initial = wait_fresh_fixture(client)

    client.wait_component_settled("dock.tab:node_editor")
    client.click_component("dock.tab:node_editor")
    client.wait_until(
        "smoke Node Editor activation",
        lambda: client.state()
        if "Node Editor" in client.state()["dock"]["active_tabs"]
        else None,
    )
    clip_header = "node_editor.container_header.clip:" + CLIP_A2
    reveal_node_editor_component(client, clip_header)
    client.click_component(clip_header)
    client.wait_until(
        "Clip A2 coordinate selection",
        lambda: client.state()
        if selection_matches(client.state(), "clip", CLIP_A2)
        else None,
    )
    client.wait_component("inspector.owner.clip:" + CLIP_A2)

    tx_control = "inspector.property.node:{}:tx".format(TRANSFORM_EFFECTOR)
    _, control = ensure_operation_property(client, TRANSFORM_EFFECTOR, "tx")
    definition = (control.get("metadata") or {}).get("definition", {})
    if definition.get("ui", {}).get("kind") != "float":
        raise QaFailure("Transform tx did not use its descriptor Float UI")

    before = client.state()
    old_tx = property_value(before["project"]["nodes"][TRANSFORM_EFFECTOR], "tx")
    old_opacity = property_value(
        before["project"]["nodes"][OPACITY_EFFECTOR], "opacity"
    )
    preview_before = before["editor"]["preview"]
    # `drag_component_by` re-fetches the latest rect immediately before input,
    # emits a multi-frame egui drag in logical points, and polls `injected`.
    client.drag_component_by(tx_control, 32.0, 0.0, steps=10)
    edited = client.wait_project(
        "Transform operation tx coordinate edit",
        lambda project: property_value(project["nodes"][TRANSFORM_EFFECTOR], "tx")
        != old_tx,
    )
    if (
        property_value(edited["project"]["nodes"][OPACITY_EFFECTOR], "opacity")
        != old_opacity
    ):
        raise QaFailure("Transform tx edit mutated the independent Opacity operation")
    assert_history_delta(before, edited, 1, "Transform operation tx edit")
    validate_explicit_operation_fixture(edited["project"])
    rendered = client.wait_preview_change(
        preview_before["pixel_hash"], preview_before["render_revision"]
    )

    # End with a normal coordinate gesture as an independent hit-testing
    # check, then capture the viewport that reflects the authored edit.
    final = verify_final_preview_drag(client)
    capture = client.capture(capture_path)
    print("[qa-e2e] smoke Clip A2 -> Transform tx -> Project/Preview passed")
    return {
        "ok": True,
        "suite": "smoke",
        "health_frame": health["frame"],
        "initial_frame": initial["frame"],
        "edited_frame": edited["frame"],
        "rendered_frame": rendered["frame"],
        "final_frame": final["frame"],
        "final_history": final["history"],
        "final_preview": final["editor"]["preview"],
        "capture": capture,
        "actions": client.evidence,
    }


def run_node_wire_suite(client):
    """Exercise Node Add/context/wire/knife paths only through screen coordinates."""
    health = client.wait_health()
    initial = wait_fresh_fixture(client)
    client.wait_component_settled("dock.tab:node_editor")
    client.click_component("dock.tab:node_editor")
    client.wait_until(
        "Node Editor dock activation",
        lambda: client.state()
        if "Node Editor" in client.state()["dock"]["active_tabs"]
        else None,
    )

    # The shared Add catalog must be searchable by descriptor metadata, and
    # clicking the result must create the selected runtime operation Node.
    added_blur, blur_state, blur_metadata = create_node_from_add_search(
        client, "effect blur", "node_editor.menu.create.effect:blur"
    )
    blur_content = blur_state["project"]["nodes"][added_blur]["content"]
    if not (
        blur_content.get("type") == "PluginOperation"
        and blur_content.get("data", {}).get("category") == "effect"
        and blur_content.get("data", {}).get("component_id") == "blur"
    ):
        raise QaFailure("searchable Add menu did not create a Blur operation Node")
    if not (
        blur_metadata.get("component_id") == "blur"
        and str(blur_metadata.get("category", "")).startswith("Image Effects /")
    ):
        raise QaFailure("Blur Add item omitted descriptor/category QA metadata")
    delete_node_through_context_menu(client, added_blur)

    # Native numeric operations live in the same categorized Add catalog. Time
    # is deliberately not implicit: zoom to real Snarl pin interaction and
    # drag the already-connected container Time output to a new value input.
    # This is a real fan-out gesture and must leave the original Time wire in
    # place while adding the canonical Time -> value connection.
    fmod_node, fmod_state, fmod_metadata = create_node_from_add_search(
        client,
        "loop value",
        "node_editor.menu.create.value:fmod",
        "node_editor.container.clip:{}".format(CLIP_A1),
    )
    fmod_content = fmod_state["project"]["nodes"][fmod_node]["content"]
    if fmod_content != {"type": "Value", "data": "Fmod"}:
        raise QaFailure("Fmod Add item did not create the native numeric Node")
    if not (
        fmod_metadata.get("kind") == "fmod"
        and fmod_metadata.get("category") == "Math / Values"
    ):
        raise QaFailure("Fmod Add item omitted numeric/category QA metadata")
    owner_key = validate_canonical_ownership(fmod_state["project"])["node_owners"][
        fmod_node
    ]
    time_output = "node_editor.container_port.{}.internal_output:time".format(
        owner_key
    )
    value_input = "node_editor.port.node:{}.input:x".format(fmod_node)
    time_header = "node_editor.node_header:" + fmod_node
    move_snapshot, (header_component, output_component) = reveal_node_editor_components(
        client, [time_header, time_output]
    )
    old_position = list(fmod_state["project"]["nodes"][fmod_node]["ui_position"])
    start = client.point(header_component["rect_points"])
    output_point = client.point(output_component["rect_points"])
    canvas = next(
        item
        for item in move_snapshot["components"]
        if item["id"] == "node_editor.canvas"
    )["rect_points"]
    end = {
        "x": min(canvas["max_x"] - 20.0, output_point["x"] + 72.0),
        "y": min(canvas["max_y"] - 20.0, output_point["y"] + 28.0),
    }
    client.inject(
        "drag",
        {
            "from": start,
            "to": end,
            "coordinate_space": "points",
            "steps": 14,
            "button": "primary",
        },
        {
            "source_component_id": time_header,
            "target_component_id": time_output,
            "component_frame": move_snapshot["frame"],
            "coordinate_reason": "place the new value Node near its explicit Time source",
        },
    )
    client.wait_project(
        "Fmod coordinate move near Time output",
        lambda project: project["nodes"][fmod_node]["ui_position"] != old_position,
    )
    ensure_node_editor_ports_interactive(client, [time_output, value_input])
    connect_before = client.state()
    original_time_connections = [
        connection
        for connection in connect_before["project"]["connections"]
        if connection["from"]["owner"].get("owner_type") == "Clip"
        and connection["from"]["owner"].get("owner_id") == CLIP_A1
        and connection["from"]["port"] == "time"
    ]
    if not original_time_connections:
        raise QaFailure("fan-out fixture has no pre-existing Clip Time connection")
    client.drag_components(time_output, value_input, steps=16)

    def explicit_time_connection(project):
        return next(
            (
                connection
                for connection in project["connections"]
                if connection["from"]["port"] == "time"
                and connection["to"]["owner"].get("owner_type") == "Node"
                and connection["to"]["owner"].get("owner_id") == fmod_node
                and connection["to"]["port"] == "x"
            ),
            None,
        )

    connected = client.wait_project(
        "explicit container Time to Fmod.x connection",
        lambda project: explicit_time_connection(project) is not None,
    )
    for original_connection in original_time_connections:
        if project_connection(
            connected["project"], original_connection["id"]
        ) != original_connection:
            raise QaFailure("Time fan-out mutated an existing connection")
    time_connection = explicit_time_connection(connected["project"])
    assert_history_delta(connect_before, connected, 1, "explicit Time value connection")
    undo_project_edit(
        client,
        "explicit Time value connection",
        lambda project: all(
            connection["id"] != time_connection["id"]
            for connection in project["connections"]
        ),
    )
    delete_node_through_context_menu(client, fmod_node)

    # Node right-click owns Node commands, not the blank-canvas Add menu.
    text_header = "node_editor.node_header:" + TEXT
    reveal_node_editor_component(client, text_header)
    enabled_before = client.state()
    client.click_component(text_header, button="secondary")
    toggle_id = "node_editor.menu.toggle_enabled.node:" + TEXT
    client.wait_component(toggle_id)
    client.click_component(toggle_id)
    disabled = client.wait_project(
        "Node Disable command", lambda project: not project["nodes"][TEXT]["enabled"]
    )
    assert_history_delta(enabled_before, disabled, 1, "Node Disable command")
    client.click_component(text_header, button="secondary")
    client.wait_component(toggle_id)
    client.click_component(toggle_id)
    reenabled = client.wait_project(
        "Node Enable command", lambda project: project["nodes"][TEXT]["enabled"]
    )
    assert_history_delta(disabled, reenabled, 1, "Node Enable command")

    # A containment-derived wire is visible graph information, not an authored
    # connection. Its real secondary-click hit belongs to the wire surface but
    # opens neither wire commands nor the blank-canvas Add menu.
    derived_edge_id = "node_editor.edge.derived:track:{}:clip:{}".format(
        TRACK_A, CLIP_A1
    )
    reveal_node_editor_components(
        client,
        [
            "node_editor.container_port.clip:{}.external_output:image".format(
                CLIP_A1
            ),
            "node_editor.container_port.track:{}.image_sink:output_binding".format(
                TRACK_A
            ),
        ],
        margin=24.0,
    )
    reveal_node_editor_component(client, derived_edge_id)
    open_create_menu(client, operation="derived wire Add-menu precondition")
    client.wait_component("node_editor.menu.search")
    if not client.state()["editor"]["node_editor"]["context_menu_open"]:
        raise QaFailure("derived wire regression did not begin with an open Add menu")
    derived_before = client.wait_preview_settled("derived wire context no-op")
    _, derived_component, derived_hit_point = click_node_wire_hit_point(
        client,
        derived_edge_id,
        button="secondary",
        require_display_only=True,
    )
    derived_metadata = derived_component.get("metadata") or {}
    if not (
        derived_metadata.get("kind") == "derived_output"
        and derived_metadata.get("editable") is False
        and derived_metadata.get("action") is None
        and derived_metadata.get("edit_blocked_reason")
    ):
        raise QaFailure("derived wire omitted display-only QA semantics")
    derived_after = client.state()

    def fresh_derived_menu_snapshot():
        snapshot = client.component_snapshot()
        return snapshot if snapshot["frame"] >= derived_after["frame"] else None

    derived_snapshot = client.wait_until(
        "component registry after derived wire secondary click",
        fresh_derived_menu_snapshot,
    )
    node_editor_after = derived_after["editor"]["node_editor"]
    if (
        node_editor_after["context_menu_open"]
        or node_editor_after["wire_context_menu_open"]
        or node_editor_after["wire_context_menu_target"] is not None
    ):
        raise QaFailure("derived wire secondary click opened an editor menu")
    component_ids = {item["id"] for item in derived_snapshot["components"]}
    if any(
        component_id.startswith("node_editor.menu.create.")
        or component_id.startswith("node_editor.wire_menu")
        for component_id in component_ids
    ) or "node_editor.menu.search" in component_ids:
        raise QaFailure("derived wire secondary click exposed an Add or wire menu item")
    if derived_after["project"] != derived_before["project"]:
        raise QaFailure("derived wire secondary click changed the Project")
    if derived_after["history"] != derived_before["history"]:
        raise QaFailure("derived wire secondary click changed Project history")
    preview_fields = (
        "pixel_hash",
        "nontransparent_pixels",
        "modal_error",
        "texture_width",
        "texture_height",
        "region",
    )
    if any(
        derived_after["editor"]["preview"][field]
        != derived_before["editor"]["preview"][field]
        for field in preview_fields
    ):
        raise QaFailure("derived wire secondary click changed the Preview")

    original = find_project_connection(
        reenabled["project"], "Node", SOLID, "image", "Node", MERGE, "images"
    )
    connection_id = original["id"]
    edge_id = "node_editor.edge:" + connection_id
    reveal_node_editor_components(
        client,
        ["node_editor.node_header:" + SOLID, "node_editor.node_header:" + MERGE],
    )
    client.wait_component(edge_id)

    client.click_component(edge_id)
    client.wait_until(
        "wire coordinate selection",
        lambda: client.state()
        if client.state()["editor"]["node_editor"]["selected_connection_id"]
        == connection_id
        else None,
    )
    selected_before_clear = client.state()
    blank_snapshot, blank_point = find_free_canvas_point(client)
    client.inject(
        "click",
        {
            "x": blank_point["x"],
            "y": blank_point["y"],
            "coordinate_space": "points",
            "button": "primary",
        },
        {
            "component_id": "node_editor.canvas",
            "component_frame": blank_snapshot["frame"],
            "coordinate_reason": "fresh unobstructed canvas point clears wire selection",
            "cleared_connection_id": connection_id,
        },
    )
    deselected = client.wait_until(
        "wire coordinate deselection",
        lambda: client.state()
        if client.state()["editor"]["node_editor"]["selected_connection_id"] is None
        else None,
    )
    if deselected["project"] != selected_before_clear["project"]:
        raise QaFailure("clearing a wire selection changed the Project")
    if deselected["history"] != selected_before_clear["history"]:
        raise QaFailure("clearing a wire selection changed Project history")

    # Merge rows expose the canonical Back->Front order and per-wire authored
    # blend without requiring a precision wire hit. Every action below is a
    # fresh screen-coordinate click through the normal egui event path.
    def valid_preview():
        state = client.state()
        preview = state["editor"]["preview"]
        if (
            preview["pixel_hash"] is not None
            and preview["nontransparent_pixels"] is not None
            and preview["nontransparent_pixels"] > 0
            and preview["modal_error"] is None
        ):
            return state
        return None

    def merge_layers(project):
        return sorted(
            [
                connection
                for connection in project["connections"]
                if connection["to"]["owner"].get("owner_type") == "Node"
                and connection["to"]["owner"].get("owner_id") == MERGE
                and connection["to"]["port"] == "images"
            ],
            key=lambda connection: (connection["order"], connection["id"]),
        )

    def merge_row_id(kind, layer_connection_id, blend=None):
        if kind == "blend":
            return "node_editor.merge_layer.blend.{}:{}:{}".format(
                blend, MERGE, layer_connection_id
            )
        return "node_editor.merge_layer.{}:{}:{}".format(
            kind, MERGE, layer_connection_id
        )

    def click_merge_row_blend(layer_connection_id, current_blend, target_blend):
        select_id = merge_row_id("blend_select", layer_connection_id)
        reveal_node_editor_components(client, [select_id])
        _, select_component = client.wait_component_settled(select_id)
        select_metadata = select_component.get("metadata") or {}
        if not (
            select_metadata.get("action") == "open_authored_blend"
            and select_metadata.get("merge_id") == MERGE
            and select_metadata.get("connection_id") == layer_connection_id
            and select_metadata.get("authored_blend_mode") == current_blend
            and select_metadata.get("blend_ownership") == "connection"
        ):
            raise QaFailure("Merge row blend selector omitted wire-owned metadata")
        client.click_component(select_id)

        option_id = merge_row_id("blend", layer_connection_id, target_blend)
        _, option_component = client.wait_component_settled(option_id)
        option_metadata = option_component.get("metadata") or {}
        if not (
            option_metadata.get("action") == "set_authored_blend"
            and option_metadata.get("merge_id") == MERGE
            and option_metadata.get("connection_id") == layer_connection_id
            and option_metadata.get("blend_mode") == target_blend
            and option_metadata.get("coordinate_space") == "screen_points"
        ):
            raise QaFailure(
                "Merge popup blend option omitted its canonical wire or screen geometry"
            )
        client.click_component(option_id)

    layer_initial = client.wait_until("valid Merge preview", valid_preview)
    original_layers = merge_layers(layer_initial["project"])
    if len(original_layers) != 3 or original_layers[0]["id"] != connection_id:
        raise QaFailure(
            "Merge fixture must start with the Solid as the back-most of three layers"
        )
    original_connections = list(layer_initial["project"]["connections"])
    original_preview = dict(layer_initial["editor"]["preview"])
    original_by_id = {item["id"]: item for item in original_layers}
    second_blend_connection_id = original_layers[2]["id"]
    _, edge_component = client.component(edge_id)
    edge_metadata = edge_component.get("metadata") or {}
    if not (
        edge_metadata.get("authored_order") == original["order"]
        and edge_metadata.get("back_to_front_index") == 0
        and edge_metadata.get("layer_count") == 3
        and edge_metadata.get("authored_blend_mode") == "normal"
        and edge_metadata.get("authored_blend_available") is True
    ):
        raise QaFailure("Merge edge omitted canonical order/blend QA metadata")

    # Keep the precision wire menu as a second presentation of the same
    # ProjectConnection, then close it before driving the persistent rows.
    client.click_component(edge_id, button="secondary")
    order_id = "node_editor.wire_menu.order:" + connection_id
    _, order_component = client.wait_component(order_id)
    order_metadata = order_component.get("metadata") or {}
    if not (
        order_metadata.get("back_to_front_index") == 0
        and order_metadata.get("layer_count") == 3
        and order_metadata.get("authored_blend_mode") == "normal"
    ):
        raise QaFailure("Merge wire menu did not identify Back->Front index 0/3")
    client.key("escape", True)
    client.key("escape", False)

    def wire_menu_closed():
        snapshot = client.component_snapshot()
        return (
            snapshot
            if all(item["id"] != order_id for item in snapshot["components"])
            else None
        )

    client.wait_until("Merge wire menu close", wire_menu_closed)

    # Zoom by real Cmd/Ctrl-wheel input so the persistent row controls are
    # normal-size, then resolve their latest rectangles.
    ensure_node_editor_ports_interactive(
        client, ["node_editor.node_header:" + MERGE]
    )
    first_row_id = "node_editor.merge_layer:{}:{}".format(MERGE, connection_id)
    first_back_id = merge_row_id("order_back", connection_id)
    first_front_id = merge_row_id("order_front", connection_id)
    reveal_node_editor_components(client, [first_row_id, first_back_id, first_front_id])
    _, first_row = client.wait_component_settled(first_row_id)
    first_row_metadata = first_row.get("metadata") or {}
    if not (
        first_row_metadata.get("merge_id") == MERGE
        and first_row_metadata.get("connection_id") == connection_id
        and first_row_metadata.get("back_to_front_index") == 0
        and first_row_metadata.get("layer_count") == 3
        and first_row_metadata.get("authored_order") == original["order"]
        and first_row_metadata.get("authored_blend_mode") == "normal"
        and first_row_metadata.get("blend_ownership") == "connection"
        and (first_row_metadata.get("source") or {}).get("owner")
        == "node:" + SOLID
        and (first_row_metadata.get("source") or {}).get("label")
        == "Node · QA Solid"
    ):
        raise QaFailure("visible Merge row omitted source/order/blend identity")

    boundary_before = client.state()
    click_disabled_component(client, first_back_id)
    boundary_after = client.state()
    if (
        boundary_after["project"]["connections"] != original_connections
        or history_depth(boundary_after) != history_depth(boundary_before)
    ):
        raise QaFailure("disabled Merge row Move Back changed Project or history")

    client.click_component(first_front_id)
    expected_order_one = [
        original_layers[1]["id"],
        connection_id,
        original_layers[2]["id"],
    ]
    order_one_project = client.wait_project(
        "Solid Merge row moved one step toward Front",
        lambda project: [item["id"] for item in merge_layers(project)]
        == expected_order_one,
    )
    order_one_connection = project_connection(order_one_project["project"], connection_id)
    if not (
        order_one_connection["id"] == original["id"]
        and order_one_connection["from"] == original["from"]
        and order_one_connection["to"] == original["to"]
        and order_one_connection["order"] == 1
        and order_one_connection["blend_mode"] == original["blend_mode"]
    ):
        raise QaFailure("Merge row Move Front changed wire identity, endpoints, or blend")
    assert_history_delta(
        boundary_after, order_one_project, 1, "Merge row Move Front"
    )
    order_one_rendered = client.wait_preview_change(
        original_preview["pixel_hash"], original_preview["render_revision"]
    )
    order_one_connections = list(order_one_rendered["project"]["connections"])
    order_one_preview = dict(order_one_rendered["editor"]["preview"])

    blend_one_before = client.state()
    click_merge_row_blend(connection_id, "normal", "multiply")
    blend_one_project = client.wait_project(
        "first Merge row authored Multiply",
        lambda project: (
            project_connection(project, connection_id)["blend_mode"] == "Multiply"
            and project_connection(project, connection_id)["order"] == 1
        ),
    )
    blend_one_connection = project_connection(
        blend_one_project["project"], connection_id
    )
    for field in ("id", "from", "to", "order"):
        if blend_one_connection[field] != order_one_connection[field]:
            raise QaFailure("first Merge row blend changed wire {}".format(field))
    assert_history_delta(
        blend_one_before, blend_one_project, 1, "first Merge row authored Multiply"
    )
    blend_one_rendered = client.wait_preview_change(
        order_one_preview["pixel_hash"], order_one_preview["render_revision"]
    )
    blend_one_connections = list(blend_one_rendered["project"]["connections"])
    blended_preview = dict(blend_one_rendered["editor"]["preview"])

    second_blend_before = client.state()
    click_merge_row_blend(second_blend_connection_id, "normal", "add")
    dual_blend_project = client.wait_project(
        "second Merge row authored Add",
        lambda project: (
            project_connection(project, connection_id)["blend_mode"] == "Multiply"
            and project_connection(project, second_blend_connection_id)["blend_mode"]
            == "Add"
        ),
    )
    assert_history_delta(
        second_blend_before,
        dual_blend_project,
        1,
        "second Merge row authored Add",
    )
    for layer_id, authored in original_by_id.items():
        current = project_connection(dual_blend_project["project"], layer_id)
        for field in ("id", "from", "to"):
            if current[field] != authored[field]:
                raise QaFailure(
                    "independent Merge blend changed {} on {}".format(field, layer_id)
                )
    dual_blend_rendered = client.wait_preview_change(
        blended_preview["pixel_hash"], blended_preview["render_revision"]
    )
    dual_blend_connections = list(dual_blend_rendered["project"]["connections"])
    dual_blend_preview = dict(dual_blend_rendered["editor"]["preview"])

    # Reordering one row preserves both independently-authored modes.
    reveal_node_editor_components(client, [first_front_id])
    second_front_before = client.state()
    client.click_component(first_front_id)
    expected_front = [
        original_layers[1]["id"],
        second_blend_connection_id,
        connection_id,
    ]
    front_project = client.wait_project(
        "Multiply Merge row moved to the front",
        lambda project: [item["id"] for item in merge_layers(project)]
        == expected_front,
    )
    front_connection = project_connection(front_project["project"], connection_id)
    second_blended_connection = project_connection(
        front_project["project"], second_blend_connection_id
    )
    if not (
        front_connection["id"] == original["id"]
        and front_connection["from"] == original["from"]
        and front_connection["to"] == original["to"]
        and front_connection["order"] == 2
        and front_connection["blend_mode"] == "Multiply"
        and second_blended_connection["blend_mode"] == "Add"
        and second_blended_connection["order"] == 1
    ):
        raise QaFailure("Merge row reorder lost wire identity or independent blend")
    for layer_id, authored in original_by_id.items():
        current = project_connection(front_project["project"], layer_id)
        for field in ("id", "from", "to"):
            if current[field] != authored[field]:
                raise QaFailure(
                    "front-most Merge reorder changed {} on {}".format(field, layer_id)
                )
    assert_history_delta(
        second_front_before, front_project, 1, "Merge row second Move Front"
    )
    front_rendered = client.wait_preview_change(
        dual_blend_preview["pixel_hash"], dual_blend_preview["render_revision"]
    )
    front_connections = list(front_rendered["project"]["connections"])
    front_preview = dict(front_rendered["editor"]["preview"])

    _, front_row = client.wait_component_settled(first_row_id)
    front_row_metadata = front_row.get("metadata") or {}
    second_row_id = "node_editor.merge_layer:{}:{}".format(
        MERGE, second_blend_connection_id
    )
    _, second_row = client.wait_component_settled(second_row_id)
    second_row_metadata = second_row.get("metadata") or {}
    if not (
        front_row_metadata.get("back_to_front_index") == 2
        and front_row_metadata.get("authored_blend_mode") == "multiply"
        and second_row_metadata.get("back_to_front_index") == 1
        and second_row_metadata.get("authored_blend_mode") == "add"
    ):
        raise QaFailure("Merge rows did not retain two independent authored modes")

    last_boundary_before = client.state()
    click_disabled_component(client, first_front_id)
    last_boundary_after = client.state()
    if (
        last_boundary_after["project"]["connections"] != front_connections
        or history_depth(last_boundary_after) != history_depth(last_boundary_before)
        or last_boundary_after["editor"]["preview"]["pixel_hash"]
        != front_preview["pixel_hash"]
    ):
        raise QaFailure("disabled front-most Merge row action was not a strict no-op")

    # Undo all four row mutations, redo the exact sequence, then undo again so
    # subsequent wire tests receive the pristine fixture.
    undo_front = undo_project_edit(
        client,
        "front-most Merge row reorder",
        lambda project: project["connections"] == dual_blend_connections,
    )
    dual_restored = wait_preview_hash_after(
        client,
        dual_blend_preview["pixel_hash"],
        last_boundary_after["editor"]["preview"]["render_revision"],
        "front-most Merge row reorder Undo",
    )
    undo_second_blend = undo_project_edit(
        client,
        "second Merge row authored Add",
        lambda project: project["connections"] == blend_one_connections,
    )
    blend_one_restored = wait_preview_hash_after(
        client,
        blended_preview["pixel_hash"],
        dual_restored["editor"]["preview"]["render_revision"],
        "second Merge row authored Add Undo",
    )
    undo_first_blend = undo_project_edit(
        client,
        "first Merge row authored Multiply",
        lambda project: project["connections"] == order_one_connections,
    )
    order_one_restored = wait_preview_hash_after(
        client,
        order_one_preview["pixel_hash"],
        blend_one_restored["editor"]["preview"]["render_revision"],
        "first Merge row authored Multiply Undo",
    )
    undo_order = undo_project_edit(
        client,
        "Merge row Move Front",
        lambda project: project["connections"] == original_connections,
    )
    merge_restored = wait_preview_hash_after(
        client,
        original_preview["pixel_hash"],
        order_one_restored["editor"]["preview"]["render_revision"],
        "Merge row order Undo",
    )

    redo_order = redo_project_edit(
        client,
        "Merge row Move Front",
        lambda project: project["connections"] == order_one_connections,
    )
    redo_first_blend = redo_project_edit(
        client,
        "first Merge row authored Multiply",
        lambda project: project["connections"] == blend_one_connections,
    )
    redo_second_blend = redo_project_edit(
        client,
        "second Merge row authored Add",
        lambda project: project["connections"] == dual_blend_connections,
    )
    redo_front = redo_project_edit(
        client,
        "front-most Merge row reorder",
        lambda project: project["connections"] == front_connections,
    )
    redone_rendered = wait_preview_hash_after(
        client,
        front_preview["pixel_hash"],
        merge_restored["editor"]["preview"]["render_revision"],
        "Merge row order/blend Redo sequence",
    )

    undo_project_edit(
        client,
        "front-most Merge row reorder after Redo",
        lambda project: project["connections"] == dual_blend_connections,
    )
    undo_project_edit(
        client,
        "second Merge row authored Add after Redo",
        lambda project: project["connections"] == blend_one_connections,
    )
    undo_project_edit(
        client,
        "first Merge row authored Multiply after Redo",
        lambda project: project["connections"] == order_one_connections,
    )
    undo_project_edit(
        client,
        "Merge row Move Front after Redo",
        lambda project: project["connections"] == original_connections,
    )
    merge_restored = wait_preview_hash_after(
        client,
        original_preview["pixel_hash"],
        redone_rendered["editor"]["preview"]["render_revision"],
        "Merge row order/blend final restoration",
    )
    if any(
        state["editor"]["preview"]["modal_error"] is not None
        for state in (
            undo_front,
            undo_second_blend,
            undo_first_blend,
            undo_order,
            redo_order,
            redo_first_blend,
            redo_second_blend,
            redo_front,
            merge_restored,
        )
    ):
        raise QaFailure("Merge row order/blend Undo/Redo surfaced a render error")

    # Secondary-click opens a wire command menu; Delete is one undoable
    # canonical mutation and Undo restores the exact UUID/target/order tuple.
    delete_before = client.state()
    client.click_component(edge_id, button="secondary")
    wire_delete_id = "node_editor.wire_menu.delete:" + connection_id
    client.wait_component(wire_delete_id)
    client.click_component(wire_delete_id)
    deleted = client.wait_project(
        "wire context Delete",
        lambda project: project_connection(project, connection_id) is None,
    )
    assert_history_delta(delete_before, deleted, 1, "wire context Delete")
    restored = undo_project_edit(
        client,
        "wire context Delete",
        lambda project: project_connection(project, connection_id) == original,
    )

    # A container output binding is authored state, but not a ProjectConnection.
    # Its visible curve owns the same coordinate context-menu workflow. Clear
    # only output_node_id, prove the Preview changes, then exercise both Undo
    # and Redo before restoring the fixture for the remaining scenarios.
    binding_owner_key = "clip:" + CLIP_A1
    binding_edge_id = "node_editor.edge.output_binding:{}:{}".format(
        binding_owner_key, MERGE
    )
    reveal_node_editor_components(
        client,
        ["node_editor.node_header:" + MERGE, binding_edge_id],
    )
    _, binding_component = client.wait_component(binding_edge_id)
    binding_metadata = binding_component.get("metadata") or {}
    if not (
        binding_metadata.get("kind") == "output_binding"
        and binding_metadata.get("editable") is True
        and binding_metadata.get("action") == "delete_output_binding"
        and binding_metadata.get("binding_owner") == binding_owner_key
        and binding_metadata.get("binding_node_id") == MERGE
        and binding_metadata.get("connection_id") is None
    ):
        raise QaFailure("output binding edge omitted typed editable QA metadata")
    binding_before = client.state()
    binding_connections = list(binding_before["project"]["connections"])
    binding_preview = dict(binding_before["editor"]["preview"])
    client.click_component(binding_edge_id, button="secondary")
    binding_stable_key = "output_binding:{}:{}".format(binding_owner_key, MERGE)
    binding_delete_id = "node_editor.wire_menu.delete:" + binding_stable_key
    _, binding_delete = client.wait_component(binding_delete_id)
    if (binding_delete.get("metadata") or {}).get("action") != "clear_output_binding":
        raise QaFailure("output binding Delete menu omitted canonical action metadata")
    client.click_component(binding_delete_id)
    binding_deleted = client.wait_project(
        "output binding context Delete",
        lambda project: (
            project["clips"][CLIP_A1]["output_node_id"] is None
            and project["connections"] == binding_connections
        ),
    )
    assert_history_delta(
        binding_before, binding_deleted, 1, "output binding context Delete"
    )
    binding_deleted_rendered = client.wait_preview_change(
        binding_preview["pixel_hash"], binding_preview["render_revision"]
    )
    binding_restored = undo_project_edit(
        client,
        "output binding context Delete",
        lambda project: (
            project["clips"][CLIP_A1]["output_node_id"] == MERGE
            and project["connections"] == binding_connections
        ),
    )
    binding_restored_rendered = wait_preview_hash_after(
        client,
        binding_preview["pixel_hash"],
        binding_deleted_rendered["editor"]["preview"]["render_revision"],
        "output binding context Delete Undo",
    )
    binding_redone = redo_project_edit(
        client,
        "output binding context Delete",
        lambda project: (
            project["clips"][CLIP_A1]["output_node_id"] is None
            and project["connections"] == binding_connections
        ),
    )
    binding_redone_rendered = wait_preview_hash_after(
        client,
        binding_deleted_rendered["editor"]["preview"]["pixel_hash"],
        binding_restored_rendered["editor"]["preview"]["render_revision"],
        "output binding context Delete Redo",
    )
    binding_final_restore = undo_project_edit(
        client,
        "output binding context Delete after Redo",
        lambda project: (
            project["clips"][CLIP_A1]["output_node_id"] == MERGE
            and project["connections"] == binding_connections
        ),
    )
    wait_preview_hash_after(
        client,
        binding_preview["pixel_hash"],
        binding_redone_rendered["editor"]["preview"]["render_revision"],
        "output binding final Undo",
    )

    # A normal primary drag from the actual curve midpoint is disconnect, not
    # a test-only command. It uses the same canonical operation and undo path.
    reveal_node_editor_components(
        client,
        ["node_editor.node_header:" + SOLID, "node_editor.node_header:" + MERGE],
    )
    client.wait_component(edge_id)
    drag_delete_before = client.state()
    client.drag_component_by(edge_id, 0.0, 44.0, steps=10)
    drag_deleted = client.wait_project(
        "wire body drag disconnect",
        lambda project: project_connection(project, connection_id) is None,
    )
    assert_history_delta(
        drag_delete_before, drag_deleted, 1, "wire body drag disconnect"
    )
    restored = undo_project_edit(
        client,
        "wire body drag disconnect",
        lambda project: project_connection(project, connection_id) == original,
    )

    # Reconnect the persistent source endpoint to Track B's independent image
    # output. This also exercises a legal cross-container wire without adding
    # test-only graph state. UUID, target, and order must remain byte-for-byte.
    alternate_output = (
        "node_editor.container_port.track:{}.external_output:image".format(TRACK_B)
    )
    source_handle = edge_id + ".from_handle"
    reveal_node_editor_components(client, [source_handle, alternate_output])
    reconnect_before = client.state()
    client.drag_components(source_handle, alternate_output, steps=14)
    reconnected = client.wait_project(
        "wire endpoint reconnect",
        lambda project: (
            project_connection(project, connection_id) is not None
            and project_connection(project, connection_id)["from"]["owner"]["owner_id"]
            == TRACK_B
            and project_connection(project, connection_id)["from"]["owner"]["owner_type"]
            == "Track"
        ),
    )
    moved_connection = project_connection(reconnected["project"], connection_id)
    if not (
        moved_connection["id"] == original["id"]
        and moved_connection["to"] == original["to"]
        and moved_connection["order"] == original["order"]
    ):
        raise QaFailure("endpoint reconnect changed downstream wire identity/order/target")
    assert_history_delta(reconnect_before, reconnected, 1, "wire endpoint reconnect")
    restored = undo_project_edit(
        client,
        "wire endpoint reconnect",
        lambda project: project_connection(project, connection_id) == original,
    )

    # Wire right-click insertion uses the same categorized searchable catalog.
    reveal_node_editor_components(
        client,
        ["node_editor.node_header:" + SOLID, "node_editor.node_header:" + MERGE],
    )
    client.wait_component(edge_id)
    insert_before = client.state()
    nodes_before = set(insert_before["project"]["nodes"])
    client.click_component(edge_id, button="secondary")
    client.wait_component("node_editor.wire_menu.insert:" + connection_id)
    client.click_component("node_editor.wire_menu.insert:" + connection_id)
    client.wait_component("node_editor.wire_menu.search")
    client.replace_component_text("node_editor.wire_menu.search", "blur")
    splice_item = "node_editor.wire_menu.operation.effect:blur"
    _, splice_component = client.wait_component_settled(splice_item)
    if (splice_component.get("metadata") or {}).get("action") != "splice":
        raise QaFailure("wire insertion item omitted splice QA action metadata")
    client.click_component(splice_item)
    inserted = client.wait_project(
        "wire menu Blur insertion",
        lambda project: len(project["nodes"]) == len(nodes_before) + 1
        and project_connection(project, connection_id) is not None
        and project_connection(project, connection_id)["from"]["owner"]["owner_id"]
        not in nodes_before,
    )
    inserted_node = (set(inserted["project"]["nodes"]) - nodes_before).pop()
    inserted_connection = project_connection(inserted["project"], connection_id)
    if not (
        inserted_connection["to"] == original["to"]
        and inserted_connection["order"] == original["order"]
        and inserted["project"]["nodes"][inserted_node]["content"]["data"]
        ["component_id"]
        == "blur"
    ):
        raise QaFailure("wire menu insertion did not preserve downstream identity")
    assert_history_delta(insert_before, inserted, 1, "wire menu Blur insertion")
    restored = undo_project_edit(
        client,
        "wire menu Blur insertion",
        lambda project: project_connection(project, connection_id) == original
        and inserted_node not in project["nodes"],
    )

    # Dropping an existing operation Node on the rendered curve splices it
    # through the same canonical API and remains one undoable coordinate drag.
    drop_blur, _, _ = create_node_from_add_search(
        client,
        "effect blur",
        "node_editor.menu.create.effect:blur",
        "node_editor.container.clip:" + CLIP_A1,
    )
    if (
        validate_canonical_ownership(client.state()["project"])["node_owners"].get(
            drop_blur
        )
        != "clip:" + CLIP_A1
    ):
        raise QaFailure("wire-drop operation was not created in the wire's Clip")
    reveal_node_editor_components(
        client, ["node_editor.node_header:" + drop_blur, edge_id]
    )
    drop_before = client.state()
    drag_component_to_node_wire_hit_point(
        client, "node_editor.node_header:" + drop_blur, edge_id, steps=16
    )
    dropped = client.wait_project(
        "existing operation drop splice",
        lambda project: project_connection(project, connection_id) is not None
        and project_connection(project, connection_id)["from"]["owner"]["owner_id"]
        == drop_blur,
    )
    dropped_connection = project_connection(dropped["project"], connection_id)
    if not (
        dropped_connection["to"] == original["to"]
        and dropped_connection["order"] == original["order"]
        and any(
            connection["to"]["owner"].get("owner_id") == drop_blur
            and connection["from"] == original["from"]
            for connection in dropped["project"]["connections"]
        )
    ):
        raise QaFailure("existing Node drop splice changed downstream identity")
    assert_history_delta(drop_before, dropped, 1, "existing operation drop splice")
    restored = undo_project_edit(
        client,
        "existing operation drop splice",
        lambda project: project_connection(project, connection_id) == original,
    )
    delete_node_through_context_menu(client, drop_blur)

    # Finally, execute the Blender-like multi-wire knife as an Alt/Option
    # primary drag beginning on a verified blank canvas coordinate.
    reveal_node_editor_components(
        client,
        ["node_editor.node_header:" + TEXT, "node_editor.node_header:" + BLUR_EFFECT],
    )
    knife_before = client.state()
    connections_before_knife = knife_before["project"]["connections"]
    connection_ids_before = {
        connection["id"] for connection in connections_before_knife
    }
    knife_snapshot = wait_wire_snapshot_for_project(client, connection_ids_before)
    knife_start, knife_end, planned_ids = find_wire_knife_gesture(knife_snapshot)
    client.inject(
        "drag",
        {
            "from": knife_start,
            "to": knife_end,
            "coordinate_space": "points",
            "steps": 18,
            "button": "primary",
            "modifiers": {"alt": True},
        },
        {
            "component_id": "node_editor.knife_surface",
            "component_frame": knife_snapshot["frame"],
            "target_connection_ids": planned_ids,
            "coordinate_reason": "blank-origin line through two canonical wire hit points",
        },
    )
    knifed = client.wait_project(
        "multi-wire knife",
        lambda project: len(connection_ids_before)
        - len({connection["id"] for connection in project["connections"]})
        >= 2,
    )
    remaining_ids = {
        connection["id"] for connection in knifed["project"]["connections"]
    }
    removed_ids = connection_ids_before - remaining_ids
    if len(removed_ids) < 2 or not set(planned_ids).issubset(removed_ids):
        raise QaFailure(
            "Alt/Option wire knife removed {}, but planned curve hits were {}".format(
                sorted(removed_ids), sorted(planned_ids)
            )
        )
    assert_history_delta(knife_before, knifed, 1, "multi-wire knife")
    undo_project_edit(
        client,
        "multi-wire knife",
        lambda project: project["connections"] == connections_before_knife,
    )

    # One Alt stroke may cross heterogeneous authored wires. The output
    # binding and every explicit connection are committed as one Project
    # snapshot, while derived containment wires remain display-only.
    reveal_node_editor_components(client, [edge_id, binding_edge_id])
    mixed_before = client.state()
    mixed_connections_before = list(mixed_before["project"]["connections"])
    mixed_preview_before = dict(mixed_before["editor"]["preview"])
    mixed_snapshot = wait_wire_snapshot_for_project(
        client, {connection["id"] for connection in mixed_connections_before}
    )
    derived_edges = [
        item
        for item in mixed_snapshot["components"]
        if (item.get("metadata") or {}).get("kind") == "derived_output"
    ]
    if not derived_edges or any(
        (item.get("metadata") or {}).get("editable") is not False
        or not (item.get("metadata") or {}).get("edit_blocked_reason")
        or (item.get("metadata") or {}).get("action") is not None
        for item in derived_edges
    ):
        raise QaFailure("derived wires did not publish display-only QA semantics")
    mixed_start, mixed_end, mixed_planned = find_mixed_wire_knife_gesture(
        mixed_snapshot, binding_edge_id
    )
    client.inject(
        "drag",
        {
            "from": mixed_start,
            "to": mixed_end,
            "coordinate_space": "points",
            "steps": 18,
            "button": "primary",
            "modifiers": {"alt": True},
        },
        {
            "component_id": "node_editor.knife_surface",
            "component_frame": mixed_snapshot["frame"],
            "target_wires": mixed_planned,
            "coordinate_reason": "blank-origin line through explicit and output-binding hit points",
        },
    )
    mixed_knifed = client.wait_project(
        "mixed explicit/output-binding knife",
        lambda project: (
            project["clips"][CLIP_A1]["output_node_id"] is None
            and project_connection(project, mixed_planned["connection_id"]) is None
        ),
    )
    mixed_remaining_ids = {
        connection["id"] for connection in mixed_knifed["project"]["connections"]
    }
    mixed_removed_ids = {
        connection["id"] for connection in mixed_connections_before
    } - mixed_remaining_ids
    if mixed_planned["connection_id"] not in mixed_removed_ids:
        raise QaFailure("mixed knife did not remove its planned explicit wire")
    validate_canonical_ownership(mixed_knifed["project"])
    assert_history_delta(mixed_before, mixed_knifed, 1, "mixed wire knife")
    mixed_knifed_rendered = client.wait_preview_change(
        mixed_preview_before["pixel_hash"], mixed_preview_before["render_revision"]
    )
    mixed_restored = undo_project_edit(
        client,
        "mixed wire knife",
        lambda project: (
            project["connections"] == mixed_connections_before
            and project["clips"][CLIP_A1]["output_node_id"] == MERGE
        ),
    )
    mixed_restored_rendered = wait_preview_hash_after(
        client,
        mixed_preview_before["pixel_hash"],
        mixed_knifed_rendered["editor"]["preview"]["render_revision"],
        "mixed wire knife Undo",
    )
    mixed_redone = redo_project_edit(
        client,
        "mixed wire knife",
        lambda project: (
            project["clips"][CLIP_A1]["output_node_id"] is None
            and project_connection(project, mixed_planned["connection_id"]) is None
        ),
    )
    mixed_redone_rendered = wait_preview_hash_after(
        client,
        mixed_knifed_rendered["editor"]["preview"]["pixel_hash"],
        mixed_restored_rendered["editor"]["preview"]["render_revision"],
        "mixed wire knife Redo",
    )
    undo_project_edit(
        client,
        "mixed wire knife after Redo",
        lambda project: (
            project["connections"] == mixed_connections_before
            and project["clips"][CLIP_A1]["output_node_id"] == MERGE
        ),
    )
    wait_preview_hash_after(
        client,
        mixed_preview_before["pixel_hash"],
        mixed_redone_rendered["editor"]["preview"]["render_revision"],
        "mixed wire knife final Undo",
    )

    final = client.state()
    validate_explicit_operation_fixture(final["project"])
    validate_canonical_ownership(final["project"])
    print(
        "[qa-e2e] searchable Add/context/wire/order/blend/reconnect/splice/knife passed"
    )
    return {
        "ok": True,
        "suite": "node-wire",
        "health_frame": health["frame"],
        "initial_frame": initial["frame"],
        "final_frame": final["frame"],
        "removed_by_knife": sorted(removed_ids),
        "derived_wire_context": {
            "edge_id": derived_edge_id,
            "hit_point": derived_hit_point,
            "blocked_reason": derived_metadata["edit_blocked_reason"],
            "preview_hash": derived_before["editor"]["preview"]["pixel_hash"],
        },
        "output_binding_wire": {
            "edge_id": binding_edge_id,
            "owner": binding_owner_key,
            "node_id": MERGE,
            "preview_hashes": {
                "bound": binding_preview["pixel_hash"],
                "cleared": binding_deleted_rendered["editor"]["preview"]["pixel_hash"],
            },
        },
        "mixed_knife": {
            "planned": mixed_planned,
            "removed_connection_ids": sorted(mixed_removed_ids),
            "preview_hashes": {
                "bound": mixed_preview_before["pixel_hash"],
                "cut": mixed_knifed_rendered["editor"]["preview"]["pixel_hash"],
            },
        },
        "merge_wire": {
            "connection_id": connection_id,
            "back_to_front_ids": [item["id"] for item in original_layers],
            "preview_hashes": {
                "original": original_preview["pixel_hash"],
                "order_one": order_one_preview["pixel_hash"],
                "multiply": blended_preview["pixel_hash"],
                "front": front_preview["pixel_hash"],
                "restored": merge_restored["editor"]["preview"]["pixel_hash"],
            },
        },
        "final_history": final["history"],
        "actions": client.evidence,
    }


def run_suite(client):
    health = client.wait_health()
    initial = wait_fresh_fixture(client)

    print("[qa-e2e] bridge healthy at frame {}".format(health["frame"]))
    client.wait_component_settled("dock.tab:node_editor")
    client.click_component("dock.tab:node_editor")
    client.wait_until(
        "Node Editor dock activation",
        lambda: client.state() if "Node Editor" in client.state()["dock"]["active_tabs"] else None,
    )

    # Selection is a genuine transformed-canvas click on the latest title rect.
    client.click_component("node_editor.node_header:" + TEXT)
    client.wait_until(
        "Text Node selection",
        lambda: client.state()
        if selection_matches(client.state(), "node", TEXT)
        else None,
    )
    client.wait_component("inspector.owner.node:" + TEXT)
    print("[qa-e2e] transformed Node title selection passed")

    # Create and delete a real Node through the two context menus.
    node_create_before = client.state()
    node_ids_before = set(node_create_before["project"]["nodes"])
    positions_before = node_editor_layout_positions(node_create_before["project"])
    connections_before = node_create_before["project"]["connections"]
    open_create_menu(client)
    client.wait_component("node_editor.menu.search")
    client.replace_component_text("node_editor.menu.search", "Text")
    client.wait_component("node_editor.menu.create.text")
    client.click_component("node_editor.menu.create.text")
    created_state = client.wait_project(
        "Text Node creation", lambda current: len(current["nodes"]) == len(node_ids_before) + 1
    )
    created_node = (set(created_state["project"]["nodes"]) - node_ids_before).pop()
    positions_after = node_editor_layout_positions(created_state["project"])
    positions_after["nodes"].pop(created_node)
    if positions_after != positions_before:
        raise QaFailure("local Node creation globally reflowed an existing item")
    if created_state["project"]["connections"] != connections_before:
        raise QaFailure("local Node creation changed an existing connection")
    validate_canonical_ownership(created_state["project"])
    client.wait_component("node_editor.node:" + created_node)
    client.click_component("node_editor.node_header:" + created_node, button="secondary")
    delete_id = "node_editor.menu.delete.node:" + created_node
    client.wait_component(delete_id)
    if client.state()["editor"]["node_editor"]["context_menu_open"]:
        raise QaFailure("Node delete gesture also opened the canvas create menu")
    client.click_component(delete_id)
    client.wait_project("created Node deletion", lambda current: created_node not in current["nodes"])
    print("[qa-e2e] Node create/right-click delete passed")

    # Create and remove a Track container, proving container UI is model-backed.
    track_create_before = client.state()
    tracks_before = set(track_create_before["project"]["tracks"])
    positions_before = node_editor_layout_positions(track_create_before["project"])
    connections_before = track_create_before["project"]["connections"]
    open_create_menu(client)
    client.wait_component("node_editor.menu.create.track")
    client.click_component("node_editor.menu.create.track")
    track_state = client.wait_project(
        "Track container creation",
        lambda current: len(current["tracks"]) == len(tracks_before) + 1,
    )
    created_track = (set(track_state["project"]["tracks"]) - tracks_before).pop()
    positions_after = node_editor_layout_positions(track_state["project"])
    positions_after["tracks"].pop(created_track)
    if positions_after != positions_before:
        raise QaFailure("local Track creation globally reflowed an existing item")
    if track_state["project"]["connections"] != connections_before:
        raise QaFailure("local Track creation changed an existing connection")
    validate_canonical_ownership(track_state["project"])
    header_id = "node_editor.container_header.track:" + created_track
    reveal_node_editor_component(client, header_id)
    client.click_component(header_id, button="secondary")
    delete_track_id = "node_editor.menu.delete.track:" + created_track
    client.wait_component(delete_track_id)
    if client.state()["editor"]["node_editor"]["context_menu_open"]:
        raise QaFailure("Track delete gesture also opened the canvas create menu")
    client.click_component(delete_track_id)
    client.wait_project(
        "created Track container deletion",
        lambda current: created_track not in current["tracks"],
    )
    print("[qa-e2e] Track container create/delete passed")

    # Track creation may use the first non-overlapping slot below a dense
    # composition. Return to the fixture Nodes through real middle-button pan.
    reveal_node_editor_component(client, "node_editor.node_header:" + TEXT)

    validate_explicit_operation_fixture(client.state()["project"])
    print("[qa-e2e] explicit Shape/Image operation graph contract passed")

    # Text edit through the Inspector. Commit by clicking the owner heading.
    reveal_node_editor_component(client, "node_editor.node_header:" + TEXT)
    client.click_component("node_editor.node_header:" + TEXT)
    text_before = client.state()
    text_preview = text_before["editor"]["preview"]
    new_text = "QA Text Updated"
    text_control = "inspector.property.node:{}:text".format(TEXT)
    client.replace_text(text_control, new_text)
    client.click_component("inspector.owner.node:" + TEXT)
    text_after = client.wait_project(
        "Text property update",
        lambda current: property_value(current["nodes"][TEXT], "text") == new_text
        and current["nodes"][TEXT]["content"]["data"] == "Text",
    )
    client.wait_preview_change(text_preview["pixel_hash"], text_preview["render_revision"])
    assert_history_delta(text_before, text_after, 1, "Text edit")
    print("[qa-e2e] Text UI -> Project -> Preview passed")

    # Shape path uses the same UI event path, including real wheel scrolling if
    # the multiline control is below the current Inspector viewport.
    reveal_node_editor_component(client, "node_editor.node_header:" + SHAPE)
    client.click_component("node_editor.node_header:" + SHAPE)
    client.wait_until(
        "Shape Node selection",
        lambda: client.state()
        if selection_matches(client.state(), "node", SHAPE)
        else None,
    )
    shape_before = client.state()
    shape_preview = shape_before["editor"]["preview"]
    new_path = "M 0 0 H 220 V 130 H 0 Z"
    shape_control = "inspector.property.node:{}:path".format(SHAPE)
    client.replace_text(shape_control, new_path)
    client.click_component("inspector.owner.node:" + SHAPE)
    shape_after = client.wait_project(
        "Shape property update",
        lambda current: property_value(current["nodes"][SHAPE], "path") == new_path
        and current["nodes"][SHAPE]["content"]["data"] == "Shape",
    )
    client.wait_preview_change(shape_preview["pixel_hash"], shape_preview["render_revision"])
    assert_history_delta(shape_before, shape_after, 1, "Shape edit")
    print("[qa-e2e] Shape UI -> Project -> Preview passed")

    # Select Clip A2 itself, then edit its explicit Transform operation through
    # the semantic Inspector facade. The property belongs directly to Node 501.
    clip_header = "node_editor.container_header.clip:" + CLIP_A2
    reveal_node_editor_component(client, clip_header)
    client.click_component(clip_header)
    client.wait_until(
        "Clip A2 selection for operation editing",
        lambda: client.state()
        if selection_matches(client.state(), "clip", CLIP_A2)
        else None,
    )
    tx_id = "inspector.property.node:{}:tx".format(TRANSFORM_EFFECTOR)
    ensure_operation_property(client, TRANSFORM_EFFECTOR, "tx")
    tx_before = client.state()
    old_tx = property_value(tx_before["project"]["nodes"][TRANSFORM_EFFECTOR], "tx")
    opacity_before = property_value(tx_before["project"]["nodes"][OPACITY_EFFECTOR], "opacity")
    preview_before = tx_before["editor"]["preview"]
    client.drag_component_by(tx_id, 24.0, 0.0)
    tx_after = client.wait_project(
        "Transform operation property edit",
        lambda current: property_value(current["nodes"][TRANSFORM_EFFECTOR], "tx")
        != old_tx,
    )
    if property_value(tx_after["project"]["nodes"][OPACITY_EFFECTOR], "opacity") != opacity_before:
        raise QaFailure("editing Transform changed the independent Opacity operation")
    assert_history_delta(tx_before, tx_after, 1, "Transform property drag")
    client.wait_preview_change(preview_before["pixel_hash"], preview_before["render_revision"])

    keyframe_id = "inspector.keyframe.node:{}:tx".format(TRANSFORM_EFFECTOR)
    client.ensure_in_scroll_area(keyframe_id)
    key_before = client.state()
    client.click_component(keyframe_id)
    key_after = client.wait_project(
        "Transform operation keyframe",
        lambda current: current["nodes"][TRANSFORM_EFFECTOR]["properties"]["tx"]["type"]
        == "keyframe",
    )
    assert_history_delta(key_before, key_after, 1, "Transform keyframe")
    print("[qa-e2e] Clip facade -> direct operation property/keyframe passed")

    timeline_state = run_timeline_scenario(client)
    assert_node_editor_reflection(client, timeline_state)
    pan_after = verify_final_preview_drag(client)

    return {
        "ok": True,
        "suite": "all",
        "initial_frame": initial["frame"],
        "final_frame": pan_after["frame"],
        "final_history": pan_after["history"],
        "final_preview": pan_after["editor"]["preview"],
        "actions": client.evidence,
    }


def free_port():
    with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as listener:
        listener.bind(("127.0.0.1", 0))
        return listener.getsockname()[1]


def repository_git_commit():
    try:
        return subprocess.check_output(
            ["git", "rev-parse", "HEAD"],
            cwd=os.path.dirname(os.path.dirname(os.path.abspath(__file__))),
            text=True,
        ).strip()
    except (OSError, subprocess.CalledProcessError):
        return None


def parse_args():
    parser = argparse.ArgumentParser()
    parser.add_argument("--base-url", default=None)
    parser.add_argument(
        "--suite",
        choices=("all", "timeline", "selection", "smoke", "node-wire"),
        default="all",
        help="run the complete suite or a focused Timeline, smoke, or Node wire suite",
    )
    parser.add_argument(
        "--spawn",
        action="store_true",
        help="launch a fresh headful app with the deterministic fixture",
    )
    parser.add_argument(
        "--evidence",
        default="target/qa-e2e-evidence.json",
        help="JSON evidence output path",
    )
    parser.add_argument("--timeout", type=float, default=30.0)
    parser.add_argument(
        "--capture",
        default=None,
        help="PNG evidence path (required automatically by the smoke suite)",
    )
    return parser.parse_args()


def main():
    args = parse_args()
    process = None
    port = free_port() if args.spawn else 39091
    base_url = args.base_url or "http://127.0.0.1:{}".format(port)
    try:
        if args.spawn:
            environment = os.environ.copy()
            environment["RUVIE_QA_PORT"] = str(port)
            environment["RUVIE_QA_FIXTURE"] = "node_editor_e2e"
            process = subprocess.Popen(
                ["cargo", "run", "-p", "app", "--locked"],
                env=environment,
                start_new_session=True,
            )
        client = QaClient(base_url, args.timeout)
        if args.suite == "timeline":
            result = run_timeline_suite(client)
        elif args.suite == "selection":
            result = run_selection_suite(client)
        elif args.suite == "smoke":
            capture_path = args.capture or "target/qa-smoke-evidence.png"
            result = run_smoke_suite(client, capture_path)
        elif args.suite == "node-wire":
            result = run_node_wire_suite(client)
        else:
            result = run_suite(client)
        result["run_id"] = os.environ.get("RUVIE_QA_RUN_ID")
        result["git_commit"] = repository_git_commit()
        result["component_frame"] = client.component_snapshot()["frame"]
        result["action_count"] = len(result.get("actions", []))
        evidence_path = os.path.abspath(args.evidence)
        os.makedirs(os.path.dirname(evidence_path), exist_ok=True)
        with open(evidence_path, "w", encoding="utf-8") as output:
            json.dump(result, output, ensure_ascii=False, indent=2)
            output.write("\n")
        print("[qa-e2e] PASS; evidence: {}".format(evidence_path))
        return 0
    except (QaFailure, AssertionError, KeyError, IndexError) as error:
        print("[qa-e2e] FAIL: {}".format(error), file=sys.stderr)
        return 1
    finally:
        if process is not None:
            if process.poll() is None:
                try:
                    os.killpg(process.pid, signal.SIGTERM)
                except ProcessLookupError:
                    pass
            try:
                process.wait(timeout=5.0)
            except subprocess.TimeoutExpired:
                try:
                    os.killpg(process.pid, signal.SIGKILL)
                except ProcessLookupError:
                    pass
                process.wait(timeout=2.0)


if __name__ == "__main__":
    sys.exit(main())
