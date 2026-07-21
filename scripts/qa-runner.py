#!/usr/bin/env python3
"""Build RuViE once and run isolated coordinate QA suites in parallel."""

from __future__ import annotations

import argparse
import concurrent.futures
import dataclasses
import datetime as dt
import hashlib
import json
import os
import pathlib
import signal
import subprocess
import sys
import threading
import time
import urllib.error
import urllib.request
import uuid


REPOSITORY_ROOT = pathlib.Path(__file__).resolve().parent.parent
SCRIPTS_DIR = REPOSITORY_ROOT / "scripts"
FIXTURE_NAME = "node_editor_e2e"
SUITE_OUTPUT_NAMES = (
    "qa-endpoint.json",
    "evidence.json",
    "capture.png",
    "capture.json",
    "failure-state.json",
    "failure-components.json",
)

BLEND_MODE_NAMES = (
    "Normal",
    "Dissolve",
    "Behind",
    "Clear",
    "Darken",
    "Multiply",
    "ColorBurn",
    "LinearBurn",
    "DarkerColor",
    "Lighten",
    "Screen",
    "ColorDodge",
    "LinearDodge",
    "LighterColor",
    "Overlay",
    "SoftLight",
    "HardLight",
    "VividLight",
    "LinearLight",
    "PinLight",
    "HardMix",
    "Difference",
    "Exclusion",
    "Subtract",
    "Divide",
    "Hue",
    "Saturation",
    "Color",
    "Luminosity",
)
BLEND_PREVIEW_REPRESENTATIVES = (
    "Clear",
    "Multiply",
    "Screen",
    "Overlay",
    "Difference",
    "Hue",
    "Dissolve",
)


@dataclasses.dataclass(frozen=True)
class SuiteSpec:
    name: str
    script: str
    arguments: tuple[str, ...] = ()
    suite_owns_capture: bool = False
    fixture: str = FIXTURE_NAME


def blend_suite_specs() -> tuple[SuiteSpec, ...]:
    return tuple(
        SuiteSpec(
            "blend-modes-{}".format(shard),
            "qa-blend-modes-e2e.py",
            ("--shard", shard),
        )
        for shard in (
            "normal-darken",
            "lighten",
            "contrast",
            "comparative-hsl",
        )
    )


def prepare_suite_directory(suite_dir: pathlib.Path) -> None:
    """Remove evidence that could make a reused artifact directory pass stale."""
    suite_dir.mkdir(parents=True, exist_ok=True)
    for name in SUITE_OUTPUT_NAMES:
        try:
            (suite_dir / name).unlink()
        except FileNotFoundError:
            pass


def evidence_matches_run(evidence, run_identity: str) -> bool:
    return (
        isinstance(evidence, dict)
        and evidence.get("ok") is True
        and evidence.get("run_id") == run_identity
    )


def suite_specs(mode: str) -> tuple[SuiteSpec, ...]:
    if mode == "smoke":
        return (
            SuiteSpec(
                "smoke",
                "qa-e2e.py",
                ("--suite", "smoke"),
                suite_owns_capture=True,
            ),
        )
    if mode == "blend":
        return blend_suite_specs()
    if mode == "full":
        return (
            SuiteSpec("all", "qa-e2e.py", ("--suite", "all")),
            SuiteSpec("timeline", "qa-e2e.py", ("--suite", "timeline")),
            SuiteSpec("selection", "qa-e2e.py", ("--suite", "selection")),
            SuiteSpec("keyframe", "qa-keyframe-e2e.py"),
            SuiteSpec("node-editor", "qa-node-editor-e2e.py"),
            SuiteSpec("node-reparent", "qa-reparent-e2e.py"),
            SuiteSpec("merge-reorder", "qa-merge-reorder-e2e.py"),
            SuiteSpec(
                "container-output-hit", "qa-container-output-hit-e2e.py"
            ),
            *blend_suite_specs(),
            SuiteSpec(
                "composition-drop",
                "qa-composition-drop-e2e.py",
                fixture="composition_drop_e2e",
            ),
            SuiteSpec("node-wire", "qa-e2e.py", ("--suite", "node-wire")),
            SuiteSpec("node-wire-selection", "qa-wire-selection-e2e.py"),
            SuiteSpec("implicit-time", "qa-implicit-time-e2e.py"),
            SuiteSpec("preview", "qa-preview-e2e.py"),
            SuiteSpec("preview-trackpad", "qa-preview-trackpad-e2e.py"),
            SuiteSpec(
                "transform-preview",
                "qa-transform-preview-e2e.py",
                fixture="transform_preview_e2e",
            ),
        )
    raise ValueError("unknown QA mode: {}".format(mode))


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


