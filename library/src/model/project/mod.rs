//! Shared value types used by the Timeline-first authoring model.
//!
//! This namespace intentionally contains no editable Project, Composition,
//! Track, Clip, or Structural Merge graph. `AuthoringProject` is the sole
//! persisted editing model.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

pub mod asset;
pub(crate) mod color_management;
pub mod connection;
pub mod property;

pub use color_management::{
    ColorConfigIdentity, ColorManagementConfig, ColorManagementField, ColorManagementIssue,
    ColorManagementStructureIssue, DEFAULT_BUNDLED_COLOR_CONFIG_ID, DEFAULT_OUTPUT_COLOR_SPACE,
    DEFAULT_PREVIEW_DISPLAY, DEFAULT_PREVIEW_SURFACE_ENCODING, DEFAULT_PREVIEW_VIEW,
    DEFAULT_WORKING_COLOR_SPACE, ExportColorConfig, HdrColorField, HdrColorSettings,
    HdrColorSettingsError, LEGACY_BUNDLED_COLOR_CONFIG_V1_ID, ModelValidatedColorManagementConfig,
    PqLinearizationPolicy, PreviewColorConfig, PreviewSurfaceEncoding,
    RequestedColorManagementConfig, ResolvedColorManagementConfig, SrgbSurfaceColorSpaceBinding,
};
pub use connection::*;

#[derive(Serialize, Deserialize, Clone, Default, PartialEq, Debug)]
#[serde(deny_unknown_fields)]
pub struct ExportConfig {
    #[serde(default)]
    pub container: Option<String>,
    #[serde(default)]
    pub codec: Option<String>,
    #[serde(default)]
    pub pixel_format: Option<String>,
    #[serde(default)]
    pub width: Option<u64>,
    #[serde(default)]
    pub height: Option<u64>,
    #[serde(default)]
    pub fps: Option<f64>,
    #[serde(default)]
    pub video_bitrate: Option<u64>,
    #[serde(default)]
    pub audio_codec: Option<String>,
    #[serde(default)]
    pub audio_bitrate: Option<u64>,
    #[serde(default)]
    pub audio_channels: Option<u16>,
    #[serde(default)]
    pub audio_sample_rate: Option<u32>,
    #[serde(default)]
    pub crf: Option<u8>,
    #[serde(default)]
    pub preset: Option<String>,
    #[serde(default)]
    pub ffmpeg_path: Option<String>,
    #[serde(default)]
    pub parameters: HashMap<String, Value>,
}
