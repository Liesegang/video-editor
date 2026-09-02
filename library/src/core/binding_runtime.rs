use std::collections::HashMap;

use ordered_float::OrderedFloat;

use crate::model::authoring::{
    BindingOperator, BindingScope, EffectiveValue, EffectiveValueContribution, InstancePath,
    ModuleDefinitionId, ModuleInstance, ModuleInstanceId, PublishedParameter, SignalBinding,
    SignalBindingId, SignalMapping,
};
use crate::model::project::property::PropertyValue;

#[derive(Clone, PartialEq, Debug)]
pub struct ResolvedSignalContribution {
    pub binding_id: SignalBindingId,
    pub scope: BindingScope,
    pub priority: i32,
    pub label: String,
    pub operator: BindingOperator,
    pub value: OrderedFloat<f64>,
}

#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct SignalRuntimeValues {
    values: HashMap<SignalBindingId, OrderedFloat<f64>>,
    smoothing: HashMap<SignalBindingId, SmoothedSignalState>,
    generation: u64,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct SmoothedSignalState {
    value: OrderedFloat<f64>,
    sampled_at: OrderedFloat<f64>,
}

impl SignalRuntimeValues {
    pub fn set(&mut self, binding_id: SignalBindingId, value: f64) -> Result<(), String> {
        if !value.is_finite() {
            return Err("Runtime Signal value must be finite".to_string());
        }
        if self.values.get(&binding_id).copied() != Some(OrderedFloat(value)) {
            self.values.insert(binding_id, OrderedFloat(value));
            self.generation = self.generation.wrapping_add(1);
        }
        self.smoothing.remove(&binding_id);
        Ok(())
    }

    /// Samples a Binding source through its authored one-pole smoothing time.
    /// `sampled_at` belongs to the monotonic source clock, not Timeline time.
    pub fn sample(
        &mut self,
        binding: &SignalBinding,
        raw_value: f64,
        sampled_at: f64,
    ) -> Result<f64, String> {
        let smoothing_seconds = binding.smoothing_seconds.into_inner();
        if !raw_value.is_finite() || !sampled_at.is_finite() {
            return Err("Runtime Signal samples must be finite".to_string());
        }
        if smoothing_seconds < 0.0 || !smoothing_seconds.is_finite() {
            return Err("Signal smoothing must be finite and non-negative".to_string());
        }
        let value = match self.smoothing.get(&binding.id).copied() {
            None => raw_value,
            Some(previous) => {
                let delta = sampled_at - previous.sampled_at.into_inner();
                if delta < 0.0 {
                    return Err("Runtime Signal sample time must be monotonic".to_string());
                }
                if smoothing_seconds == 0.0 {
                    raw_value
                } else {
                    let alpha = 1.0 - (-delta / smoothing_seconds).exp();
                    previous.value.into_inner() + (raw_value - previous.value.into_inner()) * alpha
                }
            }
        };
        let value = OrderedFloat(value);
        self.smoothing.insert(
            binding.id,
            SmoothedSignalState {
                value,
                sampled_at: OrderedFloat(sampled_at),
            },
        );
        if self.values.get(&binding.id).copied() != Some(value) {
            self.values.insert(binding.id, value);
            self.generation = self.generation.wrapping_add(1);
        }
        Ok(value.into_inner())
    }

    pub fn get(&self, binding_id: SignalBindingId) -> Option<f64> {
        self.values.get(&binding_id).map(|value| value.into_inner())
    }

    pub fn generation(&self) -> u64 {
        self.generation
    }
}