def terminate_process_group(process: subprocess.Popen, grace_seconds: float = 5.0) -> None:
    if process.poll() is not None:
        return
    try:
        os.killpg(process.pid, signal.SIGTERM)
    except ProcessLookupError:
        return
    try:
        process.wait(timeout=grace_seconds)
        return
    except subprocess.TimeoutExpired:
        pass
    try:
        os.killpg(process.pid, signal.SIGKILL)
    except ProcessLookupError:
        return
    try:
        process.wait(timeout=2.0)
    except subprocess.TimeoutExpired:
        pass


class ProcessRegistry:
    def __init__(self) -> None:
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
            processes = list(self._processes)
        for process in processes:
            terminate_process_group(process)


def json_request(
    base_url: str,
    path: str,
    data=None,
    timeout: float = 3.0,
    method=None,
):
    body = None if data is None else json.dumps(data).encode("utf-8")
    request = urllib.request.Request(
        base_url + path,
        data=body,
        headers={"Content-Type": "application/json"},
        method=method,
    )
    with urllib.request.urlopen(request, timeout=timeout) as response:
        return json.load(response)


def wait_for_health(
    base_url: str, process: subprocess.Popen, timeout: float
) -> dict:
    deadline = time.monotonic() + timeout
    last_error = None
    while time.monotonic() < deadline:
        if process.poll() is not None:
            raise RuntimeError(
                "app exited with status {} before QA health became ready".format(
                    process.returncode
                )
            )
        try:
            health = json_request(base_url, "/health")
            if health.get("ok") is True:
                return health
        except (OSError, urllib.error.HTTPError, json.JSONDecodeError) as error:
            last_error = error
        time.sleep(0.05)
    raise RuntimeError("QA health timeout: {}".format(last_error))


def capture_viewport(base_url: str, path: pathlib.Path, timeout: float = 15.0) -> dict:
    queued = json_request(base_url, "/v1/captures", method="POST")
    capture_id = queued["capture_id"]
    deadline = time.monotonic() + timeout
    status = None
    while time.monotonic() < deadline:
        status = json_request(base_url, "/v1/captures/{}".format(capture_id))
        phase = status.get("phase")
        if phase == "ready":
            break
        if phase == "failed":
            raise RuntimeError(
                "capture {} failed: {}".format(capture_id, status.get("error"))
            )
        time.sleep(0.05)
    else:
        raise RuntimeError("capture {} timed out".format(capture_id))

    request = urllib.request.Request(
        base_url + "/v1/captures/{}.png".format(capture_id)
    )
    with urllib.request.urlopen(request, timeout=5.0) as response:
        png = response.read()
    actual_sha = hashlib.sha256(png).hexdigest()
    expected_sha = status.get("sha256")
    if actual_sha != expected_sha:
        raise RuntimeError(
            "capture SHA-256 mismatch: {} != {}".format(actual_sha, expected_sha)
        )
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_bytes(png)
    result = dict(status)
    result["path"] = str(path.resolve())
    return result


def default_app_binary() -> pathlib.Path:
    configured = os.environ.get("CARGO_TARGET_DIR")
    target_dir = pathlib.Path(configured) if configured else REPOSITORY_ROOT / "target"
    if not target_dir.is_absolute():
        target_dir = REPOSITORY_ROOT / target_dir
    return target_dir / "debug" / ("app.exe" if os.name == "nt" else "app")


def run_command_with_timeout(
    command: list[str],
    log_path: pathlib.Path,
    timeout: float,
    registry: ProcessRegistry,
    environment=None,
) -> tuple[int, bool, float]:
    log_path.parent.mkdir(parents=True, exist_ok=True)
    started = time.monotonic()
    with log_path.open("w", encoding="utf-8") as log:
        log.write("$ {}\n".format(" ".join(command)))
        log.flush()
        process = subprocess.Popen(
            command,
            cwd=REPOSITORY_ROOT,
            env=environment,
            stdout=log,
            stderr=subprocess.STDOUT,
            start_new_session=True,
        )
        registry.add(process)
        timed_out = False
        try:
            try:
                return_code = process.wait(timeout=timeout)
            except subprocess.TimeoutExpired:
                timed_out = True
                terminate_process_group(process)
                return_code = 124
        finally:
            registry.discard(process)
    return return_code, timed_out, time.monotonic() - started


