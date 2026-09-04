use std::cell::RefCell;
use std::collections::HashSet;

use crate::model::property::PropertyMap;
use crate::plugin::{EvaluationContext, PropertyEvaluatorRegistry};

pub(super) struct AudioPropertyContext<'a> {
    evaluators: &'a PropertyEvaluatorRegistry,
    fps: f64,
    resolution: (u64, u64),
    reported_diagnostics: RefCell<HashSet<String>>,
}

impl<'a> AudioPropertyContext<'a> {
    pub(super) fn new(
        evaluators: &'a PropertyEvaluatorRegistry,
        fps: f64,
        resolution: (u64, u64),
    ) -> Self {
        Self {
            evaluators,
            fps,
            resolution,
            reported_diagnostics: RefCell::new(HashSet::new()),
        }
    }

    fn report_once(&self, diagnostic: String) {
        if self
            .reported_diagnostics
            .borrow_mut()
            .insert(diagnostic.clone())
        {
            log::warn!("{diagnostic}");
        }
    }

    #[cfg(test)]
    pub(super) fn reported_diagnostic_count(&self) -> usize {
        self.reported_diagnostics.borrow().len()
    }
}

pub(super) fn volume_at(
    properties: &PropertyMap,
    time: f64,
    context: &AudioPropertyContext<'_>,
    scope: &str,
) -> f32 {
    let Some(property) = properties.get("volume") else {
        return 1.0;
    };
    let uses_python = property.evaluator == "expression"
        || property.keyframes().iter().any(|keyframe| {
            matches!(
                keyframe.easing,
                crate::animation::EasingFunction::Expression { .. }
            )
        });
    if uses_python {
        context.report_once(format!(
            "Audio property {scope}.volume uses a Python Expression or easing; realtime per-sample CPython evaluation is disabled until block/envelope pre-evaluation lands. Using the authored fallback; source={:?}",
            property.expression_text(),
        ));
        return property.value().and_then(|value| value.get_as::<f64>()).map_or_else(
            || {
                context.report_once(format!(
                    "Audio property {scope}.volume has no numeric authored fallback; output is silent; source={:?}",
                    property.expression_text(),
                ));
                0.0
            },
            |value| value as f32,
        );
    }
    let evaluation_context = EvaluationContext::new(properties, context.fps, context.resolution);
    match context
        .evaluators
        .evaluate_with_diagnostics(property, time, &evaluation_context)
    {
        Ok(outcome) => {
            if let Some(diagnostic) = outcome.diagnostic() {
                context.report_once(format!(
                    "Recovered audio property {scope}.volume ({}) with authored value: {}; source={:?}",
                    diagnostic.evaluator(),
                    diagnostic.message(),
                    property.expression_text(),
                ));
            }
            outcome.value().get_as::<f64>().map_or_else(
                || {
                    context.report_once(format!(
                        "Audio property {scope}.volume returned a non-numeric value; source={:?}",
                        property.expression_text(),
                    ));
                    0.0
                },
                |value| value as f32,
            )
        }
        Err(error) => {
            context.report_once(format!(
                "Audio property {scope}.volume failed closed: {error}; source={:?}",
                property.expression_text(),
            ));
            0.0
        }
    }
}
