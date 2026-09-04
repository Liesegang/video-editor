#!/usr/bin/env python3
"""Exercise the real Settings modal through the native loopback QA bridge."""

from __future__ import annotations

import json
import os
import pathlib
import time

from qa_support import QaFailure, capture_viewport, run_suite_main


REQUIRED_COMPONENTS = (
    "settings.dialog",
    "settings.footer",
    "settings.restore_defaults",
    "settings.save",
    "settings.cancel",
)


def _open_settings(client):
    client.key("comma", True, command=True)
    client.key("comma", False, command=True)


def _components(client):
    return {
        component["id"]: component
        for component in client.component_snapshot()["components"]
    }


def _wait_dialog(client):
    def ready():
        components = _components(client)
        return (
            components
            if all(component_id in components for component_id in REQUIRED_COMPONENTS)
            else None
        )

    return client.wait_until("Settings dialog", ready)


def _wait_dialog_gone(client):
    def gone():
        snapshot = client.component_snapshot()
        return (
            snapshot
            if not any(
                component["id"] == "settings.dialog"
                for component in snapshot["components"]
            )
            else None
        )

    return client.wait_until("Settings dialog dismissal", gone)


def _assert_stable(client, initial):
    samples = [initial["settings.dialog"]["rect_points"]]
    for _ in range(8):
        time.sleep(0.03)
        components = _components(client)
        if "settings.dialog" not in components:
            raise QaFailure("Settings dialog disappeared during geometry sampling")
        samples.append(components["settings.dialog"]["rect_points"])

    widths = [float(sample["width"]) for sample in samples]
    heights = [float(sample["height"]) for sample in samples]
    if max(widths) - min(widths) > 0.5 or max(heights) - min(heights) > 0.5:
        raise QaFailure(
            "Settings dialog grew across frames: widths={!r}, heights={!r}".format(
                widths, heights
            )
        )
    if min(widths) < 480.0 or min(heights) < 320.0:
        raise QaFailure("Settings dialog ignored its shared minimum content extent")
    return {"widths": widths, "heights": heights}


def _assert_layout(components):
    dialog = components["settings.dialog"]["rect_points"]
    footer = components["settings.footer"]["rect_points"]
    if float(footer["max_y"]) > float(dialog["max_y"]) + 0.5:
        raise QaFailure("Settings footer escaped the dialog")

    buttons = [components[component_id] for component_id in REQUIRED_COMPONENTS[2:]]
    for button in buttons:
        rect = button["rect_points"]
        if float(rect["width"]) < 103.0 or float(rect["height"]) < 27.0:
            raise QaFailure("Settings action did not use the shared dialog button size")
        if not (
            float(dialog["min_x"]) <= float(rect["min_x"])
            and float(rect["max_x"]) <= float(dialog["max_x"])
            and float(footer["min_y"]) <= float(rect["min_y"])
            and float(rect["max_y"]) <= float(footer["max_y"])
        ):
            raise QaFailure("Settings action escaped the shared footer")

    for index, left in enumerate(buttons):
        left_rect = left["rect_points"]
        for right in buttons[index + 1 :]:
            right_rect = right["rect_points"]
            separated = (
                float(left_rect["max_x"]) <= float(right_rect["min_x"])
                or float(right_rect["max_x"]) <= float(left_rect["min_x"])
            )
            if not separated:
                raise QaFailure("Settings action buttons overlap")


def _capture(client):
    artifact_dir = pathlib.Path(
        os.environ.get(
            "RUVIE_QA_ARTIFACT_DIR",
            pathlib.Path("target") / "qa-settings-dialog-e2e",
        )
    )
    metadata = capture_viewport(client, artifact_dir / "capture.png")
    (artifact_dir / "capture.json").write_text(
        json.dumps(metadata, ensure_ascii=False, indent=2) + "\n", encoding="utf-8"
    )
    return metadata


def run_suite(client):
    client.wait_health()
    _open_settings(client)
    initial = _wait_dialog(client)
    _assert_layout(initial)
    initial_geometry = _assert_stable(client, initial)
    capture = _capture(client)

    client.click_component("settings.restore_defaults")
    after_restore = _wait_dialog(client)
    _assert_layout(after_restore)
    restored_geometry = _assert_stable(client, after_restore)

    client.click_component("settings.cancel")
    _wait_dialog_gone(client)
    if client.request("/health").get("ok") is not True:
        raise QaFailure("UI loop became unresponsive after closing Settings")

    _open_settings(client)
    _wait_dialog(client)
    client.key("escape", True)
    client.key("escape", False)
    _wait_dialog_gone(client)

    return {
        "suite": "settings-dialog",
        "initial_geometry": initial_geometry,
        "restored_geometry": restored_geometry,
        "capture": capture,
        "actions": client.evidence,
    }


if __name__ == "__main__":
    raise SystemExit(
        run_suite_main(
            "qa-settings-dialog-e2e",
            run_suite,
            "target/qa-settings-dialog-e2e-evidence.json",
        )
    )
