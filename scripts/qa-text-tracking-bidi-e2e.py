#!/usr/bin/env python3
"""Exercise RTL and mixed-direction Tracking through the production UI."""

import os
import pathlib

from qa_support import (
    AUTHORING_FIXTURE,
    QaClient,
    QaFailure,
    activate_dock_tab,
    bring_timeline_component,
    capture_viewport,
    close_clean_native_app,
    component_in_inspector,
    convert_timeline_item_to_node_clip,
    free_port,
    item_by_name,
    rendered_current_revision,
    request_clean_native_close,
    run_suite_main,
    save_project_to_disk,
    spawned_authoring_app,
)
from qa_text_ensemble_support import (
    constant_number,
    open_and_choose,
    operation,
    seek_rendered,
    text_operations,
)
from qa_tracking_geometry import assert_gizmo_parity, observe_gizmos, selected_gizmo
from qa_tracking_support import (
    enter_tracking_amount,
    key_time_seconds,
    keyframe_signature,
    module_tracking,
    maybe_tracking_property_keys,
    observe_tracking_times,
    set_multiline_text,
    tracking_property_keys,
)


OPEN_EXISTING_PROJECT_ENV = "RUVIE_QA_OPEN_EXISTING_PROJECT"
BOUNDS_TOLERANCE = 0.75
TIMELINE_TAB = "dock.tab:timeline"


def _artifact_dir():
    value = pathlib.Path(
        os.environ.get(
            "RUVIE_QA_ARTIFACT_DIR",
            pathlib.Path("target") / "qa-text-tracking-bidi-e2e",
        )
    ).resolve()
    value.mkdir(parents=True, exist_ok=True)
    return value


def _amount_mode(item_id, operation_id):
    return "inspector.property_mode:text_ensemble:{}:{}:amount".format(
        item_id, operation_id
    )


def _undo(client):
    client.key("z", True, command=True)
    client.key("z", False, command=True)


def _rendered_revision(client, revision, frame, description):
    return client.wait_until(
        description,
        lambda: state
        if (state := rendered_current_revision(client))
        and state["history"]["revision"] == revision
        and state["editor"]["preview"]["rendered_frame"] == frame
        else None,
        30.0,
    )


def _set_constant_amount(client, item_id, operation_id, desired, description):
    before = client.state()
    before_value = constant_number(operation(before, item_id, operation_id), "amount")
    if abs(float(desired) - before_value) < 0.001:
        return before
    enter_tracking_amount(client, item_id, operation_id, desired)
    return client.wait_until(
        description,
        lambda: state
        if (state := client.state())["history"]["revision"]
        == before["history"]["revision"] + 1
        and abs(
            constant_number(operation(state, item_id, operation_id), "amount")
            - desired
        )
        < 0.001
        else None,
    )


def _bounds_sample(client, item_id, state, description):
    frame = state["editor"]["timeline"]["current_frame"]
    rendered = _rendered_revision(
        client, state["history"]["revision"], frame, description + " Preview"
    )
    return {
        "frame": frame,
        "pixel_hash": rendered["editor"]["preview"]["pixel_hash"],
        "gizmo": selected_gizmo(client, item_id, description + " Gizmo"),
    }


def _assert_left_anchor(reference, candidate, description):
    difference = abs(
        reference["gizmo"]["bounds"]["min_x"]
        - candidate["gizmo"]["bounds"]["min_x"]
    )
    if difference > BOUNDS_TOLERANCE:
        raise QaFailure(
            "{} changed its left anchor by {:.3f}px".format(description, difference)
        )


