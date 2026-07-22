//! Persisted color-pipeline intent owned by the authoritative [`Project`](super::Project).
//!
//! This module records identities only; it neither opens files nor creates
//! color processors. A persisted request is therefore either model-validated
//! or explicitly unavailable. Backend config availability and external file
//! bytes must still be verified when resources are opened. There is
//! intentionally no implicit fallback that could reinterpret authored
//! color-space names under a different config.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

use super::asset::{Asset, AssetSourceColorSpaceBinding, AssetSourceColorSpaceBindingError};

mod persistence;
mod validation;

pub const DEFAULT_BUNDLED_COLOR_CONFIG_ID: &str = "ruvie://color-config/builtin-linear-srgb-v1";
pub const DEFAULT_WORKING_COLOR_SPACE: &str = "linear-srgb";
pub const DEFAULT_PREVIEW_DISPLAY: &str = "srgb";
/// The built-in backend performs a direct working-space to display-space
/// transform. Named display views are an OCIO contract and are not silently
/// accepted by the built-in backend.
pub const DEFAULT_PREVIEW_VIEW: Option<&str> = None;
pub const DEFAULT_OUTPUT_COLOR_SPACE: &str = "srgb";

/// Stable identity of the color configuration used to interpret space names.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ColorConfigIdentity {
    /// A versioned config shipped as part of RuViE itself.
    Bundled { id: String },
    /// An exact entry from OpenColorIO's built-in config registry.
    OcioBuiltin { uri: String, ocio_version: String },
    /// An external `.ocio` config stored as a Project asset. `sha256` is the
    /// expected identity and must match the Asset's independently recorded
    /// imported-content digest before this request is model-validated.
    ProjectAsset {
        asset_id: Uuid,
        sha256: String,
        ocio_version: String,
    },
}

