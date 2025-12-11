use std::sync::Arc;

use log::debug;

use crate::model::frame::entity::FrameObject;
use crate::model::frame::frame::FrameInfo;
use crate::model::project::TrackClip; // Add explicit import
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

    pub fn evaluate(&self, frame_number: u64) -> FrameInfo {
        // Changed to u64
        let mut frame = self.initialize_frame();
        for track_clip in self.active_clips(frame_number) {
            // Changed to track_entity
            if let Some(object) = self.convert_entity(track_clip, frame_number) {
                // Changed to track_clip
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

    fn active_clips(&self, frame_number: u64) -> impl Iterator<Item = &TrackClip> {
        // Changed to u64
        self.composition
            .cached_entities() // Returns &[TrackClip] now - updated to cached_clips if renamed in project.rs, but currently cached_entities uses TrackClip
            .iter()
            .filter(move |track_clip| {
                track_clip.in_frame <= frame_number && track_clip.out_frame >= frame_number
            })
    }

    fn convert_entity(&self, track_clip: &TrackClip, frame_number: u64) -> Option<FrameObject> {
        // Changed to track_entity, u64
        self.entity_converter_registry.convert_entity(
            // Pass self (the FrameEvaluator) as the evaluation context
            &FrameEvaluationContext {
                composition: self.composition,
                property_evaluators: &self.property_evaluators,
            },
            track_clip, // Pass track_clip
            frame_number, // Changed to frame_number
        )
    }
}

pub fn evaluate_composition_frame(
    composition: &Composition,
    frame_number: u64, // Changed to u64
    property_evaluators: &Arc<PropertyEvaluatorRegistry>,
    entity_converter_registry: &Arc<EntityConverterRegistry>,
) -> FrameInfo {
    FrameEvaluator::new(
        composition,
        Arc::clone(property_evaluators),
        Arc::clone(entity_converter_registry),
    )
    .evaluate(frame_number) // Pass frame_number
}

pub fn get_frame_from_project(
    project: &Project,
    composition_index: usize,
    frame_number: u64, // Changed to u64
    property_evaluators: &Arc<PropertyEvaluatorRegistry>,
    entity_converter_registry: &Arc<EntityConverterRegistry>,
) -> FrameInfo {
    let _timer = ScopedTimer::debug(format!(
        "Frame assembly comp={} frame={}",
        composition_index, frame_number
    ));

    let composition = &project.compositions[composition_index];
    let frame = evaluate_composition_frame(
        composition,
        frame_number, // Pass frame_number
        property_evaluators,
        entity_converter_registry,
    );

    debug!(
        "Frame {} summary: objects={}",
        frame_number, // Changed to frame_number
        frame.objects.len()
    );
    frame
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::frame::color::Color; // Added
    use crate::model::frame::entity::FrameContent; // Added
    use crate::model::project::project::Composition; // Added
    use crate::model::project::property::{Property, PropertyMap, PropertyValue, Vec2};
    use crate::model::project::{Track, TrackClip, TrackClipKind}; // import kind

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

        let track_clip = TrackClip {
            id: uuid::Uuid::new_v4(), // Added ID
            reference_id: None,
            kind: TrackClipKind::Text,
            in_frame: 0, // Renamed
            out_frame: 150, // Renamed
            source_begin_frame: 0, // Added
            duration_frame: None, // Added
            fps: 30.0,
            properties: text_props,
            effects: Vec::new(),
        };
        let track = Track {
            id: uuid::Uuid::new_v4(), // Added ID
            name: "track".into(),
            clips: vec![track_clip],
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
        let frame = evaluator.evaluate(1);

        assert_eq!(frame.objects.len(), 1);
        match &frame.objects[0].content {
            FrameContent::Text {
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

        let early = TrackClip {
            id: uuid::Uuid::new_v4(), // Added ID
            reference_id: None,
            kind: TrackClipKind::Image,
            in_frame: 0, // Renamed
            out_frame: 30, // Renamed (1.0 sec at 30fps)
            source_begin_frame: 0,
            duration_frame: None,
            fps: 30.0,
            properties: props.clone(),
            effects: Vec::new(),
        };

        let late = TrackClip {
            id: uuid::Uuid::new_v4(), // Added ID
            reference_id: None,
            kind: TrackClipKind::Image,
            in_frame: 150, // Renamed (5.0 sec at 30fps)
            out_frame: 180, // Renamed (6.0 sec at 30fps)
            source_begin_frame: 0,
            duration_frame: None,
            fps: 30.0,
            properties: props,
            effects: Vec::new(),
        };

        let track = Track {
            id: uuid::Uuid::new_v4(),
            name: "track".into(),
            clips: vec![early, late],
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

        let frame = evaluator.evaluate(15); // 0.5s * 30fps = 15 frames
        assert_eq!(frame.objects.len(), 1, "Only early entity should render");

        let frame_late = evaluator.evaluate(165); // 5.5s * 30fps = 165 frames
        assert_eq!(
            frame_late.objects.len(),
            1,
            "Only late entity should render"
        );
    }
}
