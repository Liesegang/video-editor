#!/usr/bin/env python3
"""Shared loopback HTTP support for native authoring UI QA suites."""

from __future__ import annotations

import argparse
import contextlib
import hashlib
import json
import math
import os
import pathlib
import signal
import socket
import subprocess
import sys
import time
import urllib.error
import urllib.parse
import urllib.request
from collections.abc import Mapping


REPOSITORY_ROOT = pathlib.Path(__file__).resolve().parent.parent
AUTHORING_FIXTURE = "authoring_e2e"
AUTHORING_AUDIO_FIXTURE = "authoring_audio_e2e"
AUTHORING_PATH_FIXTURE = "authoring_path_e2e"
QA_APP_BINARY_ENV = "RUVIE_QA_APP_BINARY"


class QaFailure(RuntimeError):
    """A user-visible authoring behavior failed its QA contract."""


def free_port() -> int:
    with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as listener:
        listener.bind(("127.0.0.1", 0))
        return int(listener.getsockname()[1])


def repository_git_commit() -> str | None:
    try:
        return subprocess.check_output(
            ["git", "rev-parse", "HEAD"],
            cwd=REPOSITORY_ROOT,
            text=True,
            stderr=subprocess.DEVNULL,
        ).strip()
    except (OSError, subprocess.CalledProcessError):
        return None


def project_file_evidence(path: pathlib.Path, description: str) -> dict:
    """Return stable evidence for one non-empty persisted Project file."""

    content = path.read_bytes()
    if not content:
        raise QaFailure(description + " is empty")
    return {
        "path": str(path.resolve()),
        "bytes": len(content),
        "sha256": hashlib.sha256(content).hexdigest(),
    }


def save_project_to_disk(client: "QaClient", path: pathlib.Path, label: str):
    """Save through the production command and prove the exact Project reached disk."""

    before = client.state()
    expected_project = before["project"]
    file_before = project_file_evidence(path, label + " Project before save")
    client.key("s", True, command=True)
    client.key("s", False, command=True)

    def saved_to_disk():
        state = client.state()
        if (
            state["editor"].get("status") != "Project saved"
            or state["project"] != expected_project
        ):
            return None
        evidence = project_file_evidence(path, label + " saved Project")
        return (
            (state, evidence)
            if evidence["sha256"] != file_before["sha256"]
            else None
        )

    return client.wait_until(label + " save reaching disk", saved_to_disk)


def process_group_options() -> dict:
    if os.name == "nt":
        return {"creationflags": subprocess.CREATE_NEW_PROCESS_GROUP}
    return {"start_new_session": True}


def terminate_process(process: subprocess.Popen, grace_seconds: float = 5.0) -> None:
    """Stop one QA process group without assuming POSIX killpg on Windows."""
    if process.poll() is not None:
        return
    if os.name == "nt":
        try:
            process.send_signal(signal.CTRL_BREAK_EVENT)
        except (OSError, ValueError):
            process.terminate()
        try:
            process.wait(timeout=grace_seconds)
            return
        except subprocess.TimeoutExpired:
            process.kill()
    else:
        try:
            os.killpg(process.pid, signal.SIGTERM)
        except ProcessLookupError:
            return
        try:
            process.wait(timeout=grace_seconds)
            return
        except subprocess.TimeoutExpired:
            try:
                os.killpg(process.pid, signal.SIGKILL)
            except ProcessLookupError:
                return
    try:
        process.wait(timeout=2.0)
    except subprocess.TimeoutExpired:
        pass


def wait_endpoint_closed(
    client: "QaClient", timeout: float = 8.0, description: str = "native app"
) -> float:
    """Wait until one native QA process has actually released its HTTP endpoint."""

    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        try:
            with urllib.request.urlopen(client.base_url + "/health", timeout=0.1):
                pass
        except (urllib.error.URLError, ConnectionError, TimeoutError, OSError):
            return time.monotonic()
        time.sleep(0.025)
    raise QaFailure(
        "{} did not close within {:.1f}s".format(description, timeout)
    )


