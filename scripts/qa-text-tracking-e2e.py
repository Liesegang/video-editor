#!/usr/bin/env python3
"""Exercise Tracking across direct Text, automation, promotion, and persistence."""

import os
import pathlib

from qa_curve_support import exercise_curve_key_live_preview
from qa_tracking_geometry import (
    assert_expanded_from_neutral,
    assert_gizmo_parity,
    exercise_moved_letter_selection,
    observe_gizmos,
)
from qa_support import (
    AUTHORING_FIXTURE,
    QaClient,
    QaFailure,
    activate_dock_tab,
    bring_timeline_component,
    capture_viewport,
    close_clean_native_app,
    component_point,
    convert_timeline_item_to_node_clip,
    free_port,
    item_by_name,
    media_seconds,
    rendered_current_revision,
    request_clean_native_close,
    run_suite_main,
    save_project_to_disk,
    spawned_authoring_app,
)
from qa_text_ensemble_support import (
    component_in_inspector,
    constant_number,
    constant_value,
    open_and_choose,
    operation,
    seek_rendered,
    text_operations,
)


TIMELINE_TAB = "dock.tab:timeline"
CURVE_TAB = "dock.tab:curve_editor"
INSPECTOR_TAB = "dock.tab:inspector"
OPEN_EXISTING_PROJECT_ENV = "RUVIE_QA_OPEN_EXISTING_PROJECT"


def _artifact_dir():
    value = pathlib.Path(
        os.environ.get(
            "RUVIE_QA_ARTIFACT_DIR",
            pathlib.Path("target") / "qa-text-tracking-e2e",
        )
    ).resolve()
    value.mkdir(parents=True, exist_ok=True)
    return value


def _property(operation_value, key):
    try:
        return operation_value["properties"][key]
    except KeyError as error:
        raise QaFailure("Tracking omitted {!r}".format(key)) from error


def _property_keys(operation_value, key):
    prop = _property(operation_value, key)
    if prop.get("type") != "keyframe":
        raise QaFailure("Tracking {} is not keyframed".format(key))
    return prop["properties"]["keyframes"]


def _maybe_property_keys(operation_value, key):
    prop = _property(operation_value, key)
    return prop["properties"]["keyframes"] if prop.get("type") == "keyframe" else None


def _key_time_seconds(value):
    return media_seconds(value) if isinstance(value, dict) else float(value)


def _text_content(state, item_id):
    source = state["project"]["items"][item_id]["source"]
    if source.get("kind") != "text":
        raise QaFailure("Tracking source is no longer direct Text")
    return source["value"]["text"]


def _set_multiline_text(client, item_id, content):
    control_id = "inspector.property:item:{}:text".format(item_id)
    component_in_inspector(client, control_id)
    before = client.state()
    client.click_component(control_id)
    client.key("a", True, command=True)
    client.key("a", False, command=True)
    client.inject("text", {"text": content})
    client.key("enter", True, command=True)
    client.key("enter", False, command=True)
    return client.wait_until(
        "multiline Tracking Text commit",
        lambda: state
        if (state := client.state())["history"]["revision"]
        == before["history"]["revision"] + 1
        and _text_content(state, item_id) == content
        else None,
    )


def _press_horizontal_scrub(client, component_id, delta_x):
    _, component = component_in_inspector(client, component_id)
    start = component_point(component, 0.5, 0.5)
    end = {"x": start["x"] + delta_x, "y": start["y"]}
    client.inject(
        "press", {**start, "button": "primary", "coordinate_space": "points"}
    )
    client.inject(
        "move",
        {**end, "button": "primary", "coordinate_space": "points"},
    )
    return end


def _release_scrub(client, end):
    client.inject(
        "release", {**end, "button": "primary", "coordinate_space": "points"}
    )


def _undo(client):
    client.key("z", True, command=True)
    client.key("z", False, command=True)


def _select_dropdown(client, control_id, value, value_reader, description):
    component_in_inspector(client, control_id)
    before = client.state()
    client.click_component(control_id)
    option_id = control_id + ".option:" + value
    client.wait_component_settled(option_id)
    client.click_component(option_id)
    state = client.wait_until(
        description,
        lambda: state
        if (state := client.state())["history"]["revision"]
        == before["history"]["revision"] + 1
        and value_reader(state) == value
        else None,
    )
    return state, option_id


