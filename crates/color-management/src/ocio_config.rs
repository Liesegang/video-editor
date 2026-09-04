//! Exact OpenColorIO config resources and deterministic runtime contexts.

use std::{path::Path, sync::Arc};

use ocio_rs::{
    ColorSpaceDirection, Config, Context, EnvironmentMode, TransformDirection,
    ViewTransformDirection, transform::Transform,
};
use sha2::{Digest, Sha256};

use crate::{ColorContext, ExactColorConfigFile, ocio_error::OcioBackendError};

use crate::ocio_error::map_ocio;

const BUILTIN_URI_PREFIX: &str = "ocio://";

/// Immutable locator and portable identity for one exact OCIO configuration.
///
/// A path is only a local locator. Its identity is the verified config-file
/// checksum, so moving the same resource snapshot between machines does not
/// change Project or processor identity.
#[derive(Clone, Debug)]
pub(super) enum OcioConfigSource {
    Bytes { bytes: Arc<[u8]>, sha256: String },
    File { snapshot: ExactColorConfigFile },
    BuiltinRegistry { uri: String, registry_name: String },
}

impl OcioConfigSource {
    pub(super) fn from_exact_bytes(bytes: &[u8]) -> Result<Self, OcioBackendError> {
        ensure_real_runtime()?;
        if bytes.is_empty() {
            return Err(OcioBackendError::EmptyConfigBytes);
        }
        Ok(Self::Bytes {
            sha256: sha256_bytes(bytes),
            bytes: Arc::from(bytes),
        })
    }

    pub(super) fn from_exact_path(
        path: &Path,
        expected_sha256: &str,
    ) -> Result<Self, OcioBackendError> {
        ensure_real_runtime()?;
        let snapshot = ExactColorConfigFile::read(path)?;
        snapshot.verify_sha256(expected_sha256)?;
        Ok(Self::File { snapshot })
    }

    pub(super) fn from_builtin_registry_uri(uri: String) -> Result<Self, OcioBackendError> {
        ensure_real_runtime()?;
        let registry_name = exact_registry_name(&uri)?;
        Ok(Self::BuiltinRegistry { uri, registry_name })
    }

    pub(super) fn exact_identity(&self) -> String {
        match self {
            Self::Bytes { sha256, .. } => format!("bytes:sha256:{sha256}"),
            Self::File { snapshot } => {
                format!("path-config:sha256:{}", snapshot.sha256())
            }
            Self::BuiltinRegistry { uri, .. } => format!("builtin:{uri}"),
        }
    }

    pub(super) fn load_and_validate(&self) -> Result<Config, OcioBackendError> {
        ensure_real_runtime()?;
        let config = match self {
            Self::Bytes { bytes, .. } => {
                let text = std::str::from_utf8(bytes)
                    .map_err(|error| OcioBackendError::InvalidConfigUtf8(error.to_string()))?;
                map_ocio("load config from exact bytes", Config::from_stream(text))?
            }
            Self::File { snapshot } => {
                let text = std::str::from_utf8(snapshot.bytes())
                    .map_err(|error| OcioBackendError::InvalidConfigUtf8(error.to_string()))?;
                // Do not ask OCIO to reopen the path after checksum validation:
                // a concurrent replacement could otherwise make runtime state
                // disagree with the persisted SHA-256 identity. External file
                // resolution is rejected below, so stream loading loses no
                // supported path-relative behavior.
                map_ocio(
                    "load config from verified path bytes",
                    Config::from_stream(text),
                )?
            }
            Self::BuiltinRegistry { registry_name, .. } => {
                let registry = map_ocio(
                    "open built-in config registry",
                    ocio_rs::BuiltinConfigRegistry::get(),
                )?;
                if !registry_contains_exact_name(&registry, registry_name)? {
                    return Err(OcioBackendError::BuiltinConfigUnavailable {
                        registry_name: registry_name.clone(),
                    });
                }
                map_ocio(
                    "load exact built-in config",
                    registry.try_config_by_name(registry_name),
                )?
                .ok_or_else(|| OcioBackendError::BuiltinConfigUnavailable {
                    registry_name: registry_name.clone(),
                })?
            }
        };
        ensure_self_contained(&config)?;
        map_ocio("validate config", config.validate())?;
        Ok(config)
    }
}

