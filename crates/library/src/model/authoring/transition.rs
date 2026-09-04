use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use super::{
    AutomatableParameter, MediaOutputKind, MediaTime, OperationRef, ProcessorParameterContract,
    RationalRate, TimelineId, TimelineInterval, TimelineItemId, TransitionId,
    TransitionModuleProcessor,
};

pub const TRANSITION_CATEGORY: &str = "transition";
pub const TRANSITION_APPLY_OPERATION: &str = "transition.apply.v1";
pub const CROSS_DISSOLVE_COMPONENT_ID: &str = "cross_dissolve";
pub const AUDIO_CROSSFADE_COMPONENT_ID: &str = "audio_crossfade";
const BUILTIN_TRANSITION_VERSION: &str = "1";

/// One Timeline-owned transition between two ordinary placements.
///
/// The transition references clips; it never turns them into Nodes. Timing has
/// one persisted source of truth (`edit_point`, `duration`, and `alignment`).
/// Its concrete interval is derived for validation and RenderPlan compilation.
#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
#[serde(deny_unknown_fields)]
pub struct Transition {
    pub id: TransitionId,
    pub timeline_id: TimelineId,
    pub from_item_id: TimelineItemId,
    pub to_item_id: TimelineItemId,
    pub edit_point: MediaTime,
    pub duration: MediaTime,
    pub alignment: TransitionAlignment,
    pub processor: TransitionProcessor,
    /// Automation time is local to the transition interval: zero is the
    /// derived interval start and `duration` is its end.
    pub parameters: HashMap<String, AutomatableParameter>,
}

impl Transition {
    pub fn interval(&self) -> Result<TimelineInterval, String> {
        if self.duration <= MediaTime::zero() {
            return Err("Transition duration must be greater than zero".to_string());
        }
        let start = match self.alignment {
            TransitionAlignment::StartAtEdit => self.edit_point,
            TransitionAlignment::CenteredOnEdit => self
                .edit_point
                .checked_sub(self.duration.checked_div_rate(RationalRate::new(2, 1)?)?)?,
            TransitionAlignment::EndAtEdit => self.edit_point.checked_sub(self.duration)?,
        };
        TimelineInterval::new(start, self.duration)
    }
}

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum TransitionAlignment {
    StartAtEdit,
    CenteredOnEdit,
    EndAtEdit,
}

/// Media boundary consumed and produced by a transition processor.
///
/// This intentionally cannot describe Number, Shape, or other Node port types:
/// a first-class transition always combines two streams of one media kind.
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum TransitionMediaType {
    Image,
    Audio,
}

impl TransitionMediaType {
    pub const fn output_kind(self) -> MediaOutputKind {
        match self {
            Self::Image => MediaOutputKind::Image,
            Self::Audio => MediaOutputKind::Audio,
        }
    }
}

/// Stable processor identity plus the frozen typed interface needed to keep a
/// Project inspectable when an optional provider is unavailable.
#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
#[serde(deny_unknown_fields)]
pub struct TransitionProcessor {
    pub implementation: TransitionProcessorImplementation,
    pub contract: TransitionContractSnapshot,
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum TransitionProcessorImplementation {
    Operation(OperationRef),
    Module(TransitionModuleProcessor),
}

impl TransitionProcessor {
    pub fn from_operation(operation: OperationRef, contract: TransitionContractSnapshot) -> Self {
        Self {
            implementation: TransitionProcessorImplementation::Operation(operation),
            contract,
        }
    }

    pub fn cross_dissolve() -> Self {
        Self::builtin(CROSS_DISSOLVE_COMPONENT_ID, TransitionMediaType::Image)
    }

    pub fn audio_crossfade() -> Self {
        Self::builtin(AUDIO_CROSSFADE_COMPONENT_ID, TransitionMediaType::Audio)
    }

    pub fn module(instance_id: super::ModuleInstanceId, media_type: TransitionMediaType) -> Self {
        Self::module_with_controls(instance_id, media_type, HashMap::new(), HashMap::new())
    }

    pub fn module_with_controls(
        instance_id: super::ModuleInstanceId,
        media_type: TransitionMediaType,
        input_bindings: HashMap<super::PublishedMediaInputId, super::MediaInputBinding>,
        automation_tracks: HashMap<super::PublishedParameterId, super::AutomationTrack>,
    ) -> Self {
        Self {
            implementation: TransitionProcessorImplementation::Module(TransitionModuleProcessor {
                instance_id,
                input_bindings,
                automation_tracks,
            }),
            contract: TransitionContractSnapshot {
                media_type,
                parameters: Vec::new(),
            },
        }
    }

