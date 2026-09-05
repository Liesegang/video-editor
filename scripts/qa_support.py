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


REPOSITORY_ROOT = pathlib.Path(__file__).resolve().parent.parent
AUTHORING_FIXTURE = "authoring_e2e"
AUTHORING_AUDIO_FIXTURE = "authoring_audio_e2e"
AUTHORING_PATH_FIXTURE = "authoring_path_e2e"


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
        queued = self.request("/v1/input/" + action, payload, method="POST")
        action_id = queued["action_id"]

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
def spawned_authoring_app(port: int):
    environment = os.environ.copy()
    environment["RUVIE_QA_PORT"] = str(port)
    environment.setdefault("RUVIE_QA_FIXTURE", AUTHORING_FIXTURE)
    process = subprocess.Popen(
        ["cargo", "run", "-p", "app", "--locked"],
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
