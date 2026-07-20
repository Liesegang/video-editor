#!/usr/bin/env python3
"""Coordinate E2E for placing a reusable Composition on a Timeline Track."""

import argparse
import importlib.util
import json
import os
import signal
import subprocess
import sys


SCRIPT_DIR = os.path.dirname(os.path.abspath(__file__))
BASE_PATH = os.path.join(SCRIPT_DIR, "qa-e2e.py")
SPEC = importlib.util.spec_from_file_location("ruvie_qa_composition_drop_base", BASE_PATH)
if SPEC is None or SPEC.loader is None:
    raise RuntimeError("cannot load scripts/qa-e2e.py")
BASE = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(BASE)

QaClient = BASE.QaClient
QaFailure = BASE.QaFailure

PARENT_COMPOSITION = "00000000-0000-0000-0000-000000009100"
PARENT_TRACK = "00000000-0000-0000-0000-000000009101"
SOURCE_COMPOSITION = "00000000-0000-0000-0000-000000009200"
SOURCE_TRACK = "00000000-0000-0000-0000-000000009201"
SOURCE_COMPONENT = "assets.composition:" + SOURCE_COMPOSITION
TARGET_COMPONENT = "timeline.clip_drop_row:{}:0".format(PARENT_COMPOSITION)


def composition(project, composition_id):
    return next(
        item for item in project["compositions"] if item["id"] == composition_id
    )


def run_suite(client):
    health = client.wait_health()
    initial = client.state()
    initial_project = initial["project"]
    if initial["editor"]["navigation"]["active_composition_id"] != PARENT_COMPOSITION:
        raise QaFailure("composition-drop fixture did not activate the parent Composition")
    if initial_project["tracks"][PARENT_TRACK]["clip_ids"]:
        raise QaFailure("composition-drop fixture parent Track is not empty")
    if composition(initial_project, PARENT_COMPOSITION)["track_ids"] != [PARENT_TRACK]:
        raise QaFailure("parent Composition containment is not canonical")
    if composition(initial_project, SOURCE_COMPOSITION)["track_ids"] != [SOURCE_TRACK]:
        raise QaFailure("source Composition containment is not canonical")

    client.wait_component_settled(SOURCE_COMPONENT)
    client.wait_component_settled(TARGET_COMPONENT)
    snapshot = client.component_snapshot()
    components = {item["id"]: item for item in snapshot["components"]}
    try:
        source = components[SOURCE_COMPONENT]
        target = components[TARGET_COMPONENT]
    except KeyError as error:
        raise QaFailure(
            "composition drag component {} absent in frame {}".format(
                error, snapshot["frame"]
            )
        ) from error
    metadata = target.get("metadata") or {}
    if not (
        metadata.get("accepts_composition") is True
        and metadata.get("target_track_id") == PARENT_TRACK
        and metadata.get("composition_id") == PARENT_COMPOSITION
    ):
        raise QaFailure("Timeline drop row omitted canonical target metadata")
    source_metadata = source.get("metadata") or {}
    if not (
        source_metadata.get("composition_id") == SOURCE_COMPOSITION
        and source_metadata.get("draggable_to_timeline") is True
        and source_metadata.get("active") is False
    ):
        raise QaFailure("source Composition row omitted draggable metadata")

    start = client.point(source["rect_points"])
    # The fixture starts at 50 points/second. An intentional interior point
    # one second into the registered row proves that Timeline hit testing,
    # rather than a direct model command, chooses placement time and Track.
    end = {
        "x": target["rect_points"]["min_x"] + 50.0,
        "y": target["rect_points"]["center_y"],
    }
    if end["x"] >= target["rect_points"]["max_x"]:
        raise QaFailure("Timeline drop row is too narrow for a one-second placement")
    client.inject(
        "drag",
        {
            "from": start,
            "to": end,
            "coordinate_space": "points",
            "button": "primary",
            "steps": 16,
        },
        {
            "source_component_id": SOURCE_COMPONENT,
            "target_component_id": TARGET_COMPONENT,
            "component_frame": snapshot["frame"],
            "source_rect_points": source["rect_points"],
            "target_rect_points": target["rect_points"],
            "expected_start_time": 1.0,
        },
    )

    placed = client.wait_project(
        "Composition Instance Clip placement",
        lambda project: len(project["tracks"][PARENT_TRACK]["clip_ids"]) == 1,
    )
    BASE.assert_history_delta(initial, placed, 1, "Composition coordinate drag")
    project = placed["project"]
    clip_id = project["tracks"][PARENT_TRACK]["clip_ids"][0]
    clip = project["clips"][clip_id]
    if abs(float(clip["start_time"]) - 1.0) > 1e-6:
        raise QaFailure("coordinate drag did not author the expected Clip start time")
    if abs(float(clip["duration"]) - 3.0) > 1e-6:
        raise QaFailure("Composition Instance Clip did not inherit source duration")
    node_id = clip["output_node_id"]
    if clip["node_ids"] != [node_id]:
        raise QaFailure("Composition placement did not create one contained instance Node")
    node = project["nodes"][node_id]
    content = node["content"]
    if not (
        content.get("type") == "Reference"
        and content.get("data", {}).get("target_id") == SOURCE_COMPOSITION
    ):
        raise QaFailure("placed Clip does not reference the source Composition")

    # A placement creates only a Clip and an instance Node. The reusable
    # Composition remains a top-level definition with its own Track tree.
    if composition(project, PARENT_COMPOSITION)["track_ids"] != [PARENT_TRACK]:
        raise QaFailure("placement changed parent Composition containment")
    if composition(project, SOURCE_COMPOSITION)["track_ids"] != [SOURCE_TRACK]:
        raise QaFailure("placement reparented the source Composition into the Track")
    if placed["editor"]["navigation"]["active_composition_id"] != PARENT_COMPOSITION:
        raise QaFailure("dragging a source navigated away from the edited Composition")
    if placed["editor"]["preview"]["modal_error"] is not None:
        raise QaFailure("Composition placement surfaced a modal error")

    restored = BASE.undo_project_edit(
        client,
        "Composition coordinate drag",
        lambda candidate: candidate == initial_project,
    )
    return {
        "ok": True,
        "suite": "composition-drop",
        "health": health,
        "initial_frame": initial["frame"],
        "placed_frame": placed["frame"],
        "final_frame": restored["frame"],
        "clip_id": clip_id,
        "instance_node_id": node_id,
        "source_composition_id": SOURCE_COMPOSITION,
        "parent_composition_id": PARENT_COMPOSITION,
        "parent_track_id": PARENT_TRACK,
        "authored_start_time": clip["start_time"],
        "history_after_undo": restored["history"],
        "actions": client.evidence,
    }