def request_clean_native_close(
    client: "QaClient", label: str, timeout: float
) -> dict:
    """Request the production close path and wait for its endpoint to close."""

    started = time.monotonic()
    queued = client.request("/v1/input/close-request", {}, method="POST")
    if queued.get("queued") is not True or queued.get("action_id") is None:
        raise QaFailure("{} native close request was not queued".format(label))
    wait_endpoint_closed(client, timeout=timeout, description=label)
    return {
        "action_id": queued["action_id"],
        "seconds": time.monotonic() - started,
    }


def close_clean_native_app(
    client: "QaClient",
    process: subprocess.Popen,
    label: str,
    timeout: float,
) -> dict:
    """Close one native QA app through production UI and verify process exit."""

    closed = request_clean_native_close(client, label, timeout)
    try:
        exit_code = process.wait(timeout=min(timeout, 10.0))
    except subprocess.TimeoutExpired as error:
        raise QaFailure(
            "{} endpoint closed but its process did not exit".format(label)
        ) from error
    if exit_code != 0:
        raise QaFailure("{} exited with code {}".format(label, exit_code))
    return {**closed, "exit_code": exit_code}


def _json_request(base_url: str, path: str, data=None, method: str | None = None):
    body = None if data is None else json.dumps(data).encode("utf-8")
    request = urllib.request.Request(
        base_url + path,
        data=body,
        headers={"Content-Type": "application/json"},
        method=method,
    )
    with urllib.request.urlopen(request, timeout=5.0) as response:
        return json.load(response)


