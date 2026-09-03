pub mod animation;
pub mod authoring;
mod blend;
pub mod frame;
pub mod node;
#[allow(
    dead_code,
    reason = "numeric evaluation is activated by compiled Module operations"
)]
pub(crate) mod numeric;
pub mod path;
pub mod project;
pub mod vector;

pub use blend::{BlendMode, BlendModeGroup, BlendModeInfo};
pub use node::{
    COLOR_ALPHA_PORT, COLOR_BLUE_PORT, COLOR_GREEN_PORT, COLOR_MIX_FACTOR_PORT,
    COLOR_MIX_LEFT_PORT, COLOR_MIX_RIGHT_PORT, COLOR_RED_PORT, COLOR_SPACE_PORT,
    COLOR_TARGET_SPACE_PORT, COLOR_VALUE_PORT, ColorContent, DataContent, GeneratorContent,
    LEGACY_MEDIA_COLOR_PROPERTY_KEYS, LegacyMediaColorProperty, ListContent, MediaContent,
    NativeNodeCatalogDescriptor, NativeNodeFactory, NativeNodeRuntimeStatus,
    NativeOperationContent, Node, NodeContent, PathOperationContent, PluginOperationContent,
    SOUND_MERGE_OPERATION_KEY, SoundAnalysisContent, ValueContent,
    active_legacy_media_color_properties, is_legacy_media_color_property, native_node_catalog,
    native_node_descriptor, native_node_descriptor_for_node,
};
pub use project::asset::{self, Asset, AssetKind};
pub use project::property;
pub use project::{
    ColorConfigIdentity, ColorManagementConfig, ColorManagementField, ColorManagementIssue,
    ColorManagementStructureIssue, ExportColorConfig, ModelValidatedColorManagementConfig,
    PreviewColorConfig, RequestedColorManagementConfig, ResolvedColorManagementConfig,
};