def _settled_changed_preview(client, revision, frame, previous_hash, description):
    return client.wait_until(
        description,
        lambda: state
        if (state := rendered_current_revision(client, previous_hash))
        and state["history"]["revision"] == revision
        and state["editor"]["preview"]["rendered_frame"] == frame
        else None,
        30.0,
    )


def _live_direct_scrub(client, item_id, operation_id, control_id, baseline):
    before_project = baseline["project"]
    before_revision = baseline["history"]["revision"]
    before_hash = baseline["editor"]["preview"]["pixel_hash"]
    frame = baseline["editor"]["timeline"]["current_frame"]
    before_amount = constant_number(operation(baseline, item_id, operation_id), "amount")
    end = _press_horizontal_scrub(client, control_id, 72.0)
    held = client.wait_until(
        "live direct Tracking scrub Preview",
        lambda: state
        if (state := rendered_current_revision(client, before_hash))
        and state["history"]["revision"] == before_revision
        and state["project"] == before_project
        and state["editor"]["preview"]["rendered_frame"] == frame
        else None,
        30.0,
    )
    _release_scrub(client, end)
    committed = client.wait_until(
        "one direct Tracking scrub command",
        lambda: state
        if (state := client.state())["history"]["revision"] == before_revision + 1
        and constant_number(operation(state, item_id, operation_id), "amount")
        != before_amount
        else None,
    )
    rendered = _settled_changed_preview(
        client,
        committed["history"]["revision"],
        frame,
        before_hash,
        "committed direct Tracking Preview",
    )
    _undo(client)
    undone = client.wait_until(
        "direct Tracking scrub Undo",
        lambda: state
        if (state := client.state())["project"] == before_project
        and state["history"]["revision"] == committed["history"]["revision"] + 1
        else None,
    )
    restored = client.wait_until(
        "direct Tracking Preview after Undo",
        lambda: state
        if (state := rendered_current_revision(client))
        and state["history"]["revision"] == undone["history"]["revision"]
        and state["editor"]["preview"]["pixel_hash"] == before_hash
        else None,
        30.0,
    )
    return {
        "held_hash": held["editor"]["preview"]["pixel_hash"],
        "committed_hash": rendered["editor"]["preview"]["pixel_hash"],
        "restored": restored,
    }


def _find_component(client, component_type, predicate):
    client.state()
    return next(
        (
            component
            for component in client.component_snapshot()["components"]
            if component.get("type") == component_type
            and component.get("visible")
            and predicate(component.get("metadata") or {})
        ),
        None,
    )


def _wait_automation_surfaces(client, item_id, target, keyframe_ids):
    activate_dock_tab(client, TIMELINE_TAB, "Timeline", "Tracking Dope Sheet")
    bring_timeline_component(client, "timeline.item:" + item_id, -120.0)
    expand_id = "timeline.item_expand:" + item_id
    client.wait_component_settled(expand_id)
    state = client.state()
    if item_id not in state["editor"]["timeline"]["expanded_items"]:
        client.click_component(expand_id)
    lane = client.wait_until(
        "Tracking Timeline lane",
        lambda: _find_component(
            client,
            "timeline_property_label",
            lambda metadata: metadata.get("item_id") == item_id
            and metadata.get("target") == target,
        ),
    )
    timeline_keys = [
        client.wait_until(
            "Tracking Timeline key {}".format(keyframe_id),
            lambda keyframe_id=keyframe_id: _find_component(
                client,
                "timeline_property_keyframe",
                lambda metadata: metadata.get("item_id") == item_id
                and metadata.get("target") == target
                and metadata.get("keyframe_id") == keyframe_id,
            ),
        )
        for keyframe_id in keyframe_ids
    ]
    activate_dock_tab(client, CURVE_TAB, "Curve Editor", "Tracking Curve Editor")
    client.click_component("curve_editor.fit")
    curve_keys = [
        client.wait_until(
            "Tracking Curve key {}".format(keyframe_id),
            lambda keyframe_id=keyframe_id: _find_component(
                client,
                "curve_editor_keyframe",
                lambda metadata: metadata.get("item_id") == item_id
                and metadata.get("target") == target
                and metadata.get("component") == "value"
                and metadata.get("keyframe_id") == keyframe_id,
            ),
        )
        for keyframe_id in keyframe_ids
    ]
    return {
        "lane": lane.get("metadata"),
        "timeline_keys": [
            {"id": key["id"], "metadata": key.get("metadata")}
            for key in timeline_keys
        ],
        "curve_keys": [
            {"id": key["id"], "metadata": key.get("metadata")}
            for key in curve_keys
        ],
    }