def parse_args():
    parser = argparse.ArgumentParser()
    parser.add_argument("--base-url", default=None)
    parser.add_argument("--spawn", action="store_true")
    parser.add_argument(
        "--evidence", default="target/qa-composition-drop-evidence.json"
    )
    parser.add_argument("--timeout", type=float, default=30.0)
    return parser.parse_args()


def main():
    args = parse_args()
    process = None
    port = BASE.free_port() if args.spawn else 39091
    base_url = args.base_url or "http://127.0.0.1:{}".format(port)
    try:
        if args.spawn:
            environment = os.environ.copy()
            environment["RUVIE_QA_PORT"] = str(port)
            environment["RUVIE_QA_FIXTURE"] = "composition_drop_e2e"
            process = subprocess.Popen(
                ["cargo", "run", "-p", "app", "--locked"],
                env=environment,
                start_new_session=True,
            )
        client = QaClient(base_url, args.timeout)
        result = run_suite(client)
        result["run_id"] = os.environ.get("RUVIE_QA_RUN_ID")
        result["git_commit"] = BASE.repository_git_commit()
        result["component_frame"] = client.component_snapshot()["frame"]
        result["action_count"] = len(result["actions"])
        evidence_path = os.path.abspath(args.evidence)
        os.makedirs(os.path.dirname(evidence_path), exist_ok=True)
        with open(evidence_path, "w", encoding="utf-8") as output:
            json.dump(result, output, ensure_ascii=False, indent=2)
            output.write("\n")
        print("[qa-composition-drop-e2e] PASS; evidence: {}".format(evidence_path))
        return 0
    except (QaFailure, AssertionError, KeyError, IndexError, StopIteration) as error:
        print("[qa-composition-drop-e2e] FAIL: {}".format(error), file=sys.stderr)
        return 1
    finally:
        if process is not None:
            if process.poll() is None:
                try:
                    os.killpg(process.pid, signal.SIGTERM)
                except ProcessLookupError:
                    pass
            try:
                process.wait(timeout=5.0)
            except subprocess.TimeoutExpired:
                try:
                    os.killpg(process.pid, signal.SIGKILL)
                except ProcessLookupError:
                    pass
                process.wait(timeout=2.0)


if __name__ == "__main__":
    sys.exit(main())
