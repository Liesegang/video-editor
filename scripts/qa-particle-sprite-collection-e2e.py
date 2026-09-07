#!/usr/bin/env python3
"""Exercise Sprite Image Collections through shared Inspector and Node controls."""

import os
import pathlib

from qa_automation_support import history_shortcut
from qa_node_module_support import (
    connect_nodes,
    create_node_from_menu,
    enter_exact_numeric,
    ensure_node_editor_authoring_scale,
    open_timeline_item_definition,
    place_created_node,
    place_node_for_inline_edit,
    reload_node_clip_project,
    reset_published_node_input_parameter,
    sample_rendered_preview,
    unpublish_node_input_parameter,
)
from qa_particle_force_support import (
    PARTICLE_PUBLISHED_PARAMETERS,
    create_particle_node_clip,
    native_catalog_id,
    parameter,
    reveal_inspector_control,
)
from qa_support import (
    QaFailure,
    activate_dock_tab,
    capture_viewport,
    component_center,
    run_suite_main,
    save_project_to_disk,
)


COLLECTION_TAG = "image_collection_value"


def _asset_ids(value):
    if not isinstance(value, dict) or value.get("$type") != COLLECTION_TAG:
        raise QaFailure("Sprite collection lost its canonical tagged value")
    assets = value.get("assets")
    if not isinstance(assets, list) or len(assets) != len(set(assets)):
        raise QaFailure("Sprite collection assets are not a unique ordered list")
    return assets


def _image_assets(project):
    images = [
        asset
        for asset in project.get("assets", [])
        if str(asset.get("kind", "")).lower() == "image"
    ]
    by_name = {pathlib.Path(asset["path"]).name: asset for asset in images}
    if set(("rgba.png", "rgb.jpg")) - set(by_name):
        raise QaFailure("Sprite collection fixture omitted rgba.png or rgb.jpg")
    if len({by_name[name]["id"] for name in ("rgba.png", "rgb.jpg")}) != 2:
        raise QaFailure("Sprite collection fixture reused one Image identity")
    return by_name


def _instance_value(state, instance_id, parameter_id, default):
    overrides = state["project"]["module_instances"][instance_id].get(
        "parameter_overrides", {}
    )
    return overrides.get(parameter_id, default)


def _wait_collection(client, instance_id, parameter_id, expected, revision, label):
    def authored():
        state = client.state()
        value = _instance_value(state, instance_id, parameter_id, None)
        if not isinstance(value, dict) or value.get("$type") != COLLECTION_TAG:
            return None
        return (
            state
            if state["history"]["revision"] == revision
            and _asset_ids(value) == expected
            else None
        )

    return client.wait_until(label, authored)


def _open_collection(client, control_id):
    selector_id = control_id + ".image_collection.selector"
    reveal_inspector_control(client, control_id)
    _, selector = client.wait_component_settled(selector_id)
    metadata = selector.get("metadata") or {}
    if (
        metadata.get("maximum") != 64
        or metadata.get("empty_fallback") != "analytic_disc"
        or metadata.get("count") != len(metadata.get("asset_ids") or [])
    ):
        raise QaFailure("Sprite collection selector omitted its bounded model contract")
    client.click_component(selector_id)
    return selector_id, metadata


def _add_asset(client, control_id, instance_id, parameter_id, asset_id, expected):
    before = client.state()
    selector_id, metadata = _open_collection(client, control_id)
    if asset_id not in (metadata.get("candidate_asset_ids") or []):
        raise QaFailure("Sprite collection selector omitted an Image Asset candidate")
    candidate_id = selector_id.replace(".selector", ".candidate:" + asset_id)
    _, candidate = client.wait_component_settled(candidate_id)
    candidate_metadata = candidate.get("metadata") or {}
    if candidate_metadata.get("asset_id") != asset_id or candidate_metadata.get(
        "kind"
    ) != "image":
        raise QaFailure("Sprite collection candidate lost its Image identity")
    client.click_component(candidate_id)
    state = _wait_collection(
        client,
        instance_id,
        parameter_id,
        expected,
        before["history"]["revision"] + 1,
        "add Image Asset to Sprite collection",
    )
    client.key("escape", True)
    client.key("escape", False)
    return state