def _drag_direct_curve_key_and_undo(
    client, item_id, operation_id, keyframe_id, component_id
):
    before = client.state()
    before_keys = _property_keys(operation(before, item_id, operation_id), "amount")
    before_key = next(key for key in before_keys if key["id"] == keyframe_id)
    before_time = _key_time_seconds(before_key["time"])
    client.drag_component_by(component_id, 24.0, 0.0, steps=10)

    def retimed():
        state = client.state()
        keys = _property_keys(operation(state, item_id, operation_id), "amount")
        key = next((value for value in keys if value["id"] == keyframe_id), None)
        if (
            key is not None
            and _key_time_seconds(key["time"]) != before_time
            and state["history"]["revision"] == before["history"]["revision"] + 1
            and not state["editor"]["curve_editor"]["drag_active"]
        ):
            return state, key
        return None

    dragged, moved_key = client.wait_until("Tracking Curve key retime", retimed)
    _undo(client)
    restored = client.wait_until(
        "Tracking Curve key retime Undo",
        lambda: state
        if (state := client.state())["project"] == before["project"]
        and state["history"]["revision"] == dragged["history"]["revision"] + 1
        and not state["editor"]["curve_editor"]["drag_active"]
        else None,
    )
    return {
        "keyframe_id": keyframe_id,
        "before_time": before_time,
        "moved_time": _key_time_seconds(moved_key["time"]),
        "restored": restored,
    }


def _module_tracking(project, item_id, operation_id):
    source = project["items"][item_id]["source"]
    if source.get("kind") != "module":
        raise QaFailure("converted Tracking item is not a Node Clip")
    instance_id = source["value"]["instance_id"]
    instance = project["module_instances"][instance_id]
    definition = project["module_definitions"][instance["definition_id"]]
    node = definition["graph"]["nodes"].get(operation_id)
    content = (node or {}).get("content") or {}
    data = content.get("data") or {}
    if data.get("component_id") != "tracking":
        raise QaFailure("conversion did not retain Tracking as its operation Node")

    def parameter(port):
        matches = [
            candidate
            for candidate in definition["interface"]["parameters"]
            if candidate["target"]
            == {"node_id": operation_id, "port": "property:" + port}
        ]
        if len(matches) != 1:
            raise QaFailure("converted Tracking expected one published {}".format(port))
        return matches[0]

    return source, instance_id, definition, parameter("amount"), parameter("target")


def _keyframe_signature(keyframes):
    return [
        {
            "id": keyframe["id"],
            "time": _key_time_seconds(keyframe["time"]),
            "value": keyframe["value"],
            "easing": keyframe["easing"],
        }
        for keyframe in keyframes
    ]


def _observe_times(client, seconds, revision):
    observed = []
    for value in seconds:
        rendered = seek_rendered(client, value)
        if rendered["history"]["revision"] != revision:
            raise QaFailure("Preview observation changed Project revision")
        observed.append(
            {
                "seconds": value,
                "frame": rendered["editor"]["preview"]["rendered_frame"],
                "pixel_hash": rendered["editor"]["preview"]["pixel_hash"],
                "nontransparent_pixels": rendered["editor"]["preview"][
                    "nontransparent_pixels"
                ],
            }
        )
    return observed


