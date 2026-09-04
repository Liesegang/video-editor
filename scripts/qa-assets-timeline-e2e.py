#!/usr/bin/env python3
"""Verify Assets List/Table/Grid views and their shared Timeline drag path."""

from qa_support import QaFailure, component_center, item_by_name, run_suite_main


def _rows(snapshot):
    return [
        component
        for component in snapshot["components"]
        if component.get("type") == "asset_item" and component.get("visible")
    ]


def _assert_rows_do_not_overlap(rows):
    ordered = sorted(rows, key=lambda row: row["rect_points"]["min_y"])
    for previous, current in zip(ordered, ordered[1:]):
        if previous["rect_points"]["max_y"] > current["rect_points"]["min_y"] + 0.1:
            raise QaFailure(
                "Assets rows overlap: {} and {}".format(previous["id"], current["id"])
            )


def _assert_contained(child, parent, description):
    child_rect = child["rect_points"]
    parent_rect = parent["rect_points"]
    tolerance = 0.2
    if not (
        child_rect["min_x"] >= parent_rect["min_x"] - tolerance
        and child_rect["max_x"] <= parent_rect["max_x"] + tolerance
        and child_rect["min_y"] >= parent_rect["min_y"] - tolerance
        and child_rect["max_y"] <= parent_rect["max_y"] + tolerance
    ):
        raise QaFailure("{} is clipped outside its row".format(description))


def _wait_view_mode(client, mode):
    def active():
        state = client.state()
        snapshot = client.component_snapshot()
        view = next(
            (
                component
                for component in snapshot["components"]
                if component["id"] == "assets.view_mode"
            ),
            None,
        )
        if view is None or (view.get("metadata") or {}).get("mode") != mode:
            return None
        if state["editor"]["assets"].get("view_mode") != mode:
            return None
        return state, snapshot, view

    return client.wait_until("Assets {} view".format(mode), active)