pub fn resolve_published_numeric_value<'a>(
    definition_id: ModuleDefinitionId,
    instance: &ModuleInstance,
    instance_path: &InstancePath,
    parameter: &PublishedParameter,
    bindings: impl Iterator<Item = &'a SignalBinding>,
    runtime: &SignalRuntimeValues,
) -> Option<EffectiveValue> {
    let base = numeric_value(&parameter.default_value)?;
    let keyed = instance
        .parameter_overrides
        .get(&parameter.id)
        .and_then(numeric_value);
    let contributions = bindings
        .filter(|binding| binding.target_parameter_id == parameter.id)
        .filter(|binding| {
            binding_scope_matches(&binding.scope, definition_id, instance.id, instance_path)
        })
        .filter_map(|binding| {
            let input = runtime.get(binding.id)?;
            Some(ResolvedSignalContribution {
                binding_id: binding.id,
                scope: binding.scope.clone(),
                priority: binding.priority,
                label: format!("{:?}", binding.source),
                operator: binding.operator,
                value: OrderedFloat(map_signal_value(&binding.mapping, input)),
            })
        })
        .collect();
    Some(resolve_numeric_effective_value(
        base,
        keyed,
        contributions,
        None,
    ))
}

fn binding_scope_matches(
    scope: &BindingScope,
    definition_id: ModuleDefinitionId,
    instance_id: ModuleInstanceId,
    instance_path: &InstancePath,
) -> bool {
    match scope {
        BindingScope::Definition {
            definition_id: target,
        } => *target == definition_id,
        BindingScope::Instance {
            instance_path: target_path,
            module_instance_id: target_instance,
        } => *target_instance == instance_id && target_path == instance_path,
        // Query membership is resolved by the collection runtime; a raw
        // invocation must never guess that it belongs to a query.
        BindingScope::Query { .. } => false,
    }
}

fn map_signal_value(mapping: &SignalMapping, input: f64) -> f64 {
    let input_min = mapping.input_min.into_inner();
    let input_max = mapping.input_max.into_inner();
    let mut normalized = (input - input_min) / (input_max - input_min);
    if mapping.clamp {
        normalized = normalized.clamp(0.0, 1.0);
    }
    mapping.output_min.into_inner()
        + normalized * (mapping.output_max.into_inner() - mapping.output_min.into_inner())
}

fn numeric_value(value: &PropertyValue) -> Option<f64> {
    match value {
        PropertyValue::Number(value) => Some(value.into_inner()),
        PropertyValue::Integer(value) => Some(*value as f64),
        _ => None,
    }
}

pub fn resolve_numeric_effective_value(
    base: f64,
    keyed: Option<f64>,
    mut signals: Vec<ResolvedSignalContribution>,
    manual_override: Option<(BindingOperator, f64)>,
) -> EffectiveValue {
    signals.sort_by_key(|signal| {
        (
            scope_rank(&signal.scope),
            signal.priority,
            signal.binding_id,
        )
    });
    let mut value = keyed.unwrap_or(base);
    let mut contributions = vec![EffectiveValueContribution {
        label: "Base".to_string(),
        value: number(base),
    }];
    if let Some(keyed) = keyed {
        contributions.push(EffectiveValueContribution {
            label: "Keyframe".to_string(),
            value: number(keyed),
        });
    }
    for signal in signals {
        value = apply_numeric(signal.operator, value, signal.value.into_inner());
        contributions.push(EffectiveValueContribution {
            label: signal.label,
            value: number(value),
        });
    }
    if let Some((operator, operand)) = manual_override {
        value = apply_numeric(operator, value, operand);
        contributions.push(EffectiveValueContribution {
            label: "Manual Override".to_string(),
            value: number(value),
        });
    }
    EffectiveValue {
        value: number(value),
        contributions,
    }
}

fn scope_rank(scope: &BindingScope) -> u8 {
    match scope {
        BindingScope::Definition { .. } => 0,
        BindingScope::Query { .. } => 1,
        BindingScope::Instance { .. } => 2,
    }
}

fn apply_numeric(operator: BindingOperator, current: f64, operand: f64) -> f64 {
    match operator {
        BindingOperator::Replace => operand,
        BindingOperator::Add => current + operand,
        BindingOperator::Multiply => current * operand,
        BindingOperator::Minimum => current.min(operand),
        BindingOperator::Maximum => current.max(operand),
    }
}