def read_evidence(path: pathlib.Path):
    try:
        return json.loads(path.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError):
        return None


def write_json_artifact(path: pathlib.Path, value) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(
        json.dumps(value, ensure_ascii=False, indent=2) + "\n", encoding="utf-8"
    )


def collect_failure_diagnostics(
    base_url: str, suite_dir: pathlib.Path, capture_path: pathlib.Path
) -> dict:
    diagnostics = {}
    errors = {}
    for name, endpoint in (
        ("state", "/v1/state"),
        ("components", "/v1/components"),
    ):
        path = suite_dir / "failure-{}.json".format(name)
        try:
            write_json_artifact(path, json_request(base_url, endpoint))
            diagnostics[name] = str(path.resolve())
        except Exception as error:
            errors[name] = str(error)

    metadata_path = suite_dir / "capture.json"
    try:
        if capture_path.is_file() and metadata_path.is_file():
            metadata = json.loads(metadata_path.read_text(encoding="utf-8"))
        else:
            metadata = capture_viewport(base_url, capture_path)
            write_json_artifact(metadata_path, metadata)
        diagnostics["capture"] = str(capture_path.resolve())
        diagnostics["capture_metadata"] = str(metadata_path.resolve())
    except Exception as error:
        errors["capture"] = str(error)
    if errors:
        diagnostics["errors"] = errors
    return diagnostics


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
    suite_log = suite_dir / "suite.log"
    app_log = suite_dir / "app.log"
    evidence_path = suite_dir / "evidence.json"
    capture_path = suite_dir / "capture.png"
    endpoint_path = suite_dir / "qa-endpoint.json"
    run_identity = "{}:{}".format(spec.name, uuid.uuid4().hex)
    environment = os.environ.copy()
    environment["RUVIE_QA_PORT"] = "0"
    environment["RUVIE_QA_PORT_FILE"] = str(endpoint_path.resolve())
    environment["RUVIE_QA_FIXTURE"] = spec.fixture
    environment["RUVIE_QA_RUN_ID"] = run_identity
    started = time.monotonic()
    app_process = None
    suite_process = None
    base_url = None
    result = {
        "name": spec.name,
        "ok": False,
        "port": None,
        "endpoint_file": str(endpoint_path.resolve()),
        "suite_log": str(suite_log.resolve()),
        "app_log": str(app_log.resolve()),
        "evidence": str(evidence_path.resolve()),
        "capture": str(capture_path.resolve()),
        "run_id": run_identity,
    }

    with app_log.open("w", encoding="utf-8") as app_output:
        app_output.write("$ {}\n".format(app_binary))
        app_output.flush()
        try:
            app_process = subprocess.Popen(
                [str(app_binary)],
                cwd=REPOSITORY_ROOT,
                env=environment,
                stdout=app_output,
                stderr=subprocess.STDOUT,
                start_new_session=True,
            )
            registry.add(app_process)
            host, port = wait_for_published_endpoint(
                endpoint_path, app_process, health_timeout
            )
            base_url = "http://{}:{}".format(host, port)
            result["port"] = port
            health = wait_for_health(base_url, app_process, health_timeout)
            result["health"] = health

            command = [
                sys.executable,
                str(SCRIPTS_DIR / spec.script),
                "--base-url",
                base_url,
                "--timeout",
                str(min(60.0, suite_timeout)),
                "--evidence",
                str(evidence_path),
                *spec.arguments,
            ]
            if spec.suite_owns_capture:
                command.extend(("--capture", str(capture_path)))

            with suite_log.open("w", encoding="utf-8") as suite_output:
                suite_output.write("$ {}\n".format(" ".join(command)))
                suite_output.flush()
                suite_process = subprocess.Popen(
                    command,
                    cwd=REPOSITORY_ROOT,
                    env=environment,
                    stdout=suite_output,
                    stderr=subprocess.STDOUT,
                    start_new_session=True,
                )
                registry.add(suite_process)
                try:
                    suite_return_code = suite_process.wait(timeout=suite_timeout)
                    result["timed_out"] = False
                except subprocess.TimeoutExpired:
                    result["timed_out"] = True
                    terminate_process_group(suite_process)
                    suite_return_code = 124
                finally:
                    registry.discard(suite_process)
            result["return_code"] = suite_return_code

            capture_error = None
            try:
                if spec.suite_owns_capture and capture_path.is_file():
                    evidence = read_evidence(evidence_path) or {}
                    capture = evidence.get("capture")
                    if not isinstance(capture, dict):
                        raise RuntimeError("smoke evidence omitted capture metadata")
                    actual_sha = hashlib.sha256(capture_path.read_bytes()).hexdigest()
                    if actual_sha != capture.get("sha256"):
                        raise RuntimeError("smoke capture SHA-256 mismatch")
                    result["capture_metadata"] = capture
                else:
                    result["capture_metadata"] = capture_viewport(
                        base_url, capture_path
                    )
                write_json_artifact(
                    suite_dir / "capture.json", result["capture_metadata"]
                )
            except Exception as error:  # capture failure must be in the summary
                capture_error = str(error)
                result["capture_error"] = capture_error

            evidence = read_evidence(evidence_path)
            result["evidence_ok"] = evidence_matches_run(evidence, run_identity)
            result["ok"] = (
                suite_return_code == 0
                and not result.get("timed_out", False)
                and result["evidence_ok"]
                and capture_error is None
            )
            if not result["ok"] and "error" not in result:
                result["error"] = "suite, evidence, or capture validation failed"
        except Exception as error:
            result["error"] = str(error)
            result.setdefault("return_code", None)
            result.setdefault("timed_out", False)
            result.setdefault("evidence_ok", False)
        finally:
            if suite_process is not None:
                terminate_process_group(suite_process)
                registry.discard(suite_process)
            if (
                not result.get("ok", False)
                and base_url is not None
                and app_process is not None
                and app_process.poll() is None
            ):
                result["diagnostics"] = collect_failure_diagnostics(
                    base_url, suite_dir, capture_path
                )
            if app_process is not None:
                terminate_process_group(app_process)
                registry.discard(app_process)
            result["duration_seconds"] = round(time.monotonic() - started, 3)
    return result


