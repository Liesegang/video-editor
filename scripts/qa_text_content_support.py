"""Shared production-Inspector helpers for direct and promoted Text Content."""

from qa_support import QaFailure, component_in_inspector


def direct_text_content(project, item_id):
    source = project["items"][item_id]["source"]
    if source.get("kind") != "text":
        raise QaFailure("Text Content target is no longer a direct Text source")
    return source["value"]["text"]


def enter_multiline_text(client, control_id, content):
    component_in_inspector(client, control_id)
    client.click_component(control_id)
    client.key("a", True, command=True)
    client.key("a", False, command=True)
    client.inject("text", {"text": content})
    client.key("enter", True, command=True)
    client.key("enter", False, command=True)


def edit_direct_text(client, item_id, content, description="direct Text Content edit"):
    control_id = "inspector.property:item:{}:text".format(item_id)
    before = client.state()
    enter_multiline_text(client, control_id, content)
    return client.wait_until(
        description,
        lambda: state
        if (state := client.state())["history"]["revision"]
        == before["history"]["revision"] + 1
        and direct_text_content(state["project"], item_id) == content
        else None,
    )


def module_parameter(definition, name, data_type=None, target_port=None):
    matches = [
        parameter
        for parameter in definition["interface"]["parameters"]
        if parameter.get("name") == name
        and (data_type is None or parameter.get("data_type") == data_type)
        and (
            target_port is None
            or (parameter.get("target") or {}).get("port") == target_port
        )
    ]
    if len(matches) != 1:
        raise QaFailure(
            "Node Clip expected one published {!r} parameter, got {}".format(
                name, len(matches)
            )
        )
    return matches[0]


def module_text_content(project, item_id):
    source = project["items"][item_id]["source"]
    if source.get("kind") != "module":
        raise QaFailure("Text Content target is not a Node Clip")
    instance_id = source["value"]["instance_id"]
    instance = project["module_instances"].get(instance_id)
    if instance is None:
        raise QaFailure("Text Node Clip omitted its Module Instance")
    definition = project["module_definitions"].get(instance["definition_id"])
    if definition is None:
        raise QaFailure("Text Node Clip omitted its Module Definition")
    parameter = module_parameter(definition, "Content", "String", "text")
    value = instance["parameter_overrides"].get(
        parameter["id"], parameter["default_value"]
    )
    if not isinstance(value, str):
        raise QaFailure("published Text Content is not a String")
    return {
        "source": source,
        "instance": instance,
        "definition": definition,
        "parameter": parameter,
        "value": value,
    }


def edit_module_text(client, item_id, content, description="Node Clip Content edit"):
    before = client.state()
    context = module_text_content(before["project"], item_id)
    control_id = "inspector.property:module_instance:{}:{}".format(
        context["instance"]["id"], context["parameter"]["id"]
    )
    enter_multiline_text(client, control_id, content)
    return client.wait_until(
        description,
        lambda: state
        if (state := client.state())["history"]["revision"]
        == before["history"]["revision"] + 1
        and module_text_content(state["project"], item_id)["value"] == content
        else None,
    )
