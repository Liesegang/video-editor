use library::core::model::property::{Property, PropertyValue};
use library::extensions::traits::{EvaluationContext, PropertyEvaluator};
use library::extensions::traits::{Plugin, PluginCategory, PropertyPlugin};
use rand::rngs::StdRng;
use rand::{Rng, SeedableRng};
use std::sync::Arc;

pub struct RandomPropertyPlugin;

impl Plugin for RandomPropertyPlugin {
    fn id(&self) -> &'static str {
        "random_property"
    }

    fn name(&self) -> String {
        "Random Property".to_string()
    }

    fn category(&self) -> String {
        "Property".to_string()
    }

    fn version(&self) -> (u32, u32, u32) {
        (0, 1, 0)
    }
}

impl PropertyPlugin for RandomPropertyPlugin {
    fn get_evaluator_instance(&self) -> Arc<dyn PropertyEvaluator> {
        Arc::new(RandomNoiseEvaluator)
    }
}

struct RandomNoiseEvaluator;

impl PropertyEvaluator for RandomNoiseEvaluator {
    fn evaluate(&self, property: &Property, time: f64, _ctx: &EvaluationContext) -> PropertyValue {
        let amplitude = property
            .properties
            .get("amplitude")
            .and_then(|value| value.get_as::<f64>())
            .unwrap_or(1.0)
            .abs();

        let seed = property
            .properties
            .get("seed")
            .and_then(|value| value.get_as::<f64>())
            .unwrap_or(0.0) as u64;

        let time_bucket = (time * 1000.0).round() as u64;
        let mut rng = StdRng::seed_from_u64(seed ^ time_bucket);
        let value = rng.gen_range(-amplitude..=amplitude);
        PropertyValue::Number(ordered_float::OrderedFloat(value))
    }
}

#[allow(improper_ctypes_definitions)]
#[no_mangle]
pub extern "C" fn create_property_plugin() -> *mut dyn PropertyPlugin {
    let plugin: Box<dyn PropertyPlugin> = Box::new(RandomPropertyPlugin);
    Box::into_raw(plugin)
}
