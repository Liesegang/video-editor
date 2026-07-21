use std::path::{Path, PathBuf};
use std::sync::Arc;

use ruvie_plugin_api::{
    EFFECT_CATEGORY, LOADER_CATEGORY, PluginDescriptorV1, RuvieEffectCpuRgba8ApiV1,
    RuvieLoaderCpuRgba8ApiV1,
};
use serde::Deserialize;

use super::abi::RuntimeLibrary;
use super::descriptor::validate_descriptor;
use super::{BUNDLE_MANIFEST_NAME, MAX_MANIFEST_BYTES};
use crate::error::LibraryError;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct BundleManifest {
    manifest_version: u32,
    library: PlatformLibraries,
}

#[derive(Debug, Deserialize)]
#[allow(
    dead_code,
    reason = "all platform manifest keys are deserialized, while one is selected per host build"
)]
#[serde(deny_unknown_fields)]
struct PlatformLibraries {
    macos: Option<String>,
    linux: Option<String>,
    windows: Option<String>,
}

impl PlatformLibraries {
    fn current(&self) -> Option<&str> {
        #[cfg(target_os = "macos")]
        {
            self.macos.as_deref()
        }
        #[cfg(target_os = "linux")]
        {
            self.linux.as_deref()
        }
        #[cfg(target_os = "windows")]
        {
            self.windows.as_deref()
        }
        #[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
        {
            None
        }
    }
}

pub(crate) fn discover_manifests(path: &Path) -> Result<Vec<PathBuf>, LibraryError> {
    if !path.exists() {
        return Ok(Vec::new());
    }
    if path.is_file() {
        if path.file_name().and_then(|name| name.to_str()) == Some(BUNDLE_MANIFEST_NAME) {
            return Ok(vec![path.to_path_buf()]);
        }
        return Err(LibraryError::Plugin(format!(
            "Runtime plugin path {} is a file but not {BUNDLE_MANIFEST_NAME}",
            path.display()
        )));
    }

    let direct = path.join(BUNDLE_MANIFEST_NAME);
    if direct.is_file() {
        return Ok(vec![direct]);
    }

    let mut manifests = Vec::new();
    for entry in std::fs::read_dir(path)? {
        let entry = entry?;
        if entry.file_type()?.is_dir() {
            let manifest = entry.path().join(BUNDLE_MANIFEST_NAME);
            if manifest.is_file() {
                manifests.push(manifest);
            }
        }
    }
    manifests.sort();
    Ok(manifests)
}

pub(crate) struct PendingBundle {
    pub(super) manifest_path: PathBuf,
    pub(super) library_path: PathBuf,
    pub(super) descriptor: PluginDescriptorV1,
    pub(super) library: Arc<RuntimeLibrary>,
    pub(super) effect_api: Option<RuvieEffectCpuRgba8ApiV1>,
    pub(super) loader_api: Option<RuvieLoaderCpuRgba8ApiV1>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ResolvedBundle {
    pub manifest_path: PathBuf,
    pub library_path: PathBuf,
}

/// Resolves only the stable manifest identity. This deliberately happens
/// before reading the manifest or touching its library so an already-loaded
/// bundle remains idempotent even if its installed files are later damaged.
pub(crate) fn resolve_manifest_identity(manifest_path: &Path) -> Result<PathBuf, LibraryError> {
    let manifest_path = manifest_path.canonicalize()?;
    if manifest_path.file_name().and_then(|name| name.to_str()) != Some(BUNDLE_MANIFEST_NAME) {
        return Err(LibraryError::Plugin(format!(
            "Runtime plugin manifest must be named {BUNDLE_MANIFEST_NAME}: {}",
            manifest_path.display()
        )));
    }
    Ok(manifest_path)
}

/// Parses a manifest and resolves its platform library without loading native
/// code or invoking any plugin callback.
pub(crate) fn resolve_bundle(manifest_path: &Path) -> Result<ResolvedBundle, LibraryError> {
    let metadata = std::fs::metadata(manifest_path)?;
    if metadata.len() > MAX_MANIFEST_BYTES {
        return Err(LibraryError::Plugin(format!(
            "Runtime plugin manifest {} exceeds {} bytes",
            manifest_path.display(),
            MAX_MANIFEST_BYTES
        )));
    }
    let manifest_text = std::fs::read_to_string(manifest_path)?;
    let manifest: BundleManifest = toml::from_str(&manifest_text).map_err(|error| {
        LibraryError::Plugin(format!(
            "Invalid runtime plugin manifest {}: {error}",
            manifest_path.display()
        ))
    })?;
    if manifest.manifest_version != 1 {
        return Err(LibraryError::Plugin(format!(
            "Unsupported runtime bundle manifest version {} in {} (host supports 1)",
            manifest.manifest_version,
            manifest_path.display()
        )));
    }
    let library_name = manifest.library.current().ok_or_else(|| {
        LibraryError::Plugin(format!(
            "Runtime plugin manifest {} has no library for this operating system",
            manifest_path.display()
        ))
    })?;
    let bundle_dir = manifest_path.parent().ok_or_else(|| {
        LibraryError::Plugin(format!(
            "Runtime plugin manifest {} has no bundle directory",
            manifest_path.display()
        ))
    })?;
    let library_path = bundle_dir.join(library_name).canonicalize()?;
    if !library_path.starts_with(bundle_dir) {
        return Err(LibraryError::Plugin(format!(
            "Runtime plugin library {} escapes bundle directory {}",
            library_path.display(),
            bundle_dir.display()
        )));
    }
    validate_library_extension(&library_path)?;

    Ok(ResolvedBundle {
        manifest_path: manifest_path.to_path_buf(),
        library_path,
    })
}

/// Loads and validates a bundle only after the manager has claimed its
/// resolved identity. Entry/descriptor callbacks therefore cannot race with a
/// concurrent rescan of the same manifest or library.
pub(crate) fn open_bundle(bundle: &ResolvedBundle) -> Result<PendingBundle, LibraryError> {
    let library = Arc::new(RuntimeLibrary::open(&bundle.library_path)?);
    let descriptor: PluginDescriptorV1 = library.descriptor()?;
    validate_descriptor(&descriptor)?;
    let effect_api = descriptor
        .components
        .iter()
        .any(|component| component.category == EFFECT_CATEGORY)
        .then(|| library.effect_cpu_rgba8_extension())
        .transpose()?;
    let loader_api = descriptor
        .components
        .iter()
        .any(|component| component.category == LOADER_CATEGORY)
        .then(|| library.loader_cpu_rgba8_extension())
        .transpose()?;
    Ok(PendingBundle {
        manifest_path: bundle.manifest_path.clone(),
        library_path: bundle.library_path.clone(),
        descriptor,
        library,
        effect_api,
        loader_api,
    })
}

fn validate_library_extension(path: &Path) -> Result<(), LibraryError> {
    let extension = path.extension().and_then(|value| value.to_str());
    #[cfg(target_os = "macos")]
    let valid = extension == Some("dylib");
    #[cfg(target_os = "linux")]
    let valid = extension == Some("so");
    #[cfg(target_os = "windows")]
    let valid = extension == Some("dll");
    #[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
    let valid = false;
    if valid {
        Ok(())
    } else {
        Err(LibraryError::Plugin(format!(
            "Runtime plugin library {} has the wrong extension for this platform",
            path.display()
        )))
    }
}