def _drag_asset(client, control_id, instance_id, parameter_id, asset_id):
    reveal_inspector_control(client, control_id)
    selector_id = control_id + ".image_collection.selector"
    _, selector = client.wait_component_settled(selector_id)
    activate_dock_tab(client, "dock.tab:assets", "Assets", "Sprite Asset drag")
    _, source = client.wait_component_settled("assets.asset:" + asset_id)
    before = client.state()
    client.drag(component_center(source), component_center(selector), steps=18)
    state = _wait_collection(
        client,
        instance_id,
        parameter_id,
        [asset_id],
        before["history"]["revision"] + 1,
        "drop Image Asset into Sprite collection",
    )
    if state["editor"]["timeline"].get("library_drag_active") is not False:
        raise QaFailure("Sprite collection drop retained stale Assets drag state")
    return state


def _collection_action(
    client, control_id, instance_id, parameter_id, asset_id, action, expected
):
    before = client.state()
    selector_id, _ = _open_collection(client, control_id)
    action_id = selector_id.replace(
        ".selector", ".entry:{}.{}".format(asset_id, action)
    )
    _, component = client.wait_component_settled(action_id)
    metadata = component.get("metadata") or {}
    if metadata.get("asset_id") != asset_id or metadata.get("action") != action:
        raise QaFailure("Sprite collection action lost its exact Asset identity")
    client.click_component(action_id)
    state = _wait_collection(
        client,
        instance_id,
        parameter_id,
        expected,
        before["history"]["revision"] + 1,
        action + " Sprite Image Asset",
    )
    client.key("escape", True)
    client.key("escape", False)
    return state


def _set_dropdown(client, control_id, expected, instance_id, parameter_id):
    reveal_inspector_control(client, control_id)
    before = client.state()
    client.click_component(control_id)
    option_id = control_id + ".option:" + expected
    client.wait_component_settled(option_id)
    client.click_component(option_id)

    def selected():
        state = client.state()
        value = _instance_value(state, instance_id, parameter_id, None)
        return (
            state
            if state["history"]["revision"] == before["history"]["revision"] + 1
            and value == expected
            else None
        )

    return client.wait_until("Sprite selection mode " + expected, selected)


def _set_number(client, control_id, expected, instance_id, parameter_id):
    reveal_inspector_control(client, control_id)
    before = client.state()
    enter_exact_numeric(client, control_id, expected)

    def edited():
        state = client.state()
        value = _instance_value(state, instance_id, parameter_id, None)
        return (
            state
            if state["history"]["revision"] == before["history"]["revision"] + 1
            and isinstance(value, (int, float))
            and float(value) == float(expected)
            else None
        )

    return client.wait_until("Sprite selection exact numeric edit", edited)


def _numeric_instance_value(state, instance_id, parameter_id, expected):
    default = 0.0 if float(expected) == 0.0 else None
    value = _instance_value(state, instance_id, parameter_id, default)
    return isinstance(value, (int, float)) and float(value) == float(expected)


def _wait_shared_previews(client, control_id, assets):
    for asset_id in assets:
        preview_id = control_id + ".image_collection.preview:selected:" + asset_id

        def ready():
            _, component = client.component(preview_id)
            metadata = (component or {}).get("metadata") or {}
            return (
                component
                if metadata.get("ready") is True
                and metadata.get("pending") is False
                and metadata.get("content_hash")
                and metadata.get("uses_shared_media_cache") is True
                else None
            )

        client.wait_until("decoded Sprite Image thumbnail", ready, 30.0)


def _sprite_node(definition):
    matches = [
        (node_id, node)
        for node_id, node in definition["graph"]["nodes"].items()
        if native_catalog_id(node) == "native.particle.sprite-renderer"
    ]
    if len(matches) != 1:
        raise QaFailure("Particle Definition has no unique Sprite Renderer")
    return matches[0]


