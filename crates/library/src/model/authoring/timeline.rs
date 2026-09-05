use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::animation::EasingFunction;
use crate::model::BlendMode;
use crate::model::frame::color::Color;
use crate::model::project::PortDataType;
use crate::model::project::property::{KeyframeId, PropertyMap, PropertyValue};

use super::{
    AuthoringProject, CompositionParameterId, MediaTime, ModuleInstanceId, ModuleOutputId,
    PublishedMediaInputId, PublishedParameterId, RationalRate, TimelineId, TimelineItemId,
    TimelineTrackId, TransitionModuleInstanceOverrides,
};

#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
#[serde(deny_unknown_fields)]
pub struct Timeline {
    pub id: TimelineId,
    pub name: String,
    pub width: u64,
    pub height: u64,
    pub fps: RationalRate,
    pub duration: MediaTime,
    pub background_color: Color,
    pub color_profile: String,
    pub track_order: Vec<TimelineTrackId>,
    pub authored_properties: PropertyMap,
    /// Stable public controls for every placement of this nested Timeline.
    /// Targets remain private to the definition; instances store only IDs.
    pub published_parameters: Vec<CompositionParameter>,
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
#[serde(deny_unknown_fields)]
pub struct TimelineTrack {
    pub id: TimelineTrackId,
    pub timeline_id: TimelineId,
    pub name: String,
    pub kind: TimelineTrackKind,
    pub authored_properties: PropertyMap,
}

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum TimelineTrackKind {
    Visual,
    Audio,
    AudioVisual,
}

impl TimelineTrackKind {
    /// Whether this Track participates in the runtime pipeline for a media
    /// output. Transition validation and candidate discovery share this
    /// authority so the UI cannot offer a processor the Track will skip.
    pub const fn supports_output(self, output: MediaOutputKind) -> bool {
        matches!(
            (self, output),
            (Self::Visual | Self::AudioVisual, MediaOutputKind::Image)
                | (Self::Audio | Self::AudioVisual, MediaOutputKind::Audio)
        )
    }
}

/// One human-authored placement. It never owns Node topology.
#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
#[serde(deny_unknown_fields)]
pub struct TimelineItem {
    pub id: TimelineItemId,
    pub track_id: TimelineTrackId,
    pub name: String,
    pub source: SourceRef,
    pub interval: TimelineInterval,
    /// Maps Timeline time into this placement's local source/animation time.
    pub time_map: TimeMap,
    pub layer: i64,
    pub parent: Option<TimelineItemId>,
    /// Authored compositing mode for this placement in its Track.
    ///
    /// Blend is placement state, not Module topology: duplicating or moving a
    /// clip must never rewrite the reusable Module definition behind it.
    pub blend_mode: BlendMode,
    pub authored_properties: PropertyMap,
}

/// Persisted Track-owned visual enable property. Absence means enabled so
/// existing and newly-created Projects do not need redundant default state.
pub const TRACK_VISIBILITY_PROPERTY: &str = "visible";

impl TimelineTrack {
    /// Whether this Track contributes to the image pipeline.
    ///
    /// This control deliberately does not mute Audio output. In particular,
    /// disabling an AudioVisual Track hides only its visual contribution.
    pub fn is_visually_enabled(&self) -> Result<bool, String> {
        let Some(property) = self.authored_properties.get(TRACK_VISIBILITY_PROPERTY) else {
            return Ok(true);
        };
        if property.evaluator != "constant" {
            return Err(format!(
                "Track {} visual visibility must be a Constant Boolean",
                self.id
            ));
        }
        match property.value() {
            Some(PropertyValue::Boolean(visible)) => Ok(*visible),
            _ => Err(format!(
                "Track {} visual visibility must be a Constant Boolean",
                self.id
            )),
        }
    }
}

/// Non-persisted placement state used by atomic Timeline edit projections.
///
/// Source, parenting, authored properties, and Module topology are
/// intentionally absent because Move and Trim do not own those domains.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct TimelineItemPlacementState {
    pub track_id: TimelineTrackId,
    pub interval: TimelineInterval,
    pub time_map: TimeMap,
    pub layer: i64,
}

impl From<&TimelineItem> for TimelineItemPlacementState {
    fn from(item: &TimelineItem) -> Self {
        Self {
            track_id: item.track_id,
            interval: item.interval,
            time_map: item.time_map,
            layer: item.layer,
        }
    }
}

