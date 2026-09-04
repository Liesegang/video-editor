#!/usr/bin/env python3
"""Assign a reusable Transition Module and its required clip input atomically."""

from qa_support import (
    QaFailure,
    activate_dock_tab,
    item_by_name,
    media_seconds,
    rendered_preview_state,
    run_suite_main,
    seek_timeline_seconds,
)


TIMELINE_TAB_ID = "dock.tab:timeline"
SOURCE_NAME = "QA Image"
INPUT_SOURCE_NAME = "QA Overlap"
DEFINITION_NAME = "QA Required Input Transition"


def _component(snapshot, component_id):
    return next(
        (entry for entry in snapshot["components"] if entry["id"] == component_id),
        None,
    )


def _definition(project):
    matches = [
        (definition_id, definition)
        for definition_id, definition in project["module_definitions"].items()
        if definition["name"] == DEFINITION_NAME
    ]
    if len(matches) != 1:
        raise QaFailure(
            "expected one {!r} definition, got {}".format(DEFINITION_NAME, len(matches))
        )
    return matches[0]


def _required_public_input(definition):
    contract = definition.get("host_contract") or {}
    if contract.get("kind") != "transition":
        raise QaFailure("required-input fixture is not a Transition Module")
    protected = contract.get("contract") or {}
    protected_ids = {protected.get("from_input_id"), protected.get("to_input_id")}
    matches = [
        media_input
        for media_input in definition["interface"]["media_inputs"]
        if media_input.get("required") and media_input.get("id") not in protected_ids
    ]
    if (
        len(matches) != 1
        or str(matches[0].get("data_type", "")).lower() != "image"
    ):
        raise QaFailure("fixture must expose one required public Image input")
    return matches[0]


def _duplicate_adjacent(client, source, initial):
    source_id = source["id"]
    source_end = media_seconds(source["interval"]["start"]) + media_seconds(
        source["interval"]["duration"]
    )
    initial_ids = set(initial["project"]["items"])
    revision = initial["history"]["revision"]
    client.click_component("timeline.item:" + source_id, button="secondary")
    client.click_component("timeline.item.duplicate:" + source_id)

    def duplicated():
        state = client.state()
        new_ids = set(state["project"]["items"]) - initial_ids
        if len(new_ids) != 1 or state["history"]["revision"] != revision + 1:
            return None
        item_id = next(iter(new_ids))
        item = state["project"]["items"][item_id]
        if (
            item["track_id"] == source["track_id"]
            and abs(media_seconds(item["interval"]["start"]) - source_end) < 1e-9
        ):
            return state, item_id
        return None

    return client.wait_until("adjacent duplicate clip", duplicated)


def _add_transition(client, source_id, target_id, before):
    initial_ids = set(before["project"].get("transitions", {}))
    revision = before["history"]["revision"]
    client.click_component("timeline.item:" + source_id, button="secondary")
    client.click_component("timeline.item.add_transition_menu:" + source_id)
    client.click_component(
        "timeline.item.add_transition:{}:{}:image".format(source_id, target_id)
    )

    def added():
        state = client.state()
        new_ids = set(state["project"].get("transitions", {})) - initial_ids
        if len(new_ids) != 1 or state["history"]["revision"] != revision + 1:
            return None
        transition_id = next(iter(new_ids))
        transition = state["project"]["transitions"][transition_id]
        implementation = transition["processor"]["implementation"]
        if (
            transition["from_item_id"] == source_id
            and transition["to_item_id"] == target_id
            and implementation.get("kind") == "operation"
        ):
            return state, transition_id
        return None

    return client.wait_until("built-in Timeline transition", added)