/// Reject configs whose result may depend on files outside the exact config
/// resource. `ocio-rs` 0.2.1 can supply LUT bytes through `ConfigIOProxy`, but
/// it cannot enumerate the complete context-specialized file dependency
/// closure needed to build and verify such a proxy. Hashing an arbitrary
/// directory would both miss dependencies outside it and include unrelated
/// files, so path and byte configs remain deliberately self-contained.
fn ensure_self_contained(config: &Config) -> Result<(), OcioBackendError> {
    let mut sources = Vec::new();
    inspect_color_spaces(config, &mut sources)?;
    inspect_looks(config, &mut sources)?;
    inspect_named_transforms(config, &mut sources)?;
    inspect_view_transforms(config, &mut sources)?;

    let serialized = map_ocio("serialize config for resource audit", config.serialize())?
        .ok_or_else(|| OcioBackendError::TransformInspectionIncomplete {
            location: "serialized config".to_string(),
        })?;
    if serialized.contains("!<FileTransform>") && sources.is_empty() {
        // The v0.2.1 collection APIs do not expose inactive color-space
        // enumeration. Native serialization is therefore the completeness
        // guard for transforms the typed collection walk cannot reach.
        sources.push("<FileTransform hidden from ocio-rs 0.2.1 enumeration>".to_string());
    }

    sources.sort();
    sources.dedup();
    if sources.is_empty() {
        Ok(())
    } else {
        Err(OcioBackendError::ExternalFileTransformsUnsupported { sources })
    }
}

fn inspect_color_spaces(
    config: &Config,
    sources: &mut Vec<String>,
) -> Result<(), OcioBackendError> {
    for index in checked_count("color spaces", config.num_color_spaces())? {
        let name = required_collection_name(
            "color space",
            index,
            map_ocio(
                "read color-space name for resource audit",
                config.try_color_space_name_by_index(index),
            )?,
        )?;
        let color_space = map_ocio(
            "read color space for resource audit",
            config.try_color_space(&name),
        )?
        .ok_or_else(|| OcioBackendError::TransformInspectionIncomplete {
            location: format!("color space '{name}'"),
        })?;
        for direction in [
            ColorSpaceDirection::ToReference,
            ColorSpaceDirection::FromReference,
        ] {
            let transform = map_ocio(
                "read color-space transform for resource audit",
                color_space.try_transform(direction),
            )?;
            inspect_transform(transform, &format!("color space '{name}'"), sources)?;
        }
    }
    Ok(())
}

fn inspect_looks(config: &Config, sources: &mut Vec<String>) -> Result<(), OcioBackendError> {
    for index in checked_count("looks", config.num_looks())? {
        let name = required_collection_name(
            "look",
            index,
            map_ocio(
                "read look name for resource audit",
                config.try_look_name_by_index(index),
            )?,
        )?;
        let look =
            map_ocio("read look for resource audit", config.try_look(&name))?.ok_or_else(|| {
                OcioBackendError::TransformInspectionIncomplete {
                    location: format!("look '{name}'"),
                }
            })?;
        inspect_transform(
            map_ocio(
                "read look transform for resource audit",
                look.try_transform(),
            )?,
            &format!("look '{name}'"),
            sources,
        )?;
        inspect_transform(
            map_ocio(
                "read inverse look transform for resource audit",
                look.try_inverse_transform(),
            )?,
            &format!("inverse look '{name}'"),
            sources,
        )?;
    }
    Ok(())
}

fn inspect_named_transforms(
    config: &Config,
    sources: &mut Vec<String>,
) -> Result<(), OcioBackendError> {
    for index in checked_count("named transforms", config.num_named_transforms())? {
        let name = required_collection_name(
            "named transform",
            index,
            config.named_transform_name_by_index(index),
        )?;
        let named = map_ocio(
            "read named transform for resource audit",
            config.try_named_transform(&name),
        )?
        .ok_or_else(|| OcioBackendError::TransformInspectionIncomplete {
            location: format!("named transform '{name}'"),
        })?;
        for direction in [TransformDirection::Forward, TransformDirection::Inverse] {
            inspect_transform(
                map_ocio(
                    "read named transform for resource audit",
                    named.try_transform(direction),
                )?,
                &format!("named transform '{name}'"),
                sources,
            )?;
        }
    }
    Ok(())
}

