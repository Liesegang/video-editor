#!/usr/bin/env python3
"""Convert one Text clip source to a bounded Node Clip through native UI."""

from qa_support import QaFailure, component_center, component_point, item_by_name, run_suite_main


def _component(snapshot, component_id):
    return next(
        (component for component in snapshot["components"] if component["id"] == component_id),
        None,
    )


def _inside_scroll_area(component, scroll_area):
    if not component or not component.get("visible") or not component.get("enabled"):
        return False
    rect = component.get("rect_points") or {}
    area = scroll_area.get("rect_points") or {}
    return (
        float(rect.get("width", 0.0)) > 0.0
        and float(rect.get("height", 0.0)) > 0.0
        and float(area.get("min_y", 0.0))
        <= float(rect.get("center_y", -1.0))
        <= float(area.get("max_y", 0.0))
    )


def _bring_into_inspector(client, component_id, scroll_delta):
    for _ in range(12):
        snapshot = client.component_snapshot()
        component = _component(snapshot, component_id)
        scroll_area = _component(snapshot, "inspector.scroll_area")
        if _inside_scroll_area(component, scroll_area or {}):
            return client.wait_component_settled(component_id)[1]
        client.scroll_component("inspector.scroll_area", 0.0, scroll_delta)
    raise QaFailure("could not bring {} into the Inspector".format(component_id))


def _move_effect_to_pre_transform(client, attachment_id):
    handle = _bring_into_inspector(
        client, "inspector.effect_drag_handle:" + attachment_id, -320.0
    )
    start = component_center(handle)
    client.inject(
        "press",
        {**start, "button": "primary", "coordinate_space": "points"},
    )
    client.inject(
        "move",
        {
            "x": start["x"] + 8.0,
            "y": start["y"],
            "button": "primary",
            "coordinate_space": "points",
        },
    )
    client.wait_until(
        "Effect drag payload",
        lambda: component
        if (
            component := _component(
                client.component_snapshot(), "inspector.effects.drag_state"
            )
        )
        and (component.get("metadata") or {}).get("dragging") is True
        else None,
    )
    slot = _bring_into_inspector(
        client, "inspector.effect_drop:item_pre_transform:0", 320.0
    )
    target = component_center(slot)
    for _ in range(2):
        client.inject(
            "move",
            {**target, "button": "primary", "coordinate_space": "points"},
        )
        slot = _bring_into_inspector(
            client, "inspector.effect_drop:item_pre_transform:0", 320.0
        )
        target = component_center(slot)
    metadata = slot.get("metadata") or {}
    if metadata.get("hovered") is not True:
        raise QaFailure("pre-Transform insertion slot did not preview the move")
    client.inject(
        "release",
        {**target, "button": "primary", "coordinate_space": "points"},
    )


def _node_type(node):
    return str((node.get("content") or {}).get("type", "")).replace("_", "").lower()


def _plugin_component(node):
    content = node.get("content") or {}
    if _node_type(node) != "pluginoperation":
        return None
    return (content.get("data") or {}).get("component_id")


def _preview_for_revision(client, revision):
    state = client.state()
    preview = state["editor"]["preview"]
    if (
        preview.get("rendered_revision") == revision
        and preview.get("nontransparent_pixels", 0) > 0
        and preview.get("pixel_hash") is not None
        and state["editor"].get("error") is None
    ):
        return state
    return None


