use std::collections::HashMap;

use ordered_float::OrderedFloat;

use crate::model::authoring::{
    BindingOperator, BindingScope, EffectiveValue, EffectiveValueContribution, EventBindingId,
    InstancePath, ModuleInstanceId, PublishedActionId, SignalBindingId, TriggerPolicy,
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
    use crate::model::authoring::{ModuleDefinitionId, TimelineId};

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
