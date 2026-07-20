import importlib.util
import http.server
import pathlib
import subprocess
import sys
import tempfile
import threading
import time
import unittest


MODULE_PATH = pathlib.Path(__file__).with_name("qa-runner.py")
SPEC = importlib.util.spec_from_file_location("ruvie_qa_runner", MODULE_PATH)
if SPEC is None or SPEC.loader is None:
    raise RuntimeError("cannot load qa-runner.py")
RUNNER = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = RUNNER
SPEC.loader.exec_module(RUNNER)

E2E_PATH = pathlib.Path(__file__).with_name("qa-e2e.py")
E2E_SPEC = importlib.util.spec_from_file_location("ruvie_qa_e2e", E2E_PATH)
if E2E_SPEC is None or E2E_SPEC.loader is None:
    raise RuntimeError("cannot load qa-e2e.py")
E2E = importlib.util.module_from_spec(E2E_SPEC)
sys.modules[E2E_SPEC.name] = E2E
E2E_SPEC.loader.exec_module(E2E)

KEYFRAME_PATH = pathlib.Path(__file__).with_name("qa-keyframe-e2e.py")
KEYFRAME_SPEC = importlib.util.spec_from_file_location(
    "ruvie_qa_keyframe_e2e", KEYFRAME_PATH
)
if KEYFRAME_SPEC is None or KEYFRAME_SPEC.loader is None:
    raise RuntimeError("cannot load qa-keyframe-e2e.py")
KEYFRAME = importlib.util.module_from_spec(KEYFRAME_SPEC)
sys.modules[KEYFRAME_SPEC.name] = KEYFRAME
KEYFRAME_SPEC.loader.exec_module(KEYFRAME)


class EmptyCaptureHandler(http.server.BaseHTTPRequestHandler):
    bodies = []

    def do_POST(self):
        length = int(self.headers.get("Content-Length", "0"))
        type(self).bodies.append(self.rfile.read(length))
        payload = b'{"queued":true,"capture_id":1,"phase":"queued"}'
        self.send_response(202)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(payload)))
        self.end_headers()
        self.wfile.write(payload)

    def log_message(self, _format, *_args):
        pass


class SettlingQaClient(E2E.QaClient):
    def __init__(self):
        super().__init__("http://127.0.0.1", timeout=0.2)
        self.snapshots = iter(
            [
                (10, 1.0),
                (10, 1.0),
                (11, 1.0),
                (12, 1.01),
            ]
        )

    def state(self):
        return {"frame": 99}

    def component(self, component_id, require_visible=True):
        self.assert_component_id = component_id
        frame, min_x = next(self.snapshots)
        rect = {
            "min_x": min_x,
            "min_y": 2.0,
            "max_x": min_x + 10.0,
            "max_y": 12.0,
            "width": 10.0,
            "height": 10.0,
        }
        return {"frame": frame}, {"id": component_id, "rect_points": rect}


class InjectingQaClient(E2E.QaClient):
    def __init__(self):
        super().__init__("http://127.0.0.1", timeout=0.2)
        self.state_frames = iter((20, 21))

    def component_snapshot(self):
        return {"frame": 20, "components": []}

    def request(self, path, data=None, method=None):
        if path == "/v1/input/click":
            return {"action_id": 7}
        if path == "/v1/actions/7":
            return {"action_id": 7, "phase": "injected"}
        raise AssertionError("unexpected request: {}".format(path))

    def state(self):
        return {"frame": next(self.state_frames)}


class TimelineGeometryQaClient(E2E.QaClient):
    def __init__(self):
        super().__init__("http://127.0.0.1", timeout=0.2)
        self.injected = None

    def component_snapshot(self):
        def component(component_id, rect, metadata=None):
            return {
                "id": component_id,
                "enabled": True,
                "visible": True,
                "rect_points": rect,
                "metadata": metadata,
            }

        clip_rect = {
            "min_x": 100.0,
            "min_y": 40.0,
            "max_x": 101.0,
            "max_y": 70.0,
            "width": 1.0,
            "height": 30.0,
            "center_x": 100.5,
            "center_y": 55.0,
        }
        edge_rect = {
            "min_x": 100.0,
            "min_y": 40.0,
            "max_x": 105.0,
            "max_y": 70.0,
            "width": 5.0,
            "height": 30.0,
            "center_x": 102.5,
            "center_y": 55.0,
        }
        return {
            "frame": 42,
            "components": [
                component(
                    "timeline.clip:" + E2E.CLIP_A1,
                    clip_rect,
                    {"duration": 4.0, "pixels_per_second": 50.0},
                ),
                component("timeline.clip_edge.left:" + E2E.CLIP_A1, edge_rect),
            ],
        }

    def inject(self, endpoint, payload, evidence=None):
        self.injected = (endpoint, payload, evidence)
        return 7