    pub const fn operation(&self) -> Option<&OperationRef> {
        match &self.implementation {
            TransitionProcessorImplementation::Operation(operation) => Some(operation),
            TransitionProcessorImplementation::Module(_) => None,
        }
    }

    pub const fn module_processor(&self) -> Option<&TransitionModuleProcessor> {
        match &self.implementation {
            TransitionProcessorImplementation::Operation(_) => None,
            TransitionProcessorImplementation::Module(module) => Some(module),
        }
    }

    pub fn module_processor_mut(&mut self) -> Option<&mut TransitionModuleProcessor> {
        match &mut self.implementation {
            TransitionProcessorImplementation::Operation(_) => None,
            TransitionProcessorImplementation::Module(module) => Some(module),
        }
    }

    fn builtin(component_id: &str, media_type: TransitionMediaType) -> Self {
        Self {
            implementation: TransitionProcessorImplementation::Operation(OperationRef {
                category: TRANSITION_CATEGORY.to_string(),
                component_id: component_id.to_string(),
                operation: TRANSITION_APPLY_OPERATION.to_string(),
                version: BUILTIN_TRANSITION_VERSION.to_string(),
            }),
            contract: TransitionContractSnapshot {
                media_type,
                parameters: Vec::new(),
            },
        }
    }

    pub fn is_builtin_cross_dissolve(&self) -> bool {
        self.is_builtin(CROSS_DISSOLVE_COMPONENT_ID, TransitionMediaType::Image)
    }

    pub fn is_builtin_audio_crossfade(&self) -> bool {
        self.is_builtin(AUDIO_CROSSFADE_COMPONENT_ID, TransitionMediaType::Audio)
    }

    fn is_builtin(&self, component_id: &str, media_type: TransitionMediaType) -> bool {
        let Some(operation) = self.operation() else {
            return false;
        };
        operation.category == TRANSITION_CATEGORY
            && operation.component_id == component_id
            && operation.operation == TRANSITION_APPLY_OPERATION
            && operation.version == BUILTIN_TRANSITION_VERSION
            && self.contract.media_type == media_type
            && self.contract.parameters.is_empty()
    }
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
#[serde(deny_unknown_fields)]
pub struct TransitionContractSnapshot {
    pub media_type: TransitionMediaType,
    pub parameters: Vec<ProcessorParameterContract>,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn seconds(value: i64) -> MediaTime {
        MediaTime::new(value, 1).expect("whole seconds")
    }

    #[test]
    fn alignment_derives_one_exact_interval() {
        let transition = |alignment| Transition {
            id: TransitionId::new(),
            timeline_id: TimelineId::new(),
            from_item_id: TimelineItemId::new(),
            to_item_id: TimelineItemId::new(),
            edit_point: seconds(5),
            duration: seconds(4),
            alignment,
            processor: TransitionProcessor::cross_dissolve(),
            parameters: HashMap::new(),
        };

        assert_eq!(
            transition(TransitionAlignment::StartAtEdit)
                .interval()
                .unwrap()
                .start,
            seconds(5)
        );
        assert_eq!(
            transition(TransitionAlignment::CenteredOnEdit)
                .interval()
                .unwrap()
                .start,
            seconds(3)
        );
        assert_eq!(
            transition(TransitionAlignment::EndAtEdit)
                .interval()
                .unwrap()
                .start,
            seconds(1)
        );
    }

    #[test]
    fn builtins_publish_distinct_typed_descriptors() {
        let image = TransitionProcessor::cross_dissolve();
        assert_eq!(
            image.operation().unwrap().component_id,
            CROSS_DISSOLVE_COMPONENT_ID
        );
        assert_eq!(image.contract.media_type, TransitionMediaType::Image);

        let audio = TransitionProcessor::audio_crossfade();
        assert_eq!(
            audio.operation().unwrap().component_id,
            AUDIO_CROSSFADE_COMPONENT_ID
        );
        assert_eq!(audio.contract.media_type, TransitionMediaType::Audio);
        assert!(image.is_builtin_cross_dissolve());
        assert!(!image.is_builtin_audio_crossfade());
        assert!(audio.is_builtin_audio_crossfade());
        assert!(!audio.is_builtin_cross_dissolve());
    }
}