class QaClient:
    def __init__(self, base_url: str, timeout: float = 12.0):
        self.base_url = base_url.rstrip("/")
        self.timeout = timeout
        self.evidence: list[dict] = []

    def request(self, path: str, data=None, method: str | None = None):
        return _json_request(self.base_url, path, data=data, method=method)

    def wait_until(self, description: str, predicate, timeout: float | None = None):
        deadline = time.monotonic() + (self.timeout if timeout is None else timeout)
        last_error = None
        while time.monotonic() < deadline:
            try:
                result = predicate()
                if result is not None and result is not False:
                    return result
            except (
                KeyError,
                IndexError,
                TypeError,
                ValueError,
                urllib.error.HTTPError,
                urllib.error.URLError,
                json.JSONDecodeError,
            ) as error:
                last_error = error
            time.sleep(0.025)
        suffix = "" if last_error is None else ": {}".format(last_error)
        raise QaFailure("timed out waiting for {}{}".format(description, suffix))

    def wait_health(self):
        return self.wait_until(
            "loopback QA health",
            lambda: health
            if (health := self.request("/health")).get("ok") is True
            else None,
        )

    def state(self):
        return self.request("/v1/state")

    def component_snapshot(self):
        return self.request("/v1/components")

    def queue_input(self, action: str, payload: dict) -> int:
        queued = self.request("/v1/input/" + action, payload, method="POST")
        action_id = queued.get("action_id")
        if queued.get("queued") is not True or not isinstance(action_id, int):
            raise QaFailure("{} input was not queued: {!r}".format(action, queued))
        return action_id

    def component(self, component_id: str):
        snapshot = self.component_snapshot()
        component = next(
            (item for item in snapshot["components"] if item["id"] == component_id),
            None,
        )
        if component is None:
            raise urllib.error.HTTPError(
                self.base_url + "/v1/components/" + urllib.parse.quote(component_id, safe=""),
                404,
                "component not found",
                {},
                None,
            )
        return snapshot, component

    def wait_component(self, component_id: str):
        return self.wait_until(
            component_id,
            lambda: value
            if _interactable((value := self.component(component_id))[1])
            else None,
        )

    def wait_component_settled(self, component_id: str, consecutive_reads: int = 2):
        previous_geometry = None
        stable = 0

        def settled():
            nonlocal previous_geometry, stable
            snapshot, component = self.component(component_id)
            if not _interactable(component):
                return None
            geometry = component.get("rect_points")
            if geometry == previous_geometry:
                stable += 1
            else:
                previous_geometry = geometry
                stable = 1
            return (snapshot, component) if stable >= consecutive_reads else None

        return self.wait_until("settled " + component_id, settled)

    def inject(self, action: str, payload: dict):
        before = self.component_snapshot()["frame"]
        action_id = self.queue_input(action, payload)

        def completed():
            status = self.request("/v1/actions/{}".format(action_id))
            return status if status.get("phase") == "injected" else None

        status = self.wait_until("{} input {}".format(action, action_id), completed)
        after = self.wait_until(
            "a completed frame after input {}".format(action_id),
            lambda: snapshot
            if (snapshot := self.component_snapshot())["frame"] > before
            else None,
        )
        record = {
            "action": action,
            "action_id": action_id,
            "phase": status["phase"],
            "completed_frame": after["frame"],
            "payload": payload,
        }
        self.evidence.append(record)
        return action_id

    def click_component(self, component_id: str, button: str = "primary"):
        snapshot, component = self.wait_component(component_id)
        point = component_center(component)
        self.inject("click", {**point, "button": button, "coordinate_space": "points"})
        return snapshot, component, point

    def queue_terminal_click_component(
        self, component_id: str, button: str = "primary"
    ):
        """Queue a click whose successful action is expected to stop the QA endpoint."""

        snapshot, component = self.wait_component(component_id)
        point = component_center(component)
        payload = {**point, "button": button, "coordinate_space": "points"}
        action_id = self.queue_input("click", payload)
        self.evidence.append(
            {
                "action": "click",
                "action_id": action_id,
                "phase": "queued_for_terminal_action",
                "completed_frame": None,
                "payload": payload,
            }
        )
        return action_id, snapshot, component, point
    def double_click_component(self, component_id: str):
        snapshot, component = self.wait_component(component_id)
        point = component_center(component)
        self.inject(
            "double-click", {**point, "button": "primary", "coordinate_space": "points"}
        )
        return snapshot, component, point

    def drag_component_by(
        self,
        component_id: str,
        delta_x: float,
        delta_y: float,
        steps: int = 10,
        button: str = "primary",
        fraction_x: float = 0.5,
        fraction_y: float = 0.5,
    ):
        snapshot, component = self.wait_component(component_id)
        start = component_point(component, fraction_x, fraction_y)
        end = {"x": start["x"] + delta_x, "y": start["y"] + delta_y}
        self.drag(start, end, steps=steps, button=button)
        return snapshot, component, start, end

    def drag(self, start: dict, end: dict, steps: int = 10, button: str = "primary"):
        return self.inject(
            "drag",
            {
                "from": start,
                "to": end,
                "steps": steps,
                "button": button,
                "coordinate_space": "points",
            },
        )

    def scroll_component(
        self,
        component_id: str,
        delta_x: float,
        delta_y: float,
        modifiers: dict | None = None,
    ):
        snapshot, component = self.wait_component(component_id)
        point = component_center(component)
        self.inject(
            "scroll",
            {
                **point,
                "delta_x": delta_x,
                "delta_y": delta_y,
                "coordinate_space": "points",
                "modifiers": modifiers or {},
            },
        )
        return snapshot, component, point

    def pinch_component(
        self,
        component_id: str,
        factor: float,
        fraction_x: float = 0.5,
        fraction_y: float = 0.5,
    ):
        snapshot, component = self.wait_component(component_id)
        point = component_point(component, fraction_x, fraction_y)
        self.inject(
            "pinch",
            {**point, "factor": factor, "coordinate_space": "points"},
        )
        return snapshot, component, point

    def key(self, key: str, pressed: bool, **modifiers):
        return self.inject(
            "key",
            {"key": key.lower(), "pressed": pressed, "modifiers": modifiers},
        )


