import copy
import unittest

from qa_appearance_persistence import assert_canonical_appearance_graph
from qa_appearance_support import paint_property
from qa_support import QaFailure


def _plugin(component_id, category, operation, input_key, input_type):
    return {
        "content": {
            "type": "PluginOperation",
            "data": {
                "category": category,
                "component_id": component_id,
                "operation": operation,
                "declared_ports": [
                    {
                        "key": input_key,
                        "direction": "Input",
                        "data_type": input_type,
                    },
                    {"key": "image", "direction": "Output", "data_type": "Image"},
                ],
            },
        }
    }


def _connection(edge_id, source, source_port, target, target_port):
    return {
        "id": edge_id,
        "from": {"node_id": source, "port": source_port},
        "to": {"node_id": target, "port": target_port},
        "order": 0,
        "blend_mode": "Normal",
    }


def _project():
    nodes = {
        "text": {"content": {"type": "Generator", "data": "Text"}},
        "fill": _plugin("fill", "style", "style.apply.v1", "shape_in", "Shape"),
        "tile": _plugin("tile", "effect", "effect.apply.v1", "image_in", "Image"),
        "output": {
            "content": {"type": "ModuleOutput", "data": {"id": "output-id"}}
        },
    }
    connections = [
        _connection("shape-fill", "text", "shape", "fill", "shape_in"),
        _connection("fill-tile", "fill", "image", "tile", "image_in"),
        _connection("tile-output", "tile", "image", "output", "image_in"),
    ]
    return {
        "module_definitions": {
            "definition": {"graph": {"nodes": nodes, "connections": connections}}
        }
    }


OPERATIONS = [
    {"id": "fill", "component_id": "fill"},
    {
        "id": "tile",
        "component_id": "tile",
        "category": "effect",
        "operation": "effect.apply.v1",
    },
]


class CanonicalAppearanceGraphTests(unittest.TestCase):
    def test_accepts_shape_fill_then_image_effect(self):
        graph = assert_canonical_appearance_graph(
            _project(), "definition", "output-id", OPERATIONS
        )
        self.assertEqual(graph["operation_ids"], ["fill", "tile"])
        self.assertEqual(graph["shape_source"], ("text", "shape"))

    def test_rejects_wrong_effect_category(self):
        project = copy.deepcopy(_project())
        project["module_definitions"]["definition"]["graph"]["nodes"]["tile"][
            "content"
        ]["data"]["category"] = "style"
        with self.assertRaisesRegex(QaFailure, "expected tile Plugin operation"):
            assert_canonical_appearance_graph(
                project, "definition", "output-id", OPERATIONS
            )


class AppearancePaintTests(unittest.TestCase):
    def test_accepts_canonical_typed_paint_envelope(self):
        paint = {
            "$type": "paint_value",
            "kind": "solid",
            "value": {"space": "srgb", "components": [1.0, 0.0, 0.0, 1.0]},
        }
        operation = {
            "properties": {
                "paint": {"type": "constant", "properties": {"value": paint}}
            }
        }
        self.assertEqual(paint_property(operation), paint)

    def test_rejects_legacy_fill_color_as_paint(self):
        operation = {
            "properties": {
                "color": {
                    "type": "constant",
                    "properties": {"value": [1.0, 0.0, 0.0, 1.0]},
                }
            }
        }
        with self.assertRaisesRegex(QaFailure, "canonical Paint"):
            paint_property(operation)

    def test_rejects_unknown_paint_envelope_field(self):
        operation = {
            "properties": {
                "paint": {
                    "type": "constant",
                    "properties": {
                        "value": {
                            "$type": "paint_value",
                            "kind": "pattern",
                            "value": {},
                            "legacy": True,
                        }
                    },
                }
            }
        }
        with self.assertRaisesRegex(QaFailure, "envelope is not canonical"):
            paint_property(operation)


if __name__ == "__main__":
    unittest.main()
