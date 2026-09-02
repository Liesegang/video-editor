use ordered_float::OrderedFloat;
use serde::{Deserialize, Serialize};

use crate::model::path::PathValue;
use crate::model::project::property::{Property, PropertyMap};

use super::{ConstraintId, MaskId, ModuleInstanceId, TimelineItemId, TransitionId};

#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
#[serde(deny_unknown_fields)]
pub struct Mask {
    pub id: MaskId,
    pub path: PathValue,
    pub mode: MaskMode,
    pub inverted: bool,
    pub feather: Property,
    pub opacity: Property,
}

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
#[serde(rename_all = "snake_case")]
pub enum MaskMode {
    Add,
    Subtract,
    Intersect,
    Difference,
}

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
#[serde(rename_all = "snake_case")]
pub enum MatteMode {
    Alpha,
    AlphaInverted,
    Luma,
    LumaInverted,
}

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
#[serde(deny_unknown_fields)]
pub struct MatteRef {
    pub item_id: TimelineItemId,
    pub mode: MatteMode,
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
#[serde(deny_unknown_fields)]
pub struct Constraint {
    pub id: ConstraintId,
    pub target_item_id: TimelineItemId,
    pub kind: ConstraintKind,
    pub influence: Property,
    pub parameters: PropertyMap,
}

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
#[serde(rename_all = "snake_case")]
pub enum ConstraintKind {
    CopyPosition,
    CopyRotation,
    CopyScale,
    LookAt,
    FollowPath,
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
#[serde(deny_unknown_fields)]
pub struct Transition {
    pub id: TransitionId,
    pub from_item_id: TimelineItemId,
    pub to_item_id: TimelineItemId,
    pub duration: OrderedFloat<f64>,
    pub kind: TransitionKind,
    pub authored_properties: PropertyMap,
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum TransitionKind {
    CrossDissolve,
    DipToColor,
    Wipe,
    Module {
        module_instance_id: ModuleInstanceId,
    },
}
