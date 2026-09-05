#!/usr/bin/env python3
"""Prove Particle authoring survives a real save, exit, and process restart."""

from __future__ import annotations

import argparse
import hashlib
import importlib.util
import json
import pathlib
import subprocess
import sys
import time
import uuid

from qa_support import (
    AUTHORING_FIXTURE,
    QaClient,
    QaFailure,
    activate_dock_tab,
    free_port,
    media_seconds,
    repository_git_commit,
    seek_timeline_seconds,
    spawned_authoring_app,
    wait_endpoint_closed,
)


SCRIPTS_DIRECTORY = pathlib.Path(__file__).resolve().parent
PARTICLE_QA_FILENAME = "qa-particle-node-clip-e2e.py"
PARTICLE_QA_MODULE = "ruvie_qa_particle"
OPEN_EXISTING_PROJECT_ENV = "RUVIE_QA_OPEN_EXISTING_PROJECT"


def _load_particle_qa():
    """Load the hyphenated production suite once under one deliberate name."""

    path = (SCRIPTS_DIRECTORY / PARTICLE_QA_FILENAME).resolve()
    existing = sys.modules.get(PARTICLE_QA_MODULE)
    if existing is not None:
        existing_path = pathlib.Path(existing.__file__).resolve()
        if existing_path != path:
            raise RuntimeError(
                "{} already refers to {} instead of {}".format(
                    PARTICLE_QA_MODULE, existing_path, path
                )
            )
        return existing

    spec = importlib.util.spec_from_file_location(PARTICLE_QA_MODULE, path)
    if spec is None or spec.loader is None:
        raise RuntimeError("cannot load " + str(path))
    module = importlib.util.module_from_spec(spec)
    sys.modules[PARTICLE_QA_MODULE] = module
    try:
        spec.loader.exec_module(module)
    except BaseException:
        sys.modules.pop(PARTICLE_QA_MODULE, None)
        raise
    return module


PARTICLE_QA = _load_particle_qa()


def _file_evidence(path: pathlib.Path) -> dict:
    content = path.read_bytes()
    if not content:
        raise QaFailure("saved Particle Project is empty")
    return {
        "path": str(path),
        "bytes": len(content),
        "sha256": hashlib.sha256(content).hexdigest(),
    }


def _command_save(client: QaClient) -> None:
    client.key("s", True, command=True)
    client.key("s", False, command=True)


def _close_clean_native_app(
    client: QaClient, process: subprocess.Popen, label: str, timeout: float
) -> dict:
    started = time.monotonic()
    queued = client.request("/v1/input/close-request", {}, method="POST")
    if queued.get("queued") is not True or queued.get("action_id") is None:
        raise QaFailure("{} native close request was not queued".format(label))
    wait_endpoint_closed(client, timeout=timeout, description=label)
    try:
        exit_code = process.wait(timeout=min(timeout, 10.0))
    except subprocess.TimeoutExpired as error:
        raise QaFailure("{} endpoint closed but its process did not exit".format(label)) from error
    if exit_code != 0:
        raise QaFailure("{} exited with code {}".format(label, exit_code))
    return {
        "action_id": queued["action_id"],
        "exit_code": exit_code,
        "seconds": time.monotonic() - started,
    }


def _preview_at_active_particle_frame(client: QaClient, item: dict, revision: int):
    activate_dock_tab(
        client,
        PARTICLE_QA.TIMELINE_TAB_ID,
        "Timeline",
        "Particle persistence Timeline",
    )
    start_seconds = media_seconds(item["interval"]["start"])
    active_seconds = start_seconds + 0.2
    sought = seek_timeline_seconds(client, active_seconds)
    frame = sought["editor"]["timeline"]["current_frame"]
    rendered = PARTICLE_QA._wait_particle_preview(client, revision, frame)
    return active_seconds, frame, rendered


def _first_process(project_path: pathlib.Path, timeout: float, run_id: str) -> dict:
    port = free_port()
    environment = {
        "RUVIE_QA_FIXTURE": AUTHORING_FIXTURE,
        "RUVIE_QA_PROJECT_PATH": str(project_path),
        OPEN_EXISTING_PROJECT_ENV: None,
        "RUVIE_QA_PORT_FILE": None,
        "RUVIE_QA_RUN_ID": run_id + ":author",
    }
    with spawned_authoring_app(port, environment) as process:
        client = QaClient("http://127.0.0.1:{}".format(port), timeout)
        client.wait_health()
        initial_file = _file_evidence(project_path)

        # This is the authoritative production interaction path: Assets drag,
        # typed Inspector editing, Undo/Redo, Preview, and production Node Editor.
        authoring = PARTICLE_QA.run_suite(client)
        state = client.state()
        project = state["project"]
        item_id = authoring["created_item"]["id"]
        instance_id = authoring["created_instance_id"]
        definition_id = authoring["created_definition_id"]
        item = project["items"][item_id]
        active_seconds, frame, rendered = _preview_at_active_particle_frame(
            client, item, state["history"]["revision"]
        )
        preview = rendered["editor"]["preview"]

        _command_save(client)

        def saved_to_disk():
            saved_state = client.state()
            if saved_state["editor"].get("status") != "Project saved":
                return None
            file_state = _file_evidence(project_path)
            if file_state["sha256"] == initial_file["sha256"]:
                return None
            return saved_state, file_state

        saved_state, saved_file = client.wait_until(
            "Particle Project save reaching disk", saved_to_disk, timeout=timeout
        )
        saved_project = saved_state["project"]
        close = _close_clean_native_app(client, process, "saved Particle app", timeout)

    return {
        "authoring": authoring,
        "item_id": item_id,
        "instance_id": instance_id,
        "definition_id": definition_id,
        "parameter_id": authoring["parameter_edit"]["parameter_id"],
        "override": authoring["parameter_edit"]["override"],
        "item": saved_project["items"][item_id],
        "instance": saved_project["module_instances"][instance_id],
        "definition": saved_project["module_definitions"][definition_id],
        "active_seconds": active_seconds,
        "frame": frame,
        "preview": preview,
        "initial_file": initial_file,
        "saved_file": saved_file,
        "close": close,
        "actions": client.evidence,
    }