def _assert_definition(definition, sprite_id, sprites_parameter_id):
    if len(definition["graph"]["nodes"]) != 9 or len(
        definition["graph"]["connections"]
    ) != 8:
        raise QaFailure("Sprite collection changed the Particle factory topology")
    node = definition["graph"]["nodes"].get(sprite_id)
    if node is None or native_catalog_id(node) != "native.particle.sprite-renderer":
        raise QaFailure("Sprite collection parameter changed its Sprite Node identity")
    published = (definition.get("interface") or {}).get("parameters") or []
    if [entry.get("name") for entry in published] != PARTICLE_PUBLISHED_PARAMETERS:
        raise QaFailure("Sprite collection changed the curated Particle interface")
    sprites = next(
        (entry for entry in published if entry.get("id") == sprites_parameter_id), None
    )
    if sprites is None or sprites.get("target") != {
        "node_id": sprite_id,
        "port": "sprites",
    }:
        raise QaFailure("Sprites parameter no longer targets Sprite.sprites")
    if _asset_ids(sprites.get("default_value")) != []:
        raise QaFailure("Sprite collection factory default is not the analytic disc")


def _assert_node_collection(client, sprite_id, expected):
    activate_dock_tab(client, "dock.tab:node_editor", "Node Editor", "Sprite collection")
    selector_id = "node_editor.property.node:{}:sprites.image_collection.selector".format(
        sprite_id
    )
    selector = place_node_for_inline_edit(client, sprite_id, selector_id)
    metadata = selector.get("metadata") or {}
    if metadata.get("asset_ids") != expected or metadata.get("count") != len(expected):
        raise QaFailure("Node and Inspector Sprite collection values diverged")
    client.click_component(selector_id)
    _wait_shared_previews(
        client, "node_editor.property.node:{}:sprites".format(sprite_id), expected
    )
    for asset_id in expected:
        preview_id = "node_editor.property.node:{}:sprites.image_collection.preview:selected:{}".format(
            sprite_id, asset_id
        )
        _, preview = client.wait_component_settled(preview_id)
        preview_metadata = preview.get("metadata") or {}
        if preview_metadata.get("uses_shared_media_cache") is not True:
            raise QaFailure("Sprite collection thumbnail bypassed shared media preview")
    client.key("escape", True)
    client.key("escape", False)


def _node_collection(node):
    property_value = ((node.get("properties") or {}).get("value") or {}).get(
        "properties"
    )
    if not isinstance(property_value, dict) or "value" not in property_value:
        raise QaFailure("Image Collection Data Node lost its constant Value")
    return _asset_ids(property_value["value"])


def _add_node_asset(client, definition_id, node_id, asset_id, expected):
    selector_id = (
        "node_editor.property.node:{}:value.image_collection.selector".format(node_id)
    )
    place_node_for_inline_edit(client, node_id, selector_id)
    client.click_component(selector_id)
    candidate_id = selector_id.replace(".selector", ".candidate:" + asset_id)
    client.wait_component_settled(candidate_id)
    before = client.state()
    client.click_component(candidate_id)

    def authored():
        state = client.state()
        node = state["project"]["module_definitions"][definition_id]["graph"][
            "nodes"
        ][node_id]
        return (
            state
            if state["history"]["revision"] == before["history"]["revision"] + 1
            and _node_collection(node) == expected
            else None
        )

    state = client.wait_until("author Image Collection Data Node", authored)
    client.key("escape", True)
    client.key("escape", False)
    return state


def _unpublish_sprite_inputs(
    client, definition_id, instance_id, sprite_id, parameters
):
    for port, published in parameters:
        before_reset = client.state()
        reset = reset_published_node_input_parameter(
            client,
            definition_id,
            instance_id,
            sprite_id,
            port,
            published,
            before_reset,
        )
        definition = reset["project"]["module_definitions"][definition_id]
        unpublish_node_input_parameter(
            client,
            definition_id,
            instance_id,
            sprite_id,
            port,
            published,
            definition,
        )


