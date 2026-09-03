use std::collections::HashMap;

use ordered_float::OrderedFloat;
use uuid::Uuid;

use crate::core::render_plan::CompiledBindingIndex;
use crate::model::authoring::{
    BindingScope, EventBinding, EventBindingId, EventSource, PublishedActionId, TriggerPolicy,
};

#[derive(Clone, PartialEq, Eq, Debug)]
pub struct ReactiveInvocation {
    pub occurrence_id: Uuid,
    pub binding_id: EventBindingId,
    pub scope: BindingScope,
    pub action_id: PublishedActionId,
    pub scheduled_at: OrderedFloat<f64>,
    pub duration: OrderedFloat<f64>,
}

impl ReactiveInvocation {
    pub fn local_time(&self, now: f64) -> f64 {
        (now - self.scheduled_at.into_inner()).max(0.0)
    }

    pub fn ends_at(&self) -> f64 {
        self.scheduled_at.into_inner() + self.duration.into_inner()
    }
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub enum TriggerOutcome {
    Scheduled(ReactiveInvocation),
    IgnoredWhilePlaying,
    RejectedAtCapacity,
}

#[derive(Default, Debug)]
pub struct EventRuntime {
    invocations: HashMap<EventBindingId, Vec<ReactiveInvocation>>,
    next_ordinal: HashMap<EventBindingId, u64>,
}

impl EventRuntime {
    pub const QUEUE_CAPACITY: usize = 256;
    pub const OVERLAP_CAPACITY: usize = 64;

    pub fn trigger(
        &mut self,
        binding: &EventBinding,
        now: f64,
        duration: f64,
    ) -> Result<TriggerOutcome, String> {
        if !now.is_finite() || !duration.is_finite() || duration <= 0.0 {
            return Err(
                "Event trigger time must be finite and duration must be positive".to_string(),
            );
        }

        let ordinal = self.next_ordinal.entry(binding.id).or_default();
        let occurrence_id = deterministic_occurrence_id(binding.id, now, *ordinal);
        *ordinal = ordinal.wrapping_add(1);
        let invocations = self.invocations.entry(binding.id).or_default();
        invocations.retain(|invocation| invocation.ends_at() > now);

        let scheduled_at = match binding.trigger_policy {
            TriggerPolicy::Restart => {
                invocations.clear();
                now
            }
            TriggerPolicy::IgnoreWhilePlaying if !invocations.is_empty() => {
                return Ok(TriggerOutcome::IgnoredWhilePlaying);
            }
            TriggerPolicy::IgnoreWhilePlaying => now,
            TriggerPolicy::Queue => {
                if invocations.len() >= Self::QUEUE_CAPACITY {
                    return Ok(TriggerOutcome::RejectedAtCapacity);
                }
                invocations
                    .iter()
                    .map(ReactiveInvocation::ends_at)
                    .fold(now, f64::max)
            }
            TriggerPolicy::Overlap => {
                let active = invocations
                    .iter()
                    .filter(|invocation| invocation.scheduled_at.into_inner() <= now)
                    .count();
                if active >= Self::OVERLAP_CAPACITY {
                    return Ok(TriggerOutcome::RejectedAtCapacity);
                }
                now
            }
        };

        let invocation = ReactiveInvocation {
            occurrence_id,
            binding_id: binding.id,
            scope: binding.scope.clone(),
            action_id: binding.target_action_id,
            scheduled_at: OrderedFloat(scheduled_at),
            duration: OrderedFloat(duration),
        };
        invocations.push(invocation.clone());
        Ok(TriggerOutcome::Scheduled(invocation))
    }

    /// Routes a discrete external event to every authored Binding connected
    /// to its public source. Each target keeps an independent policy queue.
    pub fn trigger_source(
        &mut self,
        bindings: &CompiledBindingIndex,
        source: &EventSource,
        now: f64,
        duration: f64,
    ) -> Result<Vec<(EventBindingId, TriggerOutcome)>, String> {
        bindings
            .event_source_bindings(source)
            .iter()
            .map(|binding| {
                self.trigger(binding, now, duration)
                    .map(|outcome| (binding.id, outcome))
            })
            .collect()
    }