def _second_process(
    project_path: pathlib.Path, timeout: float, run_id: str, expected: dict
) -> dict:
    port = free_port()
    environment = {
        "RUVIE_QA_FIXTURE": AUTHORING_FIXTURE,
        "RUVIE_QA_PROJECT_PATH": str(project_path),
        OPEN_EXISTING_PROJECT_ENV: "1",
        "RUVIE_QA_PORT_FILE": None,
        "RUVIE_QA_RUN_ID": run_id + ":reload",
    }
    with spawned_authoring_app(port, environment) as process:
        client = QaClient("http://127.0.0.1:{}".format(port), timeout)
        client.wait_health()
        loaded = client.state()
        project = loaded["project"]
        item_id = expected["item_id"]
        instance_id = expected["instance_id"]
        definition_id = expected["definition_id"]

        if project["items"].get(item_id) != expected["item"]:
            raise QaFailure("reloaded Particle Timeline Item differs from saved state")
        if project["module_instances"].get(instance_id) != expected["instance"]:
            raise QaFailure("reloaded Particle Module Instance differs from saved state")
        if project["module_definitions"].get(definition_id) != expected["definition"]:
            raise QaFailure("reloaded Particle Module Definition differs from saved state")
        overrides = project["module_instances"][instance_id].get(
            "parameter_overrides"
        ) or {}
        if overrides.get(expected["parameter_id"]) != expected["override"]:
            raise QaFailure("reloaded Particle Seed override differs from saved value")

        active_seconds, frame, rendered = _preview_at_active_particle_frame(
            client,
            project["items"][item_id],
            loaded["history"]["revision"],
        )
        preview = rendered["editor"]["preview"]
        if frame != expected["frame"] or active_seconds != expected["active_seconds"]:
            raise QaFailure("Particle reload comparison did not seek the exact saved frame")
        if preview.get("pixel_hash") != expected["preview"].get("pixel_hash"):
            raise QaFailure("reloaded Particle Preview differs at the exact saved frame")
        if preview.get("nontransparent_pixels") != expected["preview"].get(
            "nontransparent_pixels"
        ):
            raise QaFailure("reloaded Particle Preview coverage differs at the saved frame")
        if rendered["editor"].get("error") is not None:
            raise QaFailure(
                "reloaded Particle Project reported an error: {}".format(
                    rendered["editor"]["error"]
                )
            )

        close = _close_clean_native_app(client, process, "reloaded Particle app", timeout)

    file_after_reload = _file_evidence(project_path)
    if file_after_reload["sha256"] != expected["saved_file"]["sha256"]:
        raise QaFailure("opening and closing the saved Particle Project changed its file")
    return {
        "history": loaded["history"],
        "override": overrides[expected["parameter_id"]],
        "frame": frame,
        "preview": preview,
        "file": file_after_reload,
        "close": close,
        "actions": client.evidence,
    }


def run_lifecycle(project_path: pathlib.Path, timeout: float) -> dict:
    project_path = project_path.resolve()
    project_path.parent.mkdir(parents=True, exist_ok=True)
    run_id = "particle-persistence:{}".format(uuid.uuid4().hex)
    authored = _first_process(project_path, timeout, run_id)
    reloaded = _second_process(project_path, timeout, run_id, authored)
    return {
        "suite": "particle-persistence",
        "ok": True,
        "fixture": AUTHORING_FIXTURE,
        "run_id": run_id,
        "git_commit": repository_git_commit(),
        "project_path": str(project_path),
        "identity": {
            "item_id": authored["item_id"],
            "instance_id": authored["instance_id"],
            "definition_id": authored["definition_id"],
            "parameter_id": authored["parameter_id"],
        },
        "saved": {
            "override": authored["override"],
            "frame": authored["frame"],
            "preview": authored["preview"],
            "file": authored["saved_file"],
            "close": authored["close"],
            "actions": authored["actions"],
        },
        "reloaded": reloaded,
    }


def parse_args(argv=None):
    parser = argparse.ArgumentParser(
        description="Run Particle save, native exit, process restart, and reload QA"
    )
    parser.add_argument(
        "--project",
        default="target/qa-particle-persistence-e2e/project.ruvie",
    )
    parser.add_argument(
        "--evidence",
        default="target/qa-particle-persistence-e2e-evidence.json",
    )
    parser.add_argument("--timeout", type=float, default=60.0)
    return parser.parse_args(argv)


def main(argv=None) -> int:
    args = parse_args(argv)
    try:
        result = run_lifecycle(pathlib.Path(args.project), args.timeout)
        evidence_path = pathlib.Path(args.evidence).resolve()
        evidence_path.parent.mkdir(parents=True, exist_ok=True)
        evidence_path.write_text(
            json.dumps(result, ensure_ascii=False, indent=2) + "\n", encoding="utf-8"
        )
        print("[qa-particle-persistence-e2e] PASS; evidence: {}".format(evidence_path))
        return 0
    except (
        QaFailure,
        AssertionError,
        KeyError,
        IndexError,
        OSError,
        subprocess.SubprocessError,
        TypeError,
        ValueError,
    ) as error:
        print("[qa-particle-persistence-e2e] FAIL: {}".format(error), file=sys.stderr)
        return 1


if __name__ == "__main__":
    raise SystemExit(main())
