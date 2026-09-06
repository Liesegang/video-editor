"""Shared production-UI helpers for Text Tracking authoring and promotion."""

from qa_support import QaFailure, component_in_inspector, media_seconds
from qa_text_ensemble_support import operation, seek_rendered


def tracking_property(operation_value, key):
    try:
        return operation_value["properties"][key]
    except KeyError as error:
        raise QaFailure("Tracking omitted {!r}".format(key)) from error


def tracking_property_keys(operation_value, key):
    prop = tracking_property(operation_value, key)
    if prop.get("type") != "keyframe":
        raise QaFailure("Tracking {} is not keyframed".format(key))
    return prop["properties"]["keyframes"]


def maybe_tracking_property_keys(operation_value, key):
    prop = tracking_property(operation_value, key)
    return prop["properties"]["keyframes"] if prop.get("type") == "keyframe" else None


def key_time_seconds(value):
    return media_seconds(value) if isinstance(value, dict) else float(value)


def text_content(state, item_id):
    source = state["project"]["items"][item_id]["source"]
    if source.get("kind") != "text":
        raise QaFailure("Tracking source is no longer direct Text")
    return source["value"]["text"]


def set_multiline_text(client, item_id, content):
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
        and text_content(state, item_id) == content
        else None,
    )


def enter_tracking_amount(client, item_id, operation_id, amount):
    control_id = "inspector.property:text_ensemble:{}:{}:amount".format(
        item_id, operation_id
    )
    component_in_inspector(client, control_id)
    client.click_component(control_id)
    client.key("a", True, command=True)
    client.key("a", False, command=True)
    client.inject("text", {"text": format(float(amount), "g")})
    client.key("enter", True)
    client.key("enter", False)
    return control_id


def module_tracking(project, item_id, operation_id):
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


def keyframe_signature(keyframes):
    return [
        {
            "id": keyframe["id"],
            "time": key_time_seconds(keyframe["time"]),
            "value": keyframe["value"],
            "easing": keyframe["easing"],
        }
        for keyframe in keyframes
    ]


def observe_tracking_times(client, seconds, revision):
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
