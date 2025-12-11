use pyo3::prelude::*;
use pyo3::types::{PyDict, PyString, PyTuple, PyList, PyDict as PyDictType};
use pyo3::ffi::PyThreadState; // Use PyThreadState from ffi
use crate::model::project::property::{Property, PropertyValue, Vec2};
use crate::model::frame::color::Color;
use crate::plugin::{EvaluationContext, Plugin, PluginCategory, PluginId, PropertyEvaluator, PropertyPlugin};
use ordered_float::OrderedFloat;
use std::sync::Arc;


// This will be the Rust-side representation of our Python expression evaluator.
// For now, it's a placeholder.
pub struct PythonExpressionEvaluator;

impl PythonExpressionEvaluator {
    pub fn new() -> Self {
        PythonExpressionEvaluator {}
    }
}

impl PropertyEvaluator for PythonExpressionEvaluator {
    fn evaluate(&self, property: &Property, time: f64, _ctx: &EvaluationContext) -> PropertyValue { // Using _ctx for now
        Python::with_gil(|py| {
            // Placeholder for frame and fps until EvaluationContext is properly extended
            // TODO: Get actual frame and fps from ctx
            let frame = time; // Example: assuming frame is same as time for now
            let fps = 60.0; // Example: assuming 60 fps for now
            let expression = property.evaluator.as_str();

            let locals = PyDict::new(py);
            locals.set_item("time", time).expect("Failed to set 'time' in Python locals");
            locals.set_item("frame", frame).expect("Failed to set 'frame' in Python locals");
            locals.set_item("fps", fps).expect("Failed to set 'fps' in Python locals");

            let result = PyModule::import(py, "builtins")
                .expect("Failed to import builtins")
                .getattr("eval")
                .expect("Failed to get 'eval' from builtins")
                .call1((PyString::new(py, expression).into_any(), py.None(), Some(locals)))
                .expect("Failed to call Python eval function");
            
            convert_pyobject_to_property_value(&result.extract().expect("Failed to extract Python result")).expect("Failed to convert Python object to PropertyValue")
        })
    }
}

// A simple Python module that we can embed, mostly for testing the pyo3 setup.
#[pymodule]
fn expression_evaluator_python(_py: Python, m: &Bound<PyModule>) -> PyResult<()> {
    #[pyfunction]
    fn evaluate_expression(py: Python, expression: String, time: f64, frame: f64, fps: f64) -> PyResult<PyObject> {
        let locals = PyDict::new(py);
        locals.set_item("time", time)?;
        locals.set_item("frame", frame)?;
        locals.set_item("fps", fps)?;

        let result = PyModule::import(py, "builtins")?.getattr("eval")?.call1((PyString::new(py, &expression).into_any(), py.None(), Some(&locals)))?;
        Ok(result.into_pyobject(py)?.into())
    }

    m.add_function(wrap_pyfunction!(evaluate_expression, m)?)?;
    Ok(())
}

// Helper function to convert Rust PropertyValue to Python PyObject
fn convert_property_value_to_pyobject(py: Python, value: &PropertyValue) -> PyResult<PyObject> {
    match value {
        PropertyValue::Number(n) => Ok(n.into_inner().into_pyobject(py)?.into()),
        PropertyValue::Integer(i) => Ok(i.into_pyobject(py)?.into()),
        PropertyValue::String(s) => Ok(s.into_pyobject(py)?.into()),
        PropertyValue::Boolean(b) => Ok(b.into_pyobject(py)?),
        PropertyValue::Vec2(v) => Ok(PyTuple::new(py, &[v.x.into_pyobject(py)?.into(), v.y.into_pyobject(py)?.into()])?.into()),
        PropertyValue::Color(c) => Ok(PyTuple::new(py, &[c.r.into_pyobject(py)?.into(), c.g.into_pyobject(py)?.into(), c.b.into_pyobject(py)?.into(), c.a.into_pyobject(py)?.into()])?.into()),
        PropertyValue::Array(arr) => {
            let list = PyList::new(py, Vec::<PyObject>::new())?;
            for item in arr {
                list.append(convert_property_value_to_pyobject(py, item)?)?;
            }
            Ok(list.into())
        }
        PropertyValue::Map(map) => {
            let dict = PyDictType::new(py); // Using aliased type to avoid conflict
            for (k, v) in map {
                dict.set_item(k, convert_property_value_to_pyobject(py, v)?)?;
            }
            Ok(dict.into())
        }
        _ => Err(PyErr::new::<pyo3::exceptions::PyTypeError, _>(format!(
            "Unsupported PropertyValue type: {:?}",
            value
        ))),
    }
}

