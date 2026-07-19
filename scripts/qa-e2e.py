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
            {"x": point["x"], "y": point["y"], "button": button},
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
            {"from": start, "to": end, "steps": steps, "button": "primary"},
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
            {"from": start, "to": end, "steps": steps, "button": "primary"},
            {
                "source_component_id": component_id,
                "component_frame": snapshot["frame"],
                "source_rect_points": component["rect_points"],
            },
        )
        return start, end

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
            {"from": start, "to": end, "steps": steps, "button": "primary"},
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


def find_free_canvas_point(client):
    snapshot = client.component_snapshot()
    components = snapshot["components"]
    canvas_component = next(
        item for item in components if item["id"] == "node_editor.canvas"
    )
    canvas = canvas_component["rect_points"]
    containers = [
        item
        for item in components
        if item["id"] == "node_editor.container.composition:{}".format(COMPOSITION)
    ]
    search = containers[0]["rect_points"] if containers else canvas
    obstacles = [
        item["rect_points"]
        for item in components
        if item["id"].startswith("node_editor.node:")
        or item["id"].startswith("node_editor.container_header.")
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
            ):
                return snapshot, point
    raise QaFailure("no unobstructed point was found in the Node Editor canvas")


def open_create_menu(client):
    snapshot, point = find_free_canvas_point(client)
    client.inject(
        "click",
        {"x": point["x"], "y": point["y"], "button": "secondary"},
        {
            "component_id": "node_editor.canvas",
            "component_frame": snapshot["frame"],
            "coordinate_reason": "unobstructed composition-container interior",
        },
    )
    client.wait_until(
        "Node Editor create menu",
        lambda: client.state()
        if client.state()["editor"]["node_editor"]["context_menu_open"]
        else None,
    )
    return point