def _fresh_process_parity(
    project_file,
    expected_project,
    item_id,
    operation_id,
    key_signature,
    expected_previews,
    neutral_gizmos,
    expected_gizmos,
    timeout,
):
    port = free_port()
    run_id = os.environ.get("RUVIE_QA_RUN_ID", "text-tracking")
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
            raise QaFailure("fresh process loaded a different Tracking Project")
        source, instance_id, _, amount, target = _module_tracking(
            loaded["project"], item_id, operation_id
        )
        track = source["value"]["automation_tracks"].get(amount["id"])
        if track is None or _keyframe_signature(track["keyframes"]) != key_signature:
            raise QaFailure("fresh process changed Tracking amount automation")
        if target["id"] not in loaded["project"]["module_instances"][instance_id].get(
            "parameter_overrides", {}
        ):
            raise QaFailure("fresh process lost the Tracking Target override")
        previews = _observe_times(
            client,
            [preview["seconds"] for preview in expected_previews],
            loaded["history"]["revision"],
        )
        if previews != expected_previews:
            raise QaFailure("fresh process changed Tracking Preview pixels")
        bring_timeline_component(client, "timeline.item:" + item_id, -120.0)
        client.click_component("timeline.item:" + item_id)
        component_in_inspector(
            client,
            "inspector.property:module_instance:{}:{}".format(
                instance_id, amount["id"]
            ),
        )
        client.wait_until(
            "reloaded Tracking selection",
            lambda: state
            if (state := client.state())["editor"]["selection"]["primary"]
            == {"kind": "timeline_item", "id": item_id}
            and state["history"]["revision"] == loaded["history"]["revision"]
            else None,
        )
        reloaded_gizmos = observe_gizmos(
            client,
            item_id,
            [sample["seconds"] for sample in expected_gizmos],
            loaded["history"]["revision"],
            "reloaded Tracking Gizmo",
        )
        assert_gizmo_parity(
            expected_gizmos, reloaded_gizmos, "reloaded Tracking Gizmo"
        )
        reloaded_selection = exercise_moved_letter_selection(
            client,
            item_id,
            neutral_gizmos[-1],
            reloaded_gizmos[-1],
            "reloaded Tracking",
        )
        capture = capture_viewport(client, _artifact_dir() / "capture.png")
        close = close_clean_native_app(client, process, "reloaded Tracking app", timeout)
    return {
        "previews": previews,
        "gizmos": reloaded_gizmos,
        "moved_letter_selection": reloaded_selection,
        "capture": capture,
        "close": close,
        "actions": client.evidence,
    }