    pub fn active_at(&mut self, now: f64) -> Vec<&ReactiveInvocation> {
        for invocations in self.invocations.values_mut() {
            invocations.retain(|invocation| invocation.ends_at() > now);
        }
        let mut active = self
            .invocations
            .values()
            .flatten()
            .filter(|invocation| invocation.scheduled_at.into_inner() <= now)
            .collect::<Vec<_>>();
        active.sort_by_key(|invocation| (invocation.scheduled_at, invocation.occurrence_id));
        active
    }

    pub fn clear(&mut self) {
        self.invocations.clear();
        self.next_ordinal.clear();
    }
}

fn deterministic_occurrence_id(binding_id: EventBindingId, now: f64, ordinal: u64) -> Uuid {
    use sha2::{Digest, Sha256};

    let mut hasher = Sha256::new();
    hasher.update(binding_id.as_uuid().as_bytes());
    hasher.update(now.to_bits().to_le_bytes());
    hasher.update(ordinal.to_le_bytes());
    let digest = hasher.finalize();
    let mut bytes = [0_u8; 16];
    bytes.copy_from_slice(&digest[..16]);
    Uuid::from_bytes(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::authoring::{EventSource, InstancePath, ModuleInstanceId, TimelineId};

    fn binding(policy: TriggerPolicy) -> EventBinding {
        EventBinding {
            id: EventBindingId::new(),
            source: EventSource::Marker {
                name: "kick".to_string(),
            },
            scope: BindingScope::Instance {
                instance_path: InstancePath::root(TimelineId::new()),
                module_instance_id: ModuleInstanceId::new(),
            },
            target_action_id: PublishedActionId::new(),
            trigger_policy: policy,
            priority: 0,
        }
    }

    #[test]
    fn restart_replaces_the_active_occurrence() {
        let binding = binding(TriggerPolicy::Restart);
        let mut runtime = EventRuntime::default();
        runtime.trigger(&binding, 1.0, 4.0).unwrap();
        runtime.trigger(&binding, 2.0, 4.0).unwrap();
        let active = runtime.active_at(2.5);
        assert_eq!(active.len(), 1);
        assert_eq!(active[0].local_time(2.5), 0.5);
    }

    #[test]
    fn ignore_queue_and_overlap_have_distinct_scheduling() {
        let mut runtime = EventRuntime::default();
        let ignored = binding(TriggerPolicy::IgnoreWhilePlaying);
        runtime.trigger(&ignored, 0.0, 2.0).unwrap();
        assert_eq!(
            runtime.trigger(&ignored, 1.0, 2.0).unwrap(),
            TriggerOutcome::IgnoredWhilePlaying
        );

        let queued = binding(TriggerPolicy::Queue);
        runtime.trigger(&queued, 0.0, 2.0).unwrap();
        let TriggerOutcome::Scheduled(second) = runtime.trigger(&queued, 0.5, 2.0).unwrap() else {
            panic!("queue trigger must schedule");
        };
        assert_eq!(second.scheduled_at.into_inner(), 2.0);

        let overlap = binding(TriggerPolicy::Overlap);
        runtime.trigger(&overlap, 0.0, 2.0).unwrap();
        runtime.trigger(&overlap, 0.5, 2.0).unwrap();
        let active = runtime.active_at(1.0);
        assert_eq!(
            active
                .into_iter()
                .filter(|invocation| invocation.binding_id == overlap.id)
                .count(),
            2
        );
    }

    #[test]
    fn occurrence_ids_are_deterministic_for_replayed_input() {
        let binding = binding(TriggerPolicy::Overlap);
        let replay = || {
            let mut runtime = EventRuntime::default();
            let TriggerOutcome::Scheduled(first) = runtime.trigger(&binding, 1.0, 2.0).unwrap()
            else {
                panic!("first occurrence must be scheduled");
            };
            let TriggerOutcome::Scheduled(second) = runtime.trigger(&binding, 1.0, 2.0).unwrap()
            else {
                panic!("second occurrence must be scheduled");
            };
            (first.occurrence_id, second.occurrence_id)
        };
        let first_replay = replay();
        let second_replay = replay();
        assert_eq!(first_replay, second_replay);
        assert_ne!(first_replay.0, first_replay.1);
    }
}
