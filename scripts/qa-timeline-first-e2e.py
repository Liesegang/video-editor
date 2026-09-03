#!/usr/bin/env python3
"""Headful E2E for the Timeline-first editing contract.

The suite drives real egui pointer input through the localhost-only QA bridge.
It does not mutate Project state through a test API.
"""

from __future__ import annotations

import argparse
import json
import os
from pathlib import Path
import socket
import struct
import subprocess
import sys
import time
import urllib.error
import urllib.request
import zlib


ROOT = Path(__file__).resolve().parents[1]


class QaFailure(RuntimeError):
    pass


class Client:
    def __init__(self, base_url: str, timeout: float) -> None:
        self.base_url = base_url.rstrip("/")
        self.timeout = timeout

    def request(self, method: str, path: str, body: object | None = None):
        encoded = None if body is None else json.dumps(body).encode("utf-8")
        request = urllib.request.Request(
            self.base_url + path,
            data=encoded,
            method=method,
            headers={"Content-Type": "application/json"},
        )
        with urllib.request.urlopen(request, timeout=self.timeout) as response:
            content = response.read()
            if response.headers.get_content_type() == "image/png":
                return content
            return json.loads(content.decode("utf-8"))

    def get(self, path: str):
        return self.request("GET", path)

    def post(self, path: str, body: object | None = None):
        return self.request("POST", path, body or {})

    def components(self) -> dict[str, dict]:
        return {
            component["id"]: component
            for component in self.get("/v1/components")["components"]
        }

    def state(self) -> dict:
        return self.get("/v1/state")


def wait_for(description: str, probe, timeout: float = 12.0):
    deadline = time.monotonic() + timeout
    last = None
    while time.monotonic() < deadline:
        try:
            last = probe()
            if last:
                return last
        except (ConnectionError, OSError, urllib.error.URLError, KeyError):
            pass
        time.sleep(0.1)
    raise QaFailure(f"timed out waiting for {description}; last={last!r}")


def center(component: dict) -> dict[str, float]:
    left, top, right, bottom = component["rect"]
    return {"x": (left + right) / 2.0, "y": (top + bottom) / 2.0}


def click(client: Client, component: dict) -> None:
    client.post("/v1/input/click", center(component))


def drag(
    client: Client,
    start: dict[str, float],
    end: dict[str, float],
    *,
    button: str = "primary",
) -> None:
    client.post(
        "/v1/input/drag",
        {"from": start, "to": end, "steps": 10, "button": button},
    )


def assert_close(actual: float, expected: float, message: str) -> None:
    if abs(actual - expected) > 0.05:
        raise QaFailure(f"{message}: expected {expected}, got {actual}")


def png_chunk(kind: bytes, payload: bytes) -> bytes:
    return (
        struct.pack(">I", len(payload))
        + kind
        + payload
        + struct.pack(">I", zlib.crc32(kind + payload) & 0xFFFFFFFF)
    )


