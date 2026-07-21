import pathlib
import sys
import unittest


sys.path.insert(0, str(pathlib.Path(__file__).parent))
import qa_sound_graph_actions as ACTIONS  # noqa: E402


class FakeBase:
    class QaFailure(RuntimeError):
        pass

    @staticmethod
    def reveal_node_editor_component(_client, _component_id):
        return None


def rect(min_x, min_y, max_x, max_y):
    return {
        "min_x": min_x,
        "min_y": min_y,
        "max_x": max_x,
        "max_y": max_y,
        "width": max_x - min_x,
        "height": max_y - min_y,
    }


def edge(component_id="node_editor.edge:wire", y=100.0):
    return {
        "id": component_id,
        "visible": True,
        "rect_points": rect(20.0, y - 8.0, 280.0, y + 8.0),
        "metadata": {
            "kind": "explicit",
            "from": {"x": 20.0, "y": y},
            "control_a": {"x": 90.0, "y": y},
            "control_b": {"x": 210.0, "y": y},
            "to": {"x": 280.0, "y": y},
        },
    }


class SoundGraphActionTests(unittest.TestCase):
    def test_unobstructed_wire_point_avoids_node_chrome_and_other_wires(self):
        blocker = {
            "id": "node_editor.node:cover",
            "visible": True,
            "rect_points": rect(110.0, 80.0, 190.0, 120.0),
        }
        snapshot = {
            "frame": 9,
            "components": [
                {
                    "id": "node_editor.canvas",
                    "visible": True,
                    "rect_points": rect(0.0, 0.0, 320.0, 220.0),
                },
                edge(),
                edge("node_editor.edge:other", y=130.0),
                blocker,
            ],
        }

        point = ACTIONS.unobstructed_wire_point(
            FakeBase, snapshot, "node_editor.edge:wire"
        )

        self.assertAlmostEqual(point["y"], 100.0)
        self.assertFalse(110.0 <= point["x"] <= 190.0)

    def test_connection_selection_injects_a_fresh_curve_coordinate(self):
        snapshot = {
            "frame": 17,
            "components": [
                {
                    "id": "node_editor.canvas",
                    "visible": True,
                    "rect_points": rect(0.0, 0.0, 320.0, 220.0),
                },
                edge(),
            ],
        }

        class Client:
            selected = None
            injected = None

            def component_snapshot(self):
                return snapshot

            def inject(self, endpoint, payload, evidence):
                self.injected = (endpoint, payload, evidence)
                self.selected = "wire"

            def state(self):
                return {
                    "editor": {
                        "node_editor": {"selected_connection_id": self.selected}
                    }
                }

            def wait_until(self, _description, predicate):
                value = predicate()
                if value is None:
                    raise AssertionError("selection predicate did not pass")
                return value

        client = Client()
        ACTIONS.select_connection_wire(FakeBase, client, "wire")

        self.assertEqual(client.injected[0], "click")
        self.assertEqual(client.injected[1]["button"], "primary")
        self.assertEqual(client.injected[2]["component_frame"], 17)
        self.assertEqual(
            client.injected[2]["coordinate_reason"],
            "fresh unobstructed unique cubic point",
        )

    def test_wire_identity_assertion_rejects_endpoint_replacement(self):
        original = {
            "id": "wire",
            "from": "source-a",
            "to": "target",
            "blend_mode": "normal",
            "order": 0,
        }
        changed = dict(original, from_="source-b")
        changed["from"] = changed.pop("from_")

        with self.assertRaises(FakeBase.QaFailure):
            ACTIONS.assert_wire_identity(
                FakeBase, [original], [changed], "reconnect regression"
            )


if __name__ == "__main__":
    unittest.main()