impl Default for ColorConfigIdentity {
    fn default() -> Self {
        Self::Bundled {
            id: DEFAULT_BUNDLED_COLOR_CONFIG_ID.to_string(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PreviewColorConfig {
    #[serde(default = "default_preview_display")]
    display: String,
    #[serde(default)]
    view: Option<String>,
}

impl PreviewColorConfig {
    pub fn new(display: impl Into<String>, view: Option<String>) -> Self {
        Self {
            display: display.into(),
            view,
        }
    }

    pub fn direct(display: impl Into<String>) -> Self {
        Self::new(display, None)
    }

    pub fn display(&self) -> &str {
        &self.display
    }

    pub fn view(&self) -> Option<&str> {
        self.view.as_deref()
    }
}

impl Default for PreviewColorConfig {
    fn default() -> Self {
        Self::new(
            DEFAULT_PREVIEW_DISPLAY,
            DEFAULT_PREVIEW_VIEW.map(str::to_string),
        )
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExportColorConfig {
    #[serde(default = "default_output_color_space")]
    output_space: String,
}

impl ExportColorConfig {
    pub fn new(output_space: impl Into<String>) -> Self {
        Self {
            output_space: output_space.into(),
        }
    }

    pub fn output_space(&self) -> &str {
        &self.output_space
    }
}

impl Default for ExportColorConfig {
    fn default() -> Self {
        Self::new(DEFAULT_OUTPUT_COLOR_SPACE)
    }
}

/// Project-wide color-management intent shared by every editing view.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ColorManagementConfig {
    #[serde(default)]
    config: ColorConfigIdentity,
    #[serde(default = "default_working_color_space")]
    working_space: String,
    #[serde(default)]
    preview: PreviewColorConfig,
    #[serde(default)]
    export: ExportColorConfig,
}

impl ColorManagementConfig {
    pub fn new(
        config: ColorConfigIdentity,
        working_space: impl Into<String>,
        preview: PreviewColorConfig,
        export: ExportColorConfig,
    ) -> Self {
        Self {
            config,
            working_space: working_space.into(),
            preview,
            export,
        }
    }

    pub fn config(&self) -> &ColorConfigIdentity {
        &self.config
    }

    pub fn working_space(&self) -> &str {
        &self.working_space
    }

    pub fn preview(&self) -> &PreviewColorConfig {
        &self.preview
    }

    pub fn export(&self) -> &ExportColorConfig {
        &self.export
    }

    /// Creates an Asset source-space assignment owned by this exact config.
    /// This is the public construction path so callers cannot accidentally
    /// persist an unqualified OpenColorIO color-space name.
    pub fn source_space_binding(
        &self,
        color_space: impl Into<String>,
    ) -> Result<AssetSourceColorSpaceBinding, AssetSourceColorSpaceBindingError> {
        AssetSourceColorSpaceBinding::new(self.config.clone(), color_space)
    }

    pub fn diagnostics(&self, assets: &[Asset]) -> Vec<ColorManagementIssue> {
        validation::diagnostics(self, assets)
    }

    pub(super) fn blocking_diagnostics(&self, assets: &[Asset]) -> Vec<ColorManagementIssue> {
        validation::blocking_diagnostics(self, assets)
    }

    fn stable_cache_identity(&self) -> ColorConfigCacheIdentity {
        validation::stable_cache_identity(self)
    }
}

impl Default for ColorManagementConfig {
    fn default() -> Self {
        Self::new(
            ColorConfigIdentity::default(),
            DEFAULT_WORKING_COLOR_SPACE,
            PreviewColorConfig::default(),
            ExportColorConfig::default(),
        )
    }
}

/// Exact persisted value of the `color_management` Project field.
///
/// Malformed JSON remains serializable as-is so opening and saving a Project
/// does not erase data that a future repair UI could recover.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RequestedColorManagementConfig {
    Config(ColorManagementConfig),
    Malformed {
        raw: Value,
        structure_issues: Vec<ColorManagementStructureIssue>,
    },
}

impl RequestedColorManagementConfig {
    pub fn as_config(&self) -> Option<&ColorManagementConfig> {
        match self {
            Self::Config(config) => Some(config),
            Self::Malformed { .. } => None,
        }
    }

    pub fn malformed_raw(&self) -> Option<&Value> {
        match self {
            Self::Config(_) => None,
            Self::Malformed { raw, .. } => Some(raw),
        }
    }

    pub(super) fn diagnostics(&self, assets: &[Asset]) -> Vec<ColorManagementIssue> {
        match self {
            Self::Config(config) => config.diagnostics(assets),
            Self::Malformed {
                structure_issues, ..
            } => structure_issues
                .iter()
                .cloned()
                .map(|issue| ColorManagementIssue::MalformedStructure { issue })
                .collect(),
        }
    }

    pub(super) fn blocking_diagnostics(&self, assets: &[Asset]) -> Vec<ColorManagementIssue> {
        match self {
            Self::Config(config) => config.blocking_diagnostics(assets),
            Self::Malformed {
                structure_issues, ..
            } => structure_issues
                .iter()
                .cloned()
                .map(|issue| ColorManagementIssue::MalformedStructure { issue })
                .collect(),
        }
    }
}

impl Default for RequestedColorManagementConfig {
    fn default() -> Self {
        Self::Config(ColorManagementConfig::default())
    }
}

/// Stable identity for locating processor/LUT cache entries. It exists only
/// for a model-validated intent, but is not proof that a backend has the config
/// or that an external Asset path is still readable.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize)]
#[serde(transparent)]
pub struct ColorConfigCacheIdentity(String);

impl ColorConfigCacheIdentity {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for ColorConfigCacheIdentity {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.0)
    }
}

/// Model validation never substitutes another color configuration.
///
/// `Ready` means that persisted identities are structurally and semantically
/// consistent. A backend must still verify an exact OCIO registry config, and
/// a resource loader must re-read and hash an external `.ocio` file before
/// creating a processor.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ModelValidatedColorManagementConfig {
    config: ColorManagementConfig,
    cache_identity: ColorConfigCacheIdentity,
}

impl ModelValidatedColorManagementConfig {
    pub fn config(&self) -> &ColorManagementConfig {
        &self.config
    }

    pub fn cache_identity(&self) -> &ColorConfigCacheIdentity {
        &self.cache_identity
    }

    /// Returns an Asset's explicit source-space assignment only when it belongs
    /// to this validated Project config. Callers must fail this Asset's ingress
    /// on `Err`; the rest of the Project remains usable.
    pub fn assigned_source_space<'a>(
        &self,
        asset: &'a Asset,
    ) -> Result<Option<&'a super::asset::AssetSourceColorSpaceBinding>, ColorManagementIssue> {
        validation::validate_asset_source_binding(&self.config, asset)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ResolvedColorManagementConfig {
    Ready(ModelValidatedColorManagementConfig),
    Unavailable {
        requested: RequestedColorManagementConfig,
        diagnostics: Vec<ColorManagementIssue>,
    },
}

impl ResolvedColorManagementConfig {
    pub fn model_validated_intent(&self) -> Option<&ModelValidatedColorManagementConfig> {
        match self {
            Self::Ready(intent) => Some(intent),
            Self::Unavailable { .. } => None,
        }
    }

    pub fn diagnostics(&self) -> &[ColorManagementIssue] {
        match self {
            Self::Ready(_) => &[],
            Self::Unavailable { diagnostics, .. } => diagnostics,
        }
    }

    pub fn unavailable_request(&self) -> Option<&RequestedColorManagementConfig> {
        match self {
            Self::Ready(_) => None,
            Self::Unavailable { requested, .. } => Some(requested),
        }
    }
}

/// Structural failures found while decoding `color_management`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(tag = "problem", rename_all = "snake_case")]
pub enum ColorManagementStructureIssue {
    Null {
        path: String,
    },
    WrongType {
        path: String,
        expected: String,
        actual: String,
    },
    MissingField {
        path: String,
    },
    UnknownConfigKind {
        kind: String,
    },
    UnknownField {
        path: String,
    },
    InvalidValue {
        path: String,
        detail: String,
    },
}

/// Structured non-fatal diagnostics for persisted color configuration.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(tag = "code", content = "context", rename_all = "snake_case")]
pub enum ColorManagementIssue {
    MalformedStructure {
        issue: ColorManagementStructureIssue,
    },
    BlankIdentifier {
        field: ColorManagementField,
    },
    MissingRequiredPreviewView,
    UnsupportedBundledPreviewView {
        view: String,
    },
    MovingConfigIdentifier {
        identifier: String,
    },
    InvalidBundledConfigId {
        identifier: String,
    },
    InvalidOcioBuiltinUri {
        uri: String,
    },
    UnpinnedOcioBuiltinUri {
        uri: String,
    },
    InvalidOcioVersion {
        version: String,
    },
    ConfigAssetNotFound {
        asset_id: Uuid,
    },
    ConfigAssetWrongKind {
        asset_id: Uuid,
    },
    ConfigAssetNotOcio {
        asset_id: Uuid,
        path: String,
    },
    InvalidConfigChecksum {
        asset_id: Uuid,
        sha256: String,
    },
    ConfigAssetChecksumUnverified {
        asset_id: Uuid,
    },
    InvalidImportedContentChecksum {
        asset_id: Uuid,
        sha256: String,
    },
    ConfigAssetChecksumMismatch {
        asset_id: Uuid,
        expected: String,
        imported: String,
    },
    AssetSourceColorSpaceBlank {
        asset_id: Uuid,
    },
    AssetSourceColorConfigMismatch {
        asset_id: Uuid,
        assigned: Box<ColorConfigIdentity>,
        project: Box<ColorConfigIdentity>,
    },
    AssetSourceColorBindingMalformed {
        asset_id: Uuid,
        detail: String,
    },
}

impl std::fmt::Display for ColorManagementIssue {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::MalformedStructure { issue } => {
                write!(formatter, "malformed color configuration: {issue:?}")
            }
            Self::BlankIdentifier { field } => write!(formatter, "{field} must not be blank"),
            Self::MissingRequiredPreviewView => {
                formatter.write_str("an OpenColorIO Preview requires an explicit display view")
            }
            Self::UnsupportedBundledPreviewView { view } => write!(
                formatter,
                "the built-in color config does not support named Preview view '{view}'"
            ),
            Self::MovingConfigIdentifier { identifier } => write!(
                formatter,
                "color config identifier '{identifier}' is a moving alias"
            ),
            Self::InvalidBundledConfigId { identifier } => write!(
                formatter,
                "bundled color config id '{identifier}' is not a versioned RuViE config URI"
            ),
            Self::InvalidOcioBuiltinUri { uri } => write!(
                formatter,
                "OpenColorIO built-in config URI '{uri}' is invalid"
            ),
            Self::UnpinnedOcioBuiltinUri { uri } => write!(
                formatter,
                "OpenColorIO built-in config URI '{uri}' is not an exact versioned identity"
            ),
            Self::InvalidOcioVersion { version } => {
                write!(formatter, "OpenColorIO version '{version}' is not pinned")
            }
            Self::ConfigAssetNotFound { asset_id } => {
                write!(formatter, "color config asset {asset_id} does not exist")
            }
            Self::ConfigAssetWrongKind { asset_id } => write!(
                formatter,
                "color config asset {asset_id} must use the non-media Asset kind"
            ),
            Self::ConfigAssetNotOcio { asset_id, path } => write!(
                formatter,
                "color config asset {asset_id} path '{path}' is not an .ocio file"
            ),
            Self::InvalidConfigChecksum { asset_id, sha256 } => write!(
                formatter,
                "color config asset {asset_id} has invalid expected SHA-256 '{sha256}'"
            ),
            Self::ConfigAssetChecksumUnverified { asset_id } => write!(
                formatter,
                "color config asset {asset_id} has no imported-content checksum"
            ),
            Self::InvalidImportedContentChecksum { asset_id, sha256 } => write!(
                formatter,
                "color config asset {asset_id} has invalid imported-content SHA-256 '{sha256}'"
            ),
            Self::ConfigAssetChecksumMismatch {
                asset_id,
                expected,
                imported,
            } => write!(
                formatter,
                "color config asset {asset_id} expected SHA-256 '{expected}' but imported '{imported}'"
            ),
            Self::AssetSourceColorSpaceBlank { asset_id } => write!(
                formatter,
                "asset {asset_id} has a blank assigned source color-space name"
            ),
            Self::AssetSourceColorConfigMismatch {
                asset_id,
                assigned,
                project,
            } => write!(
                formatter,
                "asset {asset_id} source color space belongs to config {assigned:?}, but the Project uses {project:?}"
            ),
            Self::AssetSourceColorBindingMalformed { asset_id, detail } => write!(
                formatter,
                "asset {asset_id} has an unrecognized source color-space binding: {detail}"
            ),
        }
    }
}

