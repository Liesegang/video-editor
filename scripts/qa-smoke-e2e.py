#!/usr/bin/env python3
"""Smoke-test the production authoring shell and first rendered Preview."""

from qa_support import QaFailure, run_suite_main


def run_suite(client):
    health = client.wait_health()

    def ready():
        state = client.state()
        preview = state["editor"]["preview"]
        if (
            state["project"].get("name") == "Authoring QA"
            and preview.get("nontransparent_pixels", 0) > 0
            and preview.get("pixel_hash") is not None
            and state["editor"].get("error") is None
        ):
            return state
        return None

    state = client.wait_until("the first meaningful authoring Preview", ready, timeout=30.0)
    snapshot = client.component_snapshot()
    component_ids = {component["id"] for component in snapshot["components"]}
    required = {
        "dock.tab:preview",
        "dock.tab:timeline",
        "dock.tab:inspector",
        "dock.tab:assets",
        "preview.canvas",
        "preview.content",
        "timeline.canvas",
        "timeline.play",
        "inspector.scroll_area",
    }
    missing = sorted(required - component_ids)
    if missing:
        raise QaFailure("authoring shell omitted components: {}".format(", ".join(missing)))
    if len(state["project"]["items"]) != 4:
        raise QaFailure("authoring fixture did not expose four Timeline clips")
    if not state["project"]["assets"]:
        raise QaFailure("the production import path produced no Asset")
    return {
        "suite": "authoring-smoke",
        "health": health,
        "frame": snapshot["frame"],
        "active_tabs": state["dock"]["active_tabs"],
        "preview": state["editor"]["preview"],
        "component_count": len(snapshot["components"]),
        "actions": client.evidence,
    }


if __name__ == "__main__":
    raise SystemExit(
        run_suite_main("qa-smoke-e2e", run_suite, "target/qa-smoke-e2e-evidence.json")
    )