impl TimelineItemPlacementState {
    pub(crate) fn apply_to(self, item: &mut TimelineItem) {
        item.track_id = self.track_id;
        item.interval = self.interval;
        item.time_map = self.time_map;
        item.layer = self.layer;
    }
}

/// Returns one Track's items in the authoritative back-to-front order.
///
/// Layer is the primary authored order. Start time and stable ID resolve old
/// or imported projects that have duplicate layer values, so mutation and UI
/// preview never disagree about the resulting order.
pub fn ordered_track_item_ids(
    project: &AuthoringProject,
    track_id: TimelineTrackId,
    excluded_item_id: Option<TimelineItemId>,
) -> Vec<TimelineItemId> {
    let mut ordered = project
        .items
        .values()
        .filter(|item| item.track_id == track_id && Some(item.id) != excluded_item_id)
        .map(|item| (item.layer, item.interval.start, item.id))
        .collect::<Vec<_>>();
    ordered.sort_by_key(|entry| *entry);
    ordered.into_iter().map(|(_, _, item_id)| item_id).collect()
}

/// Projects the exact canonical order produced by placing an item at a layer.
pub fn track_item_ids_after_placement(
    project: &AuthoringProject,
    track_id: TimelineTrackId,
    item_id: TimelineItemId,
    requested_layer: i64,
) -> Vec<TimelineItemId> {
    let mut item_ids = ordered_track_item_ids(project, track_id, Some(item_id));
    let index = usize::try_from(requested_layer.max(0))
        .unwrap_or(usize::MAX)
        .min(item_ids.len());
    item_ids.insert(index, item_id);
    item_ids
}

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
#[serde(deny_unknown_fields)]
pub struct TimelineInterval {
    pub start: MediaTime,
    pub duration: MediaTime,
}

impl TimelineInterval {
    pub fn new(start: MediaTime, duration: MediaTime) -> Result<Self, String> {
        if start.is_negative() {
            return Err("Timeline interval start must be non-negative".to_string());
        }
        if duration.is_negative() {
            return Err("Timeline interval duration must be finite and non-negative".to_string());
        }
        Ok(Self { start, duration })
    }

    pub fn contains(self, time: MediaTime) -> Result<bool, String> {
        Ok(time >= self.start && time < self.end()?)
    }

    pub fn end(self) -> Result<MediaTime, String> {
        self.start.checked_add(self.duration)
    }