impl std::error::Error for ColorManagementIssue {}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ColorManagementField {
    ConfigIdentifier,
    OcioVersion,
    WorkingSpace,
    PreviewDisplay,
    PreviewView,
    OutputSpace,
}

impl std::fmt::Display for ColorManagementField {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ConfigIdentifier => formatter.write_str("color config identifier"),
            Self::OcioVersion => formatter.write_str("OpenColorIO version"),
            Self::WorkingSpace => formatter.write_str("working color space"),
            Self::PreviewDisplay => formatter.write_str("Preview display"),
            Self::PreviewView => formatter.write_str("Preview view"),
            Self::OutputSpace => formatter.write_str("output color space"),
        }
    }
}

pub(super) fn resolve_color_management(
    requested: &RequestedColorManagementConfig,
    assets: &[Asset],
) -> ResolvedColorManagementConfig {
    let diagnostics = requested.blocking_diagnostics(assets);
    match (requested, diagnostics.is_empty()) {
        (RequestedColorManagementConfig::Config(config), true) => {
            ResolvedColorManagementConfig::Ready(ModelValidatedColorManagementConfig {
                cache_identity: config.stable_cache_identity(),
                config: config.clone(),
            })
        }
        _ => ResolvedColorManagementConfig::Unavailable {
            requested: requested.clone(),
            diagnostics,
        },
    }
}

fn default_working_color_space() -> String {
    DEFAULT_WORKING_COLOR_SPACE.to_string()
}

fn default_preview_display() -> String {
    DEFAULT_PREVIEW_DISPLAY.to_string()
}

fn default_output_color_space() -> String {
    DEFAULT_OUTPUT_COLOR_SPACE.to_string()
}
