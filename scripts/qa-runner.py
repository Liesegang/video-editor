#!/usr/bin/env python3
"""Build RuViE once and run production authoring UI suites in isolation."""

from __future__ import annotations

import argparse
import concurrent.futures
import dataclasses
import datetime as dt
import hashlib
import json
import os
import pathlib
import subprocess
import sys
import threading
import time
import urllib.request
import uuid

from qa_support import (
    AUTHORING_AUDIO_FIXTURE,
    AUTHORING_FIXTURE,
    AUTHORING_PATH_FIXTURE,
    QaClient,
    REPOSITORY_ROOT,
    process_group_options,
    terminate_process,
)


SCRIPTS_DIR = pathlib.Path(__file__).resolve().parent
SUITE_OUTPUT_NAMES = (
    "qa-endpoint.json",
    "evidence.json",
    "capture.png",
    "capture.json",
    "failure-state.json",
    "failure-components.json",
    "project.ruvie",
)


@dataclasses.dataclass(frozen=True)
class SuiteSpec:
    name: str
    script: str
    fixture: str = AUTHORING_FIXTURE
    project_file: bool = False
    expects_exit: bool = False


FULL_SUITES = (
    SuiteSpec("smoke", "qa-smoke-e2e.py"),
    SuiteSpec(
        "unsaved-changes",
        "qa-unsaved-changes-e2e.py",
        project_file=True,
        expects_exit=True,
    ),
    SuiteSpec("assets-timeline", "qa-assets-timeline-e2e.py"),
    SuiteSpec("inspector-asset-preview", "qa-inspector-asset-preview-e2e.py"),
    SuiteSpec("timeline-edit", "qa-timeline-edit-e2e.py"),
    SuiteSpec(
        "timeline-content-zoom",
        "qa-timeline-content-zoom-e2e.py",
        AUTHORING_AUDIO_FIXTURE,
    ),
    SuiteSpec("preview", "qa-preview-authoring-e2e.py"),
    SuiteSpec("path-editor", "qa-path-editor-e2e.py", AUTHORING_PATH_FIXTURE),
    SuiteSpec("inspector-property-mode", "qa-inspector-property-mode-e2e.py"),
    SuiteSpec("inspector-effects", "qa-inspector-effects-e2e.py"),
    SuiteSpec("timeline-dopesheet", "qa-timeline-dopesheet-e2e.py"),
    SuiteSpec("curve-editor", "qa-curve-editor-e2e.py"),
    SuiteSpec("node-editor", "qa-node-editor-e2e.py"),
    SuiteSpec("node-clip-conversion", "qa-node-clip-conversion-e2e.py"),
    SuiteSpec("audio-playback", "qa-audio-playback-e2e.py", AUTHORING_AUDIO_FIXTURE),
    SuiteSpec("text-ensemble", "qa-text-ensemble-e2e.py"),
)


def suite_specs(mode: str) -> tuple[SuiteSpec, ...]:
    if mode == "smoke":
        return FULL_SUITES[:1]
    if mode == "full":
        return FULL_SUITES
    raise ValueError("unknown QA mode: {}".format(mode))


def prepare_suite_directory(suite_dir: pathlib.Path) -> None:
    suite_dir.mkdir(parents=True, exist_ok=True)
    for name in SUITE_OUTPUT_NAMES:
        try:
            (suite_dir / name).unlink()
        except FileNotFoundError:
            pass


def evidence_matches_run(evidence, run_identity: str, fixture: str = AUTHORING_FIXTURE) -> bool:
    return (
        isinstance(evidence, dict)
        and evidence.get("ok") is True
        and evidence.get("run_id") == run_identity
        and evidence.get("fixture") == fixture
    )


def parse_published_endpoint(contents: str) -> tuple[str, int]:
    endpoint = json.loads(contents)
    host = endpoint.get("host")
    port = endpoint.get("port")
    if host != "127.0.0.1" or not isinstance(port, int) or not 0 < port < 65536:
        raise ValueError("invalid loopback QA endpoint: {!r}".format(endpoint))
    return host, port


def wait_for_published_endpoint(
    path: pathlib.Path, process: subprocess.Popen, timeout: float
) -> tuple[str, int]:
    deadline = time.monotonic() + timeout
    last_error = None
    while time.monotonic() < deadline:
        if process.poll() is not None:
            raise RuntimeError(
                "app exited with status {} before publishing its QA port".format(
                    process.returncode
                )
            )
        try:
            return parse_published_endpoint(path.read_text(encoding="utf-8"))
        except (OSError, ValueError, json.JSONDecodeError) as error:
            last_error = error
        time.sleep(0.02)
    raise RuntimeError("QA endpoint publication timeout: {}".format(last_error))


