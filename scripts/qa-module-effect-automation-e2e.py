#!/usr/bin/env python3
"""Author one Module Effect input through Node, Inspector, Timeline, and Curve."""

import os
import pathlib

from qa_automation_support import (
    assert_module_instance_unchanged,
    history_shortcut,
    require_exact_item_times,
    sample_pixel_hashes,
    seek_rendered,
    wait_item_automation_surfaces,
)
from qa_curve_support import exercise_curve_key_live_preview
from qa_node_module_support import (
    active_definition,
    insert_image_opacity_in_primary_route,
    place_node_for_inline_edit,
)
from qa_property_gesture_support import (
    begin_reserved_keyframe_scrub,
    release_property_scrub,
)
from qa_support import (
    AUTHORING_FIXTURE,
    QaClient,
    QaFailure,
    activate_dock_tab,
    bring_timeline_component,
    capture_viewport,
    close_clean_native_app,
    component_in_inspector,
    free_port,
    item_by_name,
    media_seconds,
    request_clean_native_close,
    run_suite_main,
    save_project_to_disk,
    settled_preview_state,
    spawned_authoring_app,
)


TIMELINE_TAB = "dock.tab:timeline"
NODE_TAB = "dock.tab:node_editor"
INSPECTOR_TAB = "dock.tab:inspector"


def _module_attachment(project, item_id):
    matches = [
        attachment
        for attachment in project["attachments"].values()
        if (attachment.get("owner") or {}).get("item_id") == item_id
        and (attachment.get("processor") or {}).get("kind") == "module"
    ]
    if len(matches) != 1:
        raise QaFailure(
            "expected one Module Effect on item {}, got {}".format(
                item_id, len(matches)
            )
        )
    return matches[0]


def _invocation(attachment):
    return attachment["processor"]["value"]


def _keys(project, attachment_id, parameter_id):
    attachment = project["attachments"][attachment_id]
    track = _invocation(attachment).get("automation_tracks", {}).get(parameter_id)
    return [] if track is None else track["keyframes"]


def _create_custom_effect(client, item_id):
    activate_dock_tab(client, INSPECTOR_TAB, "Inspector", "Custom Effect creation")
    bring_timeline_component(client, "timeline.item:" + item_id, -120.0)
    client.click_component("timeline.item:" + item_id)
    add_id = "inspector.effects.add:item_post_transform"
    component_in_inspector(client, add_id)
    before = client.state()
    before_ids = set(before["project"]["attachments"])
    client.click_component(add_id)
    client.wait_component_settled("inspector.effect.add_search")
    client.click_component("inspector.effect.add_search")
    client.inject("text", {"text": "New Custom Effect"})
    # Search and Enter are separate native frames. This read is only a frame barrier.
    client.state()
    client.key("enter", True)
    client.key("enter", False)

    def created():
        state = client.state()
        added = set(state["project"]["attachments"]) - before_ids
        if (
            len(added) != 1
            or state["history"]["revision"] != before["history"]["revision"] + 1
        ):
            return None
        attachment = state["project"]["attachments"][next(iter(added))]
        document = state["editor"]["node_editor"].get("document") or {}
        if (
            (attachment.get("owner") or {}).get("item_id") == item_id
            and attachment.get("stage") == "item_post_transform"
            and (attachment.get("processor") or {}).get("kind") == "module"
            and document.get("host") == "attachment"
            and document.get("instance_id") == _invocation(attachment)["instance_id"]
        ):
            return state, attachment
        return None

    return client.wait_until("private Custom Effect attachment", created)


def _duplicate_item(client, item_id, before):
    activate_dock_tab(client, TIMELINE_TAB, "Timeline", "Module Effect sibling")
    bring_timeline_component(client, "timeline.item:" + item_id, -120.0)
    client.click_component("timeline.item:" + item_id, button="secondary")
    duplicate_id = "timeline.item.duplicate:" + item_id
    client.wait_component_settled(duplicate_id)
    client.click_component(duplicate_id)

    def duplicated():
        state = client.state()
        added = set(state["project"]["items"]) - set(before["project"]["items"])
        if (
            len(added) != 1
            or state["history"]["revision"] != before["history"]["revision"] + 1
        ):
            return None
        sibling_id = next(iter(added))
        try:
            sibling_attachment = _module_attachment(state["project"], sibling_id)
        except QaFailure:
            return None
        return state, sibling_id, sibling_attachment

    return client.wait_until("duplicated clip and Module Effect", duplicated)


def _assert_sibling_unchanged(project, before, item_id, attachment_id):
    if project["items"][item_id] != before["items"][item_id]:
        raise QaFailure("Module Effect animation changed sibling Timeline item")
    if project["attachments"][attachment_id] != before["attachments"][attachment_id]:
        raise QaFailure("Module Effect animation changed sibling attachment")
    instance_id = _invocation(before["attachments"][attachment_id])["instance_id"]
    assert_module_instance_unchanged(
        project, before, instance_id, "Module Effect sibling"
    )


