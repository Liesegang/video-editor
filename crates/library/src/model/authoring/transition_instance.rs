//! Placement-local controls for Transition Module invocations.
//!
//! A Transition and its Module instance belong to a Timeline definition. A
//! concrete nested Composition placement may override their published
//! controls without cloning either definition. Persistence uses an owner-
//! relative path; the derived full [`InstancePath`] keeps repeated placements
//! of the same nested Timeline distinct at compile and runtime.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::model::project::property::PropertyValue;

use super::{
    AutomationTrack, InstancePath, MediaInputBinding, ModuleInstanceId, PublishedMediaInputId,
    PublishedParameterId, TimelineId, TimelineItemId, TransitionId,
};

#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, PartialOrd, Ord, Debug, Hash)]
#[serde(deny_unknown_fields)]
pub struct TransitionModuleInstanceTarget {
    pub instance_path: InstancePath,
    pub transition_id: TransitionId,
    pub module_instance_id: ModuleInstanceId,
}

/// Persisted address relative to the root-Timeline Composition placement that
/// owns the override record. Keeping the owner implicit makes duplicate/split
/// clone semantics correct without rewriting stale absolute prefixes.
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug, Hash)]
#[serde(deny_unknown_fields)]
pub struct TransitionModulePlacementTarget {
    pub composition_items: Vec<TimelineItemId>,
    pub transition_id: TransitionId,
    pub module_instance_id: ModuleInstanceId,
}

impl TransitionModulePlacementTarget {
    pub fn concrete(
        &self,
        root_timeline_id: TimelineId,
        owner_item_id: TimelineItemId,
    ) -> TransitionModuleInstanceTarget {
        let mut composition_items = Vec::with_capacity(self.composition_items.len() + 1);
        composition_items.push(owner_item_id);
        composition_items.extend(self.composition_items.iter().copied());
        TransitionModuleInstanceTarget {
            instance_path: InstancePath {
                root_timeline_id,
                composition_items,
            },
            transition_id: self.transition_id,
            module_instance_id: self.module_instance_id,
        }
    }
}

/// Sparse differences owned by the outermost Composition placement in
/// `target.instance_path`.
///
/// `None` explicitly masks a Timeline-definition binding or automation track;
/// absence from a map inherits it. Static parameter absence similarly
/// inherits the Module instance value and then the Published default.
#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
#[serde(deny_unknown_fields)]
pub struct TransitionModuleInstanceOverrides {
    pub target: TransitionModulePlacementTarget,
    pub parameter_overrides: HashMap<PublishedParameterId, PropertyValue>,
    pub input_bindings: HashMap<PublishedMediaInputId, Option<MediaInputBinding>>,
    pub automation_tracks: HashMap<PublishedParameterId, Option<AutomationTrack>>,
}

#[derive(Clone, PartialEq, Debug)]
pub struct EffectiveTransitionModuleControls {
    pub target: TransitionModuleInstanceTarget,
    pub parameter_overrides: HashMap<PublishedParameterId, PropertyValue>,
    pub input_bindings: HashMap<PublishedMediaInputId, MediaInputBinding>,
    pub automation_tracks: HashMap<PublishedParameterId, AutomationTrack>,
}

impl TransitionModuleInstanceOverrides {
    pub fn new(target: TransitionModulePlacementTarget) -> Self {
        Self {
            target,
            parameter_overrides: HashMap::new(),
            input_bindings: HashMap::new(),
            automation_tracks: HashMap::new(),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.parameter_overrides.is_empty()
            && self.input_bindings.is_empty()
            && self.automation_tracks.is_empty()
    }
}