def _open_assignment(client, transition_id, definition_id, input_id):
    overlay_id = "timeline.transition:" + transition_id
    client.click_component(overlay_id, button="secondary")
    client.click_component("timeline.transition.processor_menu:" + transition_id)
    choice_id = "timeline.transition.assign_module:{}:{}".format(
        transition_id, definition_id
    )
    _, choice = client.wait_component(choice_id)
    choice_metadata = choice.get("metadata") or {}
    if (
        choice_metadata.get("assignable") is not True
        or choice_metadata.get("requires_input_assignment") is not True
        or choice_metadata.get("assignment_error") is not None
    ):
        raise QaFailure("required-input Transition template is not configurable")
    client.click_component(choice_id)

    dialog_id = "transition.assignment.dialog:{}:{}".format(
        transition_id, definition_id
    )
    _, dialog = client.wait_component_settled(dialog_id)
    metadata = dialog.get("metadata") or {}
    if (
        metadata.get("required_input_ids") != [input_id]
        or metadata.get("required_input_count") != 1
        or metadata.get("bound_input_count") != 0
        or metadata.get("apply_enabled") is not False
    ):
        raise QaFailure("assignment dialog does not expose its incomplete public contract")
    return dialog_id, choice_metadata


def _choose_input(
    client, transition_id, input_id, source_id, dialog_id, excluded_item_ids
):
    control_id = "transition.assignment.input:{}:{}".format(transition_id, input_id)
    _, picker = client.wait_component_settled(control_id)
    candidate_ids = set((picker.get("metadata") or {}).get("candidate_item_ids") or [])
    if source_id not in candidate_ids:
        raise QaFailure("full-coverage auxiliary clip is missing from the picker")
    if candidate_ids.intersection(excluded_item_ids):
        raise QaFailure("Transition A/B participants leaked into the auxiliary picker")
    _, selector = client.wait_component_settled(control_id + ".selector")
    selector_metadata = selector.get("metadata") or {}
    if selector_metadata.get("published_input_id") != input_id:
        raise QaFailure("media picker lost the Published input identity")
    client.click_component(control_id + ".selector")
    candidate_id = control_id + ".candidate:" + source_id
    _, candidate = client.wait_component(candidate_id)
    if (candidate.get("metadata") or {}).get("item_id") != source_id:
        raise QaFailure("media picker candidate has the wrong Timeline item identity")
    client.click_component(candidate_id)

    def ready():
        snapshot = client.component_snapshot()
        picker = _component(snapshot, control_id)
        dialog = _component(snapshot, dialog_id)
        if picker is None or dialog is None:
            return None
        picker_metadata = picker.get("metadata") or {}
        dialog_metadata = dialog.get("metadata") or {}
        if (
            picker_metadata.get("bound_item_id") == source_id
            and dialog_metadata.get("bound_input_count") == 1
            and dialog_metadata.get("apply_enabled") is True
        ):
            return snapshot, dialog_metadata
        return None

    return client.wait_until("complete Transition Module assignment", ready)


def _wait_dialog_closed(client, dialog_id, description):
    return client.wait_until(
        description,
        lambda: snapshot
        if _component((snapshot := client.component_snapshot()), dialog_id) is None
        else None,
    )