def aggregate_ok(results: list[dict]) -> bool:
    return bool(results) and all(result.get("ok") is True for result in results)


def blend_catalog_validation(results: list[dict]) -> dict:
    blend_results = [
        result
        for result in results
        if result.get("name", "").startswith("blend-modes-")
    ]
    observed_modes = []
    preview_hashes = {}
    evidence_errors = []
    for result in blend_results:
        evidence = read_evidence(pathlib.Path(result["evidence"]))
        if not isinstance(evidence, dict):
            evidence_errors.append("{} omitted evidence".format(result["name"]))
            continue
        modes = evidence.get("modes")
        hashes = evidence.get("preview_hashes")
        if not isinstance(modes, list) or not all(
            isinstance(mode, str) for mode in modes
        ):
            evidence_errors.append(
                "{} has invalid modes evidence".format(result["name"])
            )
        else:
            observed_modes.extend(modes)
        if not isinstance(hashes, dict):
            evidence_errors.append(
                "{} has invalid preview hash evidence".format(result["name"])
            )
        else:
            preview_hashes.update(hashes)

    missing_modes = sorted(set(BLEND_MODE_NAMES) - set(observed_modes))
    unexpected_modes = sorted(set(observed_modes) - set(BLEND_MODE_NAMES))
    duplicate_modes = sorted(
        mode for mode in set(observed_modes) if observed_modes.count(mode) != 1
    )
    representative_hashes = {
        mode: preview_hashes.get(mode) for mode in BLEND_PREVIEW_REPRESENTATIVES
    }
    invalid_hashes = sorted(
        mode
        for mode, pixel_hash in representative_hashes.items()
        if type(pixel_hash) is not int
    )
    distinct_hashes = len(
        {
            pixel_hash
            for pixel_hash in representative_hashes.values()
            if type(pixel_hash) is int
        }
    )
    errors = list(evidence_errors)
    if missing_modes:
        errors.append("missing modes: {}".format(", ".join(missing_modes)))
    if unexpected_modes:
        errors.append("unexpected modes: {}".format(", ".join(unexpected_modes)))
    if duplicate_modes:
        errors.append("duplicate modes: {}".format(", ".join(duplicate_modes)))
    if invalid_hashes:
        errors.append(
            "missing or invalid representative preview hashes: {}".format(
                ", ".join(invalid_hashes)
            )
        )
    if distinct_hashes < 4:
        errors.append("representative modes produced fewer than four distinct previews")
    if (
        representative_hashes["Clear"] is not None
        and representative_hashes["Clear"] == representative_hashes["Dissolve"]
    ):
        errors.append("Clear and Dissolve produced the same preview")
    return {
        "ok": not errors,
        "expected_mode_count": len(BLEND_MODE_NAMES),
        "observed_mode_count": len(observed_modes),
        "representative_hashes": representative_hashes,
        "distinct_representative_hashes": distinct_hashes,
        "errors": errors,
    }