def run_suite(client):
    client.wait_health()
    initial = client.wait_until(
        "initial Tracking Preview", lambda: rendered_current_revision(client), 30.0
    )
    if initial["project"].get("name") != "Authoring QA":
        raise QaFailure("Tracking QA requires authoring_e2e")
    item = item_by_name(initial["project"], "QA Text")
    item_id = item["id"]
    client.click_component("timeline.item:" + item_id)
    authored = _set_multiline_text(client, item_id, "AB\nCD")
    neutral_gizmos = observe_gizmos(
        client,
        item_id,
        [1.5, 2.5],
        authored["history"]["revision"],
        "neutral Text Gizmo",
    )
    baseline = seek_rendered(client, 1.5)
    baseline_hash = baseline["editor"]["preview"]["pixel_hash"]

    open_and_choose(
        client,
        item_id,
        "tracking",
        "inspector.text_ensemble.add.effector:tracking",
    )
    added = client.wait_until(
        "Tracking addition",
        lambda: state
        if len(text_operations((state := client.state()), item_id)) == 1
        and text_operations(state, item_id)[0]["operation"]["component_id"]
        == "tracking"
        else None,
    )
    tracking = text_operations(added, item_id)[0]
    operation_id = tracking["id"]
    if not tracking.get("declared_ports") or set(tracking.get("properties", {})) != {
        "amount",
        "target",
    }:
        raise QaFailure("Tracking did not persist its descriptor contract")
    if constant_number(tracking, "amount") != 0.0:
        raise QaFailure("Tracking Amount default is not neutral")
    if constant_value(_property(tracking, "target"), "Tracking Target") != "Line":
        raise QaFailure("Tracking Target default must be Line")

    amount_control = "inspector.property:text_ensemble:{}:{}:amount".format(
        item_id, operation_id
    )
    target_control = "inspector.property:text_ensemble:{}:{}:target".format(
        item_id, operation_id
    )
    direct_live = _live_direct_scrub(
        client, item_id, operation_id, amount_control, seek_rendered(client, 1.5)
    )
    client.drag_component_by(amount_control, 72.0, 0.0, steps=12)
    line_state = client.wait_until(
        "authored Tracking Amount",
        lambda: state
        if (state := client.state())["history"]["revision"]
        == direct_live["restored"]["history"]["revision"] + 1
        and constant_number(operation(state, item_id, operation_id), "amount") != 0.0
        else None,
    )
    line_render = _settled_changed_preview(
        client,
        line_state["history"]["revision"],
        line_state["editor"]["timeline"]["current_frame"],
        baseline_hash,
        "Line Tracking Preview",
    )
    line_hash = line_render["editor"]["preview"]["pixel_hash"]

    target_value = lambda state: constant_value(
        _property(operation(state, item_id, operation_id), "target"), "Tracking Target"
    )
    block_state, block_option = _select_dropdown(
        client, target_control, "Block", target_value, "Tracking Target Block"
    )
    block_render = _settled_changed_preview(
        client,
        block_state["history"]["revision"],
        block_state["editor"]["timeline"]["current_frame"],
        line_hash,
        "Block Tracking Preview",
    )
    char_state, char_option = _select_dropdown(
        client, target_control, "Char", target_value, "Tracking Target Char"
    )
    char_render = client.wait_until(
        "neutral Char Tracking Preview",
        lambda: state
        if (state := rendered_current_revision(client))
        and state["history"]["revision"] == char_state["history"]["revision"]
        and state["editor"]["preview"]["pixel_hash"] == baseline_hash
        else None,
        30.0,
    )
    line_state, line_option = _select_dropdown(
        client, target_control, "Line", target_value, "Tracking Target Line"
    )
    line_restored = client.wait_until(
        "restored Line Tracking Preview",
        lambda: state
        if (state := rendered_current_revision(client))
        and state["history"]["revision"] == line_state["history"]["revision"]
        and state["editor"]["preview"]["pixel_hash"] == line_hash
        else None,
        30.0,
    )

    amount_mode = "inspector.property_mode:text_ensemble:{}:{}:amount".format(
        item_id, operation_id
    )
    component_in_inspector(client, amount_mode)
    client.click_component(amount_mode)
    client.wait_component_settled(amount_mode + ".option:keyframe")
    client.click_component(amount_mode + ".option:keyframe")
    first_key_state = client.wait_until(
        "Tracking Amount keyframe mode",
        lambda: state
        if (
            keys := _maybe_property_keys(
                operation((state := client.state()), item_id, operation_id), "amount"
            )
        )
        is not None
        and len(keys) == 1
        else None,
    )
    first_key = _property_keys(
        operation(first_key_state, item_id, operation_id), "amount"
    )[0]
    if abs(_key_time_seconds(first_key["time"]) - 0.5) > 0.001:
        raise QaFailure("first Tracking key did not use Clip-local time")

    second_seek = seek_rendered(client, 2.5)
    before_second = second_seek["project"]
    before_second_hash = second_seek["editor"]["preview"]["pixel_hash"]
    second_frame = second_seek["editor"]["timeline"]["current_frame"]
    end = _press_horizontal_scrub(client, amount_control, 48.0)
    held_key = client.wait_until(
        "live keyframed Tracking Preview",
        lambda: state
        if (state := rendered_current_revision(client, before_second_hash))
        and state["project"] == before_second
        and state["history"]["revision"] == second_seek["history"]["revision"]
        else None,
        30.0,
    )
    _release_scrub(client, end)
    second_key_state = client.wait_until(
        "second Tracking Amount key",
        lambda: state
        if (
            keys := _maybe_property_keys(
                operation((state := client.state()), item_id, operation_id), "amount"
            )
        )
        is not None
        and len(keys) == 2
        and state["history"]["revision"] == second_seek["history"]["revision"] + 1
        else None,
    )
    direct_keys = _property_keys(
        operation(second_key_state, item_id, operation_id), "amount"
    )
    direct_keys.sort(key=lambda key: _key_time_seconds(key["time"]))
    if abs(_key_time_seconds(direct_keys[1]["time"]) - 1.5) > 0.001:
        raise QaFailure("second Tracking key did not use Clip-local time")
    direct_target = {
        "kind": "authored_property",
        "owner": {
            "kind": "text_ensemble",
            "item_id": item_id,
            "operation_id": operation_id,
        },
        "key": "amount",
    }
    direct_surfaces = _wait_automation_surfaces(
        client, item_id, direct_target, [key["id"] for key in direct_keys]
    )
    direct_curve_drag = _drag_direct_curve_key_and_undo(
        client,
        item_id,
        operation_id,
        direct_keys[0]["id"],
        direct_surfaces["curve_keys"][0]["id"],
    )
    curve_capture = capture_viewport(client, _artifact_dir() / "tracking-curve.png")
    direct_previews = _observe_times(
        client,
        [1.5, 2.5],
        direct_curve_drag["restored"]["history"]["revision"],
    )
    direct_gizmos = observe_gizmos(
        client,
        item_id,
        [1.5, 2.5],
        direct_curve_drag["restored"]["history"]["revision"],
        "direct Tracking Gizmo",
    )
    assert_expanded_from_neutral(
        neutral_gizmos, direct_gizmos, "direct Tracking Gizmo"
    )
    direct_selection = exercise_moved_letter_selection(
        client,
        item_id,
        neutral_gizmos[-1],
        direct_gizmos[-1],
        "direct Tracking",
    )

    activate_dock_tab(client, TIMELINE_TAB, "Timeline", "Tracking promotion")
    bring_timeline_component(client, "timeline.item:" + item_id, -120.0)
    client.click_component("timeline.item:" + item_id)
    converted = convert_timeline_item_to_node_clip(
        client, item_id, direct_curve_drag["restored"]["history"]["revision"]
    )
    source, instance_id, definition, amount_parameter, target_parameter = _module_tracking(
        converted["project"], item_id, operation_id
    )
    converted_track = source["value"]["automation_tracks"].get(amount_parameter["id"])
    direct_signature = _keyframe_signature(direct_keys)
    if converted_track is None or _keyframe_signature(
        converted_track["keyframes"]
    ) != direct_signature:
        raise QaFailure("conversion changed Tracking key IDs/times/easing/values")
    overrides = converted["project"]["module_instances"][instance_id].get(
        "parameter_overrides", {}
    )
    if overrides.get(target_parameter["id"]) != "Line":
        raise QaFailure("conversion did not retain the Tracking Target override")
    converted_previews = _observe_times(
        client, [1.5, 2.5], converted["history"]["revision"]
    )
    if converted_previews != direct_previews:
        raise QaFailure("Tracking conversion changed multi-time Preview pixels")
    converted_gizmos = observe_gizmos(
        client,
        item_id,
        [1.5, 2.5],
        converted["history"]["revision"],
        "promoted Tracking Gizmo",
    )
    assert_gizmo_parity(direct_gizmos, converted_gizmos, "promoted Tracking Gizmo")
    promoted_selection = exercise_moved_letter_selection(
        client,
        item_id,
        neutral_gizmos[-1],
        converted_gizmos[-1],
        "promoted Tracking",
    )
    module_target = {"kind": "module_parameter", "id": amount_parameter["id"]}
    converted_surfaces = _wait_automation_surfaces(
        client, item_id, module_target, [key["id"] for key in direct_keys]
    )
    promoted_curve_baseline = client.wait_until(
        "promoted Tracking Curve Preview baseline",
        lambda: state
        if (state := rendered_current_revision(client))
        and state["history"]["revision"] == converted["history"]["revision"]
        and state["editor"]["timeline"]["current_frame"]
        == converted_previews[1]["frame"]
        else None,
        30.0,
    )
    promoted_curve_live = exercise_curve_key_live_preview(
        client,
        converted_surfaces["curve_keys"][1]["id"],
        promoted_curve_baseline,
        "promoted Tracking Curve drag",
        delta_y=18.0,
    )

    activate_dock_tab(client, TIMELINE_TAB, "Timeline", "Node Clip Tracking Inspector")
    client.click_component("timeline.item:" + item_id)
    module_control = "inspector.property:module_instance:{}:{}".format(
        instance_id, amount_parameter["id"]
    )
    component_in_inspector(client, module_control)
    module_baseline = seek_rendered(client, 2.5)
    module_project = module_baseline["project"]
    module_hash = module_baseline["editor"]["preview"]["pixel_hash"]
    module_source_before, _, _, module_amount_before, _ = _module_tracking(
        module_project, item_id, operation_id
    )
    module_keys_before = _keyframe_signature(
        module_source_before["value"]["automation_tracks"][
            module_amount_before["id"]
        ]["keyframes"]
    )
    end = _press_horizontal_scrub(client, module_control, 52.0)
    module_held = client.wait_until(
        "live promoted Tracking Preview",
        lambda: state
        if (state := rendered_current_revision(client, module_hash))
        and state["project"] == module_project
        and state["history"]["revision"] == module_baseline["history"]["revision"]
        else None,
        30.0,
    )
    _release_scrub(client, end)
    module_committed = client.wait_until(
        "one promoted Tracking scrub command",
        lambda: state
        if (state := client.state())["history"]["revision"]
        == module_baseline["history"]["revision"] + 1
        and _keyframe_signature(
            _module_tracking(state["project"], item_id, operation_id)[0]["value"][
                "automation_tracks"
            ][amount_parameter["id"]]["keyframes"]
        )
        != module_keys_before
        else None,
    )
    module_committed_render = client.wait_until(
        "committed promoted Tracking Preview",
        lambda: state
        if (state := rendered_current_revision(client))
        and state["history"]["revision"] == module_committed["history"]["revision"]
        and state["editor"]["preview"]["rendered_frame"]
        == module_baseline["editor"]["timeline"]["current_frame"]
        and state["editor"]["preview"]["pixel_hash"]
        == module_held["editor"]["preview"]["pixel_hash"]
        else None,
        30.0,
    )
    _undo(client)
    module_undone = client.wait_until(
        "promoted Tracking scrub Undo",
        lambda: state
        if (state := client.state())["project"] == module_project
        and state["history"]["revision"] == module_committed["history"]["revision"] + 1
        else None,
    )
    restored = client.wait_until(
        "promoted Tracking Preview after Undo",
        lambda: state
        if (state := rendered_current_revision(client))
        and state["history"]["revision"] == module_undone["history"]["revision"]
        and state["editor"]["preview"]["pixel_hash"] == module_hash
        else None,
        30.0,
    )

    project_file_value = os.environ.get("RUVIE_QA_PROJECT_PATH")
    if not project_file_value:
        raise QaFailure("Tracking persistence requires RUVIE_QA_PROJECT_PATH")
    project_file = pathlib.Path(project_file_value).resolve()
    expected_project = restored["project"]
    persisted_previews = _observe_times(
        client, [1.5, 2.5], restored["history"]["revision"]
    )
    saved, file_evidence = save_project_to_disk(
        client, project_file, "Tracking Node Clip"
    )
    initial_close = request_clean_native_close(
        client, "saved Tracking authoring app", client.timeout
    )
    fresh = _fresh_process_parity(
        project_file,
        expected_project,
        item_id,
        operation_id,
        direct_signature,
        persisted_previews,
        neutral_gizmos,
        converted_gizmos,
        client.timeout,
    )
    return {
        "suite": "text-tracking",
        "item_id": item_id,
        "operation_id": operation_id,
        "descriptor": tracking["operation"],
        "dropdown_options": [block_option, char_option, line_option],
        "baseline_hash": baseline_hash,
        "line_hash": line_hash,
        "block_hash": block_render["editor"]["preview"]["pixel_hash"],
        "char_hash": char_render["editor"]["preview"]["pixel_hash"],
        "direct_live": {
            key: value for key, value in direct_live.items() if key != "restored"
        },
        "direct_keyframe_live_hash": held_key["editor"]["preview"]["pixel_hash"],
        "direct_keyframes": direct_signature,
        "direct_curve_drag": {
            key: value
            for key, value in direct_curve_drag.items()
            if key != "restored"
        },
        "direct_surfaces": direct_surfaces,
        "neutral_gizmos": neutral_gizmos,
        "direct_gizmos": direct_gizmos,
        "direct_moved_letter_selection": direct_selection,
        "curve_capture": curve_capture,
        "instance_id": instance_id,
        "definition_id": definition["id"],
        "amount_parameter_id": amount_parameter["id"],
        "target_parameter_id": target_parameter["id"],
        "converted_surfaces": converted_surfaces,
        "converted_gizmos": converted_gizmos,
        "promoted_moved_letter_selection": promoted_selection,
        "promoted_curve_live": promoted_curve_live,
        "module_live_hash": module_held["editor"]["preview"]["pixel_hash"],
        "module_committed_hash": module_committed_render["editor"]["preview"][
            "pixel_hash"
        ],
        "persisted_previews": persisted_previews,
        "saved": {**file_evidence, "revision": saved["history"]["revision"]},
        "initial_close": initial_close,
        "fresh_process": fresh,
        "capture": fresh["capture"],
        "actions": client.evidence,
    }


if __name__ == "__main__":
    raise SystemExit(
        run_suite_main(
            "qa-text-tracking-e2e",
            run_suite,
            "target/qa-text-tracking-e2e-evidence.json",
        )
    )
