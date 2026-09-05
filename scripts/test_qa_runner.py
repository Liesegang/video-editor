import importlib.util
import contextlib
import hashlib
import io
import pathlib
import subprocess
import sys
import tempfile
import time
import unittest
from unittest import mock


SCRIPTS = pathlib.Path(__file__).resolve().parent
sys.path.insert(0, str(SCRIPTS))


def load(name, filename):
    spec = importlib.util.spec_from_file_location(name, SCRIPTS / filename)
    if spec is None or spec.loader is None:
        raise RuntimeError("cannot load " + filename)
    module = importlib.util.module_from_spec(spec)
    sys.modules[name] = module
    spec.loader.exec_module(module)
    return module


SUPPORT = load("ruvie_qa_support", "qa_support.py")
RUNNER = load("ruvie_qa_runner", "qa-runner.py")
ASSETS = load("ruvie_qa_assets", "qa-assets-timeline-e2e.py")
TIMELINE = load("ruvie_qa_timeline", "qa-timeline-edit-e2e.py")
CURVE = load("ruvie_qa_curve", "qa-curve-editor-e2e.py")
PARTICLE = load("ruvie_qa_particle", "qa-particle-node-clip-e2e.py")
PARTICLE_PERSISTENCE = load(
    "ruvie_qa_particle_persistence", "qa-particle-persistence-e2e.py"
)
VIDEO_EXPORT = load("ruvie_qa_video_export", "qa-video-export-e2e.py")


