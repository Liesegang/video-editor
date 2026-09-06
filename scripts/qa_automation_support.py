"""Shared native-UI discovery and observation for Timeline automation."""

from qa_support import (
    QaFailure,
    activate_dock_tab,
    bring_timeline_component,
    seek_timeline_seconds,
    settled_preview_state,
)


TIMELINE_TAB = "dock.tab:timeline"
CURVE_TAB = "dock.tab:curve_editor"


def seek_rendered(client, seconds, description="automation"):
    sought = seek_timeline_seconds(client, seconds)
    return client.wait_until(
        "rendered {} at {}s".format(description, seconds),
        lambda: settled_preview_state(
            client,
            sought["history"]["revision"],
            sought["editor"]["timeline"]["current_frame"],
        ),
        30.0,
    )


def sample_pixel_hashes(client, times, description="automation"):
    return [
        {
            "seconds": time,
            "pixel_hash": seek_rendered(client, time, description)["editor"]["preview"][
                "pixel_hash"
            ],
        }
        for time in times
    ]


def history_shortcut(client, redo=False):
    client.key("z", True, command=True, shift=redo)
    client.key("z", False, command=True, shift=redo)


def assert_module_instance_unchanged(
    project, before, instance_id, description="sibling Module"
):
    """Assert instance state and definition content survive another host's COW."""

    if (
        project["module_instances"][instance_id]
        != before["module_instances"][instance_id]
    ):
        raise QaFailure(description + " instance changed")
    definition_id = before["module_instances"][instance_id]["definition_id"]
    expected = dict(before["module_definitions"][definition_id])
    if expected["sharing"] == {"kind": "shared_local"}:
        remaining = sum(
            instance["definition_id"] == definition_id
            for instance in project["module_instances"].values()
        )
        if remaining != 1:
            raise QaFailure(
                description + " definition did not become exclusively owned"
            )
        expected["sharing"] = {"kind": "private"}
    if project["module_definitions"][definition_id] != expected:
        raise QaFailure(description + " definition changed beyond COW ownership")


def _visible_component(client, component_type, predicate):
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


def wait_item_automation_surfaces(
    client, item_id, target, keyframe_ids, description="Automation"
):
    """Find one authoritative Item lane and its identical Timeline/Curve keys."""

    activate_dock_tab(client, TIMELINE_TAB, "Timeline", description + " Dope Sheet")
    bring_timeline_component(client, "timeline.item:" + item_id, -120.0)
    expand_id = "timeline.item_expand:" + item_id
    client.wait_component_settled(expand_id)
    state = client.state()
    if item_id not in state["editor"]["timeline"]["expanded_items"]:
        client.click_component(expand_id)
    lane = client.wait_until(
        description + " Timeline lane",
        lambda: _visible_component(
            client,
            "timeline_property_label",
            lambda metadata: metadata.get("item_id") == item_id
            and metadata.get("target") == target,
        ),
    )
    for keyframe_id in keyframe_ids:
        bring_timeline_component(
            client,
            "timeline.keyframe:{}:{}".format(item_id, keyframe_id),
            -120.0,
        )
    timeline_keys = [
        client.wait_until(
            "{} Timeline key {}".format(description, keyframe_id),
            lambda keyframe_id=keyframe_id: _visible_component(
                client,
                "timeline_property_keyframe",
                lambda metadata: metadata.get("item_id") == item_id
                and metadata.get("target") == target
                and metadata.get("keyframe_id") == keyframe_id,
            ),
        )
        for keyframe_id in keyframe_ids
    ]
    activate_dock_tab(client, CURVE_TAB, "Curve Editor", description + " Curve Editor")
    client.click_component("curve_editor.fit")
    curve_keys = [
        client.wait_until(
            "{} Curve key {}".format(description, keyframe_id),
            lambda keyframe_id=keyframe_id: _visible_component(
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
            {"id": key["id"], "metadata": key.get("metadata")} for key in timeline_keys
        ],
        "curve_keys": [
            {"id": key["id"], "metadata": key.get("metadata")} for key in curve_keys
        ],
    }


def require_exact_item_times(surfaces, expected_local_times, expected_timeline_times):
    timeline = surfaces["timeline_keys"]
    curve = surfaces["curve_keys"]
    if len(timeline) != len(expected_local_times) or len(curve) != len(
        expected_local_times
    ):
        raise QaFailure("automation surfaces omitted an authoritative key")
    for index, (local_time, timeline_time) in enumerate(
        zip(expected_local_times, expected_timeline_times)
    ):
        timeline_metadata = timeline[index]["metadata"] or {}
        curve_metadata = curve[index]["metadata"] or {}
        if abs(float(timeline_metadata.get("local_time", -1.0)) - local_time) > 0.001:
            raise QaFailure("Timeline key used the wrong Item-local time")
        if (
            abs(float(timeline_metadata.get("timeline_time", -1.0)) - timeline_time)
            > 0.001
        ):
            raise QaFailure("Timeline key used the wrong host time")
        if abs(float(curve_metadata.get("time", -1.0)) - local_time) > 0.001:
            raise QaFailure("Curve key used the wrong Item-local time")
