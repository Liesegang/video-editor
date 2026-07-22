//! Persisted color-pipeline intent owned by the authoritative [`Project`](super::Project).
//!
//! This module deliberately does not create processors. It records enough
//! stable identity for a color backend to reproduce a Project, and separates
//! requested data from the safe effective fallback used when persisted data
//! is incomplete or unavailable.

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::asset::Asset;

pub const DEFAULT_BUNDLED_COLOR_CONFIG_ID: &str = "ruvie://color-config/builtin-linear-srgb-v1";
pub const DEFAULT_WORKING_COLOR_SPACE: &str = "linear-srgb";
pub const DEFAULT_PREVIEW_DISPLAY: &str = "srgb";
pub const DEFAULT_PREVIEW_VIEW: &str = "standard";
pub const DEFAULT_OUTPUT_COLOR_SPACE: &str = "srgb";

/// Stable identity of the color configuration used to interpret space names.
///
/// OpenColorIO registry aliases such as `ocio://default` are intentionally not
/// representable as valid effective configuration. Built-in OCIO configs must
/// use their exact registry URI and record the processor version. External
/// configs are Project assets pinned by their content checksum.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ColorConfigIdentity {
    /// A versioned config shipped as part of RuViE itself.
    Bundled { id: String },
    /// An exact entry from OpenColorIO's built-in config registry.
    OcioBuiltin { uri: String, ocio_version: String },
    /// An external `.ocio` config stored as a Project asset.
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
pub struct PreviewColorConfig {
    #[serde(default = "default_preview_display")]
    display: String,
    #[serde(default = "default_preview_view")]
    view: String,
}

impl PreviewColorConfig {
    pub fn new(display: impl Into<String>, view: impl Into<String>) -> Self {
        Self {
            display: display.into(),
            view: view.into(),
        }
    }

    pub fn display(&self) -> &str {
        &self.display
    }

    pub fn view(&self) -> &str {
        &self.view
    }
}