fn inspect_view_transforms(
    config: &Config,
    sources: &mut Vec<String>,
) -> Result<(), OcioBackendError> {
    for index in checked_count("view transforms", config.num_view_transforms())? {
        let name = required_collection_name(
            "view transform",
            index,
            config.view_transform_name_by_index(index),
        )?;
        let view = map_ocio(
            "read view transform for resource audit",
            config.try_view_transform(&name),
        )?
        .ok_or_else(|| OcioBackendError::TransformInspectionIncomplete {
            location: format!("view transform '{name}'"),
        })?;
        for direction in [
            ViewTransformDirection::ToReference,
            ViewTransformDirection::FromReference,
        ] {
            inspect_transform(
                map_ocio(
                    "read view transform for resource audit",
                    view.try_transform(direction),
                )?,
                &format!("view transform '{name}'"),
                sources,
            )?;
        }
    }
    Ok(())
}

fn inspect_transform(
    root: Option<Transform>,
    location: &str,
    sources: &mut Vec<String>,
) -> Result<(), OcioBackendError> {
    let mut pending = root.into_iter().collect::<Vec<_>>();
    while let Some(transform) = pending.pop() {
        match transform {
            Transform::File(file) => sources.push(
                file.src()
                    .filter(|source| !source.trim().is_empty())
                    .unwrap_or_else(|| "<empty FileTransform source>".to_string()),
            ),
            Transform::Group(group) => {
                for index in checked_count("group children", group.num_transforms())? {
                    let child = map_ocio(
                        "read group child for resource audit",
                        group.try_transform(index),
                    )?
                    .ok_or_else(|| {
                        OcioBackendError::TransformInspectionIncomplete {
                            location: format!("{location}, group child {index}"),
                        }
                    })?;
                    pending.push(child);
                }
            }
            _ => {}
        }
    }
    Ok(())
}

fn checked_count(
    collection: &'static str,
    count: i32,
) -> Result<std::ops::Range<i32>, OcioBackendError> {
    if count < 0 {
        Err(OcioBackendError::TransformInspectionIncomplete {
            location: format!("negative {collection} count ({count})"),
        })
    } else {
        Ok(0..count)
    }
}

fn required_collection_name(
    collection: &'static str,
    index: i32,
    name: Option<String>,
) -> Result<String, OcioBackendError> {
    name.filter(|name| !name.trim().is_empty()).ok_or_else(|| {
        OcioBackendError::TransformInspectionIncomplete {
            location: format!("{collection} index {index}"),
        }
    })
}

pub(super) fn ensure_runtime_version(expected: &str) -> Result<String, OcioBackendError> {
    ensure_real_runtime()?;
    let actual = ocio_rs::version().ok_or(OcioBackendError::RuntimeVersionUnavailable)?;
    if actual == expected {
        Ok(actual)
    } else {
        Err(OcioBackendError::RuntimeVersionMismatch {
            expected: expected.to_string(),
            actual,
        })
    }
}