def suite_validations(mode: str, results: list[dict]) -> dict:
    if mode in ("blend", "full"):
        return {"blend_catalog": blend_catalog_validation(results)}
    return {}


def write_summary(
    artifact_root: pathlib.Path,
    mode: str,
    build: dict,
    results: list[dict],
    jobs_requested: int = 1,
    jobs_used=None,
    suite_wall_seconds=None,
    validations=None,
) -> dict:
    results = sorted(results, key=lambda item: item["name"])
    validations = {} if validations is None else validations
    jobs_used = min(jobs_requested, len(results)) if jobs_used is None else jobs_used
    sum_suite_seconds = round(
        sum(float(result.get("duration_seconds", 0.0)) for result in results), 3
    )
    if suite_wall_seconds is None:
        suite_wall_seconds = sum_suite_seconds
    suite_wall_seconds = round(suite_wall_seconds, 3)
    concurrency_factor = (
        round(sum_suite_seconds / suite_wall_seconds, 3)
        if suite_wall_seconds > 0.0
        else None
    )
    summary = {
        "ok": build.get("ok") is True
        and aggregate_ok(results)
        and all(validation.get("ok") is True for validation in validations.values()),
        "mode": mode,
        "artifact_root": str(artifact_root.resolve()),
        "build": build,
        "jobs_requested": jobs_requested,
        "jobs_used": jobs_used,
        "suite_wall_seconds": suite_wall_seconds,
        "sum_suite_seconds": sum_suite_seconds,
        "concurrency_factor": concurrency_factor,
        "concurrency_factor_basis": "sum_suite_seconds / suite_wall_seconds within this run",
        "validations": validations,
        "suites": results,
    }
    artifact_root.mkdir(parents=True, exist_ok=True)
    (artifact_root / "summary.json").write_text(
        json.dumps(summary, ensure_ascii=False, indent=2) + "\n", encoding="utf-8"
    )
    lines = [
        "QA mode: {}".format(mode),
        "Build: {}".format("PASS" if build.get("ok") else "FAIL"),
        "Suites: {:.3f}s wall / {:.3f}s summed, jobs {}/{}, concurrency {}".format(
            suite_wall_seconds,
            sum_suite_seconds,
            jobs_used,
            jobs_requested,
            "n/a" if concurrency_factor is None else "{:.3f}x".format(concurrency_factor),
        ),
    ]
    for result in results:
        line = "{}: {} ({:.3f}s)".format(
            result["name"],
            "PASS" if result.get("ok") else "FAIL",
            float(result.get("duration_seconds", 0.0)),
        )
        if result.get("error"):
            line += " - {}".format(result["error"])
        lines.append(line)
    for name, validation in sorted(validations.items()):
        lines.append(
            "{} validation: {}".format(
                name, "PASS" if validation.get("ok") is True else "FAIL"
            )
        )
        for error in validation.get("errors", []):
            lines.append("  - {}".format(error))
    failures = [item for item in results if not item.get("ok")]
    validation_failures = [
        name
        for name, validation in validations.items()
        if validation.get("ok") is not True
    ]
    if failures:
        lines.append("Failures:")
        for item in failures:
            lines.append("  {} -> {}".format(item["name"], item["suite_log"]))
            for artifact_name, artifact_path in item.get("diagnostics", {}).items():
                if artifact_name != "errors":
                    lines.append("    {}: {}".format(artifact_name, artifact_path))
            for artifact_name, error in item.get("diagnostics", {}).get(
                "errors", {}
            ).items():
                lines.append("    {} collection failed: {}".format(artifact_name, error))
    if not failures and not validation_failures:
        lines.append("All suites passed.")
    (artifact_root / "summary.txt").write_text("\n".join(lines) + "\n", encoding="utf-8")
    return summary


