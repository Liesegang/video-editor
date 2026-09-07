"""Exact-route assertions for the shared native Node Editor QA helpers."""

import copy
import pathlib
import runpy
import unittest
from unittest import mock

import qa_node_module_support as support
import qa_support


class NodeSuiteEntrypointTests(unittest.TestCase):
    def test_point_size_uses_shared_cli_and_propagates_exit_code(self):
        script = pathlib.Path(__file__).with_name("qa-point-size-e2e.py")
        with mock.patch.object(qa_support, "run_suite_main", autospec=True, return_value=37) as main:
            with self.assertRaises(SystemExit) as exited:
                runpy.run_path(str(script), run_name="__main__")
        self.assertEqual(exited.exception.code, 37)
        main.assert_called_once()
        name, suite, evidence = main.call_args.args
        self.assertEqual(name, "qa-point-size-e2e")
        self.assertTrue(callable(suite))
        self.assertEqual(evidence, "target/qa-point-size-e2e-evidence.json")


def route(connection_id, source_port, target_port):
    return {
        "id": connection_id,
        "from": {"node_id": "source", "port": source_port},
        "to": {"node_id": "target", "port": target_port},
    }


class NodeRouteTests(unittest.TestCase):
    def connect(self, connections):
        client = mock.Mock()
        client.wait_until.side_effect = lambda _description, predicate: predicate()
        definition = {"graph": {"connections": connections}}
        source = {"rect_points": {"center_x": 20.0, "center_y": 30.0}}
        target = {"rect_points": {"center_x": 120.0, "center_y": 60.0}}
        with (
            mock.patch.object(support, "port", side_effect=[source, target]),
            mock.patch.object(support, "active_definition", return_value=("definition", definition)),
        ):
            result = support.connect_nodes(
                client, "node_clip", "source", "attribute", "target", "factor", "field"
            )
        client.drag.assert_called_once_with(
            {"x": 20.0, "y": 30.0}, {"x": 120.0, "y": 60.0}, steps=10
        )
        return result

    def test_connection_matches_ports_not_only_node_pair(self):
        expected = route("field", "attribute", "factor")
        connections = [
            route("other-source", "points", "factor"),
            route("other-target", "attribute", "points"),
            expected,
        ]
        self.assertEqual(self.connect(connections), expected)

    def test_different_ports_do_not_satisfy_connection_wait(self):
        self.assertIsNone(self.connect([route("other", "points", "points")]))

    def test_disconnect_waits_for_exact_id_without_requiring_empty_pair(self):
        client = mock.Mock()
        client.wait_until.side_effect = lambda _description, predicate: predicate()
        state = {"history": {"revision": 2}}
        client.state.return_value = state
        definition = {"graph": {"connections": [route("retained", "points", "points")]}}
        with mock.patch.object(support, "active_definition", return_value=("definition", definition)):
            result = support.disconnect_node_connection(client, "node_clip", "removed", "field")
        self.assertIs(result, state)
        self.assertEqual(
            client.click_component.call_args_list,
            [
                mock.call("node_editor.connection:removed", button="secondary"),
                mock.call("node_editor.wire_menu.disconnect"),
            ],
        )

    def test_header_drag_refuses_another_nodes_body_at_the_origin(self):
        client = mock.Mock()
        canvas = {"rect_points": {"min_x": 0.0, "min_y": 0.0, "width": 800.0}}
        header = {"rect_points": {"center_x": 40.0, "center_y": 30.0}}
        snapshot = {
            "components": [{
                "id": "node_editor.node:blocker",
                "visible": True,
                "rect_points": {"min_x": 20.0, "max_x": 60.0, "min_y": 10.0, "max_y": 50.0},
            }]
        }
        client.wait_component_settled.side_effect = [(snapshot, canvas), (snapshot, header)]
        with self.assertRaisesRegex(support.QaFailure, "drag origin is occluded"):
            support.place_created_node(client, "requested", 0.5)
        client.drag.assert_not_called()


class PreviewSamplingTests(unittest.TestCase):
    def fixture(self):
        metadata = {
            "rendered_revision": 7,
            "rendered_frame": 30,
            "nontransparent_pixels": 0,
            "pixel_hash": "empty-frame",
            "render_in_flight_request": None,
            "render_desired_pending": False,
        }
        state = {
            "history": {"revision": 7},
            "editor": {
                "timeline": {"current_frame": 30},
                "preview": copy.deepcopy(metadata),
                "error": None,
            },
        }
        client = mock.Mock()
        client.component_snapshot.return_value = {
            "components": [{"id": "preview.canvas", "metadata": metadata}]
        }
        client.state.return_value = state
        return client, metadata, state

    def test_empty_preview_requires_explicit_opt_in(self):
        client, _, state = self.fixture()
        self.assertIsNone(support.settled_preview_state(client, 7, 30))
        self.assertIs(
            support.settled_preview_state(client, 7, 30, require_visible=False), state
        )

    def test_empty_preview_still_requires_exact_settled_component(self):
        for key, value in (
            ("rendered_revision", 6), ("rendered_frame", 29), ("pixel_hash", None),
            ("render_in_flight_request", 1), ("render_desired_pending", True),
            ("render_desired_pending", None),
        ):
            with self.subTest(key=key, value=value):
                client, metadata, _ = self.fixture()
                metadata[key] = value
                self.assertIsNone(
                    support.settled_preview_state(client, 7, 30, require_visible=False)
                )

    def test_empty_preview_still_requires_matching_error_free_editor_state(self):
        for section, key, value in (
            ("history", "revision", 8), ("timeline", "current_frame", 29),
            ("preview", "rendered_revision", 6), ("preview", "rendered_frame", 29),
            ("preview", "pixel_hash", "other-frame"), ("editor", "error", "failed"),
        ):
            with self.subTest(section=section, key=key):
                client, _, state = self.fixture()
                target = state[section] if section in ("history", "editor") else state["editor"][section]
                target[key] = value
                self.assertIsNone(
                    support.settled_preview_state(client, 7, 30, require_visible=False)
                )

    def test_shared_sampler_can_return_verified_empty_pixels(self):
        client, _, state = self.fixture()
        client.wait_until.side_effect = lambda _description, predicate, _timeout: predicate()
        with (
            mock.patch.object(support, "activate_dock_tab"),
            mock.patch.object(support, "seek_timeline_seconds", return_value=state),
        ):
            sampled = support.sample_rendered_preview(client, 1.0, 7, "zero size", require_visible=False)
        self.assertEqual(sampled["frame"], 30)
        self.assertEqual(sampled["pixel_hash"], "empty-frame")
        self.assertEqual(sampled["nontransparent_pixels"], 0)


if __name__ == "__main__":
    unittest.main()