/// Builds a processor context from config-authored defaults plus explicit
/// Project variables. Values inherited from the process environment are
/// cleared, while search paths, working directory, and IO proxy state remain
/// on the editable context copy.
pub(super) fn deterministic_context(
    config: &Config,
    requested: &ColorContext,
) -> Result<Context, OcioBackendError> {
    map_ocio(
        "set deterministic config environment mode",
        config.set_environment_mode(EnvironmentMode::LoadPredefined),
    )?;
    let context = current_context(config)?;
    let context = map_ocio("copy config context", context.create_editable_copy())?;
    map_ocio(
        "clear inherited context variables",
        context.try_clear_string_vars(),
    )?;
    map_ocio(
        "set deterministic context environment mode",
        context.set_environment_mode(EnvironmentMode::LoadPredefined),
    )?;

    let count =
        usize::try_from(config.num_environment_vars()).map_err(|_| OcioBackendError::Ocio {
            operation: "enumerate config variables",
            detail: "OCIO returned a negative environment-variable count".to_string(),
        })?;
    for index in 0..count {
        let index = i32::try_from(index).map_err(|_| OcioBackendError::Ocio {
            operation: "enumerate config variables",
            detail: "environment-variable index exceeds OCIO's i32 range".to_string(),
        })?;
        let name =
            config
                .environment_var_name_by_index(index)
                .ok_or_else(|| OcioBackendError::Ocio {
                    operation: "read config variable name",
                    detail: format!("OCIO returned no variable name for index {index}"),
                })?;
        let value = requested
            .variables()
            .get(&name)
            .cloned()
            .or_else(|| config.environment_var_default(&name))
            .ok_or_else(|| OcioBackendError::MissingContextVariable { name: name.clone() })?;
        map_ocio(
            "set declared context variable",
            context.set_string_var(&name, value),
        )?;
    }
    for (name, value) in requested.variables() {
        map_ocio(
            "set Project context variable",
            context.set_string_var(name, value),
        )?;
    }
    Ok(context)
}

fn current_context(config: &Config) -> Result<Context, OcioBackendError> {
    map_ocio("read config context", config.try_current_context())?.ok_or(
        OcioBackendError::MissingRuntimeIdentity {
            identity: "config context",
        },
    )
}

fn registry_contains_exact_name(
    registry: &ocio_rs::BuiltinConfigRegistry,
    expected: &str,
) -> Result<bool, OcioBackendError> {
    let count =
        usize::try_from(registry.num_builtin_configs()).map_err(|_| OcioBackendError::Ocio {
            operation: "enumerate built-in configs",
            detail: "OCIO returned a negative built-in config count".to_string(),
        })?;
    for index in 0..count {
        let index = i32::try_from(index).map_err(|_| OcioBackendError::Ocio {
            operation: "enumerate built-in configs",
            detail: "built-in config index exceeds OCIO's i32 range".to_string(),
        })?;
        if map_ocio("read built-in config name", registry.try_config_name(index))?
            .is_some_and(|name| name == expected)
        {
            return Ok(true);
        }
    }
    Ok(false)
}

fn exact_registry_name(uri: &str) -> Result<String, OcioBackendError> {
    let Some(name) = uri.strip_prefix(BUILTIN_URI_PREFIX) else {
        return Err(OcioBackendError::InvalidBuiltinUri(uri.to_string()));
    };
    let normalized = name.trim().trim_end_matches('/').to_ascii_lowercase();
    if name.trim() != name
        || name.is_empty()
        || name.contains('/')
        || matches!(normalized.as_str(), "default" | "latest")
    {
        return Err(OcioBackendError::InvalidBuiltinUri(uri.to_string()));
    }
    Ok(name.to_string())
}

fn ensure_real_runtime() -> Result<(), OcioBackendError> {
    if ocio_rs::is_stub_build() {
        Err(OcioBackendError::StubBuild)
    } else {
        Ok(())
    }
}