class QaRunnerTests(unittest.TestCase):
    def test_timeline_reveal_scrolls_until_the_row_is_visible(self):
        client = mock.Mock()
        row = {"id": "timeline.item:target", "visible": True}
        client.component_snapshot.side_effect = [
            {"components": [{**row, "visible": False}]},
            {"components": [row]},
        ]
        self.assertEqual(
            SUPPORT.bring_timeline_component(client, row["id"], -100.0), row
        )
        client.scroll_component.assert_called_once_with("timeline.canvas", 0.0, -100.0)

    def test_timeline_reveal_cannot_succeed_for_an_absent_row(self):
        client = mock.Mock()
        client.component_snapshot.return_value = {"components": []}
        with self.assertRaises(SUPPORT.QaFailure):
            SUPPORT.bring_timeline_component(client, "timeline.item:absent", -100.0)
        self.assertEqual(client.scroll_component.call_count, 10)

    def test_runner_targets_only_production_authoring_suites(self):
        expected = [
            "smoke",
            "unsaved-changes",
            "settings-dialog",
            "assets-timeline",
            "particle-node-clip",
            "inspector-asset-preview",
            "timeline-edit",
            "timeline-transition",
            "transition-module-assignment",
            "timeline-content-zoom",
            "preview",
            "path-editor",
            "inspector-property-mode",
            "inspector-source",
            "color-palette",
            "appearance",
            "inspector-effects",
            "timeline-dopesheet",
            "curve-editor",
            "node-editor",
            "node-clip-conversion",
            "audio-playback",
            "text-ensemble",
            "text-tracking",
            "video-export",
        ]
        full = RUNNER.suite_specs("full")
        self.assertEqual([suite.name for suite in full], expected)
        self.assertEqual(
            [suite.name for suite in full if suite.fixture == "authoring_audio_e2e"],
            ["timeline-content-zoom", "node-editor", "audio-playback", "video-export"],
        )
        self.assertEqual(
            [suite.name for suite in full if suite.fixture == "authoring_path_e2e"],
            ["path-editor"],
        )
        unsaved = next(suite for suite in full if suite.name == "unsaved-changes")
        self.assertTrue(unsaved.project_file)
        self.assertTrue(unsaved.expects_exit)
        appearance = next(suite for suite in full if suite.name == "appearance")
        self.assertTrue(appearance.project_file)
        self.assertTrue(appearance.expects_exit)
        tracking = next(suite for suite in full if suite.name == "text-tracking")
        self.assertTrue(tracking.project_file)
        self.assertTrue(tracking.expects_exit)
        video_export = next(suite for suite in full if suite.name == "video-export")
        self.assertTrue(video_export.export_file)
        self.assertEqual(video_export.fixture, SUPPORT.AUTHORING_AUDIO_FIXTURE)
        self.assertEqual([suite.name for suite in RUNNER.suite_specs("smoke")], ["smoke"])
        with self.assertRaises(ValueError):
            RUNNER.suite_specs("blend")

    def test_suite_files_do_not_use_removed_project_fixture_or_ambiguous_editor_name(self):
        for suite in RUNNER.suite_specs("full"):
            source = (SCRIPTS / suite.script).read_text(encoding="utf-8")
            self.assertNotIn("retired_fixture", source)
            self.assertNotIn("qa_project_graph_base", source)
        self.assertEqual(CURVE.FIXTURE, "authoring_e2e")

    def test_every_active_qa_file_stays_below_one_thousand_lines(self):
        files = [SCRIPTS / "qa-runner.py"]
        files.extend(SCRIPTS / suite.script for suite in RUNNER.suite_specs("full"))
        files.extend(SCRIPTS.glob("qa_*.py"))
        files.append(SCRIPTS / "qa-particle-persistence-e2e.py")
        files.append(pathlib.Path(__file__))
        for path in files:
            with self.subTest(path=path.name):
                self.assertLess(len(path.read_text(encoding="utf-8").splitlines()), 1000)

    def test_published_endpoint_is_strictly_ipv4_loopback(self):
        self.assertEqual(
            RUNNER.parse_published_endpoint('{"host":"127.0.0.1","port":43123}'),
            ("127.0.0.1", 43123),
        )
        for value in (
            '{"host":"0.0.0.0","port":43123}',
            '{"host":"127.0.0.1","port":0}',
            '{"host":"127.0.0.1","port":65536}',
        ):
            with self.assertRaises(ValueError):
                RUNNER.parse_published_endpoint(value)

    def test_evidence_is_bound_to_run_and_authoring_fixture(self):
        current = {"ok": True, "run_id": "run", "fixture": "authoring_e2e"}
        self.assertTrue(RUNNER.evidence_matches_run(current, "run"))
        audio = {"ok": True, "run_id": "run", "fixture": "authoring_audio_e2e"}
        self.assertTrue(
            RUNNER.evidence_matches_run(audio, "run", "authoring_audio_e2e")
        )
        self.assertFalse(RUNNER.evidence_matches_run(current, "other"))
        self.assertFalse(
            RUNNER.evidence_matches_run({**current, "fixture": "retired_fixture"}, "run")
        )

    def test_reused_suite_directory_cannot_pass_with_stale_artifacts(self):
        with tempfile.TemporaryDirectory() as directory:
            path = pathlib.Path(directory)
            for name in RUNNER.SUITE_OUTPUT_NAMES:
                (path / name).write_text("stale", encoding="utf-8")
            keep = path / "app.log"
            keep.write_text("diagnostic", encoding="utf-8")
            RUNNER.prepare_suite_directory(path)
            self.assertTrue(
                all(not (path / name).exists() for name in RUNNER.SUITE_OUTPUT_NAMES)
            )
            self.assertEqual(keep.read_text(encoding="utf-8"), "diagnostic")

    def test_exit_suite_capture_is_bound_to_real_png_bytes(self):
        with tempfile.TemporaryDirectory() as directory:
            path = pathlib.Path(directory) / "capture.png"
            path.write_bytes(b"native screenshot")
            metadata = {
                "phase": "ready",
                "sha256": hashlib.sha256(path.read_bytes()).hexdigest(),
            }
            self.assertTrue(RUNNER.capture_file_matches_metadata(path, metadata))
            path.write_bytes(b"stale screenshot")
            self.assertFalse(RUNNER.capture_file_matches_metadata(path, metadata))

    def test_failed_exit_suite_does_not_wait_for_an_unrequested_close(self):
        process = mock.Mock()
        for exit_code in (None, 1):
            process.poll.return_value = exit_code
            self.assertEqual(
                RUNNER.wait_for_suite_app_exit(process, False, 45.0), exit_code
            )
        process.wait.assert_not_called()

    def test_successful_exit_suite_requires_real_process_completion(self):
        process = mock.Mock()
        process.wait.return_value = 0
        self.assertEqual(RUNNER.wait_for_suite_app_exit(process, True, 45.0), 0)
        process.wait.assert_called_once_with(timeout=45.0)
        process.poll.assert_not_called()
        process.wait.side_effect = subprocess.TimeoutExpired("qa-app", 45.0)
        self.assertIsNone(RUNNER.wait_for_suite_app_exit(process, True, 45.0))

    def test_media_time_and_interval_helpers_use_exact_wire_values(self):
        self.assertEqual(SUPPORT.media_seconds({"value": 3, "timescale": 2}), 1.5)
        item = {
            "interval": {
                "start": {"value": 3, "timescale": 2},
                "duration": {"value": 5, "timescale": 2},
            }
        }
        self.assertEqual(TIMELINE._interval(item), (1.5, 2.5))

    def test_asset_row_overlap_check_rejects_stacked_rows(self):
        rows = [
            {"id": "a", "rect_points": {"min_y": 10.0, "max_y": 40.0}},
            {"id": "b", "rect_points": {"min_y": 20.0, "max_y": 50.0}},
        ]
        with self.assertRaises(ASSETS.QaFailure):
            ASSETS._assert_rows_do_not_overlap(rows)
        ASSETS._assert_rows_do_not_overlap(
            [rows[0], {"id": "b", "rect_points": {"min_y": 42.0, "max_y": 72.0}}]
        )

    def test_particle_pixel_evidence_requires_a_same_frame_visible_delta(self):
        baseline = {
            "rendered_frame": 201,
            "pixel_hash": "before",
            "nontransparent_pixels": 1,
        }
        edited = {
            "rendered_frame": 201,
            "pixel_hash": "after",
            "nontransparent_pixels": 1,
        }
        PARTICLE._assert_same_frame_particle_delta(baseline, edited)
        for invalid in (
            {**edited, "rendered_frame": 202},
            {**edited, "pixel_hash": "before"},
            {**edited, "nontransparent_pixels": 0},
        ):
            with self.assertRaises(PARTICLE.QaFailure):
                PARTICLE._assert_same_frame_particle_delta(baseline, invalid)

    def test_particle_persistence_qa_reuses_the_authoring_suite_module(self):
        self.assertIs(PARTICLE_PERSISTENCE.PARTICLE_QA, PARTICLE)

    def test_spawned_authoring_app_accepts_optional_environment_overrides(self):
        process = mock.Mock()
        with (
            mock.patch.dict(
                SUPPORT.os.environ,
                {
                    "INHERITED": "kept",
                    "REMOVE_ME": "stale",
                    "RUVIE_QA_FIXTURE": "parent_fixture",
                },
                clear=True,
            ),
            mock.patch.object(SUPPORT.subprocess, "Popen", return_value=process) as popen,
            mock.patch.object(SUPPORT, "terminate_process") as terminate,
        ):
            with SUPPORT.spawned_authoring_app(
                43123,
                {
                    "RUVIE_QA_FIXTURE": "authoring_path_e2e",
                    "RUVIE_QA_OPEN_EXISTING_PROJECT": "1",
                    "RUVIE_QA_PROJECT_PATH": "saved.ruvie",
                    "REMOVE_ME": None,
                    "RUVIE_QA_PORT": "must-not-win",
                },
            ) as yielded:
                self.assertIs(yielded, process)

        environment = popen.call_args.kwargs["env"]
        self.assertEqual(environment["INHERITED"], "kept")
        self.assertNotIn("REMOVE_ME", environment)
        self.assertEqual(environment["RUVIE_QA_FIXTURE"], "authoring_path_e2e")
        self.assertEqual(environment["RUVIE_QA_OPEN_EXISTING_PROJECT"], "1")
        self.assertEqual(environment["RUVIE_QA_PROJECT_PATH"], "saved.ruvie")
        self.assertEqual(environment["RUVIE_QA_PORT"], "43123")
        self.assertEqual(
            popen.call_args.args[0], ["cargo", "run", "-p", "app", "--locked"]
        )
        terminate.assert_called_once_with(process)

    def test_spawned_authoring_app_reuses_runner_selected_binary(self):
        process = mock.Mock()
        with (
            mock.patch.dict(
                SUPPORT.os.environ,
                {SUPPORT.QA_APP_BINARY_ENV: "target/release/app.exe"},
                clear=True,
            ),
            mock.patch.object(SUPPORT.subprocess, "Popen", return_value=process) as popen,
            mock.patch.object(SUPPORT, "terminate_process"),
        ):
            with SUPPORT.spawned_authoring_app(43125):
                pass
        self.assertEqual(popen.call_args.args[0], ["target/release/app.exe"])

    def test_clean_native_close_uses_production_request_and_waits_for_exit(self):
        client = mock.Mock()
        client.request.return_value = {"queued": True, "action_id": 42}
        process = mock.Mock()
        process.wait.return_value = 0
        with mock.patch.object(SUPPORT, "wait_endpoint_closed") as wait_closed:
            result = SUPPORT.close_clean_native_app(client, process, "saved app", 7.0)
        client.request.assert_called_once_with(
            "/v1/input/close-request", {}, method="POST"
        )
        wait_closed.assert_called_once_with(
            client, timeout=7.0, description="saved app"
        )
        process.wait.assert_called_once_with(timeout=7.0)
        self.assertEqual(result["action_id"], 42)
        self.assertEqual(result["exit_code"], 0)

    def test_spawned_authoring_app_one_argument_keeps_default_fixture(self):
        process = mock.Mock()
        with (
            mock.patch.dict(SUPPORT.os.environ, {}, clear=True),
            mock.patch.object(SUPPORT.subprocess, "Popen", return_value=process) as popen,
            mock.patch.object(SUPPORT, "terminate_process"),
        ):
            with SUPPORT.spawned_authoring_app(43124):
                pass
        environment = popen.call_args.kwargs["env"]
        self.assertEqual(environment["RUVIE_QA_PORT"], "43124")
        self.assertEqual(environment["RUVIE_QA_FIXTURE"], SUPPORT.AUTHORING_FIXTURE)

    def test_video_export_metadata_validation_requires_the_delivery_contract(self):
        valid = {
            "streams": [
                {
                    "codec_type": "video",
                    "codec_name": "h264",
                    "width": 640,
                    "height": 360,
                    "pix_fmt": "yuv420p",
                    "color_range": "tv",
                    "color_space": "bt709",
                    "color_transfer": "bt709",
                    "color_primaries": "bt709",
                    "avg_frame_rate": "30/1",
                    "nb_read_frames": "360",
                    "start_time": "0.000000",
                    "duration": "12.000000",
                },
                {
                    "codec_type": "audio",
                    "codec_name": "aac",
                    "sample_rate": "48000",
                    "channels": 2,
                    "channel_layout": "stereo",
                    "start_time": "0.000000",
                    "duration": "12.000000",
                },
            ],
            "format": {
                "format_name": "mov,mp4,m4a,3gp,3g2,mj2",
                "duration": "12.000000",
            },
        }
        VIDEO_EXPORT._validate_probe(valid, 640, 360, 30, 360, 12.0)
        with self.assertRaises(VIDEO_EXPORT.QaFailure):
            VIDEO_EXPORT._validate_probe(
                {
                    **valid,
                    "streams": [
                        {**valid["streams"][0], "color_space": "unknown"},
                        valid["streams"][1],
                    ],
                },
                640,
                360,
                30,
                360,
                12.0,
            )
        with self.assertRaises(VIDEO_EXPORT.QaFailure):
            VIDEO_EXPORT._validate_probe(
                {**valid, "streams": [valid["streams"][0]]},
                640,
                360,
                30,
                360,
                12.0,
            )
        with self.assertRaises(VIDEO_EXPORT.QaFailure):
            VIDEO_EXPORT._validate_probe(
                {
                    **valid,
                    "streams": [
                        valid["streams"][0],
                        {**valid["streams"][1], "duration": "10.0"},
                    ],
                },
                640,
                360,
                30,
                360,
                12.0,
            )

    def test_video_export_sibling_diff_ignores_preexisting_runner_files(self):
        with tempfile.TemporaryDirectory() as directory:
            root = pathlib.Path(directory)
            (root / "app.log").write_text("", encoding="utf-8")
            before = VIDEO_EXPORT._directory_entries(root)
            (root / "export.mp4").write_bytes(b"output")
            self.assertEqual(
                VIDEO_EXPORT._new_unexpected_siblings(
                    before, VIDEO_EXPORT._directory_entries(root), root / "export.mp4"
                ),
                [],
            )
            staged = root / ".private-staging-name"
            staged.write_bytes(b"partial")
            self.assertEqual(
                VIDEO_EXPORT._new_unexpected_siblings(
                    before, VIDEO_EXPORT._directory_entries(root), root / "export.mp4"
                ),
                [staged.resolve()],
            )

    def test_process_cleanup_is_cross_platform(self):
        process = subprocess.Popen(
            [sys.executable, "-c", "import time; time.sleep(60)"],
            **SUPPORT.process_group_options(),
        )
        try:
            time.sleep(0.05)
            SUPPORT.terminate_process(process, grace_seconds=0.2)
            self.assertIsNotNone(process.poll())
        finally:
            SUPPORT.terminate_process(process, grace_seconds=0.1)

    def test_dock_activation_does_not_toggle_a_tab_published_one_frame_late(self):
        class DelayedDockClient:
            def __init__(self):
                self.snapshots = 0
                self.keys = []
                self.clicked = []

            def component_snapshot(self):
                self.snapshots += 1
                components = []
                if self.snapshots >= 2:
                    components = [
                        {
                            "id": "dock.tab:node_editor",
                            "visible": True,
                            "enabled": True,
                            "rect_points": {"width": 40.0, "height": 20.0},
                        }
                    ]
                return {"components": components}

            def wait_until(self, _description, predicate, timeout=None):
                self.asserted_timeout = timeout
                value = predicate()
                if value is None:
                    raise SUPPORT.QaFailure("not published")
                return value

            def key(self, *args, **kwargs):
                self.keys.append((args, kwargs))

            def inject(self, *args, **kwargs):
                raise AssertionError("late publication must not invoke Command Palette")

            def click_component(self, component_id):
                self.clicked.append(component_id)

            def wait_component_settled(self, component_id):
                return component_id

        client = DelayedDockClient()
        result = SUPPORT.activate_dock_tab(
            client, "dock.tab:node_editor", "Node Editor", "Node Editor publication"
        )
        self.assertEqual(result, "dock.tab:node_editor")
        self.assertEqual(client.keys, [])
        self.assertEqual(client.clicked, ["dock.tab:node_editor"])
        self.assertEqual(client.asserted_timeout, 0.75)

    def test_dock_activation_toggles_a_registered_but_hidden_tab(self):
        class HiddenDockClient:
            def __init__(self):
                self.keys = []
                self.injected = []
                self.clicked = []

            def component_snapshot(self):
                return {
                    "components": [
                        {
                            "id": "dock.tab:timeline",
                            "visible": False,
                            "enabled": True,
                        }
                    ]
                }

            def wait_until(self, _description, predicate, timeout=None):
                self.asserted_timeout = timeout
                if predicate() is None:
                    raise SUPPORT.QaFailure("not interactable")
                raise AssertionError("hidden tab unexpectedly became interactable")

            def key(self, *args, **kwargs):
                self.keys.append((args, kwargs))

            def inject(self, *args, **kwargs):
                self.injected.append((args, kwargs))

            def click_component(self, component_id):
                self.clicked.append(component_id)

            def wait_component_settled(self, component_id):
                return component_id

        client = HiddenDockClient()
        result = SUPPORT.activate_dock_tab(
            client, "dock.tab:timeline", "Timeline", "Timeline activation"
        )
        self.assertEqual(result, "dock.tab:timeline")
        self.assertEqual(
            [args for args, _ in client.keys],
            [("p", True), ("p", False), ("enter", True), ("enter", False)],
        )
        self.assertEqual(client.injected[0][0], ("text", {"text": "Timeline"}))
        self.assertEqual(client.clicked, ["dock.tab:timeline"])
        self.assertEqual(client.asserted_timeout, 0.75)

    def test_terminal_component_click_queues_without_polling_a_closing_endpoint(self):
        client = SUPPORT.QaClient("http://127.0.0.1:1")
        component = {
            "id": "unsaved.discard",
            "rect_points": {"center_x": 120.0, "center_y": 80.0},
        }
        with mock.patch.object(
            client, "wait_component", return_value=({"frame": 9}, component)
        ), mock.patch.object(
            client,
            "request",
            return_value={"queued": True, "action_id": 27},
        ) as request:
            action_id, snapshot, selected, point = (
                client.queue_terminal_click_component("unsaved.discard")
            )

        self.assertEqual(action_id, 27)
        self.assertEqual(snapshot, {"frame": 9})
        self.assertIs(selected, component)
        self.assertEqual(point, {"x": 120.0, "y": 80.0})
        request.assert_called_once_with(
            "/v1/input/click",
            {
                "x": 120.0,
                "y": 80.0,
                "button": "primary",
                "coordinate_space": "points",
            },
            method="POST",
        )
        self.assertEqual(client.evidence[-1]["phase"], "queued_for_terminal_action")

    def test_positive_jobs_rejects_zero(self):
        self.assertEqual(RUNNER.parse_args(["--jobs", "2"]).jobs, 2)
        with contextlib.redirect_stderr(io.StringIO()):
            with self.assertRaises(SystemExit):
                RUNNER.parse_args(["--jobs", "0"])

    def test_full_qa_uses_the_release_binary_and_locked_release_build(self):
        self.assertEqual(RUNNER.build_profile("smoke"), "debug")
        self.assertEqual(RUNNER.build_profile("full"), "release")
        self.assertIn("debug", RUNNER.default_app_binary("smoke").parts)
        self.assertIn("release", RUNNER.default_app_binary("full").parts)
        self.assertEqual(
            RUNNER.app_build_command("full"),
            ["cargo", "build", "-p", "app", "--locked", "--release"],
        )


if __name__ == "__main__":
    unittest.main()
