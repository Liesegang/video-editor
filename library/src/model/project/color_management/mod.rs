//! Persisted color-pipeline intent owned by the authoritative [`Project`](super::Project).
//!
//! This module records identities only; it neither opens files nor creates
//! color processors. A persisted request is therefore either model-validated
//! or explicitly unavailable. Backend config availability and external file
//! bytes must still be verified when resources are opened. There is
//! intentionally no implicit fallback that could reinterpret authored
//! color-space names under a different config.

use serde::{Deserialize, Deserializer, Serialize, Serializer};
use serde_json::Value;
use uuid::Uuid;

use super::asset::{Asset, AssetSourceColorSpaceBinding, AssetSourceColorSpaceBindingError};

mod hdr;
mod persistence;
mod validation;

pub use hdr::{HdrColorField, HdrColorSettings, HdrColorSettingsError, PqLinearizationPolicy};

/// Exact identity of RuViE's built-in standard scene/display-space catalog.
///
/// This is deliberately not the former sRGB-only v1 identity: color-space
/// names must never acquire wider semantics while retaining an old config
/// fingerprint.
pub const DEFAULT_BUNDLED_COLOR_CONFIG_ID: &str = "ruvie://color-config/builtin-standard-spaces-v3";
/// Exact identity persisted by RuViE before the built-in standard-space
/// catalog was expanded. It remains available with its original two-space
/// semantics so an existing pre-v1 Project is not silently reinterpreted.
pub const LEGACY_BUNDLED_COLOR_CONFIG_V1_ID: &str = "ruvie://color-config/builtin-linear-srgb-v1";
pub const DEFAULT_WORKING_COLOR_SPACE: &str = "linear-srgb";
pub const DEFAULT_PREVIEW_DISPLAY: &str = "srgb";
pub const DEFAULT_PREVIEW_SURFACE_ENCODING: PreviewSurfaceEncoding = PreviewSurfaceEncoding::Srgb;
/// The built-in backend performs a direct working-space to display-space
/// transform. Named display views are an OCIO contract and are not silently
/// accepted by the built-in backend.
pub const DEFAULT_PREVIEW_VIEW: Option<&str> = None;
pub const DEFAULT_OUTPUT_COLOR_SPACE: &str = "srgb";

/// Encoding expected by the native Preview surface after its display transform.
///
/// Unknown future strings remain intact across load/save so the Project can be
/// repaired by a newer build instead of being silently reinterpreted as sRGB.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum PreviewSurfaceEncoding {
    Srgb,
    Unknown(String),
}

impl PreviewSurfaceEncoding {
    pub const SRGB_ID: &'static str = "srgb";

    pub fn from_id(id: impl Into<String>) -> Self {
        let id = id.into();
        if id == Self::SRGB_ID {
            Self::Srgb
        } else {
            Self::Unknown(id)
        }
    }

    pub fn as_str(&self) -> &str {
        match self {
            Self::Srgb => Self::SRGB_ID,
            Self::Unknown(id) => id,
        }
    }

    pub const fn is_srgb(&self) -> bool {
        matches!(self, Self::Srgb)
    }

    pub const fn direct_destination_space(&self) -> Option<&'static str> {
        match self {
            Self::Srgb => Some(ruvie_color_management::SRGB_SPACE_ID),
            Self::Unknown(_) => None,
        }
    }
}

impl Default for PreviewSurfaceEncoding {
    fn default() -> Self {
        DEFAULT_PREVIEW_SURFACE_ENCODING
    }
}

impl Serialize for PreviewSurfaceEncoding {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for PreviewSurfaceEncoding {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        String::deserialize(deserializer).map(Self::from_id)
    }
}

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

/// Exact config-local color space used for RuViE-authored sRGBA8 values and
/// native sRGB display surfaces.
///
/// The duplicated config identity is intentional. It prevents a persisted
/// color-space name from silently acquiring different semantics if the
/// Project switches to another OCIO config. The name itself is never inferred
/// from aliases such as `sRGB`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SrgbSurfaceColorSpaceBinding {
    config: ColorConfigIdentity,
    color_space: String,
}

impl SrgbSurfaceColorSpaceBinding {
    fn new(config: ColorConfigIdentity, color_space: impl Into<String>) -> Self {
        Self {
            config,
            color_space: color_space.into(),
        }
    }

    pub fn config(&self) -> &ColorConfigIdentity {
        &self.config
    }