def capture_viewport(
    client: QaClient, path: pathlib.Path, timeout: float = 15.0
) -> dict:
    """Capture the real native viewport and verify the published PNG bytes."""

    queued = client.request("/v1/captures", method="POST")
    capture_id = queued["capture_id"]

    def ready():
        status = client.request("/v1/captures/{}".format(capture_id))
        if status.get("phase") == "failed":
            raise QaFailure("capture failed: {}".format(status.get("error")))
        return status if status.get("phase") == "ready" else None

    status = client.wait_until("capture {}".format(capture_id), ready, timeout=timeout)
    with urllib.request.urlopen(
        client.base_url + "/v1/captures/{}.png".format(capture_id), timeout=5.0
    ) as response:
        png = response.read()
    if hashlib.sha256(png).hexdigest() != status.get("sha256"):
        raise QaFailure("capture SHA-256 mismatch")
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_bytes(png)
    result = dict(status)
    result["path"] = str(path.resolve())
    return result


def _interactable(component: dict) -> bool:
    rect = component.get("rect_points") or {}
    return (
        component.get("visible") is True
        and component.get("enabled") is True
        and float(rect.get("width", 0.0)) > 0.0
        and float(rect.get("height", 0.0)) > 0.0
    )


def component_center(component: dict) -> dict:
    rect = component["rect_points"]
    return {"x": float(rect["center_x"]), "y": float(rect["center_y"])}


def component_point(component: dict, fraction_x: float, fraction_y: float) -> dict:
    rect = component["rect_points"]
    return {
        "x": float(rect["min_x"]) + float(rect["width"]) * fraction_x,
        "y": float(rect["min_y"]) + float(rect["height"]) * fraction_y,
    }


def bring_timeline_component(client: QaClient, component_id: str, direction: float):
    """Reveal an offscreen Timeline row through ordinary wheel navigation."""
    for _ in range(10):
        client.state()
        component = next(
            (component for component in client.component_snapshot()["components"]
             if component.get("id") == component_id and component.get("visible")),
            None,
        )
        if component is not None:
            return component
        client.scroll_component("timeline.canvas", 0.0, direction)
    raise QaFailure("could not bring {} into the Timeline viewport".format(component_id))


def seek_timeline_seconds(client: QaClient, seconds: float, fps: float = 30.0):
    """Seek through the production Timeline ruler and wait for its exact frame."""
    state = client.state()
    timeline = state["editor"]["timeline"]
    _, ruler = client.wait_component_settled("timeline.ruler")
    rect = ruler["rect_points"]
    x = (
        float(rect["min_x"])
        + float(seconds) * float(timeline["pixels_per_second"])
        - float(timeline["horizontal_scroll"])
    )
    client.inject(
        "click",
        {
            "x": x,
            "y": float(rect["center_y"]),
            "button": "primary",
            "coordinate_space": "points",
        },
    )
    expected_frame = int(round(float(seconds) * float(fps)))
    return client.wait_until(
        "Timeline seek to {:.3f}s".format(seconds),
        lambda: current
        if (current := client.state())["editor"]["timeline"]["current_frame"]
        == expected_frame
        else None,
    )


def rendered_preview_state(client: QaClient, revision: int):
    """Return state once Preview has rendered this revision without an error."""
    state = client.state()
    preview = state["editor"]["preview"]
    if (
        state["editor"].get("error") is None
        and preview.get("rendered_revision") == revision
        and preview.get("nontransparent_pixels", 0) > 0
    ):
        return state
    return None


def settled_preview_state(client: QaClient, revision: int, frame: int):
    """Return matching Project/UI Preview state after the render queue is idle."""
    component = next(
        (
            item
            for item in client.component_snapshot()["components"]
            if item.get("id") == "preview.canvas"
        ),
        None,
    )
    preview = (component or {}).get("metadata") or {}
    if (
        preview.get("rendered_revision") != revision
        or preview.get("rendered_frame") != frame
        or preview.get("nontransparent_pixels", 0) <= 0
        or preview.get("pixel_hash") is None
        or preview.get("render_in_flight_request") is not None
        or preview.get("render_desired_pending") is not False
    ):
        return None
    state = client.state()
    state_preview = state["editor"]["preview"]
    if (
        state["history"]["revision"] == revision
        and state["editor"]["timeline"]["current_frame"] == frame
        and state_preview.get("rendered_revision") == revision
        and state_preview.get("rendered_frame") == frame
        and state_preview.get("pixel_hash") == preview.get("pixel_hash")
        and state["editor"].get("error") is None
    ):
        return state
    return None