def reveal_node_editor_component(client, component_id, max_drags=20):
    """Pan the real Snarl canvas until an offscreen component is clickable."""
    for _ in range(max_drags):
        snapshot = client.component_snapshot()
        components = {item["id"]: item for item in snapshot["components"]}
        component = components.get(component_id)
        canvas = components.get("node_editor.canvas")
        if component is None or canvas is None:
            time.sleep(0.04)
            continue
        rect = component["rect_points"]
        if (
            component.get("visible", False)
            and rect["width"] > 0.0
            and rect["height"] > 0.0
        ):
            return snapshot, component

        unclipped = component.get("metadata", {}).get("unclipped_rect")
        if unclipped is None:
            time.sleep(0.04)
            continue
        canvas_rect = canvas["rect_points"]
        margin = 12.0
        max_vertical_step = max(canvas_rect["height"] - margin * 2.0, 1.0)
        needed = canvas_rect["center_y"] - (
            unclipped["min_y"] + unclipped["height"] * 0.5
        )
        vertical_step = max(-max_vertical_step, min(max_vertical_step, needed))
        if abs(vertical_step) < 1.0:
            raise QaFailure(
                "component {!r} is horizontally outside the Node Editor canvas".format(
                    component_id
                )
            )
        x = canvas_rect["min_x"] + min(40.0, canvas_rect["width"] * 0.05)
        start_y = (
            canvas_rect["max_y"] - margin
            if vertical_step < 0.0
            else canvas_rect["min_y"] + margin
        )
        start = {"x": x, "y": start_y}
        end = {"x": x, "y": start_y + vertical_step}
        client.inject(
            "drag",
            {
                "from": start,
                "to": end,
                "steps": 6,
                "button": "middle",
            },
            {
                "component_id": "node_editor.canvas",
                "target_component_id": component_id,
                "component_frame": snapshot["frame"],
                "component_rect_points": canvas_rect,
                "coordinate_reason": "pan to rendered offscreen component",
            },
        )
    raise QaFailure(
        "component {!r} did not become visible after {} canvas drags".format(
            component_id, max_drags
        )
    )


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
        inner_min_y = canvas_rect["min_y"] + margin
        inner_max_y = canvas_rect["max_y"] - margin
        if all(
            target.get("visible", False)
            and target["rect_points"]["width"] > 0.0
            and target["rect_points"]["height"] > 0.0
            and target["rect_points"]["min_y"] >= inner_min_y
            and target["rect_points"]["max_y"] <= inner_max_y
            for target in targets
        ):
            return snapshot, targets

        unclipped = [
            target.get("metadata", {}).get("unclipped_rect") for target in targets
        ]
        if any(rect is None for rect in unclipped):
            time.sleep(0.04)
            continue
        union_min_y = min(rect["min_y"] for rect in unclipped)
        union_max_y = max(rect["max_y"] for rect in unclipped)
        if union_max_y - union_min_y > inner_max_y - inner_min_y:
            raise QaFailure(
                "Node Editor components cannot fit together in the visible canvas: {}".format(
                    ", ".join(component_ids)
                )
            )
        needed = canvas_rect["center_y"] - (union_min_y + union_max_y) * 0.5
        max_step = max(canvas_rect["height"] - margin * 2.0, 1.0)
        vertical_step = max(-max_step, min(max_step, needed))
        if abs(vertical_step) < 1.0:
            raise QaFailure(
                "Node Editor components did not become fully visible: {}".format(
                    ", ".join(component_ids)
                )
            )
        x = canvas_rect["min_x"] + min(40.0, canvas_rect["width"] * 0.05)
        start_y = (
            canvas_rect["max_y"] - margin
            if vertical_step < 0.0
            else canvas_rect["min_y"] + margin
        )
        client.inject(
            "drag",
            {
                "from": {"x": x, "y": start_y},
                "to": {"x": x, "y": start_y + vertical_step},
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


def assert_fresh_fixture(initial):
    project = initial["project"]
    if project.get("name") != "RuViE QA E2E":
        raise QaFailure(
            "the deterministic fixture is required; start with "
            "RUVIE_QA_FIXTURE=node_editor_e2e"
        )
    if set(project["nodes"]) != {SOLID, MERGE, TEXT, SHAPE}:
        raise QaFailure("fixture Project is not fresh")
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


def run_timeline_scenario(client):
    """Exercise Timeline mutations solely through fresh screen coordinates."""
    client.wait_component_settled("dock.tab:timeline")
    client.click_component("dock.tab:timeline")
    client.wait_until(
        "Timeline dock activation",
        lambda: client.state()
        if "Timeline" in client.state()["dock"]["active_tabs"]
        else None,
    )

    move_before = client.state()
    clip_before = move_before["project"]["clips"][CLIP_A1]
    order_before = list(move_before["project"]["tracks"][TRACK_A]["clip_ids"])
    client.drag_component_by("timeline.clip:" + CLIP_A1, 60.0, 0.0)
    move_after = wait_timeline_edit(
        client,
        move_before,
        "Timeline Clip time move",
        lambda current: current["clips"][CLIP_A1]["start_time"]
        != clip_before["start_time"],
    )
    moved = move_after["project"]["clips"][CLIP_A1]
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
    client.wait_preview_render_after(move_after, "Clip time move")

    left_before = client.state()
    old = left_before["project"]["clips"][CLIP_A1]
    client.drag_component_by("timeline.clip_edge.left:" + CLIP_A1, 30.0, 0.0)
    left_after = wait_timeline_edit(
        client,
        left_before,
        "left-edge trim",
        lambda current: current["clips"][CLIP_A1]["start_time"]
        > old["start_time"],
    )
    new = left_after["project"]["clips"][CLIP_A1]
    delta = new["start_time"] - old["start_time"]
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
    client.wait_preview_render_after(left_after, "left-edge trim")

    right_before = client.state()
    old = right_before["project"]["clips"][CLIP_A1]
    client.drag_component_by("timeline.clip_edge.right:" + CLIP_A1, -25.0, 0.0)
    right_after = wait_timeline_edit(
        client,
        right_before,
        "right-edge trim",
        lambda current: current["clips"][CLIP_A1]["duration"] < old["duration"],
    )
    new = right_after["project"]["clips"][CLIP_A1]
    assert_close(new["start_time"], old["start_time"], "right trim start time")
    assert_close(new["trim_in"], old["trim_in"], "right trim trim_in")
    assert_timeline_integrity(
        right_before["project"], right_after["project"], "right-edge trim"
    )
    assert_history_delta(right_before, right_after, 1, "right-edge trim")
    client.wait_preview_render_after(right_after, "right-edge trim")

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
    client.wait_preview_render_after(reorder_after, "same-Track Clip reorder")

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
    client.wait_preview_render_after(cross_after, "cross-Track Clip move")

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
    client.wait_preview_render_after(track_after, "Track reorder")
    print("[qa-e2e] Timeline move/trim/Clip reorder/Track reorder passed")
    return track_after


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
        "node_editor.node:" + SOLID,
        "node_editor.node:" + MERGE,
        "node_editor.node:" + TEXT,
        "node_editor.node:" + SHAPE,
        "node_editor.edge.derived:track:{}:clip:{}".format(TRACK_B, CLIP_A2),
    }

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

    expected_outputs = {CLIP_A1: MERGE, CLIP_A2: TEXT, CLIP_B1: SHAPE}
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
    for node_id in (SOLID, MERGE):
        metadata = components["node_editor.node:" + node_id].get("metadata") or {}
        if metadata.get("inactive") != expected_inactive:
            raise QaFailure(
                "Node Editor Node activity did not reflect Timeline Clip timing"
            )

    # Prove the reflected Node is still hit-testable through a real coordinate
    # click, not merely present in registry metadata.
    header_id = None
    for node_id in (MERGE, SOLID, TEXT, SHAPE):
        candidate = components.get("node_editor.node_header:" + node_id)
        if candidate and candidate.get("visible") and candidate.get("enabled"):
            header_id = candidate["id"]
            break
    if header_id is None:
        raise QaFailure("no reflected Node header is visible for coordinate verification")
    selected_node = header_id.rsplit(":", 1)[-1]
    client.click_component(header_id)
    selected = client.wait_until(
        "reflected Node coordinate selection",
        lambda: client.state()
        if client.state()["editor"]["selection"]["last_selected_entity_id"]
        == selected_node
        else None,
    )
    validate_canonical_ownership(selected["project"])
    print("[qa-e2e] Timeline -> Node Editor graph/activity reflection passed")
    return selected


def verify_final_preview_drag(client):
    # This final gesture intentionally traverses normal egui arbitration.
    pan_before = client.state()
    client.key("space", True)
    client.drag_component_by("preview.canvas", 54.0, 32.0)
    client.key("space", False)
    pan_after = client.wait_until(
        "Preview hand-tool pan",
        lambda: client.state()
        if client.state()["editor"]["preview"]["pan"]
        != pan_before["editor"]["preview"]["pan"]
        else None,
    )
    if pan_after["editor"]["preview"]["primary_gesture"] != "Idle":
        raise QaFailure("Preview gesture owner did not return to Idle")
    print("[qa-e2e] final real coordinate Preview drag passed")
    return pan_after


def run_timeline_suite(client):
    health = client.wait_health()
    initial = wait_fresh_fixture(client)
    print("[qa-e2e] bridge healthy at frame {}".format(health["frame"]))
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


def run_smoke_suite(client, capture_path):
    """Short real-coordinate gate for click, Project drag, render, and capture."""
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
    source_port = (
        "node_editor.container_port.clip:{}.internal_output:frame".format(CLIP_A1)
    )
    target_port = "node_editor.port.node:{}.input:frame".format(SOLID)
    reveal_node_editor_components(client, (source_port, target_port))
    connection_before = client.state()
    connection_count = len(connection_before["project"]["connections"])
    client.drag_components(source_port, target_port, steps=8)
    connected = client.wait_project(
        "smoke metadata connection",
        lambda project: len(project["connections"]) == connection_count + 1,
    )
    assert_history_delta(connection_before, connected, 1, "smoke metadata connection")
    validate_canonical_ownership(connected["project"])
    client.wait_preview_render_after(connected, "smoke metadata connection")

    client.wait_component_settled("dock.tab:timeline")
    client.click_component("dock.tab:timeline")
    client.wait_until(
        "Timeline dock activation",
        lambda: client.state()
        if "Timeline" in client.state()["dock"]["active_tabs"]
        else None,
    )

    client.click_component("timeline.clip:" + CLIP_A1)
    client.wait_until(
        "Timeline Clip selection",
        lambda: client.state()
        if client.state()["editor"]["selection"]["last_selected_entity_id"] == CLIP_A1
        else None,
    )

    before = client.state()
    old_clip = before["project"]["clips"][CLIP_A1]
    client.drag_component_by("timeline.clip:" + CLIP_A1, 45.0, 0.0, steps=8)
    edited = wait_timeline_edit(
        client,
        before,
        "smoke Timeline Clip move",
        lambda project: project["clips"][CLIP_A1]["start_time"]
        != old_clip["start_time"],
    )
    assert_timeline_integrity(before["project"], edited["project"], "smoke Clip move")
    assert_history_delta(before, edited, 1, "smoke Clip move")
    rendered = client.wait_preview_render_after(edited, "smoke Clip move")
    final = verify_final_preview_drag(client)
    capture = client.capture(capture_path)
    print("[qa-e2e] smoke click/drag/capture/Project reflection passed")
    return {
        "ok": True,
        "suite": "smoke",
        "health_frame": health["frame"],
        "initial_frame": initial["frame"],
        "connection_frame": connected["frame"],
        "rendered_frame": rendered["frame"],
        "final_frame": final["frame"],
        "final_history": final["history"],
        "final_preview": final["editor"]["preview"],
        "capture": capture,
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
        if client.state()["editor"]["selection"]["last_selected_entity_id"] == TEXT
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

    # Connect a previously unbound internal metadata pin and verify both the
    # Project and a subsequent Preview render. The fixture's image wires are
    # already canonical, so re-dragging one would only test de-duplication.
    before_connection = client.state()
    connection_count = len(before_connection["project"]["connections"])
    source_port = "node_editor.container_port.clip:{}.internal_output:frame".format(
        CLIP_A1
    )
    target_port = "node_editor.port.node:{}.input:frame".format(SOLID)
    reveal_node_editor_components(client, (source_port, target_port))
    client.drag_components(
        source_port,
        target_port,
    )
    connected = client.wait_project(
        "canonical Node connection",
        lambda current: len(current["connections"]) == connection_count + 1,
    )
    client.wait_preview_render_after(connected, "Node metadata connection")
    assert_history_delta(before_connection, connected, 1, "Node connection")
    print("[qa-e2e] pin drag -> Project connection -> Preview passed")

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
        if client.state()["editor"]["selection"]["last_selected_entity_id"] == SHAPE
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

    # Reorder independent Effector instances, then edit/keyframe the Transform
    # instance by UUID-scoped controls (never by ambiguous legacy IDs).
    reveal_node_editor_component(client, "node_editor.node_header:" + TEXT)
    client.click_component("node_editor.node_header:" + TEXT)
    first_handle = "inspector.ensemble.effector_handle:" + TRANSFORM_EFFECTOR
    second_handle = "inspector.ensemble.effector_handle:" + OPACITY_EFFECTOR
    client.ensure_in_scroll_area(first_handle)
    client.ensure_in_scroll_area(second_handle)
    ensemble_before = client.state()
    client.drag_components(first_handle, second_handle, target_fraction=(0.5, 0.9))
    ensemble_after = client.wait_project(
        "Effector reorder",
        lambda current: [item["id"] for item in current["nodes"][TEXT]["effectors"]]
        == [OPACITY_EFFECTOR, TRANSFORM_EFFECTOR],
    )
    assert_history_delta(ensemble_before, ensemble_after, 1, "Effector reorder")

    tx_id = "inspector.property.node:{}.effector:{}:tx".format(TEXT, TRANSFORM_EFFECTOR)
    client.ensure_in_scroll_area(tx_id)
    tx_before = client.state()
    opacity_before = property_value(
        tx_before["project"]["nodes"][TEXT]["effectors"][0], "opacity"
    )
    client.drag_component_by(tx_id, 24.0, 0.0)
    tx_after = client.wait_project(
        "Transform Effector property edit",
        lambda current: property_value(current["nodes"][TEXT]["effectors"][1], "tx") != 0.0,
    )
    if property_value(tx_after["project"]["nodes"][TEXT]["effectors"][0], "opacity") != opacity_before:
        raise QaFailure("editing Transform Effector changed the Opacity Effector")
    assert_history_delta(tx_before, tx_after, 1, "Effector property drag")

    keyframe_id = "inspector.keyframe.node:{}.effector:{}:tx".format(
        TEXT, TRANSFORM_EFFECTOR
    )
    client.ensure_in_scroll_area(keyframe_id)
    key_before = client.state()
    client.click_component(keyframe_id)
    key_after = client.wait_project(
        "Transform Effector keyframe",
        lambda current: current["nodes"][TEXT]["effectors"][1]["properties"]["tx"]["type"]
        == "keyframe",
    )
    assert_history_delta(key_before, key_after, 1, "Effector keyframe")
    print("[qa-e2e] Ensemble reorder/property/keyframe passed")

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


def parse_args():
    parser = argparse.ArgumentParser()
    parser.add_argument("--base-url", default=None)
    parser.add_argument(
        "--suite",
        choices=("all", "timeline", "smoke"),
        default="all",
        help="run the complete suite or the focused Timeline/Node reflection suite",
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
        elif args.suite == "smoke":
            capture_path = args.capture or "target/qa-smoke-evidence.png"
            result = run_smoke_suite(client, capture_path)
        else:
            result = run_suite(client)
        result["run_id"] = os.environ.get("RUVIE_QA_RUN_ID")
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