def _parameter(definition, node_id):
    matches = [
        parameter
        for parameter in definition["interface"]["parameters"]
        if parameter["target"] == {"node_id": node_id, "port": "property:opacity"}
    ]
    if len(matches) != 1:
        raise QaFailure("Image Opacity did not publish exactly one opacity input")
    return matches[0]


def _assert_node_control_metadata(
    component, target, attachment_id, parameter_id, local_time
):
    metadata = component.get("metadata") or {}
    if (
        metadata.get("target") != target
        or metadata.get("attachment_id") != attachment_id
        or metadata.get("parameter_id") != parameter_id
        or abs(float(metadata.get("current_time", -1.0)) - local_time) > 0.001
    ):
        raise QaFailure("Module Effect control lost owner, target, or local clock")


def _assert_inspector_mode(component, expected_id, local_time, keyframe_count):
    metadata = component.get("metadata") or {}
    if (
        component.get("id") != expected_id
        or metadata.get("mode") != "keyframe"
        or metadata.get("keyframe_count") != keyframe_count
        or abs(float(metadata.get("current_time", -1.0)) - local_time) > 0.001
    ):
        raise QaFailure(
            "Module Effect Inspector lost its instance-qualified mode or local clock"
        )


def _fresh_process(
    project_file,
    expected_project,
    item_id,
    attachment_id,
    node_id,
    parameter_id,
    keys,
    times,
    previews,
    timeout,
):
    port_number = free_port()
    artifact_dir = pathlib.Path(os.environ["RUVIE_QA_ARTIFACT_DIR"])
    environment = {
        "RUVIE_QA_FIXTURE": AUTHORING_FIXTURE,
        "RUVIE_QA_PROJECT_PATH": str(project_file),
        "RUVIE_QA_OPEN_EXISTING_PROJECT": "1",
        "RUVIE_QA_PORT_FILE": None,
        "RUVIE_QA_RUN_ID": os.environ.get("RUVIE_QA_RUN_ID", "module-effect")
        + ":reload",
    }
    with spawned_authoring_app(port_number, environment) as process:
        client = QaClient("http://127.0.0.1:{}".format(port_number), timeout)
        client.wait_health()
        if client.state()["project"] != expected_project:
            raise QaFailure("fresh process changed Module Effect automation")
        if sample_pixel_hashes(client, times, "reloaded Module Effect") != previews:
            raise QaFailure("fresh process changed Module Effect multi-time pixels")
        seek_rendered(client, times[1], "reloaded Module Effect control")
        activate_dock_tab(client, TIMELINE_TAB, "Timeline", "reloaded Module Effect")
        bring_timeline_component(client, "timeline.item:" + item_id, -120.0)
        client.click_component("timeline.item:" + item_id)
        activate_dock_tab(
            client, INSPECTOR_TAB, "Inspector", "reloaded Module Effect Inspector"
        )
        component_in_inspector(client, "inspector.effect_node:" + attachment_id)
        client.click_component("inspector.effect_node:" + attachment_id)
        activate_dock_tab(
            client, NODE_TAB, "Node Editor", "reloaded Module Effect Node"
        )
        client.wait_component_settled("node_editor.canvas")
        control_id = "node_editor.property.node:{}:opacity".format(node_id)
        place_node_for_inline_edit(client, node_id, control_id)
        _, control = client.wait_component_settled(control_id)
        target = {
            "kind": "attachment_module_parameter",
            "attachment_id": attachment_id,
            "id": parameter_id,
        }
        _assert_node_control_metadata(control, target, attachment_id, parameter_id, 2.0)
        surfaces = wait_item_automation_surfaces(
            client,
            item_id,
            target,
            [key["id"] for key in keys],
            "Reloaded Module Effect",
        )
        capture = capture_viewport(client, artifact_dir / "capture.png")
        close = close_clean_native_app(
            client, process, "reloaded Module Effect", timeout
        )
    return {
        "surfaces": surfaces,
        "capture": capture,
        "close": close,
        "actions": client.evidence,
    }


