#!/usr/bin/env python3
import importlib.util
import pathlib
import unittest


SCRIPT = pathlib.Path(__file__).with_name("qa-vector-shape-e2e.py")


class VectorShapeQaTests(unittest.TestCase):
    def test_script_uses_coordinate_bridge_without_model_mutation_endpoint(self):
        source = SCRIPT.read_text(encoding="utf-8")
        self.assertIn("preview.vector.point:", source)
        self.assertIn("preview.vector.handle_out:", source)
        self.assertIn("preview.vector.mode.smooth", source)
        self.assertNotIn("set_project", source)
        self.assertNotIn("/v1/command", source)

    def test_path_parser_keeps_curved_close_as_one_logical_first_point(self):
        spec = importlib.util.spec_from_file_location("qa_vector_shape", SCRIPT)
        self.assertIsNotNone(spec)
        self.assertIsNotNone(spec.loader)
        module = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(module)
        points = module.parse_path("M 0,0 L 100,0 C 100,80 -20,60 0,0 Z")
        self.assertEqual(len(points), 2)
        self.assertEqual(points[0]["handle_in"], [-20.0, 60.0])


if __name__ == "__main__":
    unittest.main()
