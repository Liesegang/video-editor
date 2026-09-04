//! Trusted same-toolchain dynamic-library and SkSL plugin loading.

use std::path::Path;
use std::sync::Arc;

use libloading::{Library, Symbol};

use crate::error::LibraryError;
use crate::plugin::EntityConverterPlugin;
use crate::plugin::effects::EffectPlugin;
use crate::plugin::exporters::ExportPlugin;
use crate::plugin::loaders::LoadPlugin;
use crate::plugin::repository::PluginRegistry;
use crate::plugin::traits::Plugin;

use super::PluginManager;

impl PluginManager {
    /// Loads a Rust-ABI plugin constructor and keeps its library loaded.
    ///
    /// # Safety
    ///
    /// `path` must identify a trusted plugin built with the same Rust toolchain
    /// and the exact trait definition represented by `T`. `symbol` must return
    /// a non-null pointer produced by `Box::into_raw(Box<T>)` and transfer its
    /// sole ownership to this function.
    unsafe fn load_plugin_generic<T: ?Sized + 'static>(
        &self,
        path: &Path,
        symbol: &[u8],
        register: impl FnOnce(&mut PluginRegistry, Arc<T>) -> Option<Arc<T>>,
    ) -> Result<(), LibraryError> {
        // SAFETY: The caller guarantees that this is a trusted native plugin;
        // loading it may execute platform-specific initializers.
        let library = unsafe { Library::new(path)? };
        // SAFETY: The caller guarantees the symbol has this exact Rust trait
        // object ABI and was compiled against the same plugin API.
        let constructor: Symbol<unsafe extern "C" fn() -> *mut T> = unsafe { library.get(symbol)? };
        // SAFETY: The constructor contract described above permits one call and
        // transfers ownership of its returned allocation.
        let raw = unsafe { constructor() };
        if raw.is_null() {
            return Err(LibraryError::Plugin(format!(
                "Plugin constructor {} returned null",
                String::from_utf8_lossy(symbol)
            )));
        }
        // SAFETY: The null check and caller contract guarantee `raw` came from
        // Box::into_raw exactly once. Arc takes ownership of the reconstructed Box.
        let plugin = unsafe { Arc::from(Box::from_raw(raw)) };

        let replaced = {
            let mut inner = self.write_registry();
            let replaced = register(&mut inner, plugin);
            inner.dynamic_libraries.push(library);
            replaced
        };
        self.bump_render_revision();
        drop(replaced);
        Ok(())
    }

    pub fn load_effect_plugin_from_file<P: AsRef<Path>>(
        &self,
        path: P,
    ) -> Result<(), LibraryError> {
        // SAFETY: Dynamic plugins are a trusted same-toolchain extension point;
        // load_plugin_generic validates the pointer and retains the library.
        unsafe {
            self.load_plugin_generic::<dyn EffectPlugin>(
                path.as_ref(),
                b"create_effect_plugin",
                |inner, plugin| inner.effect_plugins.register(plugin),
            )
        }
    }

    pub fn load_load_plugin_from_file<P: AsRef<Path>>(&self, path: P) -> Result<(), LibraryError> {
        // SAFETY: Dynamic plugins are a trusted same-toolchain extension point;
        // load_plugin_generic validates the pointer and retains the library.
        unsafe {
            self.load_plugin_generic::<dyn LoadPlugin>(
                path.as_ref(),
                b"create_load_plugin",
                |inner, plugin| inner.load_plugins.register(plugin),
            )
        }
    }

    pub fn load_export_plugin_from_file<P: AsRef<Path>>(
        &self,
        path: P,
    ) -> Result<(), LibraryError> {
        // The v2 symbol is intentionally different from the former bare-Image
        // ABI. An old exporter must fail to load instead of receiving typed
        // ExportFrame bytes under an incompatible Rust trait-object vtable.
        // SAFETY: Dynamic plugins are a trusted same-toolchain extension point;
        // load_plugin_generic validates the pointer and retains the library.
        unsafe {
            self.load_plugin_generic::<dyn ExportPlugin>(
                path.as_ref(),
                b"create_export_plugin_v2",
                |inner, plugin| inner.export_plugins.register(plugin),
            )
        }
    }

    pub fn load_entity_converter_plugin_from_file<P: AsRef<Path>>(
        &self,
        path: P,
    ) -> Result<(), LibraryError> {
        // SAFETY: Dynamic plugins are a trusted same-toolchain extension point;
        // load_plugin_generic validates the pointer and retains the library.
        unsafe {
            self.load_plugin_generic::<dyn EntityConverterPlugin>(
                path.as_ref(),
                b"create_entity_converter_plugin",
                |inner, plugin| inner.entity_converter_plugins.register(plugin),
            )
        }
    }

    pub fn load_plugins_from_directory<P: AsRef<Path>>(
        &self,
        dir_path: P,
    ) -> Result<(), LibraryError> {
        let dir = dir_path.as_ref();
        if !dir.is_dir() {
            log::warn!("Plugin directory not found: {}", dir.display());
            return Ok(());
        }

        for entry in std::fs::read_dir(dir)? {
            let entry = entry?;
            let path = entry.path();
            if path.is_file() {
                let extension = path.extension().and_then(|s| s.to_str());
                if matches!(extension, Some("dll") | Some("so")) {
                    log::info!("Attempting to load plugin from: {}", path.display());
                    if let Err(e) = self.load_effect_plugin_from_file(&path) {
                        log::debug!("Not an effect plugin: {}", e);
                    } else {
                        continue;
                    }
                    if let Err(e) = self.load_load_plugin_from_file(&path) {
                        log::debug!("Not a load plugin: {}", e);
                    } else {
                        continue;
                    }
                    if let Err(e) = self.load_export_plugin_from_file(&path) {
                        log::debug!("Not an export plugin: {}", e);
                    } else {
                        continue;
                    }
                    if let Err(e) = self.load_entity_converter_plugin_from_file(&path) {
                        log::debug!("Not an entity converter plugin: {}", e);
                    } else {
                        continue;
                    }

                    log::warn!("File is not a recognized plugin type: {}", path.display());
                }
            }
        }
        Ok(())
    }

    pub fn load_sksl_plugins_from_directory<P: AsRef<Path>>(
        &self,
        dir_path: P,
    ) -> Result<(), LibraryError> {
        let dir = dir_path.as_ref();
        if !dir.exists() {
            log::warn!("SkSL plugin directory not found: {}", dir.display());
            return Ok(());
        }

        for entry in std::fs::read_dir(dir)? {
            let entry = entry?;
            let path = entry.path();
            if path.is_dir() {
                let config_path = path.join("config.toml");
                let shader_path = path.join("shader.sksl");

                if config_path.exists() && shader_path.exists() {
                    log::info!("Loading SkSL plugin from: {}", path.display());
                    let toml_content =
                        std::fs::read_to_string(&config_path).map_err(LibraryError::Io)?;
                    let sksl_content =
                        std::fs::read_to_string(&shader_path).map_err(LibraryError::Io)?;

                    match crate::plugin::effects::SkslEffectPlugin::new(
                        &toml_content,
                        &sksl_content,
                    ) {
                        Ok(plugin) => {
                            log::info!("Successfully registered SkSL plugin: {}", plugin.id());
                            self.register_effect(Arc::new(plugin));
                        }
                        Err(e) => {
                            log::error!("Failed to load SkSL plugin at {}: {}", path.display(), e);
                        }
                    }
                } else {
                    log::warn!(
                        "Skipping directory {}, missing config.toml or shader.sksl",
                        path.display()
                    );
                }
            }
        }
        Ok(())
    }
}
