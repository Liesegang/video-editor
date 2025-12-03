use std::collections::HashMap;
use std::error::Error;
use std::sync::Arc;

use crate::cache::{CacheManager, SharedCacheManager};
use crate::loader::image::Image;
use crate::model::project::entity::Entity;
use crate::model::project::property::PropertyValue;

mod exporters;
mod loaders;

use exporters::PngExportPlugin;
use loaders::{FfmpegVideoLoader, NativeImageLoader};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PluginCategory {
  Effect,
  Load,
  Export,
}

pub trait Plugin: Send + Sync {
  fn id(&self) -> &'static str;
  fn category(&self) -> PluginCategory;
}

pub trait EffectPlugin: Plugin {
  fn create(&self, params: HashMap<String, PropertyValue>) -> Entity;
}

#[derive(Debug, Clone)]
pub enum LoadRequest {
  Image { path: String },
  VideoFrame { path: String, frame_number: u64 },
}

pub enum LoadResponse {
  Image(Image),
}

pub trait LoadPlugin: Plugin {
  fn supports(&self, request: &LoadRequest) -> bool;
  fn load(
    &self,
    request: &LoadRequest,
    cache: &CacheManager,
  ) -> Result<LoadResponse, Box<dyn Error>>;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExportFormat {
  Png,
}

pub trait ExportPlugin: Plugin {
  fn supports(&self, format: ExportFormat) -> bool;
  fn export_image(
    &self,
    format: ExportFormat,
    path: &str,
    image: &Image,
  ) -> Result<(), Box<dyn Error>>;
}

pub struct PluginManager {
  effect_plugins: HashMap<String, Box<dyn EffectPlugin>>,
  load_plugins: Vec<Box<dyn LoadPlugin>>,
  export_plugins: Vec<Box<dyn ExportPlugin>>,
  cache_manager: SharedCacheManager,
}

impl PluginManager {
  pub fn new() -> Self {
    Self {
      effect_plugins: HashMap::new(),
      load_plugins: Vec::new(),
      export_plugins: Vec::new(),
      cache_manager: Arc::new(CacheManager::new()),
    }
  }

  pub fn cache_manager(&self) -> SharedCacheManager {
    Arc::clone(&self.cache_manager)
  }

  pub fn register_effect(&mut self, key: &str, plugin: Box<dyn EffectPlugin>) {
    self.effect_plugins.insert(key.to_string(), plugin);
  }

  pub fn register_load_plugin(&mut self, plugin: Box<dyn LoadPlugin>) {
    self.load_plugins.push(plugin);
  }

  pub fn register_export_plugin(&mut self, plugin: Box<dyn ExportPlugin>) {
    self.export_plugins.push(plugin);
  }

  pub fn create_entity(&self, key: &str, params: HashMap<String, PropertyValue>) -> Option<Entity> {
    self
      .effect_plugins
      .get(key)
      .map(|plugin| plugin.create(params))
  }

  pub fn load_resource(&self, request: &LoadRequest) -> Result<LoadResponse, Box<dyn Error>> {
    for plugin in &self.load_plugins {
      if plugin.supports(request) {
        return plugin.load(request, &self.cache_manager);
      }
    }
    Err(format!("No load plugin registered for request {:?}", request).into())
  }

  pub fn export_image(
    &self,
    format: ExportFormat,
    path: &str,
    image: &Image,
  ) -> Result<(), Box<dyn Error>> {
    for plugin in &self.export_plugins {
      if plugin.supports(format) {
        return plugin.export_image(format, path, image);
      }
    }
    Err("No export plugin registered for requested format".into())
  }
}

pub struct BasicTextEffectFactory;

impl Plugin for BasicTextEffectFactory {
  fn id(&self) -> &'static str {
    "basic_text_effect"
  }

  fn category(&self) -> PluginCategory {
    PluginCategory::Effect
  }
}

impl EffectPlugin for BasicTextEffectFactory {
  fn create(&self, params: HashMap<String, PropertyValue>) -> Entity {
    let mut text_entity = Entity::new("text");

    if let Some(PropertyValue::String(text)) = params.get("text") {
      text_entity.set_constant_property("text", PropertyValue::String(text.clone()));
    }

    if let Some(PropertyValue::Number(start)) = params.get("start_time") {
      text_entity.start_time = *start;
    }

    if let Some(PropertyValue::Number(end)) = params.get("end_time") {
      text_entity.end_time = *end;
    }

    text_entity.set_constant_property("size", PropertyValue::Number(24.0));
    text_entity.set_constant_property("font", PropertyValue::String("Arial".to_string()));

    text_entity
  }
}

pub fn load_plugins() -> Arc<PluginManager> {
  let mut manager = PluginManager::new();
  manager.register_effect("basic_text", Box::new(BasicTextEffectFactory));
  manager.register_load_plugin(Box::new(NativeImageLoader::new()));
  manager.register_load_plugin(Box::new(FfmpegVideoLoader::new()));
  manager.register_export_plugin(Box::new(PngExportPlugin::new()));
  Arc::new(manager)
}
