use std::sync::Arc;

use log::debug;

use crate::model::frame::entity::FrameObject;
use crate::model::frame::frame::{FrameInfo, Region};
use crate::model::project::project::{Composition, Project};
use crate::model::project::{Node, TrackClip, TrackClipKind};
use crate::util::timing::ScopedTimer;

use super::entity_converters::{EntityConverterRegistry, FrameEvaluationContext};
use crate::plugin::PropertyEvaluatorRegistry;

pub struct FrameEvaluator<'a> {
    project: &'a Project,
    composition: &'a Composition,
    property_evaluators: Arc<PropertyEvaluatorRegistry>,
    entity_converter_registry: Arc<EntityConverterRegistry>,
}

impl<'a> FrameEvaluator<'a> {
    pub fn new(
        project: &'a Project,
        composition: &'a Composition,
        property_evaluators: Arc<PropertyEvaluatorRegistry>,
        entity_converter_registry: Arc<EntityConverterRegistry>,
    ) -> Self {
        Self {
            project,
            composition,
            property_evaluators,
            entity_converter_registry,
        }
    }

    pub fn evaluate(
        &self,
        frame_number: u64,
        render_scale: f64,
        region: Option<Region>,
    ) -> FrameInfo {
        let mut frame = self.initialize_frame(frame_number, render_scale, region);

        // Collect active clips from the node registry
        let all_clips = self.collect_active_clips(frame_number);

        for track_clip in all_clips {
            if let Some(object) = self.convert_entity(track_clip, frame_number) {
                frame.objects.push(object);
            }
        }
        frame
    }

    fn initialize_frame(
        &self,
        frame_number: u64,
        render_scale: f64,
        region: Option<Region>,
    ) -> FrameInfo {
        let time = frame_number as f64 / self.composition.fps;
        FrameInfo {
            width: self.composition.width,
            height: self.composition.height,
            background_color: self.composition.background_color.clone(),
            color_profile: self.composition.color_profile.clone(),
            render_scale: ordered_float::OrderedFloat(render_scale),
            now_time: ordered_float::OrderedFloat(time),
            region,
            objects: Vec::new(),
        }
    }

    fn collect_active_clips(&self, frame_number: u64) -> Vec<&TrackClip> {
        let mut clips = Vec::new();
        self.collect_clips_recursive(self.composition.root_track_id, frame_number, &mut clips);
        clips
    }

    fn collect_clips_recursive<'b>(
        &'b self,
        node_id: uuid::Uuid,
        frame_number: u64,
        out_clips: &mut Vec<&'b TrackClip>,
    ) {
        match self.project.get_node(node_id) {
            Some(Node::Clip(clip)) => {
                if clip.kind != TrackClipKind::Audio
                    && clip.in_frame <= frame_number
                    && clip.out_frame >= frame_number
                {
                    out_clips.push(clip);
                }
            }
            Some(Node::Track(track)) => {
                for child_id in &track.child_ids {
                    self.collect_clips_recursive(*child_id, frame_number, out_clips);
                }
            }
            None => {}
        }
    }

    fn convert_entity(&self, track_clip: &TrackClip, frame_number: u64) -> Option<FrameObject> {
        self.entity_converter_registry.convert_entity(
            &FrameEvaluationContext {
                composition: self.composition,
                property_evaluators: &self.property_evaluators,
            },
            track_clip,
            frame_number,
        )
    }
}

pub fn evaluate_composition_frame(
    project: &Project,
    composition: &Composition,
    frame_number: u64,
    render_scale: f64,
    region: Option<Region>,
    property_evaluators: &Arc<PropertyEvaluatorRegistry>,
    entity_converter_registry: &Arc<EntityConverterRegistry>,
) -> FrameInfo {
    FrameEvaluator::new(
        project,
        composition,
        Arc::clone(property_evaluators),
        Arc::clone(entity_converter_registry),
    )
    .evaluate(frame_number, render_scale, region)
}