def run_suite(client):
    client.wait_health()
    activate_dock_tab(client, TIMELINE_TAB_ID, "Timeline", "Transition assignment")
    initial = client.state()
    source = item_by_name(initial["project"], SOURCE_NAME)
    input_source = item_by_name(initial["project"], INPUT_SOURCE_NAME)
    definition_id, definition = _definition(initial["project"])
    public_input = _required_public_input(definition)
    input_id = public_input["id"]

    duplicated, target_id = _duplicate_adjacent(client, source, initial)
    before_assignment, transition_id = _add_transition(
        client, source["id"], target_id, duplicated
    )
    overlay_id = "timeline.transition:" + transition_id
    client.wait_component_settled(overlay_id)
    items_before = before_assignment["project"]["items"]
    definitions_before = before_assignment["project"]["module_definitions"]
    instances_before = before_assignment["project"]["module_instances"]
    revision_before = before_assignment["history"]["revision"]

    # Draft changes and Cancel must leave both Project and history untouched.
    dialog_id, choice_metadata = _open_assignment(
        client, transition_id, definition_id, input_id
    )
    _choose_input(
        client,
        transition_id,
        input_id,
        input_source["id"],
        dialog_id,
        {source["id"], target_id},
    )
    client.click_component("transition.assignment.cancel:" + transition_id)
    _wait_dialog_closed(client, dialog_id, "cancelled assignment dialog")
    cancelled = client.state()
    if (
        cancelled["project"] != before_assignment["project"]
        or cancelled["history"]["revision"] != revision_before
    ):
        raise QaFailure("Cancel mutated the Project or Undo history")

    # Reopen, resolve the required Published input, then submit once.
    dialog_id, _ = _open_assignment(client, transition_id, definition_id, input_id)
    _, dialog_metadata = _choose_input(
        client,
        transition_id,
        input_id,
        input_source["id"],
        dialog_id,
        {source["id"], target_id},
    )
    client.click_component("transition.assignment.apply:" + transition_id)

    def applied():
        state = client.state()
        if state["history"]["revision"] != revision_before + 1:
            return None
        transition = state["project"]["transitions"][transition_id]
        implementation = transition["processor"]["implementation"]
        if implementation.get("kind") != "module":
            return None
        module = implementation.get("value") or {}
        instance = state["project"]["module_instances"].get(module.get("instance_id"))
        binding = (module.get("input_bindings") or {}).get(input_id)
        if (
            instance
            and instance.get("definition_id") == definition_id
            and binding
            and binding.get("item_id") == input_source["id"]
        ):
            return state, transition, module, instance, binding
        return None

    applied_state, transition, module, instance, binding = client.wait_until(
        "atomic reusable Transition Module assignment", applied
    )
    if binding != {
        "kind": "timeline_item_output",
        "locator": {"kind": "same_timeline"},
        "item_id": input_source["id"],
        "output": "image",
        "stage": "post_transform",
    }:
        raise QaFailure("Transition binding is not a public same-Timeline Image route")
    if (
        applied_state["project"]["items"] != items_before
        or applied_state["project"]["module_definitions"] != definitions_before
        or len(applied_state["project"]["module_instances"])
        != len(instances_before) + 1
    ):
        raise QaFailure("assignment expanded clips or mutated the reusable definition")

    edit_point = media_seconds(transition["edit_point"])
    seek_timeline_seconds(client, edit_point)
    rendered = client.wait_until(
        "required-input Transition Preview",
        lambda: rendered_preview_state(client, applied_state["history"]["revision"]),
        timeout=30.0,
    )

    client.key("z", True, command=True)
    client.key("z", False, command=True)
    restored = client.wait_until(
        "one Undo restoring the built-in Transition",
        lambda: state
        if (state := client.state())["project"] == before_assignment["project"]
        else None,
    )
    if restored["history"].get("can_redo") is not True:
        raise QaFailure("atomic assignment was not one undoable command")

    # Leave the stable shared Modal visible so the suite runner's native PNG
    # captures the actual assignment UI, not only its serialized metadata.
    final_dialog_id, _ = _open_assignment(
        client, transition_id, definition_id, input_id
    )
    _, final_dialog = client.wait_component_settled(final_dialog_id)

    return {
        "suite": "transition-module-assignment",
        "transition_id": transition_id,
        "definition_id": definition_id,
        "module_instance_id": module["instance_id"],
        "published_input_id": input_id,
        "bound_item_id": input_source["id"],
        "choice": choice_metadata,
        "dialog": dialog_metadata,
        "binding": binding,
        "rendered_revision": rendered["editor"]["preview"]["rendered_revision"],
        "applied_revision": applied_state["history"]["revision"],
        "cancelled_revision": cancelled["history"]["revision"],
        "undo_revision": restored["history"]["revision"],
        "final_dialog_rect": final_dialog["rect_points"],
        "actions": client.evidence,
    }


if __name__ == "__main__":
    raise SystemExit(
        run_suite_main(
            "qa-transition-module-assignment-e2e",
            run_suite,
            "target/qa-transition-module-assignment-e2e-evidence.json",
        )
    )
