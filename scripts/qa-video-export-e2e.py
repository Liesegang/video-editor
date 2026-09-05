#!/usr/bin/env python3
"""Export a real Timeline through the native command and production worker."""

from __future__ import annotations

import hashlib
import json
import math
import os
import pathlib
import re
import struct
import subprocess

from qa_support import (
    AUTHORING_AUDIO_FIXTURE,
    QaFailure,
    REPOSITORY_ROOT,
    run_suite_main,
)


EXPORT_PATH_ENV = "RUVIE_QA_EXPORT_PATH"
ARTIFACT_DIRECTORY_ENV = "RUVIE_QA_ARTIFACT_DIR"
ERROR_LEVEL = re.compile(r"(?:^|[\[\s])ERROR(?:[\]\s:]|$)")
SUCCESS = re.compile(r"^Exported ([0-9]+) frames to (.+)$")
SENTINEL = b"RuViE native QA existing destination; replace only after success\n"
AUDIO_CODEC = "aac"
AUDIO_SAMPLE_RATE = 48_000
AUDIO_CHANNELS = 2


def _configured_output_path() -> pathlib.Path:
    configured = os.environ.get(EXPORT_PATH_ENV)
    if not configured:
        raise QaFailure(EXPORT_PATH_ENV + " is required")
    path = pathlib.Path(configured)
    if not path.is_absolute():
        path = REPOSITORY_ROOT / path
    path = path.resolve()
    if path.suffix.lower() != ".mp4":
        raise QaFailure("native video Export QA requires an .mp4 destination")
    path.parent.mkdir(parents=True, exist_ok=True)
    return path


def _directory_entries(directory: pathlib.Path) -> set[pathlib.Path]:
    return {entry.resolve() for entry in directory.iterdir()}


def _new_unexpected_siblings(
    before: set[pathlib.Path], after: set[pathlib.Path], output: pathlib.Path
) -> list[pathlib.Path]:
    final = output.resolve()
    return sorted((after - before) - {final}, key=lambda path: str(path))


def _rational(value: str, description: str) -> float:
    parts = value.split("/", 1)
    if len(parts) != 2:
        raise QaFailure("{} is not a rational: {!r}".format(description, value))
    numerator, denominator = (int(part) for part in parts)
    if denominator == 0:
        raise QaFailure(description + " has a zero denominator")
    return numerator / denominator


def _timeline_contract(state: dict) -> tuple[int, int, float, int, float]:
    timeline_id = state["editor"]["navigation"]["active_timeline_id"]
    timeline = state["project"]["timelines"][timeline_id]
    rate = timeline["fps"]
    duration = timeline["duration"]
    numerator = int(duration["value"]) * int(rate["numerator"])
    denominator = int(duration["timescale"]) * int(rate["denominator"])
    if numerator <= 0 or denominator <= 0:
        raise QaFailure("active Timeline has no positive export range")
    frame_count = (numerator + denominator - 1) // denominator
    return (
        int(timeline["width"]),
        int(timeline["height"]),
        int(rate["numerator"]) / int(rate["denominator"]),
        frame_count,
        int(duration["value"]) / int(duration["timescale"]),
    )


def _run_json(command: list[str], description: str) -> dict:
    try:
        completed = subprocess.run(command, capture_output=True, check=False)
    except OSError as error:
        raise QaFailure("{} could not start: {}".format(description, error)) from error
    if completed.returncode != 0:
        raise QaFailure(
            "{} failed with {}: {}".format(
                description,
                completed.returncode,
                completed.stderr.decode("utf-8", errors="replace"),
            )
        )
    try:
        return json.loads(completed.stdout)
    except json.JSONDecodeError as error:
        raise QaFailure(description + " returned invalid JSON") from error


def _probe(path: pathlib.Path) -> dict:
    return _run_json(
        [
            "ffprobe",
            "-v",
            "error",
            "-count_frames",
            "-show_streams",
            "-show_format",
            "-of",
            "json",
            str(path),
        ],
        "ffprobe",
    )


