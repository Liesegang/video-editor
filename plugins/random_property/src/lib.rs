use library::framing::property::{EvaluationContext, PropertyEvaluator, PropertyEvaluatorRegistry};
use library::model::project::property::{Property, PropertyValue};
use library::plugin::{Plugin, PluginCategory, PropertyPlugin};
use rand::rngs::StdRng;
use rand::{Rng, SeedableRng};

pub struct RandomPropertyPlugin;

impl Plugin for RandomPropertyPlugin {
    fn id(&self) -> &'static str {
        "random_property_plugin"
    }

    fn category(&self) -> PluginCategory {
        PluginCategory::Property
    }
}

impl PropertyPlugin for RandomPropertyPlugin {
    fn register(&self, registry: &mut PropertyEvaluatorRegistry) {
        registry.register("random_noise", Box::new(RandomNoiseEvaluator));
    }
}

struct RandomNoiseEvaluator;

impl PropertyEvaluator for RandomNoiseEvaluator {
    fn evaluate(&self, property: &Property, time: f64, _ctx: &EvaluationContext) -> PropertyValue {
        let amplitude = property
            .properties
            .get("amplitude")
            .and_then(|value| value.as_number())
            .unwrap_or(1.0)
            .abs();

        let seed = property
            .properties
            .get("seed")
            .and_then(|value| value.as_number())
            .unwrap_or(0.0) as u64;

        let time_bucket = (time * 1000.0).round() as u64;
        let mut rng = StdRng::seed_from_u64(seed ^ time_bucket);
        let value = rng.gen_range(-amplitude..=amplitude);
        PropertyValue::Number(value)
    }
}

#[allow(improper_ctypes_definitions)]
#[no_mangle]
pub extern "C" fn create_property_plugin() -> *mut dyn PropertyPlugin {
    let plugin: Box<dyn PropertyPlugin> = Box::new(RandomPropertyPlugin);
    Box::into_raw(plugin)
}