// Helper function to convert Python PyAny to Rust PropertyValue
fn convert_pyobject_to_property_value(obj: &Bound<'_, PyAny>) -> PyResult<PropertyValue> {
    if let Ok(s) = obj.extract::<String>() {
        Ok(PropertyValue::String(s))
    } else if let Ok(f) = obj.extract::<f64>() {
        Ok(PropertyValue::Number(OrderedFloat(f)))
    } else if let Ok(i) = obj.extract::<i64>() {
        Ok(PropertyValue::Integer(i))
    } else if let Ok(b) = obj.extract::<bool>() {
        Ok(PropertyValue::Boolean(b))
    } else if let Ok(tup) = obj.extract::<Bound<'_, PyTuple>>() {
        if tup.len() == 2 {
            // Assume Vec2
            let x = tup.get_item(0)?.extract::<f64>()?;
            let y = tup.get_item(1)?.extract::<f64>()?;
            Ok(PropertyValue::Vec2(Vec2 { x: OrderedFloat(x), y: OrderedFloat(y) }))
        } else if tup.len() == 4 {
            // Assume Color
            let r = tup.get_item(0)?.extract::<u8>()?;
            let g = tup.get_item(1)?.extract::<u8>()?;
            let b = tup.get_item(2)?.extract::<u8>()?;
            let a = tup.get_item(3)?.extract::<u8>()?;
            Ok(PropertyValue::Color(Color { r, g, b, a }))
        } else {
            Err(PyErr::new::<pyo3::exceptions::PyTypeError, _>(format!(
                "Unsupported Python tuple length for PropertyValue: {}",
                tup.len()
            )))
        }
    } else if let Ok(list) = obj.extract::<Bound<'_, PyList>>() {
        let mut arr = Vec::new();
        for item in list {
            arr.push(convert_pyobject_to_property_value(&item)?);
        }
        Ok(PropertyValue::Array(arr))
    } else if let Ok(dict) = obj.extract::<Bound<'_, PyDictType>>() { // Using aliased type
        let mut map = std::collections::HashMap::new();
        for (key, value) in dict {
            let k_str = key.extract::<String>()?;
            map.insert(k_str, convert_pyobject_to_property_value(&value)?);
        }
        Ok(PropertyValue::Map(map))
    } else {
        Err(PyErr::new::<pyo3::exceptions::PyTypeError, _>(format!(
            "Unsupported Python type for PropertyValue: {:?}",
            obj
        )))
    }
}

pub struct PythonExpressionPlugin;

impl PythonExpressionPlugin {
    pub fn new() -> Self {
        Self
    }
}

impl Plugin for PythonExpressionPlugin {
    fn id(&self) -> &'static str {
        "python_expression"
    }

    fn category(&self) -> PluginCategory {
        PluginCategory::Property
    }

    fn version(&self) -> (u32, u32, u32) {
        (0, 1, 0)
    }
}

impl PropertyPlugin for PythonExpressionPlugin {
    fn get_evaluator_instance(&self) -> Arc<dyn PropertyEvaluator> {
        Arc::new(PythonExpressionEvaluator::new())
    }
}