def capture_viewport(client: QaClient, path: pathlib.Path, timeout: float = 15.0) -> dict:
    queued = client.request("/v1/captures", method="POST")
    capture_id = queued["capture_id"]
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        status = client.request("/v1/captures/{}".format(capture_id))
        if status.get("phase") == "ready":
            break
        if status.get("phase") == "failed":
            raise RuntimeError("capture failed: {}".format(status.get("error")))
        time.sleep(0.05)
    else:
        raise RuntimeError("capture {} timed out".format(capture_id))
    with urllib.request.urlopen(
        client.base_url + "/v1/captures/{}.png".format(capture_id), timeout=5.0
    ) as response:
        png = response.read()
    digest = hashlib.sha256(png).hexdigest()
    if digest != status.get("sha256"):
        raise RuntimeError("capture SHA-256 mismatch")
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_bytes(png)
    result = dict(status)
    result["path"] = str(path.resolve())
    return result


def capture_file_matches_metadata(path: pathlib.Path, metadata) -> bool:
    return (
        path.is_file()
        and isinstance(metadata, dict)
        and metadata.get("phase") == "ready"
        and hashlib.sha256(path.read_bytes()).hexdigest() == metadata.get("sha256")
    )


def default_app_binary() -> pathlib.Path:
    configured = os.environ.get("CARGO_TARGET_DIR")
    target = pathlib.Path(configured) if configured else REPOSITORY_ROOT / "target"
    if not target.is_absolute():
        target = REPOSITORY_ROOT / target
    return target / "debug" / ("app.exe" if os.name == "nt" else "app")


class ProcessRegistry:
    def __init__(self):
        self._lock = threading.Lock()
        self._processes: set[subprocess.Popen] = set()

    def add(self, process: subprocess.Popen) -> None:
        with self._lock:
            self._processes.add(process)

    def discard(self, process: subprocess.Popen) -> None:
        with self._lock:
            self._processes.discard(process)

    def terminate_all(self) -> None:
        with self._lock:
            processes = tuple(self._processes)
        for process in processes:
            terminate_process(process)


def write_json(path: pathlib.Path, value) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(
        json.dumps(value, ensure_ascii=False, indent=2) + "\n", encoding="utf-8"
    )


def read_json(path: pathlib.Path):
    try:
        return json.loads(path.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError):
        return None


def run_logged(
    command: list[str],
    log_path: pathlib.Path,
    timeout: float,
    registry: ProcessRegistry,
    environment=None,
) -> tuple[int, bool, float]:
    started = time.monotonic()
    log_path.parent.mkdir(parents=True, exist_ok=True)
    with log_path.open("w", encoding="utf-8") as output:
        output.write("$ {}\n".format(" ".join(command)))
        output.flush()
        process = subprocess.Popen(
            command,
            cwd=REPOSITORY_ROOT,
            env=environment,
            stdout=output,
            stderr=subprocess.STDOUT,
            **process_group_options(),
        )
        registry.add(process)
        timed_out = False
        try:
            try:
                code = process.wait(timeout=timeout)
            except subprocess.TimeoutExpired:
                timed_out = True
                terminate_process(process)
                code = 124
        finally:
            registry.discard(process)
    return code, timed_out, time.monotonic() - started


def collect_failure(client: QaClient, suite_dir: pathlib.Path) -> dict:
    artifacts = {}
    for name, request in (("state", client.state), ("components", client.component_snapshot)):
        try:
            path = suite_dir / "failure-{}.json".format(name)
            write_json(path, request())
            artifacts[name] = str(path.resolve())
        except Exception as error:
            artifacts[name + "_error"] = str(error)
    return artifacts