def positive_jobs(value):
    jobs = int(value)
    if jobs < 1:
        raise argparse.ArgumentTypeError("jobs must be at least 1")
    return jobs


def parse_args(argv=None):
    parser = argparse.ArgumentParser()
    parser.add_argument("--mode", choices=("smoke", "blend", "full"), default="smoke")
    parser.add_argument("--artifact-dir", default=None)
    parser.add_argument("--app-binary", default=None)
    parser.add_argument("--no-build", action="store_true")
    parser.add_argument("--build-timeout", type=float, default=900.0)
    parser.add_argument("--health-timeout", type=float, default=45.0)
    parser.add_argument("--suite-timeout", type=float, default=None)
    parser.add_argument("--jobs", type=positive_jobs, default=4)
    parser.add_argument("--self-test", action="store_true")
    return parser.parse_args(argv)


def run_self_test() -> int:
    command = [
        sys.executable,
        "-m",
        "unittest",
        "scripts/test_qa_runner.py",
        "scripts/test_qa_node_editor_e2e.py",
        "scripts/test_qa_container_output_hit_e2e.py",
        "scripts/test_qa_reparent_e2e.py",
        "scripts/test_qa_preview_e2e.py",
    ]
    return subprocess.call(command, cwd=REPOSITORY_ROOT)


def main(argv=None) -> int:
    args = parse_args(argv)
    if args.self_test:
        return run_self_test()

    run_id = "{}-{}-{}".format(
        dt.datetime.now(dt.timezone.utc).strftime("%Y%m%dT%H%M%SZ"),
        args.mode,
        os.getpid(),
    )
    artifact_root = (
        pathlib.Path(args.artifact_dir)
        if args.artifact_dir
        else REPOSITORY_ROOT / "target" / "qa-runs" / run_id
    )
    if not artifact_root.is_absolute():
        artifact_root = REPOSITORY_ROOT / artifact_root
    artifact_root.mkdir(parents=True, exist_ok=True)

    registry = ProcessRegistry()
    app_binary = pathlib.Path(args.app_binary) if args.app_binary else default_app_binary()
    if not app_binary.is_absolute():
        app_binary = REPOSITORY_ROOT / app_binary

    build = {
        "ok": False,
        "skipped": bool(args.no_build),
        "log": str((artifact_root / "build.log").resolve()),
        "binary": str(app_binary.resolve()),
    }
    try:
        if args.no_build:
            if not app_binary.is_file():
                raise RuntimeError("app binary is missing: {}".format(app_binary))
            build.update({"ok": True, "return_code": None, "duration_seconds": 0.0})
        else:
            code, timed_out, duration = run_command_with_timeout(
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
            summary = write_summary(
                artifact_root,
                args.mode,
                build,
                [],
                jobs_requested=args.jobs,
                jobs_used=0,
                suite_wall_seconds=0.0,
            )
            print(json.dumps(summary, ensure_ascii=False, indent=2))
            return 1

        specs = suite_specs(args.mode)
        suite_timeout = args.suite_timeout or (120.0 if args.mode == "smoke" else 480.0)
        jobs_used = min(args.jobs, len(specs))
        suites_started = time.monotonic()
        with concurrent.futures.ThreadPoolExecutor(
            max_workers=jobs_used
        ) as executor:
            futures = [
                executor.submit(
                    run_one_suite,
                    spec,
                    app_binary,
                    artifact_root,
                    suite_timeout,
                    args.health_timeout,
                    registry,
                )
                for spec in specs
            ]
            results = [future.result() for future in futures]
        suite_wall_seconds = time.monotonic() - suites_started
        validations = suite_validations(args.mode, results)
        summary = write_summary(
            artifact_root,
            args.mode,
            build,
            results,
            jobs_requested=args.jobs,
            jobs_used=jobs_used,
            suite_wall_seconds=suite_wall_seconds,
            validations=validations,
        )
        print((artifact_root / "summary.txt").read_text(encoding="utf-8"), end="")
        print("Artifacts: {}".format(artifact_root.resolve()))
        return 0 if summary["ok"] else 1
    except KeyboardInterrupt:
        registry.terminate_all()
        print("QA runner interrupted; all process groups were terminated", file=sys.stderr)
        return 130
    finally:
        registry.terminate_all()


if __name__ == "__main__":
    sys.exit(main())