    pub fn color_space(&self) -> &str {
        &self.color_space
    }
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
    #[serde(default)]
    surface_encoding: PreviewSurfaceEncoding,
    /// Exact config-local color-space name reported for an OCIO display/view.
    /// Its meaning is bound to the owning [`ColorConfigIdentity`].
    #[serde(default)]
    view_output_color_space: Option<String>,
}

impl PreviewColorConfig {
    pub fn new(display: impl Into<String>, view: Option<String>) -> Self {
        Self::new_with_surface_encoding(display, view, PreviewSurfaceEncoding::default())
    }

    pub fn new_with_surface_encoding(
        display: impl Into<String>,
        view: Option<String>,
        surface_encoding: PreviewSurfaceEncoding,
    ) -> Self {
        Self {
            display: display.into(),
            view,
            surface_encoding,
            view_output_color_space: None,
        }
    }

    /// Atomically bind a named OCIO view to its exact config-local output
    /// color-space name and the native surface encoding receiving its pixels.
    pub fn named_view(
        display: impl Into<String>,
        view: impl Into<String>,
        view_output_color_space: impl Into<String>,
        surface_encoding: PreviewSurfaceEncoding,
    ) -> Self {
        Self {
            display: display.into(),
            view: Some(view.into()),
            surface_encoding,
            view_output_color_space: Some(view_output_color_space.into()),
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

    pub fn surface_encoding(&self) -> &PreviewSurfaceEncoding {
        &self.surface_encoding
    }

    pub fn view_output_color_space(&self) -> Option<&str> {
        self.view_output_color_space.as_deref()
    }

    #[must_use]
    pub fn with_surface_encoding(mut self, surface_encoding: PreviewSurfaceEncoding) -> Self {
        self.surface_encoding = surface_encoding;
        self
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
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
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
    /// Exact mapping for legacy/UI-authored straight sRGBA8 values and the
    /// egui sRGB texture boundary. Custom OCIO configs must bind this
    /// explicitly; a bare `sRGB` space name is never guessed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    srgb_surface_space: Option<SrgbSurfaceColorSpaceBinding>,
    #[serde(default, skip_serializing_if = "HdrColorSettings::is_empty")]
    hdr: HdrColorSettings,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PersistedColorManagementConfig {
    #[serde(default)]
    config: ColorConfigIdentity,
    #[serde(default = "default_working_color_space")]
    working_space: String,
    #[serde(default)]
    preview: PreviewColorConfig,
    #[serde(default)]
    export: ExportColorConfig,
    #[serde(default)]
    srgb_surface_space: Option<SrgbSurfaceColorSpaceBinding>,
    #[serde(default)]
    hdr: HdrColorSettings,
}

impl<'de> Deserialize<'de> for ColorManagementConfig {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let persisted = PersistedColorManagementConfig::deserialize(deserializer)?;
        let srgb_surface_space = persisted
            .srgb_surface_space
            .or_else(|| default_srgb_surface_binding_for(&persisted.config));
        Ok(Self {
            config: persisted.config,
            working_space: persisted.working_space,
            preview: persisted.preview,
            export: persisted.export,
            srgb_surface_space,
            hdr: persisted.hdr,
        })
    }
}

impl ColorManagementConfig {
    pub fn new(
        config: ColorConfigIdentity,
        working_space: impl Into<String>,
        preview: PreviewColorConfig,
        export: ExportColorConfig,
    ) -> Self {
        let srgb_surface_space = default_srgb_surface_binding_for(&config);
        Self {
            config,
            working_space: working_space.into(),
            preview,
            export,
            srgb_surface_space,
            hdr: HdrColorSettings::default(),
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

    pub fn srgb_surface_space(&self) -> Option<&SrgbSurfaceColorSpaceBinding> {
        self.srgb_surface_space.as_ref()
    }

    pub fn hdr(&self) -> &HdrColorSettings {
        &self.hdr
    }

    #[must_use]
    pub fn with_hdr_settings(mut self, hdr: HdrColorSettings) -> Self {
        self.hdr = hdr;
        self
    }

    /// Bind RuViE's authored sRGBA8 values and native sRGB surfaces to an
    /// exact color space in this Project's current config.
    #[must_use]
    pub fn with_srgb_surface_space(mut self, color_space: impl Into<String>) -> Self {
        self.srgb_surface_space = Some(SrgbSurfaceColorSpaceBinding::new(
            self.config.clone(),
            color_space,
        ));
        self
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
    Config(Box<ColorManagementConfig>),
    Malformed {
        raw: Value,
        structure_issues: Vec<ColorManagementStructureIssue>,
    },
}

impl RequestedColorManagementConfig {
    pub(super) fn from_config(config: ColorManagementConfig) -> Self {
        Self::Config(Box::new(config))
    }

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
        Self::Config(Box::default())
    }
}

/// Stable identity for locating processor/LUT cache entries. It exists only
/// for a model-validated intent, but is not proof that a backend has the config
/// or that an external Asset path is still readable.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize)]
#[serde(transparent)]
pub(super) struct ColorConfigCacheIdentity(String);

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

    pub fn cache_identity(&self) -> &str {
        self.cache_identity.as_str()
    }

    /// Active-config-checked authority for UI-authored sRGBA8 and native sRGB
    /// surfaces. A bare color-space name cannot be obtained through this API.
    pub fn srgb_surface_space(
        &self,
    ) -> Result<&SrgbSurfaceColorSpaceBinding, ColorManagementIssue> {
        validation::validated_srgb_surface_binding(&self.config)
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
    Ready(Box<ModelValidatedColorManagementConfig>),
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
    UnsupportedPreviewSurfaceEncoding {
        encoding: String,
    },
    DirectPreviewSurfaceEncodingMismatch {
        destination: String,
        surface_encoding: PreviewSurfaceEncoding,
    },
    MissingPreviewViewOutputColorSpace,
    UnexpectedDirectPreviewViewOutputColorSpace {
        output_space: String,
    },
    MissingSrgbSurfaceColorSpaceBinding,
    SrgbSurfaceColorSpaceBindingMismatch {
        bound: Box<ColorConfigIdentity>,
        project: Box<ColorConfigIdentity>,
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
    InvalidHdrSetting {
        field: HdrColorField,
        detail: String,
    },
    MissingHdrSetting {
        field: HdrColorField,
        required_by: String,
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
            Self::UnsupportedPreviewSurfaceEncoding { encoding } => write!(
                formatter,
                "Preview surface encoding '{encoding}' is not supported by this build"
            ),
            Self::DirectPreviewSurfaceEncodingMismatch {
                destination,
                surface_encoding,
            } => write!(
                formatter,
                "direct Preview destination '{destination}' does not match surface encoding '{}'",
                surface_encoding.as_str()
            ),
            Self::MissingPreviewViewOutputColorSpace => formatter.write_str(
                "an OpenColorIO Preview requires the exact config-local display/view output color space",
            ),
            Self::UnexpectedDirectPreviewViewOutputColorSpace { output_space } => write!(
                formatter,
                "built-in direct Preview must not bind OCIO view output color space '{output_space}'"
            ),
            Self::MissingSrgbSurfaceColorSpaceBinding => formatter.write_str(
                "the Project color config has no exact sRGB authoring/surface color-space binding",
            ),
            Self::SrgbSurfaceColorSpaceBindingMismatch { bound, project } => write!(
                formatter,
                "the sRGB authoring/surface color-space binding belongs to config {bound:?}, but the Project uses {project:?}"
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
            Self::InvalidHdrSetting { field, detail } => {
                write!(formatter, "invalid {field}: {detail}")
            }
            Self::MissingHdrSetting { field, required_by } => {
                write!(formatter, "{field} is required by {required_by}")
            }
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
    PreviewViewOutputColorSpace,
    SrgbSurfaceColorSpace,
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
            Self::PreviewViewOutputColorSpace => {
                formatter.write_str("Preview view output color space")
            }
            Self::SrgbSurfaceColorSpace => {
                formatter.write_str("sRGB authoring/surface color space")
            }
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
            ResolvedColorManagementConfig::Ready(Box::new(ModelValidatedColorManagementConfig {
                cache_identity: config.stable_cache_identity(),
                config: config.as_ref().clone(),
            }))
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

fn default_srgb_surface_binding_for(
    config: &ColorConfigIdentity,
) -> Option<SrgbSurfaceColorSpaceBinding> {
    matches!(
        config,
        ColorConfigIdentity::Bundled { id } if is_supported_bundled_color_config_id(id)
    )
    .then(|| {
        SrgbSurfaceColorSpaceBinding::new(config.clone(), ruvie_color_management::SRGB_SPACE_ID)
    })
}

pub(crate) fn is_supported_bundled_color_config_id(id: &str) -> bool {
    matches!(
        id,
        DEFAULT_BUNDLED_COLOR_CONFIG_ID | LEGACY_BUNDLED_COLOR_CONFIG_V1_ID
    )
}