def _required_seconds(owner: dict, field: str, description: str) -> float:
    try:
        seconds = float(owner[field])
    except (KeyError, TypeError, ValueError) as error:
        raise QaFailure("{} has no numeric {}".format(description, field)) from error
    if not math.isfinite(seconds) or seconds < 0.0:
        raise QaFailure("{} has an invalid {}".format(description, field))
    return seconds


def _validate_timing(
    video: dict,
    audio: dict,
    container: dict,
    fps: float,
    frame_count: int,
    timeline_duration: float,
) -> dict:
    video_duration = _required_seconds(video, "duration", "video stream")
    audio_duration = _required_seconds(audio, "duration", "audio stream")
    container_duration = _required_seconds(container, "duration", "container")
    video_start = _required_seconds(video, "start_time", "video stream")
    audio_start = _required_seconds(audio, "start_time", "audio stream")
    expected_video_duration = frame_count / fps
    expected_container_duration = max(expected_video_duration, timeline_duration)
    # MP4/AAC duration can differ by one encoded audio packet. Video may differ
    # by one Timeline frame because the export range is rounded up to a frame.
    tolerance = max(1.0 / fps, 1024.0 / AUDIO_SAMPLE_RATE) + 0.002
    for description, actual, expected in (
        ("video stream", video_duration, expected_video_duration),
        ("audio stream", audio_duration, timeline_duration),
        ("container", container_duration, expected_container_duration),
    ):
        if not math.isclose(actual, expected, rel_tol=0.0, abs_tol=tolerance):
            raise QaFailure(
                "{} duration mismatch: expected {:.9f}s, got {:.9f}s".format(
                    description, expected, actual
                )
            )
    if abs(video_start) > tolerance or abs(audio_start) > tolerance:
        raise QaFailure("Export streams do not start at the Timeline origin")
    if abs(video_start - audio_start) > tolerance:
        raise QaFailure("Export Audio and Video are not synchronized at their start")
    return {
        "video_seconds": video_duration,
        "audio_seconds": audio_duration,
        "container_seconds": container_duration,
        "video_start_seconds": video_start,
        "audio_start_seconds": audio_start,
        "tolerance_seconds": tolerance,
    }


def _validate_probe(
    probe: dict,
    width: int,
    height: int,
    fps: float,
    frame_count: int,
    timeline_duration: float,
) -> dict:
    videos = [
        stream
        for stream in probe.get("streams", [])
        if stream.get("codec_type") == "video"
    ]
    if len(videos) != 1:
        raise QaFailure("Export does not contain exactly one video stream")
    video = videos[0]
    audios = [
        stream
        for stream in probe.get("streams", [])
        if stream.get("codec_type") == "audio"
    ]
    if len(audios) != 1:
        raise QaFailure("Export does not contain exactly one audio stream")
    audio = audios[0]
    expected = {
        "codec_name": "h264",
        "width": width,
        "height": height,
        "pix_fmt": "yuv420p",
        "color_range": "tv",
        "color_space": "bt709",
        "color_transfer": "bt709",
        "color_primaries": "bt709",
    }
    for key, value in expected.items():
        if video.get(key) != value:
            raise QaFailure(
                "ffprobe {} mismatch: expected {!r}, got {!r}".format(
                    key, value, video.get(key)
                )
            )
    measured_fps = _rational(str(video.get("avg_frame_rate", "")), "average frame rate")
    if not math.isclose(measured_fps, float(fps), rel_tol=0.0, abs_tol=1e-9):
        raise QaFailure("ffprobe frame rate does not match the Timeline")
    if int(video.get("nb_read_frames", -1)) != frame_count:
        raise QaFailure("ffprobe frame count does not match the Timeline")
    format_names = set(str(probe.get("format", {}).get("format_name", "")).split(","))
    if "mp4" not in format_names and "mov" not in format_names:
        raise QaFailure("ffprobe did not identify an MP4-family container")
    expected_audio = {
        "codec_name": AUDIO_CODEC,
        "sample_rate": str(AUDIO_SAMPLE_RATE),
        "channels": AUDIO_CHANNELS,
    }
    for key, value in expected_audio.items():
        if audio.get(key) != value:
            raise QaFailure(
                "ffprobe audio {} mismatch: expected {!r}, got {!r}".format(
                    key, value, audio.get(key)
                )
            )
    layout = audio.get("channel_layout")
    if layout not in (None, "stereo"):
        raise QaFailure("ffprobe Audio channel layout is not stereo")
    timing = _validate_timing(
        video,
        audio,
        probe.get("format", {}),
        fps,
        frame_count,
        timeline_duration,
    )
    return {"video": video, "audio": audio, "timing": timing}