class QaRunnerTests(unittest.TestCase):
    def test_selection_contract_preserves_entity_kind_and_derives_clip_owner(self):
        state = {
            "editor": {
                "selection": {
                    "targets": [{"kind": "clip", "id": "shared"}],
                    "primary": {"kind": "clip", "id": "shared"},
                }
            },
            "project": {
                "tracks": {
                    "track": {"clip_ids": ["shared"]},
                }
            },
        }

        self.assertTrue(E2E.selection_matches(state, "clip", "shared"))
        self.assertFalse(E2E.selection_matches(state, "node", "shared"))
        E2E.assert_selection(state, "shared", "track", "typed Clip")

    def test_e2e_fixture_contract_names_all_sixteen_explicit_nodes(self):
        self.assertEqual(len(E2E.EXPECTED_FIXTURE_NODES), 16)
        self.assertEqual(
            E2E.EXPECTED_CLIP_NODES[E2E.CLIP_A1],
            [E2E.AUDIO_A, E2E.AUDIO_B, E2E.SOLID, E2E.MERGE],
        )
        self.assertEqual(
            E2E.EXPECTED_CLIP_OUTPUTS,
            {
                E2E.CLIP_A1: E2E.MERGE,
                E2E.CLIP_A2: E2E.BLUR_EFFECT,
                E2E.CLIP_B1: E2E.SHAPE_MERGE,
            },
        )
        self.assertEqual(
            set(E2E.EXPECTED_OPERATIONS),
            {
                E2E.TEXT_TRANSFORM,
                E2E.SHAPE_TRANSFORM,
                E2E.TRANSFORM_EFFECTOR,
                E2E.OPACITY_EFFECTOR,
                E2E.BACKPLATE_DECORATOR,
                E2E.BLUR_EFFECT,
                E2E.TEXT_FILL,
                E2E.SHAPE_FILL,
                E2E.SHAPE_STROKE,
            },
        )

    def test_retired_four_node_fixture_is_rejected(self):
        project = {
            "nodes": {
                node_id: {}
                for node_id in (E2E.SOLID, E2E.MERGE, E2E.TEXT, E2E.SHAPE)
            },
            "clips": {},
            "connections": [],
        }
        with self.assertRaisesRegex(E2E.QaFailure, "16 explicit Nodes"):
            E2E.validate_explicit_operation_fixture(project)

    def test_keyframe_suite_uses_direct_operation_node_ids_only(self):
        source = KEYFRAME_PATH.read_text(encoding="utf-8")
        for retired_component_fragment in (
            "inspector.ensemble.",
            "inspector.style.node:",
            "inspector.effect.node:",
            ".effector:{}",
            ".decorator:{}",
            '"effectors"',
            '"decorators"',
            '"styles"',
            '"effects"',
        ):
            self.assertNotIn(retired_component_fragment, source)
        self.assertIn(
            'tx_control = "inspector.property.node:{}:tx".format(TRANSFORM_EFFECTOR)',
            source,
        )
        self.assertIn('tx_property = "node:tx"', source)
        self.assertIn('curve_id = "graph.curve_hit." + tx_property', source)
        self.assertIn('"graph.keyframe_menu.delete:" + added_key["id"]', source)
        self.assertIn('"action_count": len(client.evidence)', source)
        self.assertIn('result["git_commit"] = subprocess.check_output(', source)
        self.assertNotIn('tx_property = "direct:tx"', source)
        project = {
            "nodes": {
                E2E.TRANSFORM_EFFECTOR: {
                    "properties": {"tx": {"type": "constant"}}
                }
            }
        }
        self.assertEqual(
            KEYFRAME.target_property(project, E2E.TRANSFORM_EFFECTOR, "tx"),
            {"type": "constant"},
        )

    def test_keyframe_e2e_models_linear_and_cubic_inspector_values(self):
        self.assertAlmostEqual(
            KEYFRAME.numeric_easing_value(10.0, 30.0, 0.3, "Linear"),
            16.0,
        )
        self.assertAlmostEqual(
            KEYFRAME.numeric_easing_value(
                10.0, 30.0, 0.3, "EaseInOutCubic"
            ),
            12.16,
        )

    def test_double_click_evidence_matches_the_timing_independent_raw_input(self):
        frames = E2E.expected_pointer_frames(
            "double-click", {"x": 12.0, "y": 34.0}
        )
        self.assertEqual(
            frames,
            [
                {"kind": "settle", "point": {"x": 12.0, "y": 34.0}},
                {
                    "kind": "double_click",
                    "point": {"x": 12.0, "y": 34.0},
                    "events": ["press", "release", "press", "release"],
                },
            ],
        )

    def test_scroll_and_pinch_evidence_records_coordinate_raw_input(self):
        self.assertEqual(
            E2E.expected_pointer_frames(
                "scroll",
                {"x": 12.0, "y": 34.0, "delta_x": 5.0, "delta_y": -6.0},
            ),
            [
                {
                    "kind": "scroll",
                    "point": {"x": 12.0, "y": 34.0},
                    "delta": {"x": 5.0, "y": -6.0},
                }
            ],
        )
        self.assertEqual(
            E2E.expected_pointer_frames(
                "pinch", {"x": 56.0, "y": 78.0, "factor": 1.25}
            ),
            [
                {
                    "kind": "pinch",
                    "point": {"x": 56.0, "y": 78.0},
                    "factor": 1.25,
                }
            ],
        )

    def test_capture_clients_send_an_explicit_empty_body_post(self):
        EmptyCaptureHandler.bodies = []
        server = http.server.ThreadingHTTPServer(("127.0.0.1", 0), EmptyCaptureHandler)
        thread = threading.Thread(target=server.serve_forever, daemon=True)
        thread.start()
        base_url = "http://127.0.0.1:{}".format(server.server_port)
        try:
            queued = RUNNER.json_request(base_url, "/v1/captures", method="POST")
            self.assertEqual(queued["capture_id"], 1)
            queued = E2E.QaClient(base_url).request("/v1/captures", method="POST")
            self.assertEqual(queued["capture_id"], 1)
        finally:
            server.shutdown()
            server.server_close()
            thread.join(timeout=1.0)
        self.assertEqual(EmptyCaptureHandler.bodies, [b"", b""])

    def test_reused_suite_directory_cannot_supply_stale_evidence_or_capture(self):
        with tempfile.TemporaryDirectory() as directory:
            suite_dir = pathlib.Path(directory)
            for name in RUNNER.SUITE_OUTPUT_NAMES:
                (suite_dir / name).write_text("stale", encoding="utf-8")
            untouched = suite_dir / "app.log"
            untouched.write_text("diagnostic", encoding="utf-8")

            RUNNER.prepare_suite_directory(suite_dir)

            self.assertTrue(all(not (suite_dir / name).exists() for name in RUNNER.SUITE_OUTPUT_NAMES))
            self.assertEqual(untouched.read_text(encoding="utf-8"), "diagnostic")

    def test_evidence_must_belong_to_the_current_run(self):
        self.assertTrue(RUNNER.evidence_matches_run({"ok": True, "run_id": "current"}, "current"))
        self.assertFalse(RUNNER.evidence_matches_run({"ok": True, "run_id": "old"}, "current"))
        self.assertFalse(RUNNER.evidence_matches_run({"ok": True}, "current"))

    def test_zero_time_stretch_is_a_valid_freeze_clip(self):
        project = {
            "compositions": [
                {
                    "id": "composition",
                    "track_ids": ["track"],
                    "node_ids": [],
                    "output_node_id": None,
                }
            ],
            "tracks": {
                "track": {
                    "clip_ids": ["clip"],
                    "node_ids": [],
                    "output_node_id": None,
                }
            },
            "clips": {
                "clip": {
                    "start_time": 0.0,
                    "duration": 1.0,
                    "trim_in": 0.25,
                    "time_stretch": 0.0,
                    "node_ids": [],
                    "output_node_id": None,
                }
            },
            "nodes": {},
        }
        owners = E2E.validate_canonical_ownership(project)
        self.assertEqual(owners["clip_owners"], {"clip": "track"})

    def test_component_settle_requires_distinct_completed_frames(self):
        client = SettlingQaClient()

        snapshot, component = client.wait_component_settled(
            "timeline.clip", consecutive_reads=2
        )

        self.assertEqual(snapshot["frame"], 12)
        self.assertEqual(component["id"], "timeline.clip")

    def test_input_evidence_records_a_completed_frame_after_injection(self):
        client = InjectingQaClient()

        action_id = client.inject("click", {"x": 1.0, "y": 2.0})

        self.assertEqual(action_id, 7)
        self.assertEqual(client.evidence[0]["phase"], "injected")
        self.assertEqual(client.evidence[0]["completed_frame"], 21)

    def test_timeline_drag_seconds_comes_from_one_fresh_clip_rectangle(self):
        client = TimelineGeometryQaClient()

        geometry = client.drag_timeline_by_seconds(
            E2E.CLIP_A1,
            "timeline.clip_edge.left:" + E2E.CLIP_A1,
            1.25,
            steps=12,
        )

        endpoint, payload, evidence = client.injected
        self.assertEqual(endpoint, "drag")
        self.assertEqual(geometry["pixels_per_second"], 50.0)
        self.assertEqual(payload["from"], {"x": 102.5, "y": 55.0})
        self.assertEqual(payload["to"], {"x": 165.0, "y": 55.0})
        self.assertEqual(evidence["component_frame"], 42)
        self.assertEqual(evidence["expected_delta_seconds"], 1.25)
        self.assertEqual(
            evidence["coordinate_reason"],
            "authoritative Timeline pixels_per_second",
        )

    def test_modes_expand_to_the_expected_independent_suites(self):
        self.assertEqual([item.name for item in RUNNER.suite_specs("smoke")], ["smoke"])
        self.assertEqual(
            [item.name for item in RUNNER.suite_specs("full")],
            [
                "all",
                "timeline",
                "selection",
                "keyframe",
                "node-editor",
                "node-reparent",
                "merge-reorder",
                "composition-drop",
                "node-wire",
                "node-wire-selection",
                "implicit-time",
                "preview",
                "preview-trackpad",
                "transform-preview",
            ],
        )
        transform = next(
            item
            for item in RUNNER.suite_specs("full")
            if item.name == "transform-preview"
        )
        self.assertEqual(transform.fixture, "transform_preview_e2e")
        composition_drop = next(
            item
            for item in RUNNER.suite_specs("full")
            if item.name == "composition-drop"
        )
        self.assertEqual(composition_drop.fixture, "composition_drop_e2e")
        with self.assertRaises(ValueError):
            RUNNER.suite_specs("unknown")

    def test_published_endpoint_accepts_only_a_real_ipv4_loopback_port(self):
        self.assertEqual(
            RUNNER.parse_published_endpoint('{"host":"127.0.0.1","port":43123}'),
            ("127.0.0.1", 43123),
        )
        for invalid in (
            '{"host":"0.0.0.0","port":43123}',
            '{"host":"127.0.0.1","port":0}',
            '{"host":"127.0.0.1","port":70000}',
        ):
            with self.assertRaises(ValueError):
                RUNNER.parse_published_endpoint(invalid)

    def test_aggregation_fails_closed(self):
        self.assertFalse(RUNNER.aggregate_ok([]))
        self.assertTrue(RUNNER.aggregate_ok([{"ok": True}, {"ok": True}]))
        self.assertFalse(RUNNER.aggregate_ok([{"ok": True}, {"ok": False}]))
        self.assertFalse(RUNNER.aggregate_ok([{"ok": True}, {}]))

    def test_summary_records_failure_log_without_claiming_pass(self):
        with tempfile.TemporaryDirectory() as directory:
            root = pathlib.Path(directory)
            result = {
                "name": "timeline",
                "ok": False,
                "duration_seconds": 1.25,
                "suite_log": str(root / "timeline" / "suite.log"),
                "error": "coordinate drag failed",
            }
            summary = RUNNER.write_summary(
                root,
                "full",
                {"ok": True},
                [result],
            )
            self.assertFalse(summary["ok"])
            text = (root / "summary.txt").read_text(encoding="utf-8")
            self.assertIn("timeline: FAIL", text)
            self.assertIn("coordinate drag failed", text)
            self.assertNotIn("All suites passed", text)

    def test_process_group_cleanup_terminates_a_live_process(self):
        process = subprocess.Popen(
            [sys.executable, "-c", "import time; time.sleep(60)"],
            start_new_session=True,
        )
        try:
            time.sleep(0.05)
            RUNNER.terminate_process_group(process, grace_seconds=0.2)
            self.assertIsNotNone(process.poll())
        finally:
            RUNNER.terminate_process_group(process, grace_seconds=0.1)


if __name__ == "__main__":
    unittest.main()
