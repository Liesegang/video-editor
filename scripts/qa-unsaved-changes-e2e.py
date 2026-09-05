#!/usr/bin/env python3
"""Exercise the native Unsaved Changes guard, layout, and close lifecycle."""

from __future__ import annotations

import json
import os
import pathlib
import time

from qa_support import (
    QaFailure,
    capture_viewport,
    item_by_name,
    run_suite_main,
    wait_endpoint_closed,
)


TARGET_NAME = "QA Overlap"
DIALOG_CONTENT_WIDTH = 440.0
DIALOG_FRAME_CHROME = 14.0


def _command(client, key):
    client.key(key, True, command=True)
    client.key(key, False, command=True)


def _component_map(client):
    return {
        component["id"]: component
        for component in client.component_snapshot()["components"]
    }


def _wait_dialog(client, action):
    def ready():
        components = _component_map(client)
        required = (
            "unsaved.dialog",
            "unsaved.warning",
            "unsaved.content",
            "unsaved.footer",
            "unsaved.save",
            "unsaved.discard",
            "unsaved.cancel",
        )
        if any(component_id not in components for component_id in required):
            return None
        dialog = components["unsaved.dialog"]
        if (dialog.get("metadata") or {}).get("action") != action:
            return None
        return components

    components = client.wait_until("{} Unsaved Changes dialog".format(action), ready)
    _assert_dialog_layout(components)
    _assert_dialog_stable(client, action, components)
    return components


def _assert_dialog_stable(client, action, initial_components):
    """A fixed dialog must not feed its measured width back into the next frame."""

    samples = [initial_components["unsaved.dialog"]["rect_points"]]
    for _ in range(7):
        time.sleep(0.03)
        components = _component_map(client)
        dialog = components.get("unsaved.dialog")
        if dialog is None or (dialog.get("metadata") or {}).get("action") != action:
            raise QaFailure("Unsaved dialog changed while checking frame-to-frame geometry")
        samples.append(dialog["rect_points"])

    widths = [float(sample["width"]) for sample in samples]
    heights = [float(sample["height"]) for sample in samples]
    if max(widths) - min(widths) > 0.5 or max(heights) - min(heights) > 0.5:
        raise QaFailure(
            "Unsaved dialog grew across frames: widths={!r}, heights={!r}".format(
                widths, heights
            )
        )


def _assert_dialog_layout(components):
    dialog_component = components["unsaved.dialog"]
    dialog = dialog_component["rect_points"]
    warning = components["unsaved.warning"]["rect_points"]
    content = components["unsaved.content"]["rect_points"]
    footer = components["unsaved.footer"]["rect_points"]
    buttons = [
        components[component_id]
        for component_id in ("unsaved.save", "unsaved.discard", "unsaved.cancel")
    ]

    declared_width = float((dialog_component.get("metadata") or {}).get("content_width", 0.0))
    expected_outer_width = DIALOG_CONTENT_WIDTH + DIALOG_FRAME_CHROME
    if declared_width != DIALOG_CONTENT_WIDTH or abs(float(dialog["width"]) - expected_outer_width) > 1.0:
        raise QaFailure(
            "Unsaved dialog content/chrome width is inconsistent: {!r}/{!r}".format(
                declared_width, dialog["width"]
            )
        )
    if float(warning["min_x"]) < float(content["min_x"]):
        pass
    else:
        raise QaFailure("warning icon was not laid out before dialog content")
    if float(footer["min_y"]) <= float(content["max_y"]):
        raise QaFailure("dialog footer did not follow the content with spacing")

    previous = None
    expected_roles = ("primary", "destructive", "secondary")
    for index, (component, role) in enumerate(zip(buttons, expected_roles)):
        rect = component["rect_points"]
        metadata = component.get("metadata") or {}
        if metadata.get("role") != role or metadata.get("order") != index:
            raise QaFailure("dialog button role/order metadata is inconsistent")
        if float(rect["height"]) < 27.0 or float(rect["width"]) < 103.0:
            raise QaFailure("dialog action button is smaller than the shared minimum")
        if previous is not None and float(rect["min_x"]) <= float(previous["max_x"]):
            raise QaFailure("dialog buttons overlap or are not Save/Discard/Cancel")
        previous = rect


def _wait_dialog_gone(client):
    def gone():
        snapshot = client.component_snapshot()
        return (
            snapshot
            if not any(
                component["id"] == "unsaved.dialog"
                for component in snapshot["components"]
            )
            else None
        )

    return client.wait_until(
        "Unsaved Changes dialog dismissal",
        gone,
    )


def _dirty_item(client, delta_x):
    before = client.state()
    item = item_by_name(before["project"], TARGET_NAME)
    client.drag_component_by("timeline.item:" + item["id"], delta_x, 0.0, steps=10)
    return client.wait_until(
        "one dirty Timeline edit",
        lambda: state
        if (state := client.state())["history"]["revision"]
        == before["history"]["revision"] + 1
        else None,
    )


def _state_with_target(client):
    state = client.state()
    return (
        state
        if TARGET_NAME in {item["name"] for item in state["project"]["items"].values()}
        else None
    )


