#!/usr/bin/env python3
"""Verify a selected Asset gets a real color-managed Inspector preview."""

from qa_support import QaFailure, run_suite_main


def _contained(inner, outer, tolerance=0.5):
    return (
        inner["min_x"] >= outer["min_x"] - tolerance
        and inner["max_x"] <= outer["max_x"] + tolerance
        and inner["min_y"] >= outer["min_y"] - tolerance
        and inner["max_y"] <= outer["max_y"] + tolerance
    )


def run_suite(client):
    client.wait_health()
    initial = client.state()
    project = initial["project"]
    if project.get("name") != "Authoring QA":
        raise QaFailure("Asset preview QA requires authoring_e2e")
    if len(project["assets"]) != 1:
        raise QaFailure("expected exactly one imported fixture Asset")
    asset = project["assets"][0]
    asset_id = asset["id"]

    client.click_component("assets.asset:" + asset_id)

    def preview_ready():
        snapshot = client.component_snapshot()
        preview_id = "inspector.asset_preview:" + asset_id
        info_id = "inspector.asset_info:" + asset_id
        by_id = {component["id"]: component for component in snapshot["components"]}
        preview = by_id.get(preview_id)
        info = by_id.get(info_id)
        if preview is None or info is None:
            return None
        metadata = preview.get("metadata") or {}
        if metadata.get("visual") != "decoded_frame":
            return None
        return snapshot, preview, info

    snapshot, preview, info = client.wait_until(
        "decoded Asset frame in Inspector", preview_ready
    )
    metadata = preview.get("metadata") or {}
    if metadata.get("asset_id") != asset_id:
        raise QaFailure("preview omitted the selected Asset identity")
    if metadata.get("kind") != "image":
        raise QaFailure("fixture Asset did not use its image presentation")
    if metadata.get("error") is not None:
        raise QaFailure("Asset frame reported a preview error: {}".format(metadata["error"]))
    if metadata.get("uses_shared_media_cache") is not True:
        raise QaFailure("Asset preview bypassed the shared media cache")
    if metadata.get("color_managed") is not True:
        raise QaFailure("Asset frame bypassed Project Preview color management")
    if metadata.get("texture_width") != asset.get("width") or metadata.get(
        "texture_height"
    ) != asset.get("height"):
        raise QaFailure("decoded texture dimensions differ from imported Asset metadata")

    info_metadata = info.get("metadata") or {}
    expected_info = {
        "asset_id": asset_id,
        "name": asset.get("name"),
        "path": asset.get("path"),
        "width": asset.get("width"),
        "height": asset.get("height"),
    }
    for field, expected in expected_info.items():
        if info_metadata.get(field) != expected:
            raise QaFailure("Inspector Asset info mismatch for {}".format(field))

    scroll = next(
        (
            component
            for component in snapshot["components"]
            if component["id"] == "inspector.scroll_area"
        ),
        None,
    )
    if scroll is None or not _contained(preview["rect_points"], scroll["rect_points"]):
        raise QaFailure("Asset preview is clipped outside the Inspector surface")
    if not preview.get("visible") or preview["rect_points"]["width"] <= 1.0:
        raise QaFailure("Asset preview has no visible rendered area")

    final = client.state()
    if final["editor"]["selection"].get("primary") != {
        "kind": "asset",
        "id": asset_id,
    }:
        raise QaFailure("Inspector preview did not retain Asset selection")
    if final["history"] != initial["history"] or final["project"] != project:
        raise QaFailure("opening an Asset preview mutated Project history")
    return {
        "suite": "inspector-asset-preview",
        "asset_id": asset_id,
        "preview": preview,
        "info": info,
        "history": final["history"],
        "actions": client.evidence,
    }


if __name__ == "__main__":
    raise SystemExit(
        run_suite_main(
            "qa-inspector-asset-preview-e2e",
            run_suite,
            "target/qa-inspector-asset-preview-e2e-evidence.json",
        )
    )