def convert_timeline_item_to_node_clip(client: QaClient, item_id: str, revision: int):
    """Invoke the production Timeline conversion action and await its atomic edit."""
    client.click_component("timeline.item:" + item_id, button="secondary")
    menu_id = "timeline.item.convert_source_to_node_clip:" + item_id
    client.wait_component(menu_id)
    client.click_component(menu_id)

    def converted():
        state = client.state()
        source = state["project"]["items"][item_id]["source"]
        document = state["editor"]["node_editor"]["document"]
        if (
            source.get("kind") == "module"
            and state["history"]["revision"] == revision + 1
            and document
            and document.get("kind") == "module_definition"
            and document.get("host") == "node_clip"
        ):
            return state
        return None

    return client.wait_until("bounded Node Clip conversion", converted)


def create_basic_timeline_clip(client: QaClient, kind: str, expected_name: str):
    """Create one production basic clip from the Timeline context menu."""
    before = client.state()
    before_ids = set(before["project"]["items"])
    _, canvas = client.wait_component_settled("timeline.canvas")
    rect = canvas["rect_points"]
    client.inject(
        "click",
        {
            "x": float(rect["max_x"]) - 6.0,
            "y": float(rect["max_y"]) - 6.0,
            "button": "secondary",
            "coordinate_space": "points",
        },
    )
    client.wait_component("timeline.menu.new_clip")
    client.click_component("timeline.menu.new_clip")
    choice_id = "timeline.menu.new_clip." + kind
    _, choice = client.wait_component_settled(choice_id)
    metadata = choice.get("metadata") or {}
    if metadata.get("clip_kind") != kind or metadata.get("label") != expected_name:
        raise QaFailure("basic clip menu metadata is not authoritative")
    client.click_component(choice_id)

    def created():
        state = client.state()
        new_ids = set(state["project"]["items"]) - before_ids
        if len(new_ids) != 1 or state["history"]["revision"] != before["history"]["revision"] + 1:
            return None
        item = state["project"]["items"][next(iter(new_ids))]
        if item["name"] != expected_name:
            raise QaFailure("created basic clip has the wrong name")
        return state, item

    return client.wait_until("created {} clip".format(expected_name), created)


def activate_dock_tab(
    client: QaClient, component_id: str, label: str, description: str | None = None
):
    component_ids = {
        component["id"] for component in client.component_snapshot()["components"]
    }
    if component_id not in component_ids:
        # A production action can add/focus a dock tab after that frame's UI
        # was already painted. Give the component registry one short grace
        # period before invoking TogglePanel; otherwise the fallback can close
        # the tab that has just been opened.
        try:
            client.wait_until(
                description or (label + " dock tab publication"),
                lambda: component
                if (
                    component := next(
                        (
                            item
                            for item in client.component_snapshot()["components"]
                            if item["id"] == component_id
                        ),
                        None,
                    )
                )
                else None,
                timeout=0.75,
            )
        except QaFailure:
            client.key("p", True, command=True, shift=True)
            client.key("p", False, command=True, shift=True)
            client.inject("text", {"text": label})
            client.key("enter", True)
            client.key("enter", False)
    client.click_component(component_id)
    return client.wait_component_settled(component_id)


def finite_number(value, description: str) -> float:
    if isinstance(value, bool) or not isinstance(value, (int, float)):
        raise QaFailure("{} is not numeric: {!r}".format(description, value))
    value = float(value)
    if not math.isfinite(value):
        raise QaFailure("{} is not finite: {!r}".format(description, value))
    return value


