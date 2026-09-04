#!/usr/bin/env python3
"""Verify a Node Clip Sound output reaches the native Timeline output stream."""

import time

from qa_support import QaFailure, item_by_name, run_suite_main


def _audio_status(client):
    _, component = client.component("transport.audio_output")
    return component.get("metadata") or {}


def run_suite(client):
    client.wait_health()
    initial = client.state()
    audio_item = item_by_name(initial["project"], "QA Audio")
    node_audio_item = item_by_name(initial["project"], "QA Node Audio")
    if (node_audio_item.get("source") or {}).get("kind") != "module":
        raise QaFailure("QA Node Audio is not a Node Clip Module invocation")
    if float(audio_item["interval"]["start"]["value"]) <= 0.0:
        raise QaFailure("plain Audio fixture overlaps the time-zero Node Audio probe")
    audio_assets = [
        asset for asset in initial["project"]["assets"] if asset.get("kind") == "Audio"
    ]
    if len(audio_assets) != 1:
        raise QaFailure("audio fixture did not expose exactly one Audio Asset")
    initial_status = _audio_status(client)
    if initial_status.get("available") is not True:
        raise QaFailure(
            "native audio output is unavailable: {}".format(initial_status.get("error"))
        )

    start_frame = initial["editor"]["timeline"]["current_frame"]
    client.click_component("timeline.play")

    def audible():
        state = client.state()
        status = _audio_status(client)
        if (
            status.get("has_audio_routes") is True
            and status.get("output_active") is True
            and status.get("queued_frames", 0) > 0
            and status.get("rendered_non_silent_frames", 0) > 0
            and state["editor"]["timeline"]["current_frame"] > start_frame
        ):
            return state, status
        return None

    playing, status = client.wait_until(
        "authoring audio queued on the native output clock", audible, timeout=30.0
    )
    client.click_component("timeline.play")
    stopped, stopped_status = client.wait_until(
        "Timeline audio stop",
        lambda: (state, status)
        if not (state := client.state())["editor"]["timeline"]["is_playing"]
        and (status := _audio_status(client)).get("output_active") is False
        and status.get("queued_frames") == 0
        else None,
    )
    time.sleep(0.05)
    stopped_peak_frames = _audio_status(client)["rendered_non_silent_frames"]
    time.sleep(0.15)
    settled_status = _audio_status(client)
    if settled_status.get("rendered_non_silent_frames") != stopped_peak_frames:
        raise QaFailure("Timeline audio kept producing non-silent frames after Stop")

    invocation = node_audio_item["source"]["value"]
    instance = initial["project"]["module_instances"][invocation["instance_id"]]
    definition = initial["project"]["module_definitions"][instance["definition_id"]]
    output_nodes = [
        node_id
        for node_id, node in definition["graph"]["nodes"].items()
        if str((node.get("content") or {}).get("type", ""))
        .replace("_", "")
        .lower()
        == "moduleoutput"
    ]
    if len(output_nodes) != 1:
        raise QaFailure("QA Node Audio Module does not have exactly one Output terminal")
    output_node_id = output_nodes[0]
    client.click_component("timeline.item:" + node_audio_item["id"])
    client.wait_until(
        "Node Audio selection",
        lambda: state
        if ((state := client.state())["editor"]["selection"].get("primary") or {}).get("id")
        == node_audio_item["id"]
        else None,
    )
    client.click_component("inspector.node_clip.open_editor")
    client.wait_component("dock.tab:node_editor")
    client.click_component("dock.tab:node_editor")
    _, audio_port = client.wait_component_settled(
        "node_editor.port.node:{}.input:sound".format(output_node_id)
    )
    audio_port_metadata = audio_port.get("metadata") or {}
    if (
        str(audio_port_metadata.get("data_type", "")).lower() != "audio"
        or audio_port_metadata.get("label") != "Audio"
    ):
        raise QaFailure("Module Output terminal does not visibly expose an Audio input")
    return {
        "suite": "audio-playback",
        "audio_item": audio_item,
        "node_audio_item": node_audio_item,
        "audio_asset": audio_assets[0],
        "output": status,
        "advanced_frame": playing["editor"]["timeline"]["current_frame"],
        "stopped": stopped,
        "stop_transition": stopped_status,
        "stopped_output": settled_status,
        "output_audio_port": audio_port_metadata,
        "actions": client.evidence,
    }


if __name__ == "__main__":
    raise SystemExit(
        run_suite_main(
            "qa-audio-playback-e2e",
            run_suite,
            "target/qa-audio-playback-e2e-evidence.json",
        )
    )
