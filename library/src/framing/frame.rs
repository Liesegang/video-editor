use std::sync::Arc;

use log::debug;

use crate::model::frame::entity::FrameObject;
use crate::model::frame::frame::FrameInfo;
use crate::model::project::entity::Entity;
use crate::model::project::project::{Composition, Project};
use crate::util::timing::ScopedTimer;

use super::entity_converters::{EntityConverterRegistry, FrameEvaluationContext};
use crate::plugin::PropertyEvaluatorRegistry;

pub struct FrameEvaluator<'a> {
    composition: &'a Composition,
    property_evaluators: Arc<PropertyEvaluatorRegistry>,
    entity_converter_registry: Arc<EntityConverterRegistry>,
}

impl<'a> FrameEvaluator<'a> {
    pub fn new(
        composition: &'a Composition,
        property_evaluators: Arc<PropertyEvaluatorRegistry>,
        entity_converter_registry: Arc<EntityConverterRegistry>,
    ) -> Self {
        Self {
            composition,
            property_evaluators,
            entity_converter_registry,
        }
    }

    pub fn evaluate(&self, time: f64) -> FrameInfo {
        let mut frame = self.initialize_frame();
        for entity in self.active_entities(time) {
            if let Some(object) = self.convert_entity(entity, time) {
                frame.objects.push(object);
            }
        }
        frame
    }

    fn initialize_frame(&self) -> FrameInfo {
        FrameInfo {
            width: self.composition.width,
            height: self.composition.height,
            background_color: self.composition.background_color.clone(),
            color_profile: self.composition.color_profile.clone(),
            objects: Vec::new(),
        }
    }

    fn active_entities(&self, time: f64) -> impl Iterator<Item = &Entity> {
        self.composition
            .cached_entities()
            .iter()
            .filter(move |entity| entity.start_time <= time && entity.end_time >= time)
    }

    fn convert_entity(&self, entity: &Entity, time: f64) -> Option<FrameObject> {
        self.entity_converter_registry.convert_entity(
            // Pass self (the FrameEvaluator) as the evaluation context
            &FrameEvaluationContext {
                composition: self.composition,
                property_evaluators: &self.property_evaluators,
            },
            entity,
            time,
        )
    }
}

pub fn evaluate_composition_frame(
    composition: &Composition,
    time: f64,
    property_evaluators: &Arc<PropertyEvaluatorRegistry>,
    entity_converter_registry: &Arc<EntityConverterRegistry>,
) -> FrameInfo {
    FrameEvaluator::new(
        composition,
        Arc::clone(property_evaluators),
        Arc::clone(entity_converter_registry),
    )
    .evaluate(time)
}

