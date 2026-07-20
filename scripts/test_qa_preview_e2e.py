import importlib.util
import pathlib
import sys
import unittest


MODULE_PATH = pathlib.Path(__file__).with_name("qa-preview-e2e.py")
SPEC = importlib.util.spec_from_file_location("ruvie_qa_preview_e2e", MODULE_PATH)
if SPEC is None or SPEC.loader is None:
    raise RuntimeError("cannot load qa-preview-e2e.py")
PREVIEW_QA = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = PREVIEW_QA
SPEC.loader.exec_module(PREVIEW_QA)


def rect(min_x, min_y, width, height):
    return {
        "min_x": min_x,
        "min_y": min_y,
        "max_x": min_x + width,
        "max_y": min_y + height,
        "width": width,
        "height": height,
        "center_x": min_x + width * 0.5,
        "center_y": min_y + height * 0.5,
    }


def snapshot(
    pan_x=80.0,
    pan_y=90.0,
    zoom=1.0,
    auto_fit=True,
    primary_gesture="Idle",
    frame=42,
):
    canvas = rect(100.0, 50.0, 800.0, 600.0)
    content = rect(canvas["min_x"] + pan_x, canvas["min_y"] + pan_y, 640.0 * zoom, 360.0 * zoom)
    return {
        "frame": frame,
        "components": [
            {
                "id": PREVIEW_QA.CANVAS_ID,
                "enabled": True,
                "visible": True,
                "rect_points": canvas,
                "metadata": {
                    "composition_id": "composition",
                    "pan": {"x": pan_x, "y": pan_y},
                    "zoom": zoom,
                    "auto_fit": auto_fit,
                    "primary_gesture": primary_gesture,
                },
            },
            {
                "id": PREVIEW_QA.CONTENT_ID,
                "enabled": True,
                "visible": True,
                "rect_points": content,
                "metadata": {
                    "composition_id": "composition",
                    "canvas_width": 640,
                    "canvas_height": 360,
                    "pan": {"x": pan_x, "y": pan_y},
                    "zoom": zoom,
                    "auto_fit": auto_fit,
                },
            },
        ],
    }


def state(project=None, history=None, selection=None, timeline=None):
    return {
        "project": {"name": "qa"} if project is None else project,
        "history": {"undo_depth": 1, "redo_depth": 0} if history is None else history,
        "editor": {
            "selection": {"targets": [], "primary": None}
            if selection is None
            else selection,
            "timeline": {"current_time": 2.0, "is_playing": False}
            if timeline is None
            else timeline,
        },
    }


class PreviewQaTests(unittest.TestCase):
    def test_geometry_contract_matches_published_camera(self):
        geometry = PREVIEW_QA.preview_geometry(snapshot())

        self.assertEqual(geometry["composition_id"], "composition")
        self.assertEqual(geometry["pan"], {"x": 80.0, "y": 90.0})
        self.assertEqual(geometry["zoom"], 1.0)
        self.assertEqual(geometry["primary_gesture"], "Idle")

        invalid = snapshot()
        invalid["components"][1]["rect_points"]["min_x"] += 5.0
        with self.assertRaises(PREVIEW_QA.QaFailure):
            PREVIEW_QA.preview_geometry(invalid)

    def test_initial_fit_requires_canvas_and_content_centers_to_match(self):
        centered = snapshot(pan_x=80.0, pan_y=120.0)
        PREVIEW_QA.assert_centered(PREVIEW_QA.preview_geometry(centered))

        off_center = snapshot(pan_x=60.0, pan_y=120.0)
        with self.assertRaises(PREVIEW_QA.QaFailure):
            PREVIEW_QA.assert_centered(PREVIEW_QA.preview_geometry(off_center))

    def test_space_pan_changes_only_pan_and_content_translation(self):
        before = PREVIEW_QA.preview_geometry(snapshot(pan_x=80.0, pan_y=120.0))
        after = PREVIEW_QA.preview_geometry(
            snapshot(pan_x=152.0, pan_y=164.0, auto_fit=False, frame=52)
        )

        self.assertEqual(
            PREVIEW_QA.assert_space_pan(before, after),
            PREVIEW_QA.PAN_DELTA,
        )
        self.assertTrue(PREVIEW_QA.pan_matches(before, after, PREVIEW_QA.PAN_DELTA))

        changed_zoom = PREVIEW_QA.preview_geometry(
            snapshot(pan_x=152.0, pan_y=164.0, zoom=1.1, auto_fit=False, frame=53)
        )
        with self.assertRaises(PREVIEW_QA.QaFailure):
            PREVIEW_QA.assert_space_pan(before, changed_zoom)

    def test_pan_plan_uses_content_center_and_stays_inside_canvas(self):
        geometry = PREVIEW_QA.preview_geometry(snapshot(pan_x=80.0, pan_y=120.0))
        start, end = PREVIEW_QA.plan_space_pan(geometry)

        self.assertEqual(start, {"x": 500.0, "y": 350.0})
        self.assertEqual(end["x"] - start["x"], PREVIEW_QA.PAN_DELTA["x"])
        self.assertEqual(end["y"] - start["y"], PREVIEW_QA.PAN_DELTA["y"])

    def test_navigation_guard_rejects_project_history_selection_or_timeline_changes(self):
        initial = state()
        PREVIEW_QA.assert_navigation_state_unchanged(initial, state())

        for changed in (
            state(project={"name": "changed"}),
            state(history={"undo_depth": 2, "redo_depth": 0}),
            state(
                selection={
                    "targets": [{"kind": "node", "id": "node"}],
                    "primary": {"kind": "node", "id": "node"},
                }
            ),
            state(timeline={"current_time": 2.0, "is_playing": True}),
        ):
            with self.assertRaises(PREVIEW_QA.QaFailure):
                PREVIEW_QA.assert_navigation_state_unchanged(initial, changed)


if __name__ == "__main__":
    unittest.main()
