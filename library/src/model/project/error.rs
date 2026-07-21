use serde::Serialize;
use thiserror::Error;
use uuid::Uuid;

use crate::model::BlendMode;

use super::{
    CompositionSettingsError, NodeContainer, PortAddress, PortDataType, PortDirection,
    PortExposure, PortOwner,
};

/// A side-effect-free Project integrity diagnostic. The adjacent JSON
/// representation is part of the QA API contract: `code` is stable and all
/// variant fields remain available as structured `context`.
#[derive(Clone, Debug, Error, PartialEq, Eq, Serialize)]
#[serde(tag = "code", content = "context", rename_all = "snake_case")]
pub enum ProjectGraphError {
    #[error("composition id {0} occurs more than once")]
    DuplicateCompositionId(Uuid),
    #[error("track map key {key} does not match entity id {entity_id}")]
    TrackKeyMismatch { key: Uuid, entity_id: Uuid },
    #[error("track id {0} already exists")]
    TrackAlreadyExists(Uuid),
    #[error("clip map key {key} does not match entity id {entity_id}")]
    ClipKeyMismatch { key: Uuid, entity_id: Uuid },
    #[error("node map key {key} does not match entity id {entity_id}")]
    NodeKeyMismatch { key: Uuid, entity_id: Uuid },
    #[error("resource map key {key} does not match entity id {entity_id}")]
    ResourceKeyMismatch { key: Uuid, entity_id: Uuid },
    #[error("asset id {0} occurs more than once")]
    DuplicateAssetId(Uuid),
    #[error("connection id {0} occurs more than once")]
    DuplicateConnectionId(Uuid),
    #[error("composition {composition_id} has invalid settings: {reason}")]
    InvalidCompositionSettings {
        composition_id: Uuid,
        reason: CompositionSettingsError,
    },
    #[error(
        "composition {composition_id} has invalid half-open work area {work_area_in}..{work_area_out}; valid frames are 0..{frame_count}"
    )]
    InvalidCompositionWorkArea {
        composition_id: Uuid,
        work_area_in: u64,
        work_area_out: u64,
        frame_count: u64,
    },
    #[error("node {0} does not exist")]
    NodeNotFound(Uuid),
    #[error("node graph bundle is empty")]
    EmptyNodeGraph,
    #[error("node graph bundle contains node id {0} more than once")]
    DuplicateNodeGraphNodeId(Uuid),
    #[error("node graph bundle node {0} already exists in the Project")]
    NodeGraphNodeAlreadyExists(Uuid),
    #[error("node graph bundle contains connection id {0} more than once")]
    DuplicateNodeGraphConnectionId(Uuid),
    #[error("node graph bundle connection {0} already exists in the Project")]
    NodeGraphConnectionAlreadyExists(Uuid),
    #[error("node graph bundle connection {0} does not touch a bundled node")]
    NodeGraphConnectionOutsideBundle(Uuid),
    #[error("node graph output {0} is not one of the bundled nodes")]
    NodeGraphOutputNotBundled(Uuid),
    #[error("clip {0} does not exist")]
    ClipNotFound(Uuid),
    #[error("track {0} does not exist")]
    TrackNotFound(Uuid),
    #[error("composition {0} does not exist")]
    CompositionNotFound(Uuid),
    #[error("composition instance node {node_id} targets missing composition {composition_id}")]
    CompositionInstanceTargetNotFound { node_id: Uuid, composition_id: Uuid },
    #[error(
        "composition instance node {node_id} must be directly contained by a clip, found {container:?}"
    )]
    CompositionInstanceOutsideClip {
        node_id: Uuid,
        container: NodeContainer,
    },
    #[error(
        "composition instance node {node_id} creates a cycle from composition {containing_composition_id} to {target_composition_id}"
    )]
    CompositionInstanceCycle {
        node_id: Uuid,
        containing_composition_id: Uuid,
        target_composition_id: Uuid,
    },
    #[error("track {track_id} is not a top-level child of composition {composition_id}")]
    TrackNotInComposition {
        track_id: Uuid,
        composition_id: Uuid,
    },
    #[error("track {track_id} is contained by more than one composition (first: {composition_id})")]
    TrackAlreadyContained {
        track_id: Uuid,
        composition_id: Uuid,
    },
    #[error("track {0} is not contained by a composition")]
    TrackHasNoComposition(Uuid),
    #[error("clip {clip_id} is not a child of track {track_id}")]
    ClipNotInTrack { clip_id: Uuid, track_id: Uuid },
    #[error("clip {clip_id} is contained by more than one track (first: {track_id})")]
    ClipAlreadyContained { clip_id: Uuid, track_id: Uuid },
    #[error("clip {0} is not contained by a track")]
    ClipHasNoTrack(Uuid),
    #[error("node {node_id} is already contained by {container:?}")]
    NodeAlreadyContained {
        node_id: Uuid,
        container: NodeContainer,
    },
    #[error("node {node_id} is not directly contained by {container:?}")]
    NodeNotInContainer {
        node_id: Uuid,
        container: NodeContainer,
    },
    #[error("node {0} is not contained by a composition, track, or clip")]
    NodeHasNoContainer(Uuid),
    #[error("output node {node_id} is not directly contained by {container:?}")]
    OutputNodeOutsideContainer {
        node_id: Uuid,
        container: NodeContainer,
    },
    #[error("output node {node_id} in {container:?} does not declare an image output port")]
    OutputNodeHasNoImagePort {
        node_id: Uuid,
        container: NodeContainer,
    },
    #[error("audio output node {node_id} in {container:?} does not declare an audio output port")]
    OutputNodeHasNoAudioPort {
        node_id: Uuid,
        container: NodeContainer,
    },
    #[error("structural Merge node {node_id} for {container:?} does not exist")]
    StructuralMergeNodeMissing {
        container: NodeContainer,
        node_id: Uuid,
    },
    #[error("structural Merge node {node_id} is not directly contained by {container:?}")]
    StructuralMergeNodeOutsideContainer {
        container: NodeContainer,
        node_id: Uuid,
    },
    #[error("structural Merge node {node_id} for {container:?} is not a Merge")]
    StructuralMergeNodeWrongType {
        container: NodeContainer,
        node_id: Uuid,
    },
    #[error(
        "structural Merge node {node_id} does not reach output node {output_node_id} in {container:?}"
    )]
    StructuralMergeDoesNotReachOutput {
        container: NodeContainer,
        node_id: Uuid,
        output_node_id: Uuid,
    },
    #[error(
        "direct child {child:?} has more than one structural edge into Merge node {node_id} for {container:?}"
    )]
    DuplicateStructuralChildEdge {
        container: NodeContainer,
        node_id: Uuid,
        child: PortOwner,
    },
    #[error(
        "structural Merge node {node_id} owned by {container:?} cannot be removed directly; remove the container instead"
    )]
    CannotRemoveStructuralMerge {
        container: NodeContainer,
        node_id: Uuid,
    },
    #[error("plugin operation node {node_id} has an empty {field} identifier")]
    EmptyPluginOperationIdentifier { node_id: Uuid, field: String },
    #[error("plugin operation node {node_id} declares an empty {direction:?} port key")]
    EmptyNodePortKey {
        node_id: Uuid,
        direction: PortDirection,
    },
    #[error("plugin operation node {node_id} declares {direction:?} port '{key}' more than once")]
    DuplicateNodePort {
        node_id: Uuid,
        key: String,
        direction: PortDirection,
    },
    #[error(
        "plugin operation node {node_id} port '{key}' uses container-only exposure {exposure:?}"
    )]
    InvalidNodePortExposure {
        node_id: Uuid,
        key: String,
        exposure: PortExposure,
    },
    #[error("plugin operation node {node_id} output port '{key}' cannot be variadic")]
    VariadicNodeOutput { node_id: Uuid, key: String },
    #[error("port does not exist: {0:?}")]
    PortNotFound(PortAddress),
    #[error("connection {0} does not exist")]
    ConnectionNotFound(Uuid),
    #[error(
        "connection {connection_id} cannot use {blend_mode:?}: non-normal blending requires an Image source"
    )]
    ConnectionBlendRequiresImageSource {
        connection_id: Uuid,
        blend_mode: BlendMode,
    },
    #[error(
        "connection {connection_id} cannot use {blend_mode:?}: target {target:?} is not a Merge images input"
    )]
    ConnectionBlendRequiresMergeImagesInput {
        connection_id: Uuid,
        blend_mode: BlendMode,
        target: PortAddress,
    },
    #[error("cannot splice through occupied single input: {target:?}")]
    SpliceInputOccupied { target: PortAddress },
    #[error("invalid Path Effect stack: {reason}")]
    InvalidPathEffectStack { reason: String },
    #[error("cannot connect {source_type:?} to {target_type:?}")]
    IncompatiblePortTypes {
        source_type: PortDataType,
        target_type: PortDataType,
    },
    #[error("internal port on {source_owner:?} cannot target {target_owner:?}")]
    InternalPortEscapesContainer {
        source_owner: PortOwner,
        target_owner: PortOwner,
    },
    #[error("single input port has more than one source: {target:?}")]
    DuplicateInputConnection { target: PortAddress },
    #[error("variadic input {target:?} has duplicate order {order}")]
    DuplicateConnectionOrder { target: PortAddress, order: i64 },
    #[error("connection {connection_id} uses unsupported render-time type {data_type:?}")]
    UnsupportedConnectionType {
        connection_id: Uuid,
        data_type: PortDataType,
    },
    #[error("graph connection would create an evaluation cycle from {from:?} to {to:?}")]
    ConnectionCycle { from: PortOwner, to: PortOwner },
}