def write_fixture(path: Path) -> None:
    width, height = 96, 54
    rows = []
    for y in range(height):
        row = bytearray([0])
        for x in range(width):
            bright = 220 if (x // 12 + y // 12) % 2 == 0 else 45
            row.extend((bright, 90, 235 - bright // 2, 255))
        rows.append(bytes(row))
    signature = b"\x89PNG\r\n\x1a\n"
    header = struct.pack(">IIBBBBB", width, height, 8, 6, 0, 0, 0)
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_bytes(
        signature
        + png_chunk(b"IHDR", header)
        + png_chunk(b"IDAT", zlib.compress(b"".join(rows), 9))
        + png_chunk(b"IEND", b"")
    )


def free_port() -> int:
    with socket.socket() as listener:
        listener.bind(("127.0.0.1", 0))
        return int(listener.getsockname()[1])


def run_suite(client: Client, capture: Path | None) -> dict:
    wait_for(
        "QA bridge",
        lambda: client.get("/health").get("status") == "ok",
        timeout=90.0,
    )

    def initial_components():
        values = client.components()
        if any(item["kind"] == "draggable_asset" for item in values.values()) and any(
            item["kind"] == "layer" for item in values.values()
        ):
            return values
        return None

    components = wait_for(
        "Asset and Timeline Layer",
        initial_components,
    )
    asset = next(item for item in components.values() if item["kind"] == "draggable_asset")
    layer = next(item for item in components.values() if item["kind"] == "layer")
    ruler = components["timeline.ruler"]
    ruler_left = float(ruler["rect"][0])
    pixels_per_second = 80.0

    destination = {
        "x": ruler_left + 3.0 * pixels_per_second,
        "y": center(layer)["y"],
    }
    drag(client, center(asset), destination)
    state = wait_for(
        "Asset drop",
        lambda: (state if len(state.get("layers", [])) == 1 else None)
        if (state := client.state())
        else None,
    )
    clip = state["layers"][0]
    assert_close(float(clip["start"]), 3.0, "Asset drop time")
    item_id = state["frame"]["selected_item"]

    clip_id = f"timeline.clip:{item_id}"
    label_id = f"timeline.clip_label:{item_id}"
    components = wait_for(
        "collapsed Clip on Layer row",
        lambda: (items if clip_id in items and label_id not in items else None)
        if (items := client.components())
        else None,
    )

    expander = next(item for item in components.values() if item["kind"] == "expander")
    click(client, expander)
    components = wait_for(
        "expanded Clip row", lambda: (items if label_id in items else None)
        if (items := client.components())
        else None,
    )
    if client.state()["layers"][0]["layer_expanded"] is not True:
        raise QaFailure("Layer expansion state was not retained")

    clip_component = components[clip_id]
    start = center(clip_component)
    drag(client, start, {"x": start["x"] + 160.0, "y": start["y"]})
    state = wait_for(
        "Clip move",
        lambda: (state if abs(state["layers"][0]["start"] - 5.0) < 0.05 else None)
        if (state := client.state())
        else None,
    )
    assert_close(float(state["layers"][0]["start"]), 5.0, "Clip drag time")

    components = client.components()
    ruler = components["timeline.ruler"]
    ruler_origin = float(ruler["rect"][0])
    scroll_anchor = {
        "x": min(float(ruler["rect"][2]) - 1.0, 1_200.0),
        "y": center(ruler)["y"],
    }
    client.post(
        "/v1/input/scroll",
        {**scroll_anchor, "delta_x": -480.0, "delta_y": 0.0},
    )
    scrolled_ruler = wait_for(
        "Timeline horizontal scroll",
        lambda: (
            current
            if abs(float(current["rect"][0]) - ruler_origin) > 1.0
            else None
        )
        if (current := client.components().get("timeline.ruler"))
        else None,
    )
    client.post(
        "/v1/input/scroll",
        {
            "x": scroll_anchor["x"],
            "y": center(scrolled_ruler)["y"],
            "delta_x": 480.0,
            "delta_y": 0.0,
        },
    )
    ruler = wait_for(
        "Timeline scroll return",
        lambda: (
            current
            if abs(float(current["rect"][0]) - ruler_origin) < 1.0
            else None
        )
        if (current := client.components().get("timeline.ruler"))
        else None,
    )
    client.post(
        "/v1/input/click",
        {"x": float(ruler["rect"][0]) + 6.0 * pixels_per_second, "y": center(ruler)["y"]},
    )
    state = wait_for(
        "Preview seek",
        lambda: (state if abs(state["frame"]["current_time"] - 6.0) < 0.05 else None)
        if (state := client.state())
        else None,
    )
    if state["preview"]["has_image"] is not True:
        raise QaFailure("Preview did not render the placed Asset")

    components = client.components()
    expected_controls = [
        f"inspector.value:{item_id}:position:x",
        f"inspector.value:{item_id}:position:y",
        f"inspector.value:{item_id}:scale:x",
        f"inspector.value:{item_id}:scale:y",
        f"inspector.value:{item_id}:rotation",
        f"inspector.value:{item_id}:anchor:x",
        f"inspector.value:{item_id}:anchor:y",
        f"inspector.value:{item_id}:opacity",
    ]
    missing = [control for control in expected_controls if control not in components]
    if missing:
        raise QaFailure(f"compact Inspector controls missing: {missing}")
    assert_close(
        float(components[f"inspector.value:{item_id}:scale:x"]["metadata"]["value"]),
        100.0,
        "Scale presentation",
    )

    key_id = f"inspector.keyframe:{item_id}:position"
    click(client, components[key_id])
    key = wait_for(
        "Position keyframe",
        lambda: (component if component["metadata"]["key_at_current_time"] else None)
        if (component := client.components().get(key_id))
        else None,
    )
    if key["metadata"]["keyframed"] is not True:
        raise QaFailure("Position was not promoted to keyframe animation")

    x_id = f"inspector.value:{item_id}:position:x"
    x_control = client.components()[x_id]
    x_start = center(x_control)
    drag(client, x_start, {"x": x_start["x"] + 32.0, "y": x_start["y"]})
    x_value = wait_for(
        "Position numeric drag",
        lambda: (value if abs(value) > 0.01 else None)
        if (
            value := float(client.components()[x_id]["metadata"]["value"])
        ) is not None
        else None,
    )

    click(client, client.components()["inspector.effects.add"])
    effect_options = wait_for(
        "Effect catalog",
        lambda: (options if options else None)
        if (
            options := [
                component
                for component_id, component in client.components().items()
                if component_id.startswith("inspector.effects.option:")
            ]
        )
        else None,
    )
    effect_option = next(
        (
            option
            for option in effect_options
            if option["metadata"].get("effect_id") == "blur"
        ),
        effect_options[0],
    )
    effect_name = effect_option["metadata"]["name"]
    click(client, effect_option)
    remove_effect = wait_for(
        "Effect attachment",
        lambda: (controls[0] if controls else None)
        if (
            controls := [
                component
                for component_id, component in client.components().items()
                if component_id.startswith("inspector.effects.remove:")
            ]
        )
        else None,
    )
    add_binding = wait_for(
        "Effect PublishedParameter binding control",
        lambda: (controls[0] if controls else None)
        if (
            controls := [
                component
                for component_id, component in client.components().items()
                if component_id.startswith("inspector.binding.audio:")
                and component["metadata"].get("enabled")
            ]
        )
        else None,
    )
    binding_deadline = time.monotonic() + 12.0
    while time.monotonic() < binding_deadline:
        if client.state().get("runtime", {}).get("signal_binding_count") == 1:
            break
        current = next(
            (
                value
                for component_id, value in client.components().items()
                if component_id.startswith("inspector.binding.audio:")
                and value["metadata"].get("enabled")
            ),
            None,
        )
        if current is not None:
            click(client, current)
        time.sleep(0.35)
    else:
        raise QaFailure("timed out creating the compiled Signal Binding")

    click(client, client.components()["workspace.logic"])
    signal_preview = wait_for(
        "Logic Signal preview section",
        lambda: client.components().get("logic.signal_preview"),
    )
    if not signal_preview["metadata"].get("open"):
        click(client, signal_preview)
    signal_source = wait_for(
        "Logic Signal source routed through RenderPlan",
        lambda: (controls[0] if controls else None)
        if (
            controls := [
                component
                for component in client.components().values()
                if component["kind"] == "signal_source"
            ]
        )
        else None,
    )
    operation_menu = wait_for(
        "Module operation catalog",
        lambda: (controls[0] if controls else None)
        if (
            controls := [
                component
                for component in client.components().values()
                if component["kind"] == "module_operation_menu"
            ]
        )
        else None,
    )
    if int(operation_menu["metadata"]["operation_count"]) < 1:
        raise QaFailure("Module operation catalog is empty")
    left, top, right, bottom = signal_source["rect"]
    drag(
        client,
        {"x": left + (right - left) * 0.2, "y": (top + bottom) / 2.0},
        {"x": left + (right - left) * 0.8, "y": (top + bottom) / 2.0},
    )
    routed_signal = wait_for(
        "live Signal source sample",
        lambda: (component if float(component["metadata"]["value"]) > 0.25 else None)
        if (
            component := next(
                (
                    value
                    for value in client.components().values()
                    if value["kind"] == "signal_source"
                ),
                None,
            )
        )
        else None,
    )
    wait_for(
        "Signal route status",
        lambda: (state if "Signal sample routed to" in state.get("status", "") else None)
        if (state := client.state())
        else None,
    )
    click(client, client.components()["workspace.edit"])
    remove_effect = wait_for(
        "Effect attachment after returning to Edit",
        lambda: (controls[0] if controls else None)
        if (
            controls := [
                component
                for component_id, component in client.components().items()
                if component_id.startswith("inspector.effects.remove:")
            ]
        )
        else None,
    )
    removal_deadline = time.monotonic() + 12.0
    while time.monotonic() < removal_deadline:
        controls = [
            component
            for component_id, component in client.components().items()
            if component_id.startswith("inspector.effects.remove:")
        ]
        if not controls:
            break
        click(client, controls[0])
        time.sleep(0.35)
    else:
        raise QaFailure(
            f"timed out removing Effect; status={client.state().get('status')!r}"
        )

    preview = client.components()["preview.canvas"]
    preview_center = center(preview)
    before = client.state()["preview"]
    client.post(
        "/v1/input/scroll",
        {
            **preview_center,
            "delta_y": 120.0,
            "modifiers": {"command": True},
        },
    )
    zoomed = wait_for(
        "Preview zoom",
        lambda: (value if value["zoom"] != before["zoom"] else None)
        if (value := client.state()["preview"])
        else None,
    )
    drag(
        client,
        preview_center,
        {"x": preview_center["x"] + 48.0, "y": preview_center["y"] + 24.0},
        button="middle",
    )
    panned = wait_for(
        "Preview pan",
        lambda: (value if value["pan"] != zoomed["pan"] else None)
        if (value := client.state()["preview"])
        else None,
    )

    click(client, client.components()["timeline.play"])
    playing = wait_for(
        "playback start",
        lambda: (state if state["frame"]["is_playing"] else None)
        if (state := client.state())
        else None,
    )
    started_at = float(playing["frame"]["current_time"])
    advanced = wait_for(
        "playback advance",
        lambda: (state if state["frame"]["current_time"] > started_at + 0.05 else None)
        if (state := client.state())
        else None,
    )
    click(client, client.components()["timeline.play"])
    wait_for(
        "playback pause",
        lambda: (state if not state["frame"]["is_playing"] else None)
        if (state := client.state())
        else None,
    )

    components = client.components()
    ruler = components["timeline.ruler"]
    client.post(
        "/v1/input/click",
        {"x": float(ruler["rect"][0]) + 6.0 * pixels_per_second, "y": center(ruler)["y"]},
    )
    wait_for(
        "final Preview frame",
        lambda: (state if abs(state["frame"]["current_time"] - 6.0) < 0.05 else None)
        if (state := client.state())
        else None,
    )
    click(client, client.components()["preview.fit"])
    wait_for(
        "Preview fit",
        lambda: (
            state
            if state["preview"]["has_image"]
            and (
                state["preview"]["pan"] != panned["pan"]
                or state["preview"]["zoom"] != panned["zoom"]
            )
            else None
        )
        if (state := client.state())
        else None,
    )

    if capture is not None:
        client.post("/v1/captures")
        wait_for(
            "screenshot", lambda: client.get("/v1/captures/1")["phase"] == "ready"
        )
        capture.parent.mkdir(parents=True, exist_ok=True)
        capture.write_bytes(client.get("/v1/captures/1.png"))

    return {
        "asset_drop_start": 3.0,
        "clip_drag_start": state["layers"][0]["start"],
        "selected_item": item_id,
        "position_x": x_value,
        "position_keyframed": True,
        "effect_added_and_removed": effect_name,
        "signal_source_sample": routed_signal["metadata"]["value"],
        "preview_zoom": zoomed["zoom"],
        "preview_pan": panned["pan"],
        "playback_advanced_to": advanced["frame"]["current_time"],
    }


def parse_args():
    parser = argparse.ArgumentParser()
    parser.add_argument("--base-url")
    parser.add_argument("--spawn", action="store_true")
    parser.add_argument("--timeout", type=float, default=30.0)
    parser.add_argument("--capture", type=Path)
    parser.add_argument(
        "--evidence", type=Path, default=ROOT / "target/qa-timeline-first-evidence.json"
    )
    return parser.parse_args()


def main() -> int:
    args = parse_args()
    process = None
    port = free_port() if args.spawn else 39091
    base_url = args.base_url or f"http://127.0.0.1:{port}"
    try:
        if args.spawn:
            fixture = ROOT / "target/qa-timeline-first-fixture.png"
            write_fixture(fixture)
            environment = os.environ.copy()
            environment["RUVIE_QA_PORT"] = str(port)
            environment["RUVIE_QA_ASSET"] = str(fixture.resolve())
            process = subprocess.Popen(
                ["cargo", "run", "-p", "app", "--locked"],
                cwd=ROOT,
                env=environment,
            )
        result = run_suite(Client(base_url, args.timeout), args.capture)
        args.evidence.parent.mkdir(parents=True, exist_ok=True)
        args.evidence.write_text(json.dumps(result, indent=2) + "\n", encoding="utf-8")
        print(f"[qa-timeline-first-e2e] PASS; evidence: {args.evidence.resolve()}")
        return 0
    except (QaFailure, KeyError, TypeError, ValueError, urllib.error.URLError) as error:
        print(f"[qa-timeline-first-e2e] FAIL: {error}", file=sys.stderr)
        return 1
    finally:
        if process is not None:
            process.terminate()
            try:
                process.wait(timeout=5.0)
            except subprocess.TimeoutExpired:
                process.kill()
                process.wait(timeout=5.0)


if __name__ == "__main__":
    sys.exit(main())
