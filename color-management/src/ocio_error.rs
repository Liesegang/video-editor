use std::fmt;

use crate::ExactColorConfigFileError;

/// Failure before or around the OpenColorIO backend boundary.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum OcioBackendError {
    ExactConfigFile(ExactColorConfigFileError),
    StubBuild,
    EmptyConfigBytes,
    InvalidConfigUtf8(String),
    InvalidBuiltinUri(String),
    BuiltinConfigUnavailable {
        registry_name: String,
    },
    ExternalFileTransformsUnsupported {
        sources: Vec<String>,
    },
    TransformInspectionIncomplete {
        location: String,
    },
    RuntimeVersionUnavailable,
    RuntimeVersionMismatch {
        expected: String,
        actual: String,
    },
    Ocio {
        operation: &'static str,
        detail: String,
    },
    MissingRuntimeIdentity {
        identity: &'static str,
    },
    MissingContextVariable {
        name: String,
    },
    ProcessorLockPoisoned,
    PixelCountOverflow,
}

impl fmt::Display for OcioBackendError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ExactConfigFile(error) => error.fmt(formatter),
            Self::StubBuild => formatter.write_str(
                "ocio-rs was built in stub mode; a real OpenColorIO runtime is required",
            ),
            Self::EmptyConfigBytes => formatter.write_str("OpenColorIO config bytes are empty"),
            Self::InvalidConfigUtf8(detail) => {
                write!(formatter, "OpenColorIO config is not UTF-8: {detail}")
            }
            Self::InvalidBuiltinUri(uri) => write!(
                formatter,
                "OpenColorIO built-in config URI '{uri}' is not an exact ocio:// registry identity"
            ),
            Self::BuiltinConfigUnavailable { registry_name } => write!(
                formatter,
                "OpenColorIO built-in config '{registry_name}' is unavailable in this runtime"
            ),
            Self::ExternalFileTransformsUnsupported { sources } => write!(
                formatter,
                "OpenColorIO config references external FileTransform resources ({}) whose exact dependency closure is not exposed by ocio-rs 0.2.1; use a self-contained config or an exact built-in registry config",
                sources.join(", ")
            ),
            Self::TransformInspectionIncomplete { location } => write!(
                formatter,
                "OpenColorIO could not completely inspect config transforms at '{location}'; refusing a config whose external resource closure cannot be proven"
            ),
            Self::RuntimeVersionUnavailable => {
                formatter.write_str("OpenColorIO did not report its runtime version")
            }
            Self::RuntimeVersionMismatch { expected, actual } => write!(
                formatter,
                "Project requires OpenColorIO {expected}, but ocio-rs linked {actual}"
            ),
            Self::Ocio { operation, detail } => {
                write!(formatter, "OpenColorIO failed to {operation}: {detail}")
            }
            Self::MissingRuntimeIdentity { identity } => {
                write!(
                    formatter,
                    "OpenColorIO did not provide a non-empty {identity}"
                )
            }
            Self::MissingContextVariable { name } => write!(
                formatter,
                "OpenColorIO config variable '{name}' has neither an authored default nor a Project value"
            ),
            Self::ProcessorLockPoisoned => {
                formatter.write_str("OpenColorIO CPU processor lock is poisoned")
            }
            Self::PixelCountOverflow => {
                formatter.write_str("RGB pixel count exceeds OpenColorIO's i64 API range")
            }
        }
    }
}

impl std::error::Error for OcioBackendError {}

impl From<ExactColorConfigFileError> for OcioBackendError {
    fn from(error: ExactColorConfigFileError) -> Self {
        Self::ExactConfigFile(error)
    }
}

pub(super) fn map_ocio<T>(
    operation: &'static str,
    result: ocio_rs::Result<T>,
) -> Result<T, OcioBackendError> {
    result.map_err(|error| OcioBackendError::Ocio {
        operation,
        detail: error.to_string(),
    })
}