def _author_per_point_selection(
    client, definition_id, instance_id, sprite_id, source_id, sprites, selection, assets
):
    _unpublish_sprite_inputs(
        client,
        definition_id,
        instance_id,
        sprite_id,
        (("sprites", sprites), ("selection", selection)),
    )
    ensure_node_editor_authoring_scale(client)
    # Keep the factory-adjacent Sprite visible when parking its immediate
    # Collision predecessor. Moving both independently would pan between the
    # far ends of the wide factory graph and strand the first endpoint.
    place_created_node(client, source_id, 0.06)
    _, info_id = create_node_from_menu(
        client, "node_clip", "Point Info", "node_editor.menu.create.point_info"
    )
    place_created_node(client, info_id, 0.30, vertical_offset=140.0)
    _, collection_id = create_node_from_menu(
        client,
        "node_clip",
        "Image Collection",
        "node_editor.menu.create.data:image_collection",
    )
    place_created_node(client, collection_id, 0.48, vertical_offset=140.0)
    _add_node_asset(client, definition_id, collection_id, assets[0], [assets[0]])
    _add_node_asset(client, definition_id, collection_id, assets[1], assets)
    routes = [
        connect_nodes(
            client,
            "node_clip",
            source_id,
            "particles",
            info_id,
            "points",
            "Particle stream to Point Info",
        ),
        connect_nodes(
            client,
            "node_clip",
            info_id,
            "random",
            sprite_id,
            "selection",
            "Point random to Sprite selection",
        ),
        connect_nodes(
            client,
            "node_clip",
            collection_id,
            "value",
            sprite_id,
            "sprites",
            "Image Collection to Sprite",
        ),
    ]
    state = client.state()
    definition = state["project"]["module_definitions"][definition_id]
    if len(definition["graph"]["nodes"]) != 11 or len(
        definition["graph"]["connections"]
    ) != 11:
        raise QaFailure("per-point Sprite authoring changed the exact graph cardinality")
    collection_node = definition["graph"]["nodes"][collection_id]
    content = collection_node.get("content") or {}
    if (
        str(content.get("type", "")).replace("_", "").lower() != "data"
        or str(content.get("data", "")).replace("_", "").lower()
        != "imagecollection"
        or _node_collection(collection_node) != assets
    ):
        raise QaFailure("Image Collection Data Node changed its exact ordered assets")
    if any(
        entry.get("id") in (sprites["id"], selection["id"])
        for entry in definition["interface"]["parameters"]
    ):
        raise QaFailure("connected Sprite inputs remained externally published")
    return state, info_id, collection_id, routes


def _route_set(definition):
    return {
        (
            route["from"]["node_id"],
            route["from"]["port"],
            route["to"]["node_id"],
            route["to"]["port"],
        )
        for route in definition["graph"]["connections"]
    }


def _assert_connected_definition(
    definition, sprite_id, info_id, collection_id, assets, expected_routes
):
    if len(definition["graph"]["nodes"]) != 11 or _route_set(definition) != expected_routes:
        raise QaFailure("Sprite collection graph changed its exact persisted routes")
    if native_catalog_id(definition["graph"]["nodes"][sprite_id]) != (
        "native.particle.sprite-renderer"
    ):
        raise QaFailure("reloaded Sprite Renderer changed catalog identity")
    if native_catalog_id(definition["graph"]["nodes"][info_id]) != "native.point.info":
        raise QaFailure("reloaded Point Info changed catalog identity")
    if _node_collection(definition["graph"]["nodes"][collection_id]) != assets:
        raise QaFailure("reloaded Image Collection changed ordered assets")