def media_seconds(value: dict) -> float:
    timescale = int(value["timescale"])
    if timescale <= 0:
        raise QaFailure("invalid MediaTime timescale")
    return int(value["value"]) / timescale


def item_by_name(project: dict, name: str) -> dict:
    matches = [item for item in project["items"].values() if item["name"] == name]
    if len(matches) != 1:
        raise QaFailure("expected one {!r} Timeline item, got {}".format(name, len(matches)))
    return matches[0]


def find_clear_canvas_point(snapshot: dict, canvas_id: str, obstacle_prefixes: tuple[str, ...]):
    components = {component["id"]: component for component in snapshot["components"]}
    canvas = components.get(canvas_id)
    if canvas is None or not _interactable(canvas):
        raise QaFailure("{} is not interactable".format(canvas_id))
    rect = canvas["rect_points"]
    obstacles = [
        component["rect_points"]
        for component in snapshot["components"]
        if component.get("visible")
        and component["id"].startswith(obstacle_prefixes)
    ]
    for fx, fy in ((0.80, 0.25), (0.80, 0.75), (0.55, 0.75), (0.25, 0.75)):
        point = {
            "x": rect["min_x"] + rect["width"] * fx,
            "y": rect["min_y"] + rect["height"] * fy,
        }
        if all(not _point_in_rect(point, obstacle, 8.0) for obstacle in obstacles):
            return point
    raise QaFailure("no unobstructed point in {}".format(canvas_id))


def _point_in_rect(point: dict, rect: dict, padding: float) -> bool:
    return (
        rect["min_x"] - padding <= point["x"] <= rect["max_x"] + padding
        and rect["min_y"] - padding <= point["y"] <= rect["max_y"] + padding
    )


@contextlib.contextmanager
def spawned_authoring_app(
    port: int, environment_overrides: Mapping[str, str | None] | None = None
):
    environment = os.environ.copy()
    for name, value in (environment_overrides or {}).items():
        if value is None:
            environment.pop(name, None)
        else:
            environment[name] = value
    environment["RUVIE_QA_PORT"] = str(port)
    environment.setdefault("RUVIE_QA_FIXTURE", AUTHORING_FIXTURE)
    configured_binary = environment.get(QA_APP_BINARY_ENV)
    command = (
        [configured_binary]
        if configured_binary
        else ["cargo", "run", "-p", "app", "--locked"]
    )
    process = subprocess.Popen(
        command,
        cwd=REPOSITORY_ROOT,
        env=environment,
        **process_group_options(),
    )
    try:
        yield process
    finally:
        terminate_process(process)


def run_suite_main(name: str, suite, default_evidence: str) -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--base-url", default=None)
    parser.add_argument("--spawn", action="store_true")
    parser.add_argument("--evidence", default=default_evidence)
    parser.add_argument("--timeout", type=float, default=15.0)
    args = parser.parse_args()
    port = free_port() if args.spawn else 39091
    base_url = args.base_url or "http://127.0.0.1:{}".format(port)
    process_scope = spawned_authoring_app(port) if args.spawn else contextlib.nullcontext()
    try:
        with process_scope:
            result = suite(QaClient(base_url, args.timeout))
        result.update(
            {
                "ok": True,
                "fixture": os.environ.get("RUVIE_QA_FIXTURE", AUTHORING_FIXTURE),
                "run_id": os.environ.get("RUVIE_QA_RUN_ID"),
                "git_commit": repository_git_commit(),
            }
        )
        evidence = pathlib.Path(args.evidence)
        if not evidence.is_absolute():
            evidence = REPOSITORY_ROOT / evidence
        evidence.parent.mkdir(parents=True, exist_ok=True)
        evidence.write_text(
            json.dumps(result, ensure_ascii=False, indent=2) + "\n", encoding="utf-8"
        )
        print("[{}] PASS; evidence: {}".format(name, evidence))
        return 0
    except (QaFailure, AssertionError, KeyError, IndexError, TypeError, ValueError) as error:
        print("[{}] FAIL: {}".format(name, error), file=sys.stderr)
        return 1