def run_one_suite(
    spec: SuiteSpec,
    app_binary: pathlib.Path,
    artifact_root: pathlib.Path,
    suite_timeout: float,
    health_timeout: float,
    registry: ProcessRegistry,
) -> dict:
    suite_dir = artifact_root / spec.name
    prepare_suite_directory(suite_dir)
    endpoint_path = suite_dir / "qa-endpoint.json"
    evidence_path = suite_dir / "evidence.json"
    capture_path = suite_dir / "capture.png"
    run_identity = "{}:{}".format(spec.name, uuid.uuid4().hex)
    environment = os.environ.copy()
    environment.update(
        {
            "RUVIE_QA_PORT": "0",
            "RUVIE_QA_PORT_FILE": str(endpoint_path.resolve()),
            "RUVIE_QA_FIXTURE": spec.fixture,
            "RUVIE_QA_RUN_ID": run_identity,
            "RUVIE_QA_ARTIFACT_DIR": str(suite_dir.resolve()),
        }
    )
    if spec.project_file:
        environment["RUVIE_QA_PROJECT_PATH"] = str((suite_dir / "project.ruvie").resolve())
    started = time.monotonic()
    result = {
        "name": spec.name,
        "ok": False,
        "fixture": spec.fixture,
        "run_id": run_identity,
        "suite_log": str((suite_dir / "suite.log").resolve()),
        "app_log": str((suite_dir / "app.log").resolve()),
        "evidence": str(evidence_path.resolve()),
        "capture": str(capture_path.resolve()),
    }
    app_process = None
    client = None
    with (suite_dir / "app.log").open("w", encoding="utf-8") as app_log:
        try:
            app_process = subprocess.Popen(
                [str(app_binary)],
                cwd=REPOSITORY_ROOT,
                env=environment,
                stdout=app_log,
                stderr=subprocess.STDOUT,
                **process_group_options(),
            )
            registry.add(app_process)
            host, port = wait_for_published_endpoint(
                endpoint_path, app_process, health_timeout
            )
            result["port"] = port
            client = QaClient("http://{}:{}".format(host, port), health_timeout)
            result["health"] = client.wait_health()
            command = [
                sys.executable,
                str(SCRIPTS_DIR / spec.script),
                "--base-url",
                client.base_url,
                "--timeout",
                str(min(suite_timeout, 60.0)),
                "--evidence",
                str(evidence_path),
            ]
            code, timed_out, duration = run_logged(
                command, suite_dir / "suite.log", suite_timeout, registry, environment
            )
            result.update(
                {"return_code": code, "timed_out": timed_out, "suite_seconds": duration}
            )
            evidence = read_json(evidence_path)
            result["evidence_ok"] = evidence_matches_run(evidence, run_identity, spec.fixture)
            if spec.expects_exit:
                exit_wait_started = time.monotonic()
                try:
                    app_exit_code = app_process.wait(timeout=health_timeout)
                except subprocess.TimeoutExpired:
                    app_exit_code = None
                capture = read_json(suite_dir / "capture.json")
                result["app_exit_code"] = app_exit_code
                result["app_exit_seconds_after_suite"] = round(
                    time.monotonic() - exit_wait_started, 3
                )
                result["capture_metadata"] = capture
                result["capture_ok"] = capture_file_matches_metadata(capture_path, capture)
                result["ok"] = (
                    code == 0
                    and not timed_out
                    and result["evidence_ok"]
                    and app_exit_code == 0
                    and result["capture_ok"]
                )
            else:
                capture = capture_viewport(client, capture_path)
                write_json(suite_dir / "capture.json", capture)
                result["capture_metadata"] = capture
                result["ok"] = code == 0 and not timed_out and result["evidence_ok"]
            if not result["ok"]:
                result["error"] = "suite or current-run evidence validation failed"
                result["diagnostics"] = collect_failure(client, suite_dir)
        except Exception as error:
            result["error"] = str(error)
            if client is not None:
                result["diagnostics"] = collect_failure(client, suite_dir)
        finally:
            if app_process is not None:
                terminate_process(app_process)
                registry.discard(app_process)
    result["duration_seconds"] = round(time.monotonic() - started, 3)
    return result


def aggregate_ok(results: list[dict]) -> bool:
    return bool(results) and all(result.get("ok") is True for result in results)


def write_summary(
    artifact_root: pathlib.Path,
    mode: str,
    build: dict,
    results: list[dict],
    jobs_requested: int,
    suite_wall_seconds: float,
) -> dict:
    results = sorted(results, key=lambda result: result["name"])
    summary = {
        "ok": build.get("ok") is True and aggregate_ok(results),
        "mode": mode,
        "fixture": AUTHORING_FIXTURE,
        "artifact_root": str(artifact_root.resolve()),
        "build": build,
        "jobs_requested": jobs_requested,
        "jobs_used": min(jobs_requested, len(results)),
        "suite_wall_seconds": round(suite_wall_seconds, 3),
        "sum_suite_seconds": round(
            sum(result.get("duration_seconds", 0.0) for result in results), 3
        ),
        "suites": results,
    }
    write_json(artifact_root / "summary.json", summary)
    lines = [
        "QA mode: {}".format(mode),
        "Fixture: {}".format(AUTHORING_FIXTURE),
        "Build: {}".format("PASS" if build.get("ok") else "FAIL"),
    ]
    for result in results:
        line = "{}: {} ({:.3f}s)".format(
            result["name"],
            "PASS" if result.get("ok") else "FAIL",
            result.get("duration_seconds", 0.0),
        )
        if result.get("error"):
            line += " - " + result["error"]
        lines.append(line)
    lines.append("All suites passed." if summary["ok"] else "One or more suites failed.")
    (artifact_root / "summary.txt").write_text("\n".join(lines) + "\n", encoding="utf-8")
    return summary