def run_suite(client):
    client.wait_health()
    initial = client.state()
    project = initial["project"]
    initial_history = initial["history"]
    if project.get("name") != "Authoring QA":
        raise QaFailure("Assets QA requires authoring_e2e")
    if len(project["assets"]) != 1:
        raise QaFailure("expected the fixture import to produce exactly one Asset")
    if initial["editor"]["assets"].get("view_mode") != "list":
        raise QaFailure("Assets must open in its compact List view")

    asset = project["assets"][0]
    asset_id = asset["id"]
    asset_component_id = "assets.asset:" + asset_id
    initial_snapshot, asset_component = client.wait_component_settled(asset_component_id)
    rows = _rows(initial_snapshot)
    if len(rows) < 2:
        raise QaFailure("Assets did not render its imported media and Node Clip rows")
    _assert_rows_do_not_overlap(rows)
    metadata = asset_component.get("metadata") or {}
    if metadata.get("asset_id") != asset_id or metadata.get("draggable_to_timeline") is not True:
        raise QaFailure("imported Asset row omitted drag provenance")
    if metadata.get("view_mode") != "list":
        raise QaFailure("initial Asset row did not report List presentation")

    _, asset_metadata = client.wait_component_settled(
        "assets.asset_metadata:" + asset_id
    )
    asset_metadata_values = asset_metadata.get("metadata") or {}
    expected_size = "{} x {}".format(asset["width"], asset["height"])
    if expected_size not in asset_metadata_values.get("text", ""):
        raise QaFailure("Asset metadata omitted its complete source size")
    if asset_metadata_values.get("fully_visible") is not True:
        raise QaFailure("Asset metadata reports clipped layout")
    _assert_contained(asset_metadata, asset_component, "Asset metadata")

    root_timeline_id = project["root_timeline_id"]
    _, composition = client.wait_component_settled(
        "assets.composition:" + root_timeline_id
    )
    _, composition_metadata = client.wait_component_settled(
        "assets.composition_metadata:" + root_timeline_id
    )
    composition_text = (composition_metadata.get("metadata") or {}).get("text", "")
    for field in ("640 x 360", "30.000 fps", "0:12"):
        if field not in composition_text:
            raise QaFailure("Assets metadata omitted {!r}".format(field))
    _assert_contained(composition_metadata, composition, "Composition metadata")

    _, toolbar = client.wait_component("assets.toolbar")
    if (toolbar.get("metadata") or {}).get("drag_instruction_visible") is not False:
        raise QaFailure("obsolete Assets drag instruction is still visible")

    client.click_component("assets.view.table")
    table_state, _, table_mode = _wait_view_mode(client, "table")
    if table_state["project"] != project or table_state["history"] != initial_history:
        raise QaFailure("switching to Table view mutated Project history")
    _, table_asset = client.wait_component_settled(asset_component_id)
    table_asset_metadata = table_asset.get("metadata") or {}
    if table_asset_metadata.get("view_mode") != "table":
        raise QaFailure("Table view changed the Asset's semantic identity")
    _, table_header = client.wait_component_settled(
        "assets.table.columns:assets.section.media"
    )
    header_metadata = table_header.get("metadata") or {}
    if header_metadata.get("columns") != ["Name", "Kind", "Size", "FPS", "Duration"]:
        raise QaFailure("Table view omitted a required metadata column")
    if header_metadata.get("horizontal_scroll") is not True:
        raise QaFailure("narrow Table view has no horizontal overflow path")

    client.click_component(asset_component_id)

    def table_selection():
        state = client.state()
        expected = {"kind": "asset", "id": asset_id}
        return state if state["editor"]["selection"].get("primary") == expected else None

    client.wait_until("Asset selection in Table view", table_selection)
    client.scroll_component(asset_component_id, delta_x=-600.0, delta_y=0.0)
    _, duration_column = client.wait_component_settled(
        "assets.table.column:assets.section.media:duration"
    )
    if (duration_column.get("metadata") or {}).get("label") != "Duration":
        raise QaFailure("Table horizontal scroll did not reveal Duration")

    client.click_component("assets.view.grid")
    grid_state, _, grid_mode = _wait_view_mode(client, "grid")
    if grid_state["project"] != project or grid_state["history"] != initial_history:
        raise QaFailure("switching to Grid view mutated Project history")
    _, grid_asset = client.wait_component_settled(asset_component_id)
    grid_asset_metadata = grid_asset.get("metadata") or {}
    if grid_asset_metadata.get("asset_id") != asset_id or grid_asset_metadata.get(
        "view_mode"
    ) != "grid":
        raise QaFailure("Grid card changed the Asset's semantic identity")

    preview_id = "assets.preview:" + asset_id

    def preview_ready():
        _, preview = client.component(preview_id)
        values = preview.get("metadata") or {}
        if (
            values.get("ready") is True
            and values.get("pending") is False
            and isinstance(values.get("content_hash"), str)
            and values.get("content_hash")
        ):
            return preview
        return None

    grid_preview = client.wait_until("decoded Grid Asset preview", preview_ready)
    preview_metadata = grid_preview.get("metadata") or {}
    if preview_metadata.get("asset_id") != asset_id:
        raise QaFailure("Grid preview omitted its Asset identity")
    if preview_metadata.get("uses_shared_media_cache") is not True:
        raise QaFailure("Grid preview bypassed AuthoringMediaPreviewService")
    preview_hash = preview_metadata["content_hash"]

    # Re-entering Grid must reuse the resident texture instead of painting an
    # empty card while an equivalent request is pending.
    client.click_component("assets.view.table")
    _wait_view_mode(client, "table")
    client.click_component("assets.view.grid")
    _wait_view_mode(client, "grid")
    stable_previews = []
    for _ in range(4):
        _, preview = client.wait_component(preview_id)
        values = preview.get("metadata") or {}
        stable_previews.append(values)
        if values.get("ready") is not True or values.get("content_hash") != preview_hash:
            raise QaFailure("resident Grid preview flickered or changed identity")

    _, canvas = client.wait_component("timeline.canvas")
    _, track = client.wait_component(
        next(
            component["id"]
            for component in initial_snapshot["components"]
            if component.get("type") == "timeline_track"
        )
    )
    _, grid_asset = client.wait_component_settled(asset_component_id)
    start = component_center(grid_asset)
    canvas_rect = canvas["rect_points"]
    target = {
        "x": canvas_rect["min_x"] + canvas_rect["width"] * 0.82,
        "y": component_center(track)["y"],
    }
    before_items = dict(project["items"])
    before_revision = initial_history["revision"]
    client.drag(start, target, steps=18)

    def placed():
        state = client.state()
        if (
            len(state["project"]["items"]) == len(before_items) + 1
            and state["history"]["revision"] == before_revision + 1
            and state["editor"]["timeline"]["library_drag_active"] is False
        ):
            return state
        return None

    final = client.wait_until("the imported Asset placement", placed)
    new_items = [
        item for item_id, item in final["project"]["items"].items() if item_id not in before_items
    ]
    if len(new_items) != 1:
        raise QaFailure("Asset drag did not create exactly one Timeline clip")
    new_item = new_items[0]
    if new_item["source"] != {"kind": "asset", "value": {"asset_id": asset_id}}:
        raise QaFailure("placed clip does not reference the dragged Asset")
    for item_id, before in before_items.items():
        if final["project"]["items"][item_id] != before:
            raise QaFailure("Asset placement mutated existing clip {}".format(item_id))
    if final["editor"].get("error") is not None:
        raise QaFailure(
            "Assets interaction reported an error: {}".format(final["editor"]["error"])
        )
    client.wait_component("timeline.item:" + new_item["id"])
    item_by_name(final["project"], asset["name"])
    return {
        "suite": "assets-view-modes-to-timeline",
        "asset_id": asset_id,
        "asset_row": asset_component,
        "asset_metadata": asset_metadata,
        "composition_metadata": composition_metadata,
        "visible_asset_rows": [row["id"] for row in rows],
        "view_modes": {
            "table": table_mode,
            "grid": grid_mode,
            "table_asset": table_asset,
            "grid_asset": grid_asset,
            "table_header": table_header,
            "duration_column_after_scroll": duration_column,
        },
        "grid_preview": grid_preview,
        "stable_grid_previews": stable_previews,
        "drag": {"from": start, "to": target},
        "created_item": new_item,
        "history": final["history"],
        "final_editor": {
            "error": final["editor"].get("error"),
            "status": final["editor"].get("status"),
            "view_mode": final["editor"]["assets"].get("view_mode"),
        },
        "actions": client.evidence,
    }


if __name__ == "__main__":
    raise SystemExit(
        run_suite_main(
            "qa-assets-timeline-e2e",
            run_suite,
            "target/qa-assets-timeline-e2e-evidence.json",
        )
    )
