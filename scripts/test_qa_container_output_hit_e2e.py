import copy
import importlib.util
import os
import unittest


SCRIPT_DIR = os.path.dirname(os.path.abspath(__file__))
MODULE_PATH = os.path.join(SCRIPT_DIR, "qa-container-output-hit-e2e.py")
SPEC = importlib.util.spec_from_file_location(
    "ruvie_qa_container_output_hit_e2e", MODULE_PATH
)
if SPEC is None or SPEC.loader is None:
    raise RuntimeError("cannot load qa-container-output-hit-e2e.py")
MODULE = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(MODULE)


class ContainerOutputHitE2eTests(unittest.TestCase):
    def test_padding_point_is_in_container_but_outside_normal_pin(self):
        source = {
            "min_x": 88.5,
            "min_y": 100.5,
            "max_x": 111.5,
            "max_y": 123.5,
        }
        container = {
            "min_x": 0.0,
            "min_y": 100.0,
            "max_x": 100.0,
            "max_y": 124.0,
        }
        resize = {
            "min_x": 84.0,
            "min_y": 96.0,
            "max_x": 104.0,
            "max_y": 116.0,
        }

        point, normal = MODULE.padded_output_container_point(
            source, source, container, resize
        )

        self.assertTrue(MODULE.point_in_rect(point, source))
        self.assertTrue(MODULE.point_in_rect(point, container))
        self.assertTrue(MODULE.point_in_rect(point, resize))
        self.assertFalse(MODULE.point_in_rect(point, normal))

    def test_only_connection_added_accepts_exact_connection_delta(self):
        before = {"name": "test", "connections": [], "tracks": {"a": {"x": 1}}}
        after = copy.deepcopy(before)
        after["connections"].append({"id": "wire"})

        MODULE.assert_only_connection_added(before, after, "wire")

    def test_only_connection_added_rejects_container_motion(self):
        before = {"connections": [], "tracks": {"a": {"ui_position": [1, 2]}}}
        after = copy.deepcopy(before)
        after["connections"].append({"id": "wire"})
        after["tracks"]["a"]["ui_position"] = [3, 4]

        with self.assertRaises(MODULE.QaFailure):
            MODULE.assert_only_connection_added(before, after, "wire")


if __name__ == "__main__":
    unittest.main()
