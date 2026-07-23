import importlib.util
import pathlib
import sys
import unittest


MODULE_PATH = pathlib.Path(__file__).with_name("qa-reparent-e2e.py")
SPEC = importlib.util.spec_from_file_location("ruvie_qa_reparent_e2e", MODULE_PATH)
if SPEC is None or SPEC.loader is None:
    raise RuntimeError("cannot load qa-reparent-e2e.py")
E2E = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = E2E
SPEC.loader.exec_module(E2E)


class ReparentE2ETests(unittest.TestCase):
    def test_centered_drop_preserves_the_physical_header_grab_offset(self):
        node = {
            "min_x": 100.0,
            "min_y": 200.0,
            "width": 240.0,
            "height": 160.0,
        }
        source_pointer = {"x": 160.0, "y": 220.0}
        target = {"center_x": 800.0, "center_y": 500.0}

        point = E2E.point_for_centered_node(node, source_pointer, target)

        self.assertEqual(point, {"x": 740.0, "y": 440.0})

    def test_owner_and_wire_helpers_read_only_authoritative_project_state(self):
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
                    "trim_in": 0.0,
                    "time_stretch": 1.0,
                    "node_ids": ["source", "target"],
                    "output_node_id": "target",
                }
            },
            "nodes": {"source": {}, "target": {}},
            "connections": [
                {
                    "id": "wire",
                    "from": {
                        "owner": {"owner_type": "Node", "owner_id": "source"},
                        "port": "image",
                    },
                    "to": {
                        "owner": {"owner_type": "Node", "owner_id": "target"},
                        "port": "images",
                    },
                }
            ],
        }

        self.assertEqual(E2E.owner_for_node(project, "target"), "clip:clip")
        self.assertEqual(
            E2E.connection_by_endpoints(project, "source", "target")["id"],
            "wire",
        )

    def test_suite_requires_real_pointer_lifecycle_and_published_target_geometry(self):
        source = MODULE_PATH.read_text(encoding="utf-8")
        for endpoint in ('"press"', '"move"', '"release"'):
            self.assertIn(endpoint, source)
        self.assertIn('client.key("escape", True)', source)
        self.assertIn("assert_cancelled_move_state", source)
        self.assertIn("node_editor.reparent_target.clip:", source)
        self.assertIn("source_header_rect_points", source)
        self.assertIn("target_content_rect_points", source)
        self.assertNotIn("set_project", source)
        self.assertNotIn("/v1/command", source)


if __name__ == "__main__":
    unittest.main()