fn number(value: f64) -> PropertyValue {
    PropertyValue::Number(OrderedFloat(value))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::authoring::{
        ModuleDefinitionId, ModulePortAddress, PublishedParameterId, SignalMapping, SignalSource,
        TimelineId, TimelineItemId,
    };
    use crate::model::project::PortDataType;

    #[test]
    fn provenance_matches_documented_composition_order() {
        let definition = ModuleDefinitionId::new();
        let result = resolve_numeric_effective_value(
            100.0,
            Some(90.0),
            vec![ResolvedSignalContribution {
                binding_id: SignalBindingId::new(),
                scope: BindingScope::Definition {
                    definition_id: definition,
                },
                priority: 0,
                label: "Kick Envelope".to_string(),
                operator: BindingOperator::Multiply,
                value: OrderedFloat(0.8),
            }],
            Some((BindingOperator::Add, 10.0)),
        );
        assert_eq!(result.value, number(82.0));
        assert_eq!(result.contributions.len(), 4);
    }

    #[test]
    fn instance_binding_matches_the_exact_nested_instance_path() {
        let definition_id = ModuleDefinitionId::new();
        let instance = ModuleInstance {
            id: ModuleInstanceId::new(),
            definition_id,
            parameter_overrides: HashMap::new(),
        };
        let parameter = PublishedParameter {
            id: PublishedParameterId::new(),
            name: "Amount".to_string(),
            data_type: PortDataType::Number,
            default_value: number(10.0),
            target: ModulePortAddress {
                node_id: uuid::Uuid::new_v4(),
                port: "property:amount".to_string(),
            },
        };
        let root = TimelineId::new();
        let target_path = InstancePath::root(root).nested(TimelineItemId::new());
        let sibling_path = InstancePath::root(root).nested(TimelineItemId::new());
        let binding = SignalBinding {
            id: SignalBindingId::new(),
            source: SignalSource::AudioEnvelope {
                channel: "music".to_string(),
            },
            scope: BindingScope::Instance {
                instance_path: target_path.clone(),
                module_instance_id: instance.id,
            },
            target_parameter_id: parameter.id,
            mapping: SignalMapping {
                input_min: OrderedFloat(0.0),
                input_max: OrderedFloat(1.0),
                output_min: OrderedFloat(0.0),
                output_max: OrderedFloat(1.0),
                clamp: true,
            },
            operator: BindingOperator::Multiply,
            smoothing_seconds: OrderedFloat(0.0),
            priority: 0,
        };
        let mut runtime = SignalRuntimeValues::default();
        runtime.set(binding.id, 0.5).unwrap();

        let targeted = resolve_published_numeric_value(
            definition_id,
            &instance,
            &target_path,
            &parameter,
            std::iter::once(&binding),
            &runtime,
        )
        .unwrap();
        let sibling = resolve_published_numeric_value(
            definition_id,
            &instance,
            &sibling_path,
            &parameter,
            std::iter::once(&binding),
            &runtime,
        )
        .unwrap();
        assert_eq!(targeted.value, number(5.0));
        assert_eq!(sibling.value, number(10.0));
    }

    #[test]
    fn authored_smoothing_is_deterministic_and_uses_a_monotonic_source_clock() {
        let mut binding = SignalBinding {
            id: SignalBindingId::new(),
            source: SignalSource::AudioEnvelope {
                channel: "music".to_string(),
            },
            scope: BindingScope::Definition {
                definition_id: ModuleDefinitionId::new(),
            },
            target_parameter_id: PublishedParameterId::new(),
            mapping: SignalMapping {
                input_min: OrderedFloat(0.0),
                input_max: OrderedFloat(1.0),
                output_min: OrderedFloat(0.0),
                output_max: OrderedFloat(1.0),
                clamp: true,
            },
            operator: BindingOperator::Replace,
            smoothing_seconds: OrderedFloat(1.0),
            priority: 0,
        };
        let mut runtime = SignalRuntimeValues::default();
        assert_eq!(runtime.sample(&binding, 0.0, 10.0).unwrap(), 0.0);
        let smoothed = runtime.sample(&binding, 1.0, 11.0).unwrap();
        assert!((smoothed - (1.0 - (-1.0_f64).exp())).abs() < 1.0e-12);
        assert!(runtime.sample(&binding, 1.0, 10.5).is_err());

        binding.smoothing_seconds = OrderedFloat(0.0);
        assert_eq!(runtime.sample(&binding, 0.25, 12.0).unwrap(), 0.25);
    }
}