def _decode_frame(path: pathlib.Path, width: int, height: int, seconds: float) -> dict:
    command = ["ffmpeg", "-hide_banner", "-loglevel", "error"]
    if seconds > 0.0:
        command.extend(["-ss", str(seconds)])
    command.extend(
        [
            "-i",
            str(path),
            "-map",
            "0:v:0",
            "-frames:v",
            "1",
            "-f",
            "rawvideo",
            "-pix_fmt",
            "rgba",
            "-",
        ]
    )
    try:
        completed = subprocess.run(command, capture_output=True, check=False)
    except OSError as error:
        raise QaFailure("ffmpeg decode could not start: {}".format(error)) from error
    if completed.returncode != 0:
        raise QaFailure(
            "ffmpeg decode failed: "
            + completed.stderr.decode("utf-8", errors="replace")
        )
    expected_bytes = width * height * 4
    if len(completed.stdout) != expected_bytes:
        raise QaFailure(
            "decoded frame has {} bytes instead of {}".format(
                len(completed.stdout), expected_bytes
            )
        )
    pixels = completed.stdout
    if not any(pixels[index : index + 3] != b"\x00\x00\x00" for index in range(0, len(pixels), 4)):
        raise QaFailure("decoded Export frame is entirely black")
    return {
        "seconds": seconds,
        "bytes": len(pixels),
        "sha256": hashlib.sha256(pixels).hexdigest(),
    }


def _decode_audible_audio(path: pathlib.Path) -> dict:
    command = [
        "ffmpeg",
        "-hide_banner",
        "-loglevel",
        "error",
        "-i",
        str(path),
        "-map",
        "0:a:0",
        "-t",
        "1",
        "-ar",
        str(AUDIO_SAMPLE_RATE),
        "-ac",
        str(AUDIO_CHANNELS),
        "-f",
        "f32le",
        "-",
    ]
    try:
        completed = subprocess.run(command, capture_output=True, check=False)
    except OSError as error:
        raise QaFailure("ffmpeg Audio decode could not start: {}".format(error)) from error
    if completed.returncode != 0:
        raise QaFailure(
            "ffmpeg Audio decode failed: "
            + completed.stderr.decode("utf-8", errors="replace")
        )
    samples = completed.stdout
    if not samples or len(samples) % 4 != 0:
        raise QaFailure("decoded Audio is empty or not aligned f32le data")
    sample_count = len(samples) // 4
    if sample_count < AUDIO_SAMPLE_RATE * AUDIO_CHANNELS * 9 // 10:
        raise QaFailure("decoded Audio does not cover the first Timeline second")
    peak = 0.0
    sum_of_squares = 0.0
    for (sample,) in struct.iter_unpack("<f", samples):
        if not math.isfinite(sample):
            raise QaFailure("decoded Audio contains a non-finite sample")
        peak = max(peak, abs(sample))
        sum_of_squares += sample * sample
    if peak <= 1e-5:
        raise QaFailure("decoded Audio is silent despite the audible fixture")
    return {
        "bytes": len(samples),
        "sample_values": sample_count,
        "peak": peak,
        "rms": math.sqrt(sum_of_squares / sample_count),
        "sha256": hashlib.sha256(samples).hexdigest(),
    }


