import importlib.util
import pathlib
import sys
import unittest


MODULE_PATH = pathlib.Path(__file__).with_name("qa-e2e.py")
SPEC = importlib.util.spec_from_file_location("ruvie_qa_e2e_wire", MODULE_PATH)
if SPEC is None or SPEC.loader is None:
    raise RuntimeError("cannot load qa-e2e.py")
QA = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = QA
SPEC.loader.exec_module(QA)


def rect(min_x, min_y, max_x, max_y):
    return {
        "min_x": min_x,
        "min_y": min_y,
        "max_x": max_x,
        "max_y": max_y,
        "width": max_x - min_x,
        "height": max_y - min_y,
        "center_x": (min_x + max_x) / 2.0,
        "center_y": (min_y + max_y) / 2.0,
    }


class NodeWireQaTests(unittest.TestCase):
    def test_wire_hit_click_uses_fresh_bezier_point_instead_of_bbox_center(self):
        component = {
            "id": "node_editor.edge.derived:track:clip",
            "rect_points": rect(10.0, 20.0, 110.0, 80.0),
            "metadata": {"hit_point": {"x": 35.0, "y": 52.0}},
        }

        class Client:
            injected = None

            def component(self, component_id):
                if component_id != component["id"]:
                    raise AssertionError("unexpected component id")
                return {"frame": 17}, component

            def inject(self, endpoint, payload, evidence):
                self.injected = (endpoint, payload, evidence)

        client = Client()
        snapshot, clicked_component, point = QA.click_node_wire_hit_point(
            client, component["id"]
        )

        self.assertEqual(snapshot["frame"], 17)
        self.assertIs(clicked_component, component)
        self.assertEqual(point, {"x": 35.0, "y": 52.0})
        self.assertEqual(client.injected[0], "click")
        self.assertEqual(client.injected[1]["x"], 35.0)
        self.assertEqual(client.injected[1]["y"], 52.0)
        self.assertEqual(client.injected[1]["button"], "secondary")
        self.assertEqual(client.injected[2]["component_frame"], 17)

    def test_free_canvas_wire_distance_uses_rendered_bezier_not_only_midpoint(self):
        wire = {
            "metadata": {
                "kind": "output_binding",
                "overview_painted": False,
                "from": {"x": 100.0, "y": 80.0},
                "to": {"x": 300.0, "y": 160.0},
            }
        }

        self.assertTrue(
            QA.point_near_node_wire({"x": 120.0, "y": 82.0}, wire)
        )
        self.assertFalse(
            QA.point_near_node_wire({"x": 200.0, "y": 20.0}, wire)
        )

    def test_node_editor_reveal_plans_a_bounded_two_axis_pan(self):
        canvas = rect(0.0, 0.0, 300.0, 200.0)
        targets = [rect(330.0, 220.0, 350.0, 240.0)]

        self.assertEqual(
            QA.node_editor_pan_delta(canvas, targets, margin=10.0),
            (-190.0, -130.0),
        )

    def test_node_editor_reveal_rejects_a_union_larger_than_the_canvas(self):
        canvas = rect(0.0, 0.0, 300.0, 200.0)
        targets = [rect(0.0, 0.0, 290.0, 20.0)]

        with self.assertRaises(QA.QaFailure):
            QA.node_editor_pan_delta(canvas, targets, margin=10.0)

    def test_connection_lookup_uses_tagged_owner_and_ports(self):
        connection = {
            "id": "wire",
            "from": {
                "owner": {"owner_type": "Node", "owner_id": "source"},
                "port": "image",
            },
            "to": {
                "owner": {"owner_type": "Node", "owner_id": "target"},
                "port": "images",
            },
            "order": 3,
        }
        project = {"connections": [connection]}

        self.assertIs(
            QA.find_project_connection(
                project,
                "Node",
                "source",
                "image",
                "Node",
                "target",
                "images",
            ),
            connection,
        )
        with self.assertRaises(QA.QaFailure):
            QA.find_project_connection(
                project, "Clip", "source", "image", "Node", "target", "images"
            )

    def test_knife_planner_starts_blank_and_crosses_two_edge_hit_points(self):
        canvas = rect(0.0, 0.0, 300.0, 200.0)
        first_point = {"x": 90.0, "y": 70.0}
        second_point = {"x": 210.0, "y": 130.0}
        snapshot = {
            "frame": 9,
            "components": [
                {
                    "id": "node_editor.canvas",
                    "visible": True,
                    "rect_points": canvas,
                },
                {
                    "id": "node_editor.edge:boundary",
                    "visible": True,
                    "rect_points": rect(0.0, 42.0, 13.0, 58.0),
                    "metadata": {
                        "kind": "explicit",
                        "connection_id": "boundary",
                        "hit_point": {"x": 5.0, "y": 50.0},
                    },
                },
                {
                    "id": "node_editor.edge:first",
                    "visible": True,
                    "rect_points": rect(82.0, 62.0, 98.0, 78.0),
                    "metadata": {
                        "kind": "explicit",
                        "connection_id": "first",
                        "hit_point": first_point,
                    },
                },
                {
                    "id": "node_editor.edge:second",
                    "visible": True,
                    "rect_points": rect(202.0, 122.0, 218.0, 138.0),
                    "metadata": {
                        "kind": "explicit",
                        "connection_id": "second",
                        "hit_point": second_point,
                    },
                },
                {
                    "id": "node_editor.node:unrelated",
                    "visible": True,
                    "rect_points": rect(120.0, 10.0, 170.0, 45.0),
                },
            ],
        }

        start, end, connection_ids = QA.find_wire_knife_gesture(snapshot)

        self.assertEqual(connection_ids, ["first", "second"])
        self.assertTrue(QA.point_in_component_rect(start, canvas))
        self.assertTrue(QA.point_in_component_rect(end, canvas))
        for component in snapshot["components"][1:]:
            self.assertFalse(
                QA.point_in_component_rect(start, component["rect_points"], 5.0)
            )

    def test_explicit_wire_snapshot_ids_ignore_handles_and_derived_edges(self):
        snapshot = {
            "components": [
                {
                    "id": "node_editor.edge:wire",
                    "metadata": {"kind": "explicit", "connection_id": "wire"},
                },
                {
                    "id": "node_editor.edge:wire.from_handle",
                    "metadata": {"connection_id": "wire", "endpoint": "source"},
                },
                {
                    "id": "node_editor.edge.derived:track:clip",
                    "metadata": {"kind": "derived_output", "connection_id": None},
                },
            ]
        }

        self.assertEqual(QA.explicit_wire_connection_ids(snapshot), {"wire"})

    def test_mixed_knife_planner_targets_binding_and_explicit_wire(self):
        canvas = rect(0.0, 0.0, 300.0, 200.0)
        binding_id = "node_editor.edge.output_binding:clip:owner:result"
        snapshot = {
            "frame": 10,
            "components": [
                {
                    "id": "node_editor.canvas",
                    "visible": True,
                    "rect_points": canvas,
                },
                {
                    "id": binding_id,
                    "visible": True,
                    "rect_points": rect(82.0, 62.0, 98.0, 78.0),
                    "metadata": {
                        "kind": "output_binding",
                        "editable": True,
                        "action": "delete_output_binding",
                        "binding_owner": "clip:owner",
                        "binding_node_id": "result",
                        "hit_point": {"x": 90.0, "y": 70.0},
                    },
                },
                {
                    "id": "node_editor.edge:wire",
                    "visible": True,
                    "rect_points": rect(202.0, 122.0, 218.0, 138.0),
                    "metadata": {
                        "kind": "explicit",
                        "connection_id": "wire",
                        "hit_point": {"x": 210.0, "y": 130.0},
                    },
                },
                {
                    "id": "node_editor.edge.derived:track:clip",
                    "visible": True,
                    "rect_points": rect(130.0, 20.0, 160.0, 35.0),
                    "metadata": {
                        "kind": "derived_output",
                        "editable": False,
                        "edit_blocked_reason": "authoritative containment",
                    },
                },
            ],
        }

        start, end, planned = QA.find_mixed_wire_knife_gesture(
            snapshot, binding_id
        )

        self.assertEqual(planned["binding_edge_id"], binding_id)
        self.assertEqual(planned["binding_owner"], "clip:owner")
        self.assertEqual(planned["binding_node_id"], "result")
        self.assertEqual(planned["connection_id"], "wire")
        self.assertTrue(QA.point_in_component_rect(start, canvas))
        self.assertTrue(QA.point_in_component_rect(end, canvas))
        for component in snapshot["components"][1:]:
            self.assertFalse(
                QA.point_in_component_rect(start, component["rect_points"], 5.0)
            )


if __name__ == "__main__":
    unittest.main()