pub fn get_frame_from_project(
    project: &Project,
    composition_index: usize,
    frame_number: u64,
    render_scale: f64,
    region: Option<Region>,
    property_evaluators: &Arc<PropertyEvaluatorRegistry>,
    entity_converter_registry: &Arc<EntityConverterRegistry>,
) -> FrameInfo {
    let _timer = if log::log_enabled!(log::Level::Debug) {
        Some(ScopedTimer::debug(format!(
            "Frame assembly comp={} frame={}",
            composition_index, frame_number
        )))
    } else {
        None
    };

    let composition = &project.compositions[composition_index];
    let frame = evaluate_composition_frame(
        project,
        composition,
        frame_number,
        render_scale,
        region,
        property_evaluators,
        entity_converter_registry,
    );

    debug!(
        "Frame {} summary: objects={}",
        frame_number,
        frame.objects.len()
    );
    frame
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::frame::color::Color;
    use crate::model::frame::entity::FrameContent;
    use crate::model::project::project::Composition;
    use crate::model::project::property::{Property, PropertyMap, PropertyValue, Vec2};
    use crate::model::project::{Node, TrackClip, TrackClipKind, TrackData};

    use crate::plugin::PluginManager;
    use crate::plugin::properties::{
        ConstantPropertyPlugin, ExpressionPropertyPlugin, KeyframePropertyPlugin,
    };
    use std::sync::Arc;

    fn make_vec2(x: f64, y: f64) -> PropertyValue {
        PropertyValue::Vec2(Vec2 {
            x: ordered_float::OrderedFloat(x),
            y: ordered_float::OrderedFloat(y),
        })
    }

    fn constant(value: PropertyValue) -> Property {
        Property::constant(value)
    }

    fn create_test_plugin_manager() -> Arc<PluginManager> {
        let manager = Arc::new(PluginManager::new());
        manager.register_property_plugin(Arc::new(ConstantPropertyPlugin::new()));
        manager.register_property_plugin(Arc::new(KeyframePropertyPlugin::new()));
        manager.register_property_plugin(Arc::new(ExpressionPropertyPlugin::new()));
        manager.register_entity_converter_plugin(Arc::new(
            crate::framing::entity_converters::BuiltinEntityConverterPlugin::new(),
        ));
        manager
    }

    fn create_dummy_clip() -> TrackClip {
        let mut props = PropertyMap::new();
        props.set(
            "file_path".into(),
            constant(PropertyValue::String("dummy".into())),
        );
        props.set("position".into(), constant(make_vec2(0.0, 0.0)));
        props.set("scale".into(), constant(make_vec2(100.0, 100.0)));
        props.set(
            "scale_x".into(),
            constant(PropertyValue::Number(ordered_float::OrderedFloat(100.0))),
        );
        props.set(
            "scale_y".into(),
            constant(PropertyValue::Number(ordered_float::OrderedFloat(100.0))),
        );

        TrackClip {
            id: uuid::Uuid::new_v4(),
            reference_id: None,
            kind: TrackClipKind::Image,
            in_frame: 0,
            out_frame: 100,
            source_begin_frame: 0,
            duration_frame: None,
            fps: 30.0,
            properties: props,
            effects: Vec::new(),
            styles: Vec::new(),
        }
    }

    fn setup_test_project() -> (Project, uuid::Uuid) {
        let mut project = Project::new("Test");
        let root_track = TrackData::new("Root");
        let root_id = root_track.id;
        project.add_node(Node::Track(root_track));

        let comp = Composition::new_with_root("comp", 1920, 1080, 30.0, 10.0, root_id);
        let comp_id = comp.id;
        project.add_composition(comp);

        (project, comp_id)
    }

    #[test]
    fn frame_evaluator_builds_text_object() {
        let (mut project, _comp_id) = setup_test_project();
        let comp = &project.compositions[0];
        let root_id = comp.root_track_id;

        let mut text_props = PropertyMap::new();
        text_props.set(
            "text".into(),
            constant(PropertyValue::String("Hello".into())),
        );
        text_props.set(
            "font_family".into(),
            constant(PropertyValue::String("Roboto".into())),
        );
        text_props.set(
            "size".into(),
            constant(PropertyValue::Number(ordered_float::OrderedFloat(48.0))),
        );
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
        text_props.set(
            "scale_x".into(),
            constant(PropertyValue::Number(ordered_float::OrderedFloat(100.0))),
        );
        text_props.set(
            "scale_y".into(),
            constant(PropertyValue::Number(ordered_float::OrderedFloat(100.0))),
        );
        text_props.set(
            "anchor_x".into(),
            constant(PropertyValue::Number(ordered_float::OrderedFloat(0.0))),
        );
        text_props.set(
            "anchor_y".into(),
            constant(PropertyValue::Number(ordered_float::OrderedFloat(0.0))),
        );
        text_props.set(
            "rotation".into(),
            constant(PropertyValue::Number(ordered_float::OrderedFloat(0.0))),
        );
        text_props.set(
            "opacity".into(),
            constant(PropertyValue::Number(ordered_float::OrderedFloat(100.0))),
        );

        let track_clip = TrackClip {
            id: uuid::Uuid::new_v4(),
            reference_id: None,
            kind: TrackClipKind::Text,
            in_frame: 0,
            out_frame: 150,
            source_begin_frame: 0,
            duration_frame: None,
            fps: 30.0,
            properties: text_props,
            effects: Vec::new(),
            styles: Vec::new(),
        };
        let clip_id = track_clip.id;
        project.add_node(Node::Clip(track_clip));
        project.get_track_mut(root_id).unwrap().add_child(clip_id);

        let plugin_manager = create_test_plugin_manager();
        let registry = plugin_manager.get_property_evaluators();
        let entity_converter_registry = plugin_manager.get_entity_converter_registry();

        let composition = &project.compositions[0];
        let evaluator = FrameEvaluator::new(
            &project,
            composition,
            Arc::clone(&registry),
            Arc::clone(&entity_converter_registry),
        );
        let frame = evaluator.evaluate(1, 1.0, None);

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
        let (mut project, _comp_id) = setup_test_project();
        let comp = &project.compositions[0];
        let root_id = comp.root_track_id;

        let mut props = PropertyMap::new();
        props.set(
            "file_path".into(),
            constant(PropertyValue::String("foo.png".into())),
        );
        props.set("position".into(), constant(make_vec2(0.0, 0.0)));
        props.set(
            "scale_x".into(),
            constant(PropertyValue::Number(ordered_float::OrderedFloat(100.0))),
        );
        props.set(
            "scale_y".into(),
            constant(PropertyValue::Number(ordered_float::OrderedFloat(100.0))),
        );
        props.set(
            "anchor_x".into(),
            constant(PropertyValue::Number(ordered_float::OrderedFloat(0.0))),
        );
        props.set(
            "anchor_y".into(),
            constant(PropertyValue::Number(ordered_float::OrderedFloat(0.0))),
        );
        props.set(
            "rotation".into(),
            constant(PropertyValue::Number(ordered_float::OrderedFloat(0.0))),
        );
        props.set(
            "opacity".into(),
            constant(PropertyValue::Number(ordered_float::OrderedFloat(100.0))),
        );

        let early = TrackClip {
            id: uuid::Uuid::new_v4(),
            reference_id: None,
            kind: TrackClipKind::Image,
            in_frame: 0,
            out_frame: 30,
            source_begin_frame: 0,
            duration_frame: None,
            fps: 30.0,
            properties: props.clone(),
            effects: Vec::new(),
            styles: Vec::new(),
        };

        let late = TrackClip {
            id: uuid::Uuid::new_v4(),
            reference_id: None,
            kind: TrackClipKind::Image,
            in_frame: 150,
            out_frame: 180,
            source_begin_frame: 0,
            duration_frame: None,
            fps: 30.0,
            properties: props,
            effects: Vec::new(),
            styles: Vec::new(),
        };

        let early_id = early.id;
        let late_id = late.id;
        project.add_node(Node::Clip(early));
        project.add_node(Node::Clip(late));
        project.get_track_mut(root_id).unwrap().add_child(early_id);
        project.get_track_mut(root_id).unwrap().add_child(late_id);

        let plugin_manager = create_test_plugin_manager();
        let registry = plugin_manager.get_property_evaluators();
        let entity_converter_registry = plugin_manager.get_entity_converter_registry();

        let composition = &project.compositions[0];
        let evaluator = FrameEvaluator::new(
            &project,
            composition,
            Arc::clone(&registry),
            Arc::clone(&entity_converter_registry),
        );

        let frame = evaluator.evaluate(15, 1.0, None);
        assert_eq!(frame.objects.len(), 1, "Only early entity should render");

        let frame_late = evaluator.evaluate(165, 1.0, None);
        assert_eq!(
            frame_late.objects.len(),
            1,
            "Only late entity should render"
        );
    }

    #[test]
    fn frame_evaluator_flattens_nested_tracks() {
        let (mut project, _comp_id) = setup_test_project();
        let comp = &project.compositions[0];
        let root_id = comp.root_track_id;

        let clip1 = create_dummy_clip();
        let clip2 = create_dummy_clip();
        let clip1_id = clip1.id;
        let clip2_id = clip2.id;

        // Create child track
        let child_track = TrackData::new("Child Track");
        let child_track_id = child_track.id;
        project.add_node(Node::Track(child_track));

        // Add clips
        project.add_node(Node::Clip(clip1));
        project.add_node(Node::Clip(clip2));

        // Link hierarchy
        project.get_track_mut(root_id).unwrap().add_child(clip1_id);
        project
            .get_track_mut(root_id)
            .unwrap()
            .add_child(child_track_id);
        project
            .get_track_mut(child_track_id)
            .unwrap()
            .add_child(clip2_id);

        let plugin_manager = create_test_plugin_manager();
        let registry = plugin_manager.get_property_evaluators();
        let entity_converter_registry = plugin_manager.get_entity_converter_registry();

        let composition = &project.compositions[0];
        let evaluator = FrameEvaluator::new(
            &project,
            composition,
            Arc::clone(&registry),
            Arc::clone(&entity_converter_registry),
        );

        let frame = evaluator.evaluate(10, 1.0, None);
        assert_eq!(
            frame.objects.len(),
            2,
            "Should find clips from both parent and child tracks"
        );
    }
}