def _application_error_lines() -> tuple[pathlib.Path, list[str]]:
    configured = os.environ.get(ARTIFACT_DIRECTORY_ENV)
    if not configured:
        raise QaFailure(ARTIFACT_DIRECTORY_ENV + " is required for native log verification")
    log_path = pathlib.Path(configured).resolve() / "app.log"
    try:
        lines = log_path.read_text(encoding="utf-8", errors="replace").splitlines()
    except OSError as error:
        raise QaFailure("cannot inspect native app log: {}".format(error)) from error
    return log_path, [line for line in lines if ERROR_LEVEL.search(line)]


def run_suite(client):
    client.wait_health()
    initial = client.wait_until(
        "a rendered authoring Project",
        lambda: state
        if (state := client.state())["editor"]["preview"].get(
            "nontransparent_pixels", 0
        )
        > 0
        and state["editor"].get("error") is None
        else None,
        timeout=30.0,
    )
    width, height, fps, expected_frames, timeline_duration = _timeline_contract(initial)
    output = _configured_output_path()
    output.write_bytes(SENTINEL)
    sentinel_hash = hashlib.sha256(SENTINEL).hexdigest()
    before_entries = _directory_entries(output.parent)

    client.key("e", True, command=True)
    client.key("e", False, command=True)

    def completed_export():
        state = client.state()
        if state["editor"].get("error") is not None:
            raise QaFailure("native Export failed: " + state["editor"]["error"])
        match = SUCCESS.match(state["editor"].get("status", ""))
        if match is None:
            return None
        if pathlib.Path(match.group(2)).resolve() != output:
            raise QaFailure("Export completed for an unexpected destination")
        if int(match.group(1)) != expected_frames:
            raise QaFailure("Export status reported an unexpected frame count")
        return state

    completed = client.wait_until("production video Export completion", completed_export, timeout=60.0)
    encoded = output.read_bytes()
    if not encoded or hashlib.sha256(encoded).hexdigest() == sentinel_hash:
        raise QaFailure("successful Export did not replace the existing destination")

    probe = _probe(output)
    streams = _validate_probe(
        probe, width, height, fps, expected_frames, timeline_duration
    )
    decoded = [
        _decode_frame(output, width, height, 0.0),
        _decode_frame(output, width, height, expected_frames / fps / 2.0),
    ]
    decoded_audio = _decode_audible_audio(output)
    unexpected = _new_unexpected_siblings(
        before_entries, _directory_entries(output.parent), output
    )
    if unexpected:
        raise QaFailure(
            "Export left new sibling files: "
            + ", ".join(str(path) for path in unexpected)
        )
    log_path, error_lines = _application_error_lines()
    if error_lines:
        raise QaFailure("native app emitted ERROR logs: " + " | ".join(error_lines))

    return {
        "suite": "video-export",
        "output": {
            "path": str(output),
            "bytes": len(encoded),
            "sha256": hashlib.sha256(encoded).hexdigest(),
            "replaced_sentinel_sha256": sentinel_hash,
        },
        "timeline": {
            "width": width,
            "height": height,
            "fps": fps,
            "frames": expected_frames,
            "duration_seconds": timeline_duration,
        },
        "status": completed["editor"]["status"],
        "video_stream": streams["video"],
        "audio_stream": streams["audio"],
        "stream_timing": streams["timing"],
        "decoded_frames": decoded,
        "decoded_audio": decoded_audio,
        "new_unexpected_siblings": [],
        "app_log": {"path": str(log_path), "error_count": 0},
        "actions": client.evidence,
    }


if __name__ == "__main__":
    default_root = (REPOSITORY_ROOT / "target" / "qa-video-export-e2e").resolve()
    default_root.mkdir(parents=True, exist_ok=True)
    os.environ.setdefault("RUVIE_QA_FIXTURE", AUTHORING_AUDIO_FIXTURE)
    os.environ.setdefault(ARTIFACT_DIRECTORY_ENV, str(default_root))
    os.environ.setdefault(EXPORT_PATH_ENV, str(default_root / "export.mp4"))
    raise SystemExit(
        run_suite_main(
            "qa-video-export-e2e",
            run_suite,
            "target/qa-video-export-e2e/evidence.json",
        )
    )