def _exercise_expanding_text(
    client, item_id, operation_id, text, description, artifact_stem
):
    authored = set_multiline_text(client, item_id, text)
    zero_state = seek_rendered(client, 1.5)
    zero = _bounds_sample(client, item_id, zero_state, description + " zero")

    positive_state = _set_constant_amount(
        client, item_id, operation_id, 12.0, description + " positive Tracking"
    )
    positive = _bounds_sample(client, item_id, positive_state, description + " positive")
    _assert_left_anchor(zero, positive, description + " positive Tracking")
    if positive["gizmo"]["bounds"]["width"] <= zero["gizmo"]["bounds"]["width"] + 4.0:
        raise QaFailure(description + " positive Tracking did not expand visual width")
    if positive["pixel_hash"] == zero["pixel_hash"]:
        raise QaFailure(description + " positive Tracking did not change rendered pixels")
    positive["capture"] = capture_viewport(
        client, _artifact_dir() / (artifact_stem + "-positive.png")
    )

    _undo(client)
    restored = client.wait_until(
        description + " positive Tracking Undo",
        lambda: state
        if (state := client.state())["project"] == authored["project"]
        and state["history"]["revision"] == positive_state["history"]["revision"] + 1
        else None,
    )
    restored_sample = _bounds_sample(
        client, item_id, restored, description + " restored zero"
    )
    if restored_sample["pixel_hash"] != zero["pixel_hash"]:
        raise QaFailure(description + " Undo did not restore zero Tracking pixels")
    assert_gizmo_parity(
        [{"seconds": 1.5, "frame": zero["frame"], "gizmo": zero["gizmo"]}],
        [
            {
                "seconds": 1.5,
                "frame": restored_sample["frame"],
                "gizmo": restored_sample["gizmo"],
            }
        ],
        description + " Undo",
    )

    negative_state = _set_constant_amount(
        client, item_id, operation_id, -3.0, description + " negative Tracking"
    )
    negative = _bounds_sample(client, item_id, negative_state, description + " negative")
    _assert_left_anchor(zero, negative, description + " negative Tracking")
    if negative["gizmo"]["bounds"]["width"] >= zero["gizmo"]["bounds"]["width"] - 2.0:
        raise QaFailure(description + " negative Tracking did not contract visual width")
    if negative["pixel_hash"] == zero["pixel_hash"]:
        raise QaFailure(description + " negative Tracking did not change rendered pixels")

    _undo(client)
    client.wait_until(
        description + " negative Tracking Undo",
        lambda: state
        if (state := client.state())["project"] == authored["project"]
        and state["history"]["revision"] == negative_state["history"]["revision"] + 1
        else None,
    )
    return {"zero": zero, "positive": positive, "negative": negative}


def _exercise_atomic_arabic(client, item_id, operation_id):
    authored = set_multiline_text(client, item_id, "سلام")
    zero = _bounds_sample(client, item_id, seek_rendered(client, 1.5), "Arabic zero")
    samples = {}
    for amount, label in ((12.0, "positive"), (-3.0, "negative")):
        changed = _set_constant_amount(
            client, item_id, operation_id, amount, "Arabic " + label + " Tracking"
        )
        sample = _bounds_sample(client, item_id, changed, "Arabic " + label)
        if sample["pixel_hash"] != zero["pixel_hash"]:
            raise QaFailure(
                "Arabic joined word changed pixels under {} Tracking".format(label)
            )
        assert_gizmo_parity(
            [{"seconds": 1.5, "frame": zero["frame"], "gizmo": zero["gizmo"]}],
            [{"seconds": 1.5, "frame": sample["frame"], "gizmo": sample["gizmo"]}],
            "Arabic " + label + " Tracking",
        )
        if label == "positive":
            sample["capture"] = capture_viewport(
                client, _artifact_dir() / "arabic-positive.png"
            )
        _undo(client)
        client.wait_until(
            "Arabic {} Tracking Undo".format(label),
            lambda: state
            if (state := client.state())["project"] == authored["project"]
            and state["history"]["revision"] == changed["history"]["revision"] + 1
            else None,
        )
        samples[label] = sample
    return {"zero": zero, **samples}


def _enable_keyframes(client, item_id, operation_id):
    control = _amount_mode(item_id, operation_id)
    component_in_inspector(client, control)
    client.click_component(control)
    option = control + ".option:keyframe"
    client.wait_component_settled(option)
    client.click_component(option)

    def keyframed_state():
        state = client.state()
        keys = maybe_tracking_property_keys(
            operation(state, item_id, operation_id), "amount"
        )
        return state if keys is not None and len(keys) == 1 else None

    return client.wait_until(
        "BiDi Tracking keyframe mode",
        keyframed_state,
    )


def _insert_key_amount(client, item_id, operation_id, seconds, desired, key_count):
    baseline = seek_rendered(client, seconds)
    enter_tracking_amount(client, item_id, operation_id, desired)
    return client.wait_until(
        "BiDi Tracking key at {:.3f}s".format(seconds),
        lambda: state
        if len(
            tracking_property_keys(
                operation((state := client.state()), item_id, operation_id), "amount"
            )
        )
        == key_count
        and state["history"]["revision"] == baseline["history"]["revision"] + 1
        else None,
    )


