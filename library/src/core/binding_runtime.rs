use std::collections::HashMap;

use ordered_float::OrderedFloat;

use crate::model::authoring::{
    BindingOperator, BindingScope, EffectiveValue, EffectiveValueContribution, EventBindingId,
    InstancePath, ModuleDefinitionId, ModuleInstance, ModuleInstanceId, PublishedActionId,
    PublishedParameter, SignalBinding, SignalBindingId, SignalMapping, TriggerPolicy,
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
    generation: u64,
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
        Ok(())
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

#[derive(Clone, PartialEq, Eq, Debug, Hash)]
pub struct EventTarget {
    pub instance_path: InstancePath,
    pub module_instance_id: ModuleInstanceId,
    pub action_id: PublishedActionId,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum EventDecision {
    Started { spawn_sequence: u64 },
    Restarted,
    Ignored,
    Queued { position: usize },
    RejectedAtCapacity,
}

#[derive(Default)]
pub struct EventRuntime {
    targets: HashMap<EventTarget, EventTargetState>,
    next_spawn_sequence: u64,
}

#[derive(Default)]
struct EventTargetState {
    active: usize,
    queued: usize,
}

impl EventRuntime {
    pub const QUEUE_CAPACITY: usize = 256;
    pub const OVERLAP_CAPACITY: usize = 64;

    pub fn trigger(
        &mut self,
        _binding_id: EventBindingId,
        target: EventTarget,
        policy: TriggerPolicy,
    ) -> EventDecision {
        let spawn_sequence = self.next_spawn_sequence;
        self.next_spawn_sequence = self.next_spawn_sequence.wrapping_add(1);
        let state = self.targets.entry(target).or_default();
        match policy {
            TriggerPolicy::Restart => {
                let restarted = state.active > 0;
                state.active = 1;
                state.queued = 0;
                if restarted {
                    EventDecision::Restarted
                } else {
                    EventDecision::Started { spawn_sequence }
                }
            }
            TriggerPolicy::IgnoreWhilePlaying if state.active > 0 => EventDecision::Ignored,
            TriggerPolicy::IgnoreWhilePlaying => {
                state.active = 1;
                EventDecision::Started { spawn_sequence }
            }
            TriggerPolicy::Queue if state.active == 0 => {
                state.active = 1;
                EventDecision::Started { spawn_sequence }
            }
            TriggerPolicy::Queue if state.queued >= Self::QUEUE_CAPACITY => {
                EventDecision::RejectedAtCapacity
            }
            TriggerPolicy::Queue => {
                state.queued += 1;
                EventDecision::Queued {
                    position: state.queued,
                }
            }
            TriggerPolicy::Overlap if state.active >= Self::OVERLAP_CAPACITY => {
                EventDecision::RejectedAtCapacity
            }
            TriggerPolicy::Overlap => {
                state.active += 1;
                EventDecision::Started { spawn_sequence }
            }
        }
    }

    pub fn complete(&mut self, target: &EventTarget) -> Option<EventDecision> {
        let spawn_sequence = self.next_spawn_sequence;
        let state = self.targets.get_mut(target)?;
        state.active = state.active.saturating_sub(1);
        if state.active == 0 && state.queued > 0 {
            state.queued -= 1;
            state.active = 1;
            self.next_spawn_sequence = self.next_spawn_sequence.wrapping_add(1);
            Some(EventDecision::Started { spawn_sequence })
        } else {
            None
        }
    }
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
    fn queued_event_starts_after_active_event_completes() {
        let target = EventTarget {
            instance_path: InstancePath::root(TimelineId::new()),
            module_instance_id: ModuleInstanceId::new(),
            action_id: PublishedActionId::new(),
        };
        let mut runtime = EventRuntime::default();
        assert!(matches!(
            runtime.trigger(EventBindingId::new(), target.clone(), TriggerPolicy::Queue),
            EventDecision::Started { .. }
        ));
        assert_eq!(
            runtime.trigger(EventBindingId::new(), target.clone(), TriggerPolicy::Queue),
            EventDecision::Queued { position: 1 }
        );
        assert!(matches!(
            runtime.complete(&target),
            Some(EventDecision::Started { .. })
        ));
    }
}