def run_suite(client):
    initial = client.state()
    assets = _image_assets(initial["project"])
    rgba_id = assets["rgba.png"]["id"]
    rgb_id = assets["rgb.jpg"]["id"]
    created = create_particle_node_clip(client, requested_start_seconds=6.5)
    item_id = created["item_id"]
    instance_id = created["instance_id"]
    definition_id = created["definition_id"]
    parameters = created["parameters"]
    sprites = parameter(parameters, "Sprites")
    selection_mode = parameter(parameters, "Selection Mode")
    selection = parameter(parameters, "Selection")
    tint = parameter(parameters, "Tint")
    if tint.get("default_value") != {"r": 255, "g": 255, "b": 255, "a": 255}:
        raise QaFailure("Sprite Tint is not canonical opaque white")
    sprite_id, _ = _sprite_node(created["definition"])
    source_id = next(
        route["from"]["node_id"]
        for route in created["definition"]["graph"]["connections"]
        if route["to"] == {"node_id": sprite_id, "port": "particles"}
    )
    _assert_definition(created["definition"], sprite_id, sprites["id"])

    activate_dock_tab(client, "dock.tab:inspector", "Inspector", "Sprite collection")
    control_id = "inspector.property:module_instance:{}:{}".format(
        instance_id, sprites["id"]
    )
    first = _drag_asset(client, control_id, instance_id, sprites["id"], rgba_id)
    activate_dock_tab(client, "dock.tab:inspector", "Inspector", "Sprite collection")
    second = _add_asset(
        client, control_id, instance_id, sprites["id"], rgb_id, [rgba_id, rgb_id]
    )
    reordered = _collection_action(
        client,
        control_id,
        instance_id,
        sprites["id"],
        rgb_id,
        "move_up",
        [rgb_id, rgba_id],
    )
    removed = _collection_action(
        client,
        control_id,
        instance_id,
        sprites["id"],
        rgba_id,
        "remove",
        [rgb_id],
    )
    history_shortcut(client)
    restored = _wait_collection(
        client,
        instance_id,
        sprites["id"],
        [rgb_id, rgba_id],
        removed["history"]["revision"] + 1,
        "Undo Sprite collection removal",
    )
    history_shortcut(client, redo=True)
    removed_again = _wait_collection(
        client,
        instance_id,
        sprites["id"],
        [rgb_id],
        restored["history"]["revision"] + 1,
        "Redo Sprite collection removal",
    )
    history_shortcut(client)
    restored_again = _wait_collection(
        client,
        instance_id,
        sprites["id"],
        [rgb_id, rgba_id],
        removed_again["history"]["revision"] + 1,
        "Undo Sprite collection removal for authored result",
    )

    mode_control = "inspector.property:module_instance:{}:{}".format(
        instance_id, selection_mode["id"]
    )
    value_mode = _set_dropdown(
        client, mode_control, "value", instance_id, selection_mode["id"]
    )
    start = 6.5
    first_pixels = sample_rendered_preview(
        client, start + 0.75, value_mode["history"]["revision"], "first Sprite image"
    )

    selection_control = "inspector.property:module_instance:{}:{}".format(
        instance_id, selection["id"]
    )
    second_choice = _set_number(
        client, selection_control, 1.0, instance_id, selection["id"]
    )
    second_pixels = sample_rendered_preview(
        client,
        start + 0.75,
        second_choice["history"]["revision"],
        "second Sprite image",
    )
    if second_pixels["pixel_hash"] == first_pixels["pixel_hash"]:
        raise QaFailure("Selection 0 and 1 rendered the same Sprite Image pixels")
    history_shortcut(client)
    selection_undone = client.wait_until(
        "Undo Sprite selection",
        lambda: state
        if (state := client.state())["history"]["revision"]
        == second_choice["history"]["revision"] + 1
        and _numeric_instance_value(state, instance_id, selection["id"], 0.0)
        else None,
    )
    undone_pixels = sample_rendered_preview(
        client,
        start + 0.75,
        selection_undone["history"]["revision"],
        "undone Sprite selection",
    )
    if undone_pixels["pixel_hash"] != first_pixels["pixel_hash"]:
        raise QaFailure("Undo Sprite selection did not restore exact first-image pixels")
    history_shortcut(client, redo=True)
    selection_redone = client.wait_until(
        "Redo Sprite selection",
        lambda: state
        if (state := client.state())["history"]["revision"]
        == selection_undone["history"]["revision"] + 1
        and _numeric_instance_value(state, instance_id, selection["id"], 1.0)
        else None,
    )
    redone_pixels = sample_rendered_preview(
        client,
        start + 0.75,
        selection_redone["history"]["revision"],
        "redone Sprite selection",
    )
    if redone_pixels["pixel_hash"] != second_pixels["pixel_hash"]:
        raise QaFailure("Redo Sprite selection did not restore exact second-image pixels")

    activate_dock_tab(client, "dock.tab:timeline", "Timeline", "Sprite collection Inspector")
    client.click_component("timeline.item:" + item_id)
    activate_dock_tab(client, "dock.tab:inspector", "Inspector", "Sprite collection Inspector")
    _open_collection(client, control_id)
    _wait_shared_previews(client, control_id, [rgb_id, rgba_id])
    artifact_dir = pathlib.Path(os.environ["RUVIE_QA_ARTIFACT_DIR"])
    inspector_capture = capture_viewport(
        client, artifact_dir / "sprite-collection-inspector.png"
    )
    client.key("escape", True)
    client.key("escape", False)

    open_timeline_item_definition(client, item_id, "node_clip", "Sprite collection Node")
    _assert_node_collection(client, sprite_id, [rgb_id, rgba_id])
    connected, info_id, collection_id, routes = _author_per_point_selection(
        client,
        definition_id,
        instance_id,
        sprite_id,
        source_id,
        sprites,
        selection,
        [rgb_id, rgba_id],
    )
    per_point_pixels = sample_rendered_preview(
        client,
        start + 0.75,
        connected["history"]["revision"],
        "per-point Sprite selection",
    )
    if per_point_pixels["pixel_hash"] in {
        first_pixels["pixel_hash"],
        second_pixels["pixel_hash"],
    }:
        raise QaFailure("per-point Sprite selection rendered as one uniform Image Asset")
    open_timeline_item_definition(client, item_id, "node_clip", "Sprite collection capture")
    node_capture = capture_viewport(client, artifact_dir / "capture.png")

    final = client.state()
    expected_routes = _route_set(
        final["project"]["module_definitions"][definition_id]
    )
    samples = [
        per_point_pixels,
        sample_rendered_preview(
            client, start + 1.25, final["history"]["revision"], "Sprite collection later frame"
        ),
    ]
    project_path = os.environ.get("RUVIE_QA_PROJECT_PATH")
    if not project_path:
        raise QaFailure("Sprite collection persistence QA requires a Project file")
    project_file = pathlib.Path(project_path)
    saved, saved_file = save_project_to_disk(client, project_file, "Sprite collection")
    reloaded = reload_node_clip_project(
        client,
        project_file,
        saved,
        item_id,
        definition_id,
        samples,
        artifact_dir,
        "Sprite collection",
        "particle-sprite-collection",
        "sprite-collection-reloaded.png",
        lambda definition: _assert_connected_definition(
            definition,
            sprite_id,
            info_id,
            collection_id,
            [rgb_id, rgba_id],
            expected_routes,
        ),
    )
    return {
        "suite": "particle-sprite-collection",
        "item_id": item_id,
        "definition_id": definition_id,
        "instance_id": instance_id,
        "asset_ids": [rgb_id, rgba_id],
        "point_info_id": info_id,
        "collection_node_id": collection_id,
        "connections": routes,
        "history_revisions": {
            "first": first["history"]["revision"],
            "second": second["history"]["revision"],
            "reordered": reordered["history"]["revision"],
            "final": final["history"]["revision"],
        },
        "samples": samples,
        "capture": node_capture,
        "inspector_capture": inspector_capture,
        "saved": saved_file,
        "reloaded": reloaded,
        "actions": client.evidence,
    }


if __name__ == "__main__":
    raise SystemExit(
        run_suite_main(
            "qa-particle-sprite-collection-e2e",
            run_suite,
            "target/qa-particle-sprite-collection-e2e-evidence.json",
        )
    )