pub fn get_frame_from_project(
    project: &Project,
    composition_index: usize,
    frame_index: f64,
    property_evaluators: &Arc<PropertyEvaluatorRegistry>,
    entity_converter_registry: &Arc<EntityConverterRegistry>,
) -> FrameInfo {
    let _timer = ScopedTimer::debug(format!(
        "Frame assembly comp={} frame={}",
        composition_index, frame_index
    ));

    let composition = &project.compositions[composition_index];
    let frame = evaluate_composition_frame(
        composition,
        frame_index,
        property_evaluators,
        entity_converter_registry,
    );

    debug!(
        "Frame {} summary: objects={}",
        frame_index,
        frame.objects.len()
    );
    frame
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::frame::color::Color; // Added
    use crate::model::frame::entity::FrameEntity; // Added
    use crate::model::project::project::Composition; // Added
    use crate::model::project::property::{Property, PropertyMap, PropertyValue, Vec2};
    use crate::model::project::{Track, TrackEntity};
    use crate::plugin::PluginManager;
    use crate::plugin::properties::{
        ConstantPropertyPlugin, ExpressionPropertyPlugin, KeyframePropertyPlugin,
    };
    use std::sync::Arc; // Added

    fn make_vec2(x: f64, y: f64) -> PropertyValue {
        PropertyValue::Vec2(Vec2 { x, y })
    }

    fn constant(value: PropertyValue) -> Property {
        Property::constant(value)
    }

    // Helper function to create a PluginManager with registered property plugins for tests
    fn create_test_plugin_manager() -> Arc<PluginManager> {
        let manager = Arc::new(PluginManager::new());
        manager.register_property_plugin(Arc::new(ConstantPropertyPlugin::new()));
        manager.register_property_plugin(Arc::new(KeyframePropertyPlugin::new()));
        manager.register_property_plugin(Arc::new(ExpressionPropertyPlugin::new()));
        manager.register_entity_converter_plugin(Arc::new(
            crate::framing::entity_converters::BuiltinEntityConverterPlugin::new(),
        )); // Added
        manager
    }

    #[test]
    fn frame_evaluator_builds_text_object() {
        let mut composition = Composition::new("comp", 1920, 1080, 30.0, 10.0);

        let mut text_props = PropertyMap::new();
        text_props.set(
            "text".into(),
            constant(PropertyValue::String("Hello".into())),
        );
        text_props.set(
            "font".into(),
            constant(PropertyValue::String("Roboto".into())),
        );
        text_props.set("size".into(), constant(PropertyValue::Number(48.0)));
        text_props.set(
            "color".into(),
            constant(PropertyValue::Color(Color {
                r: 255,
                g: 255,
                b: 0,
                a: 255,
            })),
        );
        text_props.set("position".into(), constant(make_vec2(10.0, 20.0)));
        text_props.set("scale".into(), constant(make_vec2(1.0, 1.0)));
        text_props.set("anchor".into(), constant(make_vec2(0.0, 0.0)));
        text_props.set("rotation".into(), constant(PropertyValue::Number(0.0)));

        let track_entity = TrackEntity {
            entity_type: "text".into(),
            start_time: 0.0,
            end_time: 5.0,
            fps: 30.0,
            properties: text_props,
            effects: Vec::new(),
        };
        let track = Track {
            name: "track".into(),
            entities: vec![track_entity],
        };
        composition.add_track(track);

        let plugin_manager = create_test_plugin_manager();
        let registry = plugin_manager.get_property_evaluators();
        let entity_converter_registry = plugin_manager.get_entity_converter_registry();
        let evaluator = FrameEvaluator::new(
            &composition,
            Arc::clone(&registry),
            Arc::clone(&entity_converter_registry),
        );
        let frame = evaluator.evaluate(1.0);

        assert_eq!(frame.objects.len(), 1);
        match &frame.objects[0].entity {
            FrameEntity::Text {
                text, font, size, ..
            } => {
                assert_eq!(text, "Hello");
                assert_eq!(font, "Roboto");
                assert!((*size - 48.0).abs() < f64::EPSILON);
            }
            other => panic!("Expected text entity, got {:?}", other),
        }
    }

    #[test]
    fn frame_evaluator_filters_inactive_entities() {
        let mut composition = Composition::new("comp", 1920, 1080, 30.0, 10.0);

        let mut props = PropertyMap::new();
        props.set(
            "file_path".into(),
            constant(PropertyValue::String("foo.png".into())),
        );
        props.set("position".into(), constant(make_vec2(0.0, 0.0)));
        props.set("scale".into(), constant(make_vec2(1.0, 1.0)));
        props.set("anchor".into(), constant(make_vec2(0.0, 0.0)));
        props.set("rotation".into(), constant(PropertyValue::Number(0.0)));

        let early = TrackEntity {
            entity_type: "image".into(),
            start_time: 0.0,
            end_time: 1.0,
            fps: 30.0,
            properties: props.clone(),
            effects: Vec::new(),
        };

        let late = TrackEntity {
            entity_type: "image".into(),
            start_time: 5.0,
            end_time: 6.0,
            fps: 30.0,
            properties: props,
            effects: Vec::new(),
        };

        let track = Track {
            name: "track".into(),
            entities: vec![early, late],
        };
        composition.add_track(track);

        let plugin_manager = create_test_plugin_manager();
        let registry = plugin_manager.get_property_evaluators();
        let entity_converter_registry = plugin_manager.get_entity_converter_registry();
        let evaluator = FrameEvaluator::new(
            &composition,
            Arc::clone(&registry),
            Arc::clone(&entity_converter_registry),
        );

        let frame = evaluator.evaluate(0.5);
        assert_eq!(frame.objects.len(), 1, "Only early entity should render");

        let frame_late = evaluator.evaluate(5.5);
        assert_eq!(
            frame_late.objects.len(),
            1,
            "Only late entity should render"
        );
    }
}
