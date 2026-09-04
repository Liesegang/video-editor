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


class QaRunnerTests(unittest.TestCase):
    def test_runner_targets_only_production_authoring_suites(self):
        expected = [
            "smoke",
            "unsaved-changes",
            "settings-dialog",
            "assets-timeline",
            "inspector-asset-preview",
            "timeline-edit",
            "timeline-transition",
            "timeline-content-zoom",
            "preview",
            "path-editor",
            "inspector-property-mode",
            "inspector-effects",
            "timeline-dopesheet",
            "curve-editor",
            "node-editor",
            "node-clip-conversion",
            "audio-playback",
            "text-ensemble",
        ]
        full = RUNNER.suite_specs("full")
        self.assertEqual([suite.name for suite in full], expected)
        self.assertEqual(
            [suite.name for suite in full if suite.fixture == "authoring_audio_e2e"],
            ["timeline-content-zoom", "audio-playback"],
        )
        self.assertEqual(
            [suite.name for suite in full if suite.fixture == "authoring_path_e2e"],
            ["path-editor"],
        )
        unsaved = next(suite for suite in full if suite.name == "unsaved-changes")
        self.assertTrue(unsaved.project_file)
        self.assertTrue(unsaved.expects_exit)
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
        files = [SCRIPTS / "qa-runner.py", SCRIPTS / "qa_support.py"]
        files.extend(SCRIPTS / suite.script for suite in RUNNER.suite_specs("full"))
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

    def test_positive_jobs_rejects_zero(self):
        self.assertEqual(RUNNER.parse_args(["--jobs", "2"]).jobs, 2)
        with contextlib.redirect_stderr(io.StringIO()):
            with self.assertRaises(SystemExit):
                RUNNER.parse_args(["--jobs", "0"])


if __name__ == "__main__":
    unittest.main()