def run_suite(client):
    client.wait_health()
    initial = client.state()
    text_item = item_by_name(initial["project"], "QA Text")
    item_id = text_item["id"]
    client.click_component("timeline.item:" + item_id)

    def fixture_effects():
        state = client.state()
        effects = [
            (attachment_id, attachment)
            for attachment_id, attachment in state["project"]["attachments"].items()
            if attachment["owner"]
            == {"kind": "item", "item_id": item_id}
        ]
        return (state, effects) if len(effects) == 2 else None

    _, effects = client.wait_until("fixture Text Effects", fixture_effects)
    tile_id, _ = next(
        pair
        for pair in effects
        if pair[1]["processor"]["value"]["operation"]["component_id"] == "tile"
    )
    blur_id, _ = next(pair for pair in effects if pair[0] != tile_id)
    _move_effect_to_pre_transform(client, tile_id)

    moved = client.wait_until(
        "one pre- and one post-Transform Effect",
        lambda: state
        if (
            (state := client.state())["project"]["attachments"][tile_id]["stage"]
            == "item_pre_transform"
            and state["project"]["attachments"][blur_id]["stage"]
            == "item_post_transform"
        )
        else None,
    )

    # Seek inside the Text interval so the pixel hash validates the converted
    # source and both sides of the host Transform boundary.
    _, ruler = client.wait_component("timeline.ruler")
    client.inject(
        "click",
        {
            **component_point(ruler, 0.16, 0.5),
            "button": "primary",
            "coordinate_space": "points",
        },
    )
    before = client.wait_until(
        "pre-conversion Preview",
        lambda: _preview_for_revision(client, moved["history"]["revision"]),
        timeout=30.0,
    )
    project_before = before["project"]
    item_before = project_before["items"][item_id]
    attachments_before = project_before["attachments"]
    preview_hash = before["editor"]["preview"]["pixel_hash"]
    revision_before = before["history"]["revision"]

    client.click_component("timeline.item:" + item_id, button="secondary")
    menu_id = "timeline.item.convert_source_to_node_clip:" + item_id
    client.wait_component(menu_id)
    client.click_component(menu_id)

    def converted():
        state = client.state()
        source = state["project"]["items"][item_id]["source"]
        document = state["editor"]["node_editor"]["document"]
        if (
            source.get("kind") == "module"
            and state["history"]["revision"] == revision_before + 1
            and document
            and document.get("kind") == "module_definition"
            and document.get("host") == "node_clip"
        ):
            return state
        return None

    after = client.wait_until("bounded Node Clip conversion", converted)
    item_after = after["project"]["items"][item_id]
    source = item_after["source"]["value"]
    document = after["editor"]["node_editor"]["document"]
    definition_id = document["definition_id"]
    instance_id = document["instance_id"]
    if "Node Editor" not in after["dock"]["active_tabs"]:
        raise QaFailure("conversion did not focus the existing Node Editor panel")
    if "moved 1 pre-Transform effect(s), kept 1 post-Transform effect(s) outside" not in after[
        "editor"
    ]["status"]:
        raise QaFailure("conversion status did not explain the preserved stage boundary")
    if source["instance_id"] != instance_id:
        raise QaFailure("opened Node Editor does not target the converted instance")
    if item_after["authored_properties"] != item_before["authored_properties"]:
        raise QaFailure("conversion moved Timeline-owned placement Keyframes")
    for key in ("track_id", "interval", "time_map", "layer", "parent", "blend_mode"):
        if item_after[key] != item_before[key]:
            raise QaFailure("conversion changed Timeline placement field {}".format(key))
    for sibling_id, sibling in project_before["items"].items():
        if sibling_id != item_id and after["project"]["items"][sibling_id] != sibling:
            raise QaFailure("conversion mutated sibling {}".format(sibling_id))
    if tile_id in after["project"]["attachments"]:
        raise QaFailure("pre-Transform Effect was not moved into the Node Clip")
    if after["project"]["attachments"].get(blur_id) != attachments_before[blur_id]:
        raise QaFailure("post-Transform Effect was not retained unchanged outside the Module")

    definition = after["project"]["module_definitions"][definition_id]
    nodes = definition["graph"]["nodes"]
    output_nodes = [node_id for node_id, node in nodes.items() if _node_type(node) == "moduleoutput"]
    text_nodes = [
        node_id
        for node_id, node in nodes.items()
        if _node_type(node) == "generator"
        and str((node.get("content") or {}).get("data", "")).lower() == "text"
    ]
    tile_nodes = [node_id for node_id, node in nodes.items() if _plugin_component(node) == "tile"]
    fill_nodes = [node_id for node_id, node in nodes.items() if _plugin_component(node) == "fill"]
    if not (
        len(nodes) == 4
        and len(output_nodes) == 1
        and len(text_nodes) == 1
        and tile_nodes == [tile_id]
        and len(fill_nodes) == 1
        and len(definition["graph"]["connections"]) == 3
    ):
        raise QaFailure("converted topology is not Text -> Fill -> Tile -> Output")

    _, canvas = client.wait_component_settled("node_editor.canvas")
    metadata = canvas.get("metadata") or {}
    if metadata.get("production_surface") != "egui_snarl":
        raise QaFailure("conversion did not open the production Node Editor")
    if metadata.get("timeline_graph_expansion") is not False:
        raise QaFailure("conversion expanded Timeline structure into Nodes")
    for node_id in nodes:
        client.wait_component("node_editor.node:" + node_id)

    rendered_after = client.wait_until(
        "converted Preview publication",
        lambda: _preview_for_revision(client, revision_before + 1),
        timeout=30.0,
    )
    if rendered_after["editor"]["preview"]["pixel_hash"] != preview_hash:
        raise QaFailure("Node Clip conversion changed Preview pixels")

    client.key("z", True, command=True)
    client.key("z", False, command=True)
    undone = client.wait_until(
        "one-step conversion Undo",
        lambda: state if (state := client.state())["project"] == project_before else None,
    )
    restored_preview = client.wait_until(
        "Preview after conversion Undo",
        lambda: state
        if (state := client.state())["editor"]["preview"].get("pixel_hash") == preview_hash
        and state["editor"]["preview"].get("nontransparent_pixels", 0) > 0
        else None,
        timeout=30.0,
    )
    if not undone["history"]["can_redo"]:
        raise QaFailure("single Undo did not preserve the conversion as one redo step")
    return {
        "suite": "node-clip-conversion",
        "item_id": item_id,
        "definition_id": definition_id,
        "instance_id": instance_id,
        "topology": {
            "nodes": list(nodes),
            "connections": definition["graph"]["connections"],
            "output_node": output_nodes[0],
            "pre_effect_node": tile_nodes[0],
        },
        "preview_hash": preview_hash,
        "rendered_revision": rendered_after["editor"]["preview"]["rendered_revision"],
        "undo_revision": restored_preview["history"]["revision"],
        "node_editor_canvas": metadata,
        "actions": client.evidence,
    }


if __name__ == "__main__":
    raise SystemExit(
        run_suite_main(
            "qa-node-clip-conversion-e2e",
            run_suite,
            "target/qa-node-clip-conversion-e2e-evidence.json",
        )
    )