    /// Reports whether this placement is active for the complete required
    /// interval. Both intervals are half-open: `[start, end)`.
    pub fn covers(self, required: Self) -> Result<bool, String> {
        Ok(self.start <= required.start && self.end()? >= required.end()?)
    }
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum SourceRef {
    Asset {
        asset_id: uuid::Uuid,
    },
    Text {
        text: String,
        /// Ordered descriptor-backed styles composed around one text body.
        /// Underlays, body styles, and overlays retain their authored order
        /// within each renderer phase and share the same content alpha.
        appearance_operations: Vec<AppearanceOperation>,
        /// Ordered descriptor-backed operations applied to the transient text
        /// Shape before rasterization. The operation identity and authored
        /// properties are the same contract used by production Node graphs;
        /// no evaluated Ensemble output is persisted here.
        ensemble_operations: Vec<TextEnsembleOperation>,
    },
    Shape {
        shape: ShapeSource,
    },
    Solid {
        color: Color,
    },
    Composition(CompositionInstance),
    /// A user-visible Node Clip. Only the referenced Module owns topology.
    Module(ModuleInvocation),
}

/// One authored Effector or Decorator in a Text source's Ensemble stack.
///
/// The stable operation reference keeps an unavailable/newer plugin
/// round-trippable, while [`PropertyMap`] remains the single authored value
/// representation shared with descriptor-backed Node operations. Entries are
/// stored in production execution phases: all Effectors first, followed by all
/// Decorators; ordering is meaningful only within one phase.
#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
#[serde(deny_unknown_fields)]
pub struct TextEnsembleOperation {
    pub id: uuid::Uuid,
    pub operation: super::OperationRef,
    /// Frozen descriptor port contract. This is the same execution snapshot
    /// persisted by a plugin operation Node and lets a Project reject direct
    /// Text operations that require another authored media input even when
    /// the matching plugin is unavailable.
    pub declared_ports: Vec<crate::model::project::PortDefinition>,
    pub properties: PropertyMap,
}

/// One descriptor-backed paint operation in a direct Text or Shape
/// appearance stack.
///
/// The operation snapshot is deliberately the same contract persisted by a
/// graph Node. Timeline authoring owns only this short ordered stack; it does
/// not persist or expose generated Node topology.
#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
#[serde(deny_unknown_fields)]
pub struct AppearanceOperation {
    pub id: uuid::Uuid,
    pub operation: super::OperationRef,
    pub declared_ports: Vec<crate::model::project::PortDefinition>,
    pub properties: PropertyMap,
}

/// Whether a descriptor is a self-contained Shape-to-Image appearance
/// operation suitable for a direct Timeline Text or Shape source.
pub fn appearance_direct_contract_is_compatible(
    ports: &[crate::model::project::PortDefinition],
) -> bool {
    use std::collections::HashSet;

    use crate::model::project::{
        IMAGE_OUTPUT_PORT, PortDirection, PortMultiplicity, SHAPE_INPUT_PORT, STYLE_OUTPUT_PORT,
        TIME_PORT,
    };

    let mut shape_inputs = 0;
    let mut image_outputs = 0;
    let mut style_outputs = 0;
    let mut keys = HashSet::new();
    for port in ports {
        if !keys.insert(port.key.as_str()) || port.multiplicity != PortMultiplicity::Single {
            return false;
        }
        match (port.direction, port.key.as_str()) {
            (PortDirection::Input, SHAPE_INPUT_PORT)
                if port.data_type == crate::model::project::PortDataType::Shape =>
            {
                shape_inputs += 1;
            }
            (PortDirection::Input, TIME_PORT)
                if port.data_type == crate::model::project::PortDataType::Number => {}
            (PortDirection::Input, key)
                if key
                    .strip_prefix(crate::plugin::PROPERTY_PORT_PREFIX)
                    .is_some_and(|name| !name.is_empty()) => {}
            (PortDirection::Output, IMAGE_OUTPUT_PORT)
                if port.data_type == crate::model::project::PortDataType::Image =>
            {
                image_outputs += 1;
            }
            (PortDirection::Output, STYLE_OUTPUT_PORT)
                if port.data_type == crate::model::project::PortDataType::Style =>
            {
                style_outputs += 1;
            }
            _ => return false,
        }
    }
    shape_inputs == 1 && image_outputs == 1 && style_outputs == 1
}

/// Whether a descriptor can run as an inline Text Ensemble operation.
///
/// The Text source supplies exactly one implicit Shape target. Time and
/// descriptor properties come from the authoring evaluator. Any second media
/// input (for example the geometry-only Backplate background Shape) requires
/// real graph topology and therefore remains a Node Editor operation.
pub fn text_ensemble_direct_contract_is_compatible(
    ports: &[crate::model::project::PortDefinition],
) -> bool {
    use std::collections::HashSet;

    use crate::model::project::{
        PortDataType, PortDirection, PortMultiplicity, SHAPE_INPUT_PORT, SHAPE_OUTPUT_PORT,
        TIME_PORT,
    };

    let mut target_inputs = 0;
    let mut shape_outputs = 0;
    let mut keys = HashSet::new();
    for port in ports {
        if !keys.insert(port.key.as_str()) || port.multiplicity != PortMultiplicity::Single {
            return false;
        }
        match (port.direction, port.key.as_str()) {
            (PortDirection::Input, SHAPE_INPUT_PORT) if port.data_type == PortDataType::Shape => {
                target_inputs += 1;
            }
            (PortDirection::Input, TIME_PORT) if port.data_type == PortDataType::Number => {}
            (PortDirection::Input, key)
                if key
                    .strip_prefix(crate::plugin::PROPERTY_PORT_PREFIX)
                    .is_some_and(|name| !name.is_empty()) => {}
            (PortDirection::Output, SHAPE_OUTPUT_PORT) if port.data_type == PortDataType::Shape => {
                shape_outputs += 1;
            }
            _ => return false,
        }
    }
    target_inputs == 1 && shape_outputs == 1
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
#[serde(deny_unknown_fields)]
pub struct ShapeSource {
    pub shape_kind: ShapeKind,
    pub parameters: HashMap<String, PropertyValue>,
    /// Ordered paint entries sharing this Shape's body and composite phases.
    pub appearance_operations: Vec<AppearanceOperation>,
}

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum ShapeKind {
    Rectangle,
    Ellipse,
    Path,
}

/// Placement payload for a nested Timeline definition.
#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
#[serde(deny_unknown_fields)]
pub struct CompositionInstance {
    pub timeline_id: TimelineId,
    pub duration_policy: DurationPolicy,
    pub parameter_overrides: HashMap<CompositionParameterId, PropertyValue>,
    /// Concrete nested-placement differences for Transition Modules below
    /// this outermost Composition placement.
    pub transition_module_overrides: Vec<TransitionModuleInstanceOverrides>,
}

/// One public control owned by a Timeline definition.
///
/// The target may refer to definition-internal items, but callers placing the
/// Timeline can address it only through [`CompositionParameterId`].
#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
#[serde(deny_unknown_fields)]
pub struct CompositionParameter {
    pub id: CompositionParameterId,
    pub name: String,
    pub data_type: PortDataType,
    pub default_value: PropertyValue,
    pub target: CompositionParameterTarget,
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug, Hash)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum CompositionParameterTarget {
    TextContent {
        item_id: TimelineItemId,
    },
    ItemProperty {
        item_id: TimelineItemId,
        property_key: String,
    },
}

impl CompositionParameterTarget {
    pub const fn item_id(&self) -> TimelineItemId {
        match self {
            Self::TextContent { item_id } | Self::ItemProperty { item_id, .. } => *item_id,
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug, Default)]
#[serde(deny_unknown_fields)]
pub struct TimeMap {
    pub source_start: MediaTime,
    pub playback_rate: RationalRate,
}

impl TimeMap {
    pub fn local_time(
        self,
        interval: TimelineInterval,
        timeline_time: MediaTime,
    ) -> Result<MediaTime, String> {
        timeline_time
            .checked_sub(interval.start)?
            .checked_mul_rate(self.playback_rate)?
            .checked_add(self.source_start)
    }
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum DurationPolicy {
    Fixed,
    Scale,
    Loop,
    Responsive {
        intro_end: MediaTime,
        outro_start: MediaTime,
    },
}

/// Runtime nesting address. Item IDs, rather than Module-internal IDs, form
/// the path so repeated nested Timeline placements remain distinguishable.
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, PartialOrd, Ord, Debug, Hash)]
#[serde(deny_unknown_fields)]
pub struct InstancePath {
    pub root_timeline_id: TimelineId,
    pub composition_items: Vec<TimelineItemId>,
}

impl InstancePath {
    pub fn root(root_timeline_id: TimelineId) -> Self {
        Self {
            root_timeline_id,
            composition_items: Vec::new(),
        }
    }

    pub fn nested(&self, item_id: TimelineItemId) -> Self {
        let mut composition_items = self.composition_items.clone();
        composition_items.push(item_id);
        Self {
            root_timeline_id: self.root_timeline_id,
            composition_items,
        }
    }
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
#[serde(deny_unknown_fields)]
pub struct ModuleInvocation {
    pub instance_id: ModuleInstanceId,
    pub output_id: ModuleOutputId,
    pub input_bindings: HashMap<PublishedMediaInputId, MediaInputBinding>,
    /// Keyframes remain owned by the Timeline host, never the Module graph.
    pub automation_tracks: HashMap<PublishedParameterId, AutomationTrack>,
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum MediaInputBinding {
    TimelineItemOutput {
        locator: InstanceLocator,
        item_id: TimelineItemId,
        output: MediaOutputKind,
        stage: ItemOutputStage,
    },
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug)]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum InstanceLocator {
    SameTimeline,
    Exact(InstancePath),
}

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum MediaOutputKind {
    Image,
    Audio,
}

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum ItemOutputStage {
    Content,
    PostEffects,
    PostTransform,
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
#[serde(deny_unknown_fields)]
pub struct AutomationTrack {
    pub keyframes: Vec<AutomationKeyframe>,
}

impl AutomationTrack {
    pub fn new(keyframe: AutomationKeyframe) -> Result<Self, String> {
        Ok(Self {
            keyframes: vec![keyframe],
        })
    }

    pub fn upsert(
        &mut self,
        time: MediaTime,
        value: PropertyValue,
        easing: Option<EasingFunction>,
    ) -> Result<KeyframeId, String> {
        if let Some(existing) = self
            .keyframes
            .iter_mut()
            .find(|keyframe| keyframe.time == time)
        {
            existing.value = value;
            if let Some(easing) = easing {
                existing.easing = easing;
            }
            return Ok(existing.id);
        }
        let keyframe =
            AutomationKeyframe::new(time, value, easing.unwrap_or(EasingFunction::Linear));
        let id = keyframe.id;
        self.keyframes.push(keyframe);
        self.keyframes.sort_by_key(|keyframe| keyframe.time);
        Ok(id)
    }

    pub fn update_keyframe(
        &mut self,
        keyframe_id: KeyframeId,
        time: Option<MediaTime>,
        value: Option<PropertyValue>,
        easing: Option<EasingFunction>,
    ) -> Result<(), String> {
        if time.is_some_and(MediaTime::is_negative) {
            return Err("Automation Keyframe time must be non-negative".to_string());
        }
        if time.is_some_and(|time| {
            self.keyframes
                .iter()
                .any(|keyframe| keyframe.id != keyframe_id && keyframe.time == time)
        }) {
            return Err("Automation already has a Keyframe at that time".to_string());
        }
        let keyframe = self
            .keyframes
            .iter_mut()
            .find(|keyframe| keyframe.id == keyframe_id)
            .ok_or_else(|| format!("Missing Automation Keyframe {keyframe_id}"))?;
        if let Some(time) = time {
            keyframe.time = time;
        }
        if let Some(value) = value {
            keyframe.value = value;
        }
        if let Some(easing) = easing {
            keyframe.easing = easing;
        }
        self.keyframes.sort_by_key(|keyframe| keyframe.time);
        Ok(())
    }

    pub fn remove_keyframe(&mut self, keyframe_id: KeyframeId) -> bool {
        let before = self.keyframes.len();
        self.keyframes.retain(|keyframe| keyframe.id != keyframe_id);
        self.keyframes.len() != before
    }

    /// Samples the effective Timeline-owned value using the same interpolation
    /// for rendering, Inspector fields, and any future automation consumer.
    pub fn evaluate_at(
        &self,
        time: MediaTime,
    ) -> Result<PropertyValue, crate::error::LibraryError> {
        let first = self.keyframes.first().ok_or_else(|| {
            crate::error::LibraryError::Validation("Automation Track has no Keyframes".to_string())
        })?;
        if time <= first.time {
            return Ok(first.value.clone());
        }
        let last = self.keyframes.last().ok_or_else(|| {
            crate::error::LibraryError::Validation(
                "Automation Track has no last Keyframe".to_string(),
            )
        })?;
        if time >= last.time {
            return Ok(last.value.clone());
        }
        for window in self.keyframes.windows(2) {
            let Some(start) = window.first() else {
                continue;
            };
            let Some(end) = window.get(1) else {
                continue;
            };
            if time < start.time || time >= end.time {
                continue;
            }
            let elapsed = time
                .checked_sub(start.time)
                .map_err(crate::error::LibraryError::Validation)?
                .to_seconds_f64();
            let duration = end
                .time
                .checked_sub(start.time)
                .map_err(crate::error::LibraryError::Validation)?
                .to_seconds_f64();
            if duration <= f64::EPSILON {
                return Ok(start.value.clone());
            }
            let amount = start.easing.try_apply(elapsed / duration)?;
            return Ok(PropertyValue::interpolate(&start.value, &end.value, amount));
        }
        Err(crate::error::LibraryError::Render(
            "Automation time did not resolve to a Keyframe segment".to_string(),
        ))
    }
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug)]
#[serde(deny_unknown_fields)]
pub struct AutomationKeyframe {
    pub id: KeyframeId,
    pub time: MediaTime,
    pub value: PropertyValue,
    pub easing: EasingFunction,
}

impl AutomationKeyframe {
    pub fn new(time: MediaTime, value: PropertyValue, easing: EasingFunction) -> Self {
        Self {
            id: KeyframeId::new(),
            time,
            value,
            easing,
        }
    }
}

#[cfg(test)]
mod automation_tests {
    use super::*;
    use ordered_float::OrderedFloat;

    #[test]
    fn automation_midpoint_uses_the_authoritative_interpolation() {
        let track = AutomationTrack {
            keyframes: vec![
                AutomationKeyframe::new(
                    MediaTime::zero(),
                    PropertyValue::Number(OrderedFloat(10.0)),
                    EasingFunction::Linear,
                ),
                AutomationKeyframe::new(
                    MediaTime::new(2, 1).expect("end time"),
                    PropertyValue::Number(OrderedFloat(30.0)),
                    EasingFunction::Linear,
                ),
            ],
        };

        assert_eq!(
            track
                .evaluate_at(MediaTime::new(1, 1).expect("sample time"))
                .expect("sample"),
            PropertyValue::Number(OrderedFloat(20.0))
        );
    }
}
