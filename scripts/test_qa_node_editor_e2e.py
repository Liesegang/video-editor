import importlib.util
import pathlib
import sys
import unittest


MODULE_PATH = pathlib.Path(__file__).with_name("qa-node-editor-e2e.py")
SPEC = importlib.util.spec_from_file_location("ruvie_qa_node_editor_e2e", MODULE_PATH)
if SPEC is None or SPEC.loader is None:
    raise RuntimeError("cannot load qa-node-editor-e2e.py")
NODE_QA = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = NODE_QA
SPEC.loader.exec_module(NODE_QA)


def transform(scale=1.0, x=10.0, y=20.0):
    return {
        "scale": scale,
        "translation": {"x": x, "y": y},
        "min_scale": 0.0065,
        "max_scale": 1.25,
        "detail_enabled": scale >= 0.18,
        "port_interaction_enabled": scale >= 0.18,
        "resize_interaction_enabled": scale >= 0.12,
    }


class NodeEditorQaTests(unittest.TestCase):
    def test_node_editor_tab_activation_uses_settled_coordinate_click_evidence(self):
        class Client:
            def __init__(self):
                self.calls = []
                self.evidence = []

            def wait_component_settled(self, component_id):
                self.calls.append(("settled", component_id))

            def click_component(self, component_id):
                self.calls.append(("click", component_id))
                self.evidence.append(
                    {
                        "action_id": 7,
                        "endpoint": "click",
                        "component_id": component_id,
                        "component_frame": 41,
                        "component_rect_points": {"center_x": 320.0, "center_y": 24.0},
                    }
                )
                return {"x": 320.0, "y": 24.0}

            def state(self):
                self.calls.append(("state", None))
                return {"frame": 43, "dock": {"active_tabs": ["Node Editor"]}}

            def wait_until(self, description, predicate):
                self.calls.append(("wait", description))
                return predicate()

        client = Client()
        evidence, state = NODE_QA.activate_node_editor_tab(client)

        self.assertEqual(
            client.calls[:3],
            [
                ("settled", NODE_QA.NODE_EDITOR_TAB_ID),
                ("click", NODE_QA.NODE_EDITOR_TAB_ID),
                ("wait", "Node Editor dock activation"),
            ],
        )
        self.assertEqual(evidence["point"], {"x": 320.0, "y": 24.0})
        self.assertEqual(evidence["component_frame"], 41)
        self.assertEqual(evidence["active_frame"], 43)
        self.assertIn("Node Editor", state["dock"]["active_tabs"])

    def test_canvas_metadata_contract_reads_finite_final_transform(self):
        value = transform(scale=0.0065, x=321.5, y=-87.25)
        component = {"id": NODE_QA.CANVAS_ID, "metadata": value}

        self.assertEqual(NODE_QA.canvas_transform(component), value)

        invalid = {"id": NODE_QA.CANVAS_ID, "metadata": dict(value)}
        invalid["metadata"]["scale"] = float("nan")
        with self.assertRaises(NODE_QA.QaFailure):
            NODE_QA.canvas_transform(invalid)

    def test_minimum_zoom_requires_real_decrease_clamp_and_disabled_precision(self):
        before = transform(scale=1.0)
        overview = transform(scale=0.0065, x=400.0, y=300.0)

        NODE_QA.assert_minimum_zoom(before, overview)

        with self.assertRaises(NODE_QA.QaFailure):
            NODE_QA.assert_minimum_zoom(overview, overview)
        not_clamped = transform(scale=0.01)
        with self.assertRaises(NODE_QA.QaFailure):
            NODE_QA.assert_minimum_zoom(before, not_clamped)

    def test_primary_pan_changes_only_translation_by_screen_delta(self):
        before = transform(scale=0.0065, x=400.0, y=300.0)
        after = transform(scale=0.0065, x=512.0, y=364.0)

        self.assertEqual(
            NODE_QA.assert_only_translation_changed(
                before, after, {"x": 112.0, "y": 64.0}
            ),
            {"x": 112.0, "y": 64.0},
        )

        changed_scale = dict(after)
        changed_scale["scale"] = 0.01
        with self.assertRaises(NODE_QA.QaFailure):
            NODE_QA.assert_only_translation_changed(
                before, changed_scale, {"x": 112.0, "y": 64.0}
            )

    def test_pan_origin_comes_from_canvas_and_avoids_visible_node_chrome(self):
        canvas_rect = {
            "min_x": 0.0,
            "min_y": 0.0,
            "max_x": 1000.0,
            "max_y": 800.0,
            "width": 1000.0,
            "height": 800.0,
            "center_x": 500.0,
            "center_y": 400.0,
        }
        first_candidate_obstacle = {
            "min_x": 210.0,
            "min_y": 182.0,
            "max_x": 230.0,
            "max_y": 202.0,
            "width": 20.0,
            "height": 20.0,
        }
        snapshot = {
            "frame": 42,
            "components": [
                {
                    "id": NODE_QA.CANVAS_ID,
                    "visible": True,
                    "rect_points": canvas_rect,
                },
                {
                    "id": "node_editor.node:blocked",
                    "visible": True,
                    "rect_points": first_candidate_obstacle,
                },
            ],
        }

        start, end = NODE_QA.find_primary_pan_gesture(snapshot)

        self.assertFalse(NODE_QA.point_in_rect(start, first_candidate_obstacle, 6.0))
        self.assertTrue(NODE_QA.point_in_rect(start, canvas_rect))
        self.assertTrue(NODE_QA.point_in_rect(end, canvas_rect))
        self.assertEqual(end["x"] - start["x"], 112.0)
        self.assertEqual(end["y"] - start["y"], 64.0)

    def test_header_metadata_keeps_selection_separate_from_lod_move_gate(self):
        def snapshot(move_enabled):
            return {
                "frame": 42,
                "components": [
                    {
                        "id": "node_editor.node_header:node",
                        "visible": True,
                        "enabled": True,
                        "metadata": {
                            "selection_enabled": True,
                            "move_enabled": move_enabled,
                        },
                    },
                    {
                        "id": "node_editor.container_move_header.clip:clip",
                        "visible": True,
                        "enabled": move_enabled,
                        "metadata": {
                            "selection_enabled": True,
                            "move_enabled": move_enabled,
                        },
                    },
                ],
            }

        detail = NODE_QA.assert_header_interaction_metadata(snapshot(True), True)
        overview = NODE_QA.assert_header_interaction_metadata(snapshot(False), False)
        self.assertTrue(detail["move_enabled"])
        self.assertFalse(overview["move_enabled"])

        invalid = snapshot(False)
        invalid["components"][1]["enabled"] = True
        with self.assertRaises(NODE_QA.QaFailure):
            NODE_QA.assert_header_interaction_metadata(invalid, False)

    def test_navigation_state_guard_rejects_selection_or_pending_navigation(self):
        initial = {
            "project": {"nodes": {"node": {"ui_position": [10.0, 20.0]}}},
            "history": {"undo_depth": 1, "redo_depth": 0},
            "editor": {
                "selection": {"targets": [], "primary": None},
                "node_editor": {"context_menu_open": False, "pending_navigation": None},
            },
        }
        final = {
            "project": initial["project"],
            "history": initial["history"],
            "editor": {
                "selection": dict(initial["editor"]["selection"]),
                "node_editor": dict(initial["editor"]["node_editor"]),
            },
        }
        NODE_QA.assert_navigation_state_unchanged(initial, final)

        final["editor"]["selection"]["primary"] = {
            "kind": "node",
            "id": "node",
        }
        with self.assertRaises(NODE_QA.QaFailure):
            NODE_QA.assert_navigation_state_unchanged(initial, final)
        final["editor"]["selection"]["primary"] = None
        final["editor"]["node_editor"]["pending_navigation"] = "composition"
        with self.assertRaises(NODE_QA.QaFailure):
            NODE_QA.assert_navigation_state_unchanged(initial, final)


if __name__ == "__main__":
    unittest.main()
