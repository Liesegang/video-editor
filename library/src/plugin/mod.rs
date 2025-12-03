use crate::model::entity::Entity;
use crate::model::property::PropertyValue;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

pub trait EntityFactory: Send + Sync {
  fn create(&self, params: HashMap<String, PropertyValue>) -> Entity;
}

pub trait PluginSystem {
  fn register_entity_type(&mut self, entity_type: &str, factory: Box<dyn EntityFactory>);
  fn create_entity(
    &self,
    entity_type: &str,
    params: HashMap<String, PropertyValue>,
  ) -> Option<Entity>;
}

pub struct PluginManager {
  entity_factories: HashMap<String, Box<dyn EntityFactory>>,
  property_handlers: HashMap<String, Box<dyn PropertyHandler>>,
}

impl PluginManager {
  pub fn new() -> Self {
    Self {
      entity_factories: HashMap::new(),
      property_handlers: HashMap::new(),
    }
  }
}

impl PluginSystem for PluginManager {
  fn register_entity_type(&mut self, entity_type: &str, factory: Box<dyn EntityFactory>) {
    self
      .entity_factories
      .insert(entity_type.to_string(), factory);
  }

  fn create_entity(
    &self,
    entity_type: &str,
    params: HashMap<String, PropertyValue>,
  ) -> Option<Entity> {
    if let Some(factory) = self.entity_factories.get(entity_type) {
      Some(factory.create(params))
    } else {
      None
    }
  }
}

// カスタムプロパティハンドラのためのトレイト
pub trait PropertyHandler: Send + Sync {
  fn handle(&self, time: f64, params: &HashMap<String, PropertyValue>) -> PropertyValue;
  fn get_description(&self) -> &str;
}

// 簡単なプラグイン例
pub struct BasicTextEffectFactory;

impl EntityFactory for BasicTextEffectFactory {
  fn create(&self, params: HashMap<String, PropertyValue>) -> Entity {
    let mut text_entity = Entity::new("text");

    // パラメータから必要な情報を抽出
    if let Some(PropertyValue::String(text)) = params.get("text") {
      text_entity.set_constant_property("text", PropertyValue::String(text.clone()));
    }

    if let Some(PropertyValue::Number(start)) = params.get("start_time") {
      text_entity.start_time = *start;
    }

    if let Some(PropertyValue::Number(end)) = params.get("end_time") {
      text_entity.end_time = *end;
    }

    // デフォルトのスタイル設定
    text_entity.set_constant_property("size", PropertyValue::Number(24.0));
    text_entity.set_constant_property("font", PropertyValue::String("Arial".to_string()));

    text_entity
  }
}

// プラグインの読み込みと登録
pub fn load_plugins() -> Arc<Mutex<PluginManager>> {
  let manager = Arc::new(Mutex::new(PluginManager::new()));

  // 基本的なプラグインを登録
  {
    let mut lock = manager.lock().unwrap();
    lock.register_entity_type("basic_text", Box::new(BasicTextEffectFactory));
  }

  // 将来的には外部プラグインの動的読み込みをここに実装

  manager
}