def run_suite(client):
    client.wait_health()
    initial = client.state()
    item = item_by_name(initial["project"], "QA Text")
    item_id = item["id"]
    start = media_seconds(item["interval"]["start"])
    duration = media_seconds(item["interval"]["duration"])
    _, attachment = _create_custom_effect(client, item_id)
    attachment_id = attachment["id"]
    instance_id = _invocation(attachment)["instance_id"]
    node_id = insert_image_opacity_in_primary_route(client, "attachment")

    before_duplicate = client.state()
    _, sibling_id, sibling_attachment = _duplicate_item(
        client, item_id, before_duplicate
    )
    sibling_attachment_id = sibling_attachment["id"]
    sibling_times = [start + duration + 0.5, start + duration + 2.0]
    sibling_pixels = sample_pixel_hashes(client, sibling_times, "Module Effect sibling")

    original_times = [start + 0.5, start + 2.0]
    seek_rendered(client, original_times[0], "Module Effect")
    bring_timeline_component(client, "timeline.item:" + item_id, -120.0)
    client.click_component("timeline.item:" + item_id)
    activate_dock_tab(client, INSPECTOR_TAB, "Inspector", "Module Effect selection")
    component_in_inspector(client, "inspector.effect_node:" + attachment_id)
    client.click_component("inspector.effect_node:" + attachment_id)
    activate_dock_tab(client, NODE_TAB, "Node Editor", "Module Effect Node clock")
    control_id = "node_editor.property.node:{}:opacity".format(node_id)
    mode_id = "node_editor.property_mode.node:{}:opacity".format(node_id)
    place_node_for_inline_edit(client, node_id, control_id)
    before_publish = client.state()
    old_definition_id, _ = active_definition(before_publish, "attachment")
    _, mode = client.wait_component_settled(mode_id)
    if not mode.get("enabled") or not (mode.get("metadata") or {}).get(
        "allow_keyframe"
    ):
        raise QaFailure("Module Effect Node clock cannot create Timeline automation")
    client.click_component(mode_id)
    client.wait_component_settled(mode_id + ".option:keyframe")
    client.click_component(mode_id + ".option:keyframe")

    def first_key_created():
        state = client.state()
        definition_id, definition = active_definition(state, "attachment")
        matches = [
            parameter
            for parameter in definition["interface"]["parameters"]
            if parameter["target"]["node_id"] == node_id
        ]
        if len(matches) != 1:
            return None
        keys = _keys(state["project"], attachment_id, matches[0]["id"])
        return (
            (state, definition_id, definition, matches[0], keys[0])
            if len(keys) == 1
            and state["history"]["revision"]
            == before_publish["history"]["revision"] + 1
            else None
        )

    published, definition_id, definition, parameter, first_key = client.wait_until(
        "atomic Module Effect publication and first key", first_key_created
    )
    parameter = _parameter(definition, node_id)
    parameter_id = parameter["id"]
    if (
        definition_id == old_definition_id
        or abs(media_seconds(first_key["time"]) - 0.5) > 0.001
    ):
        raise QaFailure(
            "Module Effect first key did not use private COW and local time"
        )
    _assert_sibling_unchanged(
        published["project"],
        before_publish["project"],
        sibling_id,
        sibling_attachment_id,
    )
    target = {
        "kind": "attachment_module_parameter",
        "attachment_id": attachment_id,
        "id": parameter_id,
    }
    place_node_for_inline_edit(client, node_id, control_id)
    _, node_control = client.wait_component_settled(control_id)
    _assert_node_control_metadata(
        node_control, target, attachment_id, parameter_id, 0.5
    )
    history_shortcut(client)
    client.wait_until(
        "Module Effect publication Undo",
        lambda: client.state()["project"] == before_publish["project"],
    )
    history_shortcut(client, redo=True)
    client.wait_until(
        "Module Effect publication Redo",
        lambda: client.state()["project"] == published["project"],
    )

    baseline = seek_rendered(client, original_times[1], "Module Effect")
    activate_dock_tab(
        client, INSPECTOR_TAB, "Inspector", "Module Effect Inspector scrub"
    )
    bring_timeline_component(client, "timeline.item:" + item_id, -120.0)
    client.click_component("timeline.item:" + item_id)
    inspector_control = "inspector.property:module_instance:{}:{}".format(
        instance_id, parameter_id
    )
    inspector_mode = "inspector.property_mode:module_instance:{}:{}".format(
        instance_id, parameter_id
    )
    inspector_row = "inspector.property_row:module_instance:{}:{}".format(
        instance_id, parameter_id
    )
    _, inspector_value = component_in_inspector(client, inspector_control)
    _, inspector_mode_component = component_in_inspector(client, inspector_mode)
    if inspector_value.get("id") != inspector_control:
        raise QaFailure(
            "Module Effect Inspector value lost its instance-qualified identity"
        )
    _assert_inspector_mode(inspector_mode_component, inspector_mode, 2.0, 1)
    reservation = begin_reserved_keyframe_scrub(
        client,
        inspector_control,
        inspector_row,
        -36.0,
        "Module Effect Inspector key insertion",
    )
    frame = baseline["editor"]["timeline"]["current_frame"]
    held = client.wait_until(
        "Module Effect held Inspector Preview",
        lambda: (
            state
            if (
                state := settled_preview_state(
                    client, baseline["history"]["revision"], frame
                )
            )
            and state["project"] == baseline["project"]
            and state["history"] == baseline["history"]
            and state["editor"]["preview"]["pixel_hash"]
            != baseline["editor"]["preview"]["pixel_hash"]
            else None
        ),
        30.0,
    )
    artifact_dir = pathlib.Path(os.environ["RUVIE_QA_ARTIFACT_DIR"])
    held_capture = capture_viewport(client, artifact_dir / "module-effect-held.png")
    release_property_scrub(client, reservation)
    committed = client.wait_until(
        "one Module Effect Inspector command",
        lambda: (
            state
            if (state := client.state())["history"]["revision"]
            == baseline["history"]["revision"] + 1
            and len(_keys(state["project"], attachment_id, parameter_id)) == 2
            else None
        ),
    )
    keys = sorted(
        _keys(committed["project"], attachment_id, parameter_id),
        key=lambda key: media_seconds(key["time"]),
    )
    if (
        keys[1]["id"] != reservation["id"]
        or abs(media_seconds(keys[1]["time"]) - 2.0) > 0.001
    ):
        raise QaFailure("Inspector release lost Module Effect KeyframeId/local time")
    if (
        committed["project"]["module_definitions"]
        != baseline["project"]["module_definitions"]
    ):
        raise QaFailure("Module Effect instance animation changed Node topology")
    _assert_sibling_unchanged(
        committed["project"],
        before_publish["project"],
        sibling_id,
        sibling_attachment_id,
    )
    committed_render = client.wait_until(
        "Module Effect committed pixels equal held pixels",
        lambda: (
            state
            if (
                state := settled_preview_state(
                    client, committed["history"]["revision"], frame
                )
            )
            and state["editor"]["preview"]["pixel_hash"]
            == held["editor"]["preview"]["pixel_hash"]
            else None
        ),
        30.0,
    )
    history_shortcut(client)
    client.wait_until(
        "Module Effect Inspector Undo",
        lambda: client.state()["project"] == baseline["project"],
    )
    history_shortcut(client, redo=True)
    client.wait_until(
        "Module Effect Inspector Redo",
        lambda: client.state()["project"] == committed["project"],
    )

    surfaces = wait_item_automation_surfaces(
        client, item_id, target, [key["id"] for key in keys], "Module Effect"
    )
    require_exact_item_times(surfaces, [0.5, 2.0], original_times)
    curve_baseline = seek_rendered(client, original_times[1], "Module Effect Curve")
    activate_dock_tab(
        client, "dock.tab:curve_editor", "Curve Editor", "Module Effect Curve"
    )
    curve = exercise_curve_key_live_preview(
        client,
        surfaces["curve_keys"][1]["id"],
        curve_baseline,
        "Module Effect Curve drag",
        delta_x=14.0,
        delta_y=18.0,
    )
    curve_capture = capture_viewport(client, artifact_dir / "module-effect-curve.png")
    _assert_sibling_unchanged(
        client.state()["project"],
        before_publish["project"],
        sibling_id,
        sibling_attachment_id,
    )
    if (
        sample_pixel_hashes(client, sibling_times, "Module Effect sibling")
        != sibling_pixels
    ):
        raise QaFailure("Module Effect automation changed sibling Preview pixels")

    preview_times = original_times + sibling_times
    previews = sample_pixel_hashes(client, preview_times, "Module Effect")
    project_file = pathlib.Path(os.environ["RUVIE_QA_PROJECT_PATH"])
    saved, file_evidence = save_project_to_disk(
        client, project_file, "Module Effect animation"
    )
    initial_close = request_clean_native_close(
        client, "Module Effect animation", client.timeout
    )
    fresh = _fresh_process(
        project_file,
        saved["project"],
        item_id,
        attachment_id,
        node_id,
        parameter_id,
        keys,
        preview_times,
        previews,
        client.timeout,
    )
    return {
        "suite": "module-effect-automation",
        "item_id": item_id,
        "sibling_id": sibling_id,
        "attachment_id": attachment_id,
        "sibling_attachment_id": sibling_attachment_id,
        "instance_id": instance_id,
        "definition_id": definition_id,
        "node_id": node_id,
        "parameter_id": parameter_id,
        "keys": keys,
        "surfaces": surfaces,
        "curve": curve,
        "held_capture": held_capture,
        "curve_capture": curve_capture,
        "held_pixel_hash": held["editor"]["preview"]["pixel_hash"],
        "committed_pixel_hash": committed_render["editor"]["preview"]["pixel_hash"],
        "previews": previews,
        "saved": file_evidence,
        "initial_close": initial_close,
        "fresh_process": fresh,
        "capture": fresh["capture"],
        "actions": client.evidence,
    }


if __name__ == "__main__":
    raise SystemExit(
        run_suite_main(
            "qa-module-effect-automation-e2e",
            run_suite,
            "target/qa-module-effect-automation-evidence.json",
        )
    )