def positive_jobs(value: str) -> int:
    jobs = int(value)
    if jobs < 1:
        raise argparse.ArgumentTypeError("jobs must be at least 1")
    return jobs


def parse_args(argv=None):
    parser = argparse.ArgumentParser()
    parser.add_argument("--mode", choices=("smoke", "full"), default="smoke")
    parser.add_argument("--artifact-dir", default=None)
    parser.add_argument("--app-binary", default=None)
    parser.add_argument("--no-build", action="store_true")
    parser.add_argument("--build-timeout", type=float, default=900.0)
    parser.add_argument("--health-timeout", type=float, default=45.0)
    parser.add_argument("--suite-timeout", type=float, default=90.0)
    parser.add_argument("--jobs", type=positive_jobs, default=4)
    parser.add_argument("--self-test", action="store_true")
    return parser.parse_args(argv)


def run_self_test() -> int:
    return subprocess.call(
        [sys.executable, "-m", "unittest", "scripts/test_qa_runner.py"],
        cwd=REPOSITORY_ROOT,
    )


def main(argv=None) -> int:
    args = parse_args(argv)
    if args.self_test:
        return run_self_test()
    run_id = "{}-{}-{}".format(
        dt.datetime.now(dt.timezone.utc).strftime("%Y%m%dT%H%M%SZ"),
        args.mode,
        os.getpid(),
    )
    artifact_root = pathlib.Path(
        args.artifact_dir or REPOSITORY_ROOT / "target" / "qa-runs" / run_id
    )
    if not artifact_root.is_absolute():
        artifact_root = REPOSITORY_ROOT / artifact_root
    artifact_root.mkdir(parents=True, exist_ok=True)
    app_binary = pathlib.Path(args.app_binary) if args.app_binary else default_app_binary()
    if not app_binary.is_absolute():
        app_binary = REPOSITORY_ROOT / app_binary
    registry = ProcessRegistry()
    build = {
        "ok": False,
        "skipped": args.no_build,
        "binary": str(app_binary.resolve()),
        "log": str((artifact_root / "build.log").resolve()),
    }
    try:
        if args.no_build:
            if not app_binary.is_file():
                raise RuntimeError("app binary is missing: {}".format(app_binary))
            build.update({"ok": True, "return_code": None, "duration_seconds": 0.0})
        else:
            code, timed_out, duration = run_logged(
                ["cargo", "build", "-p", "app", "--locked"],
                artifact_root / "build.log",
                args.build_timeout,
                registry,
            )
            build.update(
                {
                    "ok": code == 0 and not timed_out and app_binary.is_file(),
                    "return_code": code,
                    "timed_out": timed_out,
                    "duration_seconds": round(duration, 3),
                }
            )
        if not build["ok"]:
            summary = write_summary(artifact_root, args.mode, build, [], args.jobs, 0.0)
            print((artifact_root / "summary.txt").read_text(encoding="utf-8"))
            return 1

        specs = suite_specs(args.mode)
        started = time.monotonic()
        with concurrent.futures.ThreadPoolExecutor(
            max_workers=min(args.jobs, len(specs))
        ) as executor:
            futures = [
                executor.submit(
                    run_one_suite,
                    spec,
                    app_binary,
                    artifact_root,
                    args.suite_timeout,
                    args.health_timeout,
                    registry,
                )
                for spec in specs
            ]
            results = [future.result() for future in futures]
        summary = write_summary(
            artifact_root,
            args.mode,
            build,
            results,
            args.jobs,
            time.monotonic() - started,
        )
        print((artifact_root / "summary.txt").read_text(encoding="utf-8"), end="")
        return 0 if summary["ok"] else 1
    finally:
        registry.terminate_all()


if __name__ == "__main__":
    raise SystemExit(main())