def _assert_key_contract(keys):
    ordered = sorted(keys, key=lambda key: key_time_seconds(key["time"]))
    expected = [(0.5, 0.0), (1.5, 12.0), (2.5, -3.0)]
    actual = [
        (key_time_seconds(key["time"]), float(key["value"])) for key in ordered
    ]
    if actual != expected:
        raise QaFailure("BiDi Tracking keys changed time/value: {!r}".format(actual))
    ids = [key["id"] for key in ordered]
    if len(set(ids)) != len(ids):
        raise QaFailure("BiDi Tracking key IDs are not unique")
    return ordered


def _fresh_process_parity(
    project_file, expected_project, item_id, operation_id, signature, previews, gizmos, timeout
):
    port = free_port()
    run_id = os.environ.get("RUVIE_QA_RUN_ID", "text-tracking-bidi")
    environment = {
        "RUVIE_QA_FIXTURE": AUTHORING_FIXTURE,
        "RUVIE_QA_PROJECT_PATH": str(project_file),
        OPEN_EXISTING_PROJECT_ENV: "1",
        "RUVIE_QA_PORT_FILE": None,
        "RUVIE_QA_RUN_ID": run_id + ":reload",
    }
    with spawned_authoring_app(port, environment) as process:
        client = QaClient("http://127.0.0.1:{}".format(port), timeout)
        client.wait_health()
        loaded = client.state()
        if loaded["project"] != expected_project:
            raise QaFailure("fresh process loaded a different BiDi Tracking Project")
        source, _, _, amount, _ = module_tracking(
            loaded["project"], item_id, operation_id
        )
        track = source["value"]["automation_tracks"].get(amount["id"])
        if track is None or keyframe_signature(track["keyframes"]) != signature:
            raise QaFailure("fresh process changed BiDi Tracking automation")
        actual_previews = observe_tracking_times(
            client, [sample["seconds"] for sample in previews], loaded["history"]["revision"]
        )
        if actual_previews != previews:
            raise QaFailure("fresh process changed BiDi Tracking pixels")
        activate_dock_tab(
            client, TIMELINE_TAB, "Timeline", "reloaded BiDi Tracking selection"
        )
        bring_timeline_component(client, "timeline.item:" + item_id, -120.0)
        client.click_component("timeline.item:" + item_id)
        actual_gizmos = observe_gizmos(
            client,
            item_id,
            [sample["seconds"] for sample in gizmos],
            loaded["history"]["revision"],
            "reloaded BiDi Tracking Gizmo",
        )
        assert_gizmo_parity(gizmos, actual_gizmos, "reloaded BiDi Tracking Gizmo")
        capture = capture_viewport(client, _artifact_dir() / "capture.png")
        close = close_clean_native_app(client, process, "reloaded BiDi Tracking app", timeout)
    return {
        "previews": actual_previews,
        "gizmos": actual_gizmos,
        "capture": capture,
        "close": close,
        "actions": client.evidence,
    }