impl Default for PreviewColorConfig {
    fn default() -> Self {
        Self::new(DEFAULT_PREVIEW_DISPLAY, DEFAULT_PREVIEW_VIEW)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
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

    pub fn diagnostics(&self, assets: &[Asset]) -> Vec<ColorManagementIssue> {
        let mut diagnostics = Vec::new();
        validate_config_identity(&self.config, assets, &mut diagnostics);
        validate_named_field(
            &self.working_space,
            ColorManagementField::WorkingSpace,
            &mut diagnostics,
        );
        validate_named_field(
            &self.preview.display,
            ColorManagementField::PreviewDisplay,
            &mut diagnostics,
        );
        validate_named_field(
            &self.preview.view,
            ColorManagementField::PreviewView,
            &mut diagnostics,
        );
        validate_named_field(
            &self.export.output_space,
            ColorManagementField::OutputSpace,
            &mut diagnostics,
        );
        diagnostics
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

/// A validated runtime-facing color configuration plus diagnostics from the
/// persisted request. Invalid requests resolve as a whole to the deterministic
/// bundled fallback so space names are never interpreted under the wrong
/// config.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ResolvedColorManagementConfig {
    effective: ColorManagementConfig,
    diagnostics: Vec<ColorManagementIssue>,
}

impl ResolvedColorManagementConfig {
    pub fn effective(&self) -> &ColorManagementConfig {
        &self.effective
    }

    pub fn diagnostics(&self) -> &[ColorManagementIssue] {
        &self.diagnostics
    }

    pub fn used_safe_fallback(&self) -> bool {
        !self.diagnostics.is_empty()
    }
}

/// Structured non-fatal diagnostics for persisted color configuration.
///
/// These issues are intentionally separate from Project graph errors: a user
/// must still be able to open and repair a Project whose external color config
/// has moved or whose metadata was hand-edited.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(tag = "code", content = "context", rename_all = "snake_case")]
pub enum ColorManagementIssue {
    BlankIdentifier { field: ColorManagementField },
    MovingConfigIdentifier { identifier: String },
    InvalidBundledConfigId { identifier: String },
    InvalidOcioBuiltinUri { uri: String },
    UnpinnedOcioBuiltinUri { uri: String },
    InvalidOcioVersion { version: String },
    ConfigAssetNotFound { asset_id: Uuid },
    InvalidConfigChecksum { asset_id: Uuid, sha256: String },
}

impl std::fmt::Display for ColorManagementIssue {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::BlankIdentifier { field } => write!(formatter, "{field} must not be blank"),
            Self::MovingConfigIdentifier { identifier } => write!(
                formatter,
                "color config identifier '{identifier}' is a moving alias"
            ),
            Self::InvalidBundledConfigId { identifier } => write!(
                formatter,
                "bundled color config id '{identifier}' is not a versioned RuViE config URI"
            ),
            Self::InvalidOcioBuiltinUri { uri } => {
                write!(
                    formatter,
                    "OpenColorIO built-in config URI '{uri}' is invalid"
                )
            }
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
            Self::InvalidConfigChecksum { asset_id, sha256 } => write!(
                formatter,
                "color config asset {asset_id} has invalid SHA-256 checksum '{sha256}'"
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
    requested: &ColorManagementConfig,
    assets: &[Asset],
) -> ResolvedColorManagementConfig {
    let diagnostics = requested.diagnostics(assets);
    let effective = if diagnostics.is_empty() {
        requested.clone()
    } else {
        ColorManagementConfig::default()
    };
    ResolvedColorManagementConfig {
        effective,
        diagnostics,
    }
}

fn validate_config_identity(
    identity: &ColorConfigIdentity,
    assets: &[Asset],
    diagnostics: &mut Vec<ColorManagementIssue>,
) {
    match identity {
        ColorConfigIdentity::Bundled { id } => {
            validate_named_field(id, ColorManagementField::ConfigIdentifier, diagnostics);
            if is_moving_identifier(id) {
                diagnostics.push(ColorManagementIssue::MovingConfigIdentifier {
                    identifier: id.clone(),
                });
            } else if !id.starts_with("ruvie://color-config/") || !contains_version_token(id) {
                diagnostics.push(ColorManagementIssue::InvalidBundledConfigId {
                    identifier: id.clone(),
                });
            }
        }
        ColorConfigIdentity::OcioBuiltin { uri, ocio_version } => {
            validate_named_field(uri, ColorManagementField::ConfigIdentifier, diagnostics);
            if is_moving_identifier(uri) {
                diagnostics.push(ColorManagementIssue::MovingConfigIdentifier {
                    identifier: uri.clone(),
                });
            } else if !uri.starts_with("ocio://") {
                diagnostics.push(ColorManagementIssue::InvalidOcioBuiltinUri { uri: uri.clone() });
            } else if !contains_ocio_registry_version(uri) {
                diagnostics.push(ColorManagementIssue::UnpinnedOcioBuiltinUri { uri: uri.clone() });
            }
            validate_ocio_version(ocio_version, diagnostics);
        }
        ColorConfigIdentity::ProjectAsset {
            asset_id,
            sha256,
            ocio_version,
        } => {
            if !assets.iter().any(|asset| asset.id == *asset_id) {
                diagnostics.push(ColorManagementIssue::ConfigAssetNotFound {
                    asset_id: *asset_id,
                });
            }
            if !is_sha256(sha256) {
                diagnostics.push(ColorManagementIssue::InvalidConfigChecksum {
                    asset_id: *asset_id,
                    sha256: sha256.clone(),
                });
            }
            validate_ocio_version(ocio_version, diagnostics);
        }
    }
}

fn validate_named_field(
    value: &str,
    field: ColorManagementField,
    diagnostics: &mut Vec<ColorManagementIssue>,
) {
    if value.trim().is_empty() {
        diagnostics.push(ColorManagementIssue::BlankIdentifier { field });
    }
}

fn validate_ocio_version(version: &str, diagnostics: &mut Vec<ColorManagementIssue>) {
    if version.trim().is_empty() {
        diagnostics.push(ColorManagementIssue::BlankIdentifier {
            field: ColorManagementField::OcioVersion,
        });
    } else if !is_pinned_ocio_version(version) {
        diagnostics.push(ColorManagementIssue::InvalidOcioVersion {
            version: version.to_string(),
        });
    }
}

fn is_moving_identifier(identifier: &str) -> bool {
    let normalized = identifier.trim().to_ascii_lowercase();
    normalized
        .split(|character: char| !character.is_ascii_alphanumeric())
        .any(|part| matches!(part, "default" | "latest"))
}

fn contains_ocio_registry_version(uri: &str) -> bool {
    uri.rsplit_once("_ocio-v")
        .is_some_and(|(_, version)| is_version_number(version))
}

fn contains_version_token(identifier: &str) -> bool {
    identifier
        .split(['/', '-', '_'])
        .any(|part| part.strip_prefix('v').is_some_and(is_version_number))
}

fn is_pinned_ocio_version(version: &str) -> bool {
    let version = version.strip_prefix('v').unwrap_or(version);
    version.split('.').count() == 3 && is_version_number(version)
}

fn is_version_number(version: &str) -> bool {
    !version.is_empty()
        && version.split('.').all(|part| {
            !part.is_empty() && part.bytes().all(|character| character.is_ascii_digit())
        })
}

fn is_sha256(checksum: &str) -> bool {
    checksum.len() == 64
        && checksum
            .bytes()
            .all(|character| character.is_ascii_hexdigit())
}

fn default_working_color_space() -> String {
    DEFAULT_WORKING_COLOR_SPACE.to_string()
}

fn default_preview_display() -> String {
    DEFAULT_PREVIEW_DISPLAY.to_string()
}

fn default_preview_view() -> String {
    DEFAULT_PREVIEW_VIEW.to_string()
}

fn default_output_color_space() -> String {
    DEFAULT_OUTPUT_COLOR_SPACE.to_string()
}
