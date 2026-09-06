"""Exact-route assertions for the shared native Node Editor QA helpers."""

import unittest
from unittest import mock

import qa_node_module_support as support


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


if __name__ == "__main__":
    unittest.main()