fn sha256_bytes(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

#[cfg(test)]
mod tests {
    use std::error::Error;

    use super::{
        OcioConfigSource, deterministic_context, exact_registry_name, required_collection_name,
    };
    use crate::{ColorContext, OcioBackendError};

    const RAW_CONFIG: &[u8] = b"ocio_profile_version: 2\nroles:\n  default: raw\ncolorspaces:\n  - !<ColorSpace> {name: raw, isdata: true}\n";
    const EXTERNAL_FILE_CONFIG: &[u8] = b"ocio_profile_version: 2\nroles:\n  default: raw\ncolorspaces:\n  - !<ColorSpace> {name: raw, isdata: true}\n  - !<ColorSpace>\n    name: external\n    to_scene_reference: !<FileTransform> {src: grades/show-look.cube}\n";

    #[test]
    fn stub_build_is_rejected_instead_of_becoming_a_no_op_backend() {
        if ocio_rs::is_stub_build() {
            assert!(matches!(
                OcioConfigSource::from_exact_bytes(RAW_CONFIG),
                Err(OcioBackendError::StubBuild)
            ));
        }
    }

    #[test]
    fn built_in_uri_must_be_an_exact_non_alias_identity() {
        assert!(exact_registry_name("default").is_err());
        assert!(exact_registry_name("ocio://default").is_err());
        assert!(exact_registry_name("ocio://latest").is_err());
        assert!(exact_registry_name("ocio://show/config").is_err());
        assert_eq!(
            exact_registry_name("ocio://studio-config-v4.0.0_aces-v2.0_ocio-v2.5").as_deref(),
            Ok("studio-config-v4.0.0_aces-v2.0_ocio-v2.5")
        );
    }

    #[test]
    fn path_identity_is_resource_based_instead_of_machine_location_based() {
        let directory = tempfile::tempdir().expect("temp directory");
        let first_path = directory.path().join("machine-a.ocio");
        let second_path = directory.path().join("machine-b.ocio");
        std::fs::write(&first_path, RAW_CONFIG).expect("write first config");
        std::fs::write(&second_path, RAW_CONFIG).expect("write second config");
        let first = OcioConfigSource::File {
            snapshot: crate::ExactColorConfigFile::read(&first_path)
                .expect("read first exact config"),
        };
        let second = OcioConfigSource::File {
            snapshot: crate::ExactColorConfigFile::read(&second_path)
                .expect("read second exact config"),
        };

        assert_eq!(first.exact_identity(), second.exact_identity());
    }

    #[test]
    fn missing_collection_names_fail_closed() {
        assert!(matches!(
            required_collection_name("look", 2, None),
            Err(OcioBackendError::TransformInspectionIncomplete { .. })
        ));
        assert!(matches!(
            required_collection_name("look", 2, Some("  ".to_string())),
            Err(OcioBackendError::TransformInspectionIncomplete { .. })
        ));
    }

    #[test]
    fn real_external_file_transform_is_rejected_when_available() -> Result<(), Box<dyn Error>> {
        if ocio_rs::is_stub_build() {
            eprintln!("skipped: ocio-rs is a stub build");
            return Ok(());
        }
        let source = OcioConfigSource::from_exact_bytes(EXTERNAL_FILE_CONFIG)?;
        let error = match source.load_and_validate() {
            Ok(_) => panic!("unbundled FileTransform must fail closed"),
            Err(error) => error,
        };
        assert_eq!(
            error,
            OcioBackendError::ExternalFileTransformsUnsupported {
                sources: vec!["grades/show-look.cube".to_string()]
            }
        );
        Ok(())
    }

    #[test]
    fn real_context_uses_config_defaults_and_project_overrides_when_available()
    -> Result<(), Box<dyn Error>> {
        if ocio_rs::is_stub_build() {
            eprintln!("skipped: ocio-rs is a stub build");
            return Ok(());
        }
        let config = ocio_rs::Config::raw()?;
        config.add_environment_var("SHOT", "config-default")?;
        let inherited = config
            .try_current_context()?
            .ok_or("config context unavailable")?;
        inherited.set_string_var("PROCESS_ONLY", "must-not-leak")?;

        let defaults = deterministic_context(&config, &ColorContext::default())?;
        assert_eq!(
            defaults.string_var("SHOT").as_deref(),
            Some("config-default")
        );
        // Real OCIO represents an unknown variable with its non-null empty
        // string ABI result; `None` is reserved for an unavailable C string
        // (including the ocio-rs stub).
        assert_eq!(defaults.string_var("PROCESS_ONLY").as_deref(), Some(""));

        let requested = ColorContext::from_variables([("SHOT", "project-value")]);
        let overridden = deterministic_context(&config, &requested)?;
        assert_eq!(
            overridden.string_var("SHOT").as_deref(),
            Some("project-value")
        );
        assert_eq!(overridden.string_var("PROCESS_ONLY").as_deref(), Some(""));
        Ok(())
    }
}