def run_suite(client):
    client.wait_health()
    initial = client.wait_until(
        "initial BiDi Tracking Preview", lambda: rendered_current_revision(client), 30.0
    )
    item = item_by_name(initial["project"], "QA Text")
    item_id = item["id"]
    client.click_component("timeline.item:" + item_id)
    open_and_choose(
        client,
        item_id,
        "tracking",
        "inspector.text_ensemble.add.effector:tracking",
    )
    added = client.wait_until(
        "BiDi Tracking addition",
        lambda: state
        if len(text_operations((state := client.state()), item_id)) == 1
        else None,
    )
    operation_id = text_operations(added, item_id)[0]["id"]

    hebrew = _exercise_expanding_text(
        client, item_id, operation_id, "אבגד", "Hebrew", "hebrew"
    )
    mixed_hebrew = _exercise_expanding_text(
        client,
        item_id,
        operation_id,
        "AB אבגד CD",
        "mixed Hebrew",
        "mixed-hebrew",
    )
    arabic = _exercise_atomic_arabic(client, item_id, operation_id)
    mixed_arabic = _exercise_expanding_text(
        client,
        item_id,
        operation_id,
        "A سلام B",
        "mixed Arabic",
        "mixed-arabic",
    )

    set_multiline_text(client, item_id, "AB אבגד CD")
    seek_rendered(client, 1.5)
    first_key_state = _enable_keyframes(client, item_id, operation_id)
    first_key = tracking_property_keys(
        operation(first_key_state, item_id, operation_id), "amount"
    )[0]
    if abs(key_time_seconds(first_key["time"]) - 0.5) > 0.001:
        raise QaFailure("first BiDi Tracking key used the wrong Clip-local time")
    second = _insert_key_amount(client, item_id, operation_id, 2.5, 12.0, 2)
    before_third = second["project"]
    third = _insert_key_amount(client, item_id, operation_id, 3.5, -3.0, 3)
    _undo(client)
    undone = client.wait_until(
        "negative BiDi Tracking key Undo",
        lambda: state
        if (state := client.state())["project"] == before_third
        and state["history"]["revision"] == third["history"]["revision"] + 1
        else None,
    )
    third = _insert_key_amount(client, item_id, operation_id, 3.5, -3.0, 3)
    direct_keys = _assert_key_contract(
        tracking_property_keys(operation(third, item_id, operation_id), "amount")
    )
    direct_signature = keyframe_signature(direct_keys)
    direct_previews = observe_tracking_times(
        client, [1.5, 2.5, 3.5], third["history"]["revision"]
    )
    direct_gizmos = observe_gizmos(
        client,
        item_id,
        [1.5, 2.5, 3.5],
        third["history"]["revision"],
        "direct BiDi Tracking Gizmo",
    )

    activate_dock_tab(client, TIMELINE_TAB, "Timeline", "BiDi Tracking promotion")
    bring_timeline_component(client, "timeline.item:" + item_id, -120.0)
    client.click_component("timeline.item:" + item_id)
    converted = convert_timeline_item_to_node_clip(
        client, item_id, third["history"]["revision"]
    )
    source, instance_id, definition, amount, target = module_tracking(
        converted["project"], item_id, operation_id
    )
    converted_track = source["value"]["automation_tracks"].get(amount["id"])
    if converted_track is None or keyframe_signature(converted_track["keyframes"]) != direct_signature:
        raise QaFailure("BiDi conversion changed Tracking key IDs/times/easing/values")
    converted_previews = observe_tracking_times(
        client, [1.5, 2.5, 3.5], converted["history"]["revision"]
    )
    if converted_previews != direct_previews:
        raise QaFailure("BiDi conversion changed Preview pixels")
    converted_gizmos = observe_gizmos(
        client,
        item_id,
        [1.5, 2.5, 3.5],
        converted["history"]["revision"],
        "promoted BiDi Tracking Gizmo",
    )
    assert_gizmo_parity(direct_gizmos, converted_gizmos, "promoted BiDi Tracking Gizmo")

    project_file_value = os.environ.get("RUVIE_QA_PROJECT_PATH")
    if not project_file_value:
        raise QaFailure("BiDi Tracking persistence requires RUVIE_QA_PROJECT_PATH")
    project_file = pathlib.Path(project_file_value).resolve()
    expected_project = converted["project"]
    saved, file_evidence = save_project_to_disk(
        client, project_file, "BiDi Tracking Node Clip"
    )
    initial_close = request_clean_native_close(
        client, "saved BiDi Tracking authoring app", client.timeout
    )
    fresh = _fresh_process_parity(
        project_file,
        expected_project,
        item_id,
        operation_id,
        direct_signature,
        converted_previews,
        converted_gizmos,
        client.timeout,
    )
    return {
        "suite": "text-tracking-bidi",
        "native_scope": (
            "production UI aggregate Preview pixels and Gizmo bounds; "
            "glyph-level visual-order semantics are covered by the CPU renderer oracle"
        ),
        "item_id": item_id,
        "operation_id": operation_id,
        "constant_cases": {
            "hebrew": hebrew,
            "mixed_hebrew": mixed_hebrew,
            "arabic": arabic,
            "mixed_arabic": mixed_arabic,
        },
        "direct_keyframes": direct_signature,
        "undo_revision": undone["history"]["revision"],
        "direct_previews": direct_previews,
        "direct_gizmos": direct_gizmos,
        "instance_id": instance_id,
        "definition_id": definition["id"],
        "amount_parameter_id": amount["id"],
        "target_parameter_id": target["id"],
        "converted_previews": converted_previews,
        "converted_gizmos": converted_gizmos,
        "saved": {**file_evidence, "revision": saved["history"]["revision"]},
        "initial_close": initial_close,
        "fresh_process": fresh,
        "capture": fresh["capture"],
        "actions": client.evidence,
    }


if __name__ == "__main__":
    raise SystemExit(
        run_suite_main(
            "qa-text-tracking-bidi-e2e",
            run_suite,
            "target/qa-text-tracking-bidi-e2e-evidence.json",
        )
    )