def _capture_dialog(client):
    artifact_dir = pathlib.Path(
        os.environ.get(
            "RUVIE_QA_ARTIFACT_DIR",
            pathlib.Path("target") / "qa-unsaved-changes-e2e",
        )
    )
    artifact_dir.mkdir(parents=True, exist_ok=True)
    png_path = artifact_dir / "capture.png"
    metadata = capture_viewport(client, png_path)
    (artifact_dir / "capture.json").write_text(
        json.dumps(metadata, ensure_ascii=False, indent=2) + "\n", encoding="utf-8"
    )
    return metadata


def run_suite(client):
    client.wait_health()
    initial = client.state()
    first_dirty = _dirty_item(client, 18.0)

    _command(client, "n")
    new_dialog = _wait_dialog(client, "new_project")
    capture = _capture_dialog(client)

    # Competing commands and native close requests must not replace the one
    # action already represented by this dialog.
    _command(client, "o")
    client.inject("close-request", {})
    still_new = _wait_dialog(client, "new_project")
    client.key("escape", True)
    client.key("escape", False)
    _wait_dialog_gone(client)
    after_cancel = client.state()
    if after_cancel["project"] != first_dirty["project"]:
        raise QaFailure("Cancel changed the authoritative Project")
    if after_cancel["history"] != first_dirty["history"]:
        raise QaFailure("Cancel changed history")

    # A real ViewportEvent::Close must be cancelled while dirty and mapped to
    # exactly one guarded Quit request.
    client.inject("close-request", {})
    _wait_dialog(client, "quit")
    if client.request("/health").get("ok") is not True:
        raise QaFailure("window close was not cancelled while the prompt was active")
    client.click_component("unsaved.cancel")
    _wait_dialog_gone(client)

    # Discard returns from its click frame first; project replacement happens
    # on a later frame and the HTTP/UI loop remains responsive.
    _command(client, "n")
    _wait_dialog(client, "new_project")
    discard_started = time.monotonic()
    client.click_component("unsaved.discard")
    after_discard = client.wait_until(
        "deferred Discard New Project",
        lambda: state
        if (state := client.state())["project"]["name"] == "Untitled Project"
        else None,
    )
    discard_seconds = time.monotonic() - discard_started
    if after_discard["project"]["items"]:
        raise QaFailure("Discard New Project executed more than one project action")
    if client.request("/health").get("ok") is not True:
        raise QaFailure("UI loop became unresponsive after Discard")

    # The QA fixture has a real file path. Reopen it, dirty it, choose Save,
    # then reopen again to prove the edit reached disk before New executed.
    _command(client, "o")
    reopened = client.wait_until(
        "opening the saved QA Project",
        lambda: _state_with_target(client),
    )
    saved_dirty = _dirty_item(client, 26.0)
    saved_item = item_by_name(saved_dirty["project"], TARGET_NAME)
    _command(client, "n")
    _wait_dialog(client, "new_project")
    client.click_component("unsaved.save")
    client.wait_until(
        "Save followed by exactly one New Project",
        lambda: state
        if (state := client.state())["project"]["name"] == "Untitled Project"
        else None,
    )
    _command(client, "o")
    reopened_saved = client.wait_until(
        "reopening the saved edit",
        lambda: _state_with_target(client),
    )
    if item_by_name(reopened_saved["project"], TARGET_NAME)["interval"] != saved_item["interval"]:
        raise QaFailure("Save did not persist the dirty Project before New")

    final_dirty = _dirty_item(client, 14.0)
    _command(client, "o")
    _wait_dialog(client, "open_project")
    client.click_component("unsaved.cancel")
    _wait_dialog_gone(client)
    if client.state()["project"] != final_dirty["project"]:
        raise QaFailure("Cancel on guarded Open changed the Project")

    client.inject("close-request", {})
    quit_dialog = _wait_dialog(client, "quit")
    quit_started = time.monotonic()
    # Do not poll the action endpoint after this click: successful Discard
    # closes the native app and its QA server in the same UI frame.
    client.queue_terminal_click_component("unsaved.discard")
    closed_at = wait_endpoint_closed(client, description="Discard Quit")

    return {
        "suite": "unsaved-changes",
        "initial_revision": initial["history"]["revision"],
        "cancel_revision": after_cancel["history"]["revision"],
        "discard_new_seconds": discard_seconds,
        "discard_quit_seconds": closed_at - quit_started,
        "dialog": new_dialog["unsaved.dialog"],
        "single_flight_dialog": still_new["unsaved.dialog"],
        "quit_dialog": quit_dialog["unsaved.dialog"],
        "capture": capture,
        "reopened_revision": reopened["history"]["revision"],
        "saved_revision": saved_dirty["history"]["revision"],
        "actions": client.evidence,
    }


if __name__ == "__main__":
    qa_project_path = pathlib.Path(
        os.environ.get(
            "RUVIE_QA_PROJECT_PATH",
            "target/qa-unsaved-changes-e2e/project.ruvie",
        )
    ).resolve()
    qa_project_path.parent.mkdir(parents=True, exist_ok=True)
    os.environ["RUVIE_QA_PROJECT_PATH"] = str(qa_project_path)
    raise SystemExit(
        run_suite_main(
            "qa-unsaved-changes-e2e",
            run_suite,
            "target/qa-unsaved-changes-e2e-evidence.json",
        )
    )
