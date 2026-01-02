use std::sync::Arc;

use log::debug;

use crate::model::frame::entity::FrameObject;
use crate::model::frame::frame::{FrameInfo, Region};

use crate::model::project::Composite;
use crate::model::project::Project;
use crate::model::{GeneratorContent, Layer, LayerContent, Node};
use crate::util::timing::ScopedTimer;

use crate::plugin::FrameEvaluationContext;
use crate::plugin::{PluginManager, PropertyEvaluatorRegistry};

pub struct FrameEvaluator<'a> {
    project: &'a Project,
    composition: &'a Composite,
    property_evaluators: Arc<PropertyEvaluatorRegistry>,
    plugin_manager: Arc<PluginManager>,
}

impl<'a> FrameEvaluator<'a> {
    pub fn new(
        project: &'a Project,
        composition: &'a Composite,
        property_evaluators: Arc<PropertyEvaluatorRegistry>,
        plugin_manager: Arc<PluginManager>,
    ) -> Self {
        Self {
            project,
            composition,
            property_evaluators,
            plugin_manager,
        }
    }

    pub fn evaluate(
        &self,
        frame_number: u64,
        render_scale: f64,
        region: Option<Region>,
    ) -> FrameInfo {
        let mut frame = self.initialize_frame(frame_number, render_scale, region);
        let time = frame_number as f64 / self.composition.fps;

        // Collect active layers from the node registry
        let active_layers = self.collect_active_layers(time);

        for layer in active_layers {
            if let Some(object) = self.convert_entity(layer, time) {
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
            // Trinity: NodeGraph in composition is likely legacy or separate.
            // FrameInfo expects Option<NodeGraph>.
            node_graph: if !self.composition.node_graph.nodes.is_empty() {
                Some(self.composition.node_graph.clone())
            } else {
                None
            },
            objects: Vec::new(),
        }
    }

    fn collect_active_layers(&self, time: f64) -> Vec<&Layer> {
        let mut layers = Vec::new();
        self.collect_layers_recursive(self.composition.root_track_id, time, &mut layers);
        layers
    }

    fn collect_layers_recursive<'b>(
        &'b self,
        node_id: uuid::Uuid,
        time: f64,
        out_layers: &mut Vec<&'b Layer>,
    ) {
        match self.project.nodes.get(&node_id) {
            Some(Node::Layer(layer)) => {
                let start = layer.start_time.into_inner();
                let duration = layer.duration.into_inner();
                if time >= start && time < start + duration {
                    out_layers.push(&layer);
                }
            }
            Some(Node::Track(track)) => {
                for child_id in &track.children {
                    self.collect_layers_recursive(*child_id, time, out_layers);
                }
            }
            None => {}
        }
    }

    fn convert_entity(&self, layer: &Layer, time: f64) -> Option<FrameObject> {
        let kind_str = match &layer.content {
            LayerContent::Media(media) => {
                // Inspect asset to determine type
                if let Some(asset) = self.project.assets.iter().find(|a| a.id == media.asset_id) {
                    match asset.kind {
                        crate::model::asset::AssetKind::Video => "Video",
                        crate::model::asset::AssetKind::Image => "Image",
                        crate::model::asset::AssetKind::Audio => "Audio",
                        _ => "Unknown",
                    }
                } else {
                    "Unknown"
                }
            }
            LayerContent::Generator(generator) => match generator {
                GeneratorContent::Shape { .. } => "Shape",
                GeneratorContent::Text { .. } => "Text",
                GeneratorContent::Solid { .. } => "Solid",
                GeneratorContent::SkSL { .. } => "SkSL",
            },
            LayerContent::Reference(_) => "Reference",
        };

        if let Some(converter) = self.plugin_manager.get_entity_converter(kind_str) {
            converter.convert_entity(
                &FrameEvaluationContext {
                    composition: self.composition,
                    property_evaluators: &self.property_evaluators,
                    plugin_manager: &self.plugin_manager,
                },
                layer,
                time,
            )
        } else {
            None
        }
    }
}

pub fn evaluate_composition_frame(
    project: &Project,
    composition: &Composite,
    frame_number: u64,
    render_scale: f64,
    region: Option<Region>,
    property_evaluators: &Arc<PropertyEvaluatorRegistry>,
    plugin_manager: &Arc<PluginManager>,
) -> FrameInfo {
    FrameEvaluator::new(
        project,
        composition,
        Arc::clone(property_evaluators),
        Arc::clone(plugin_manager),
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
    plugin_manager: &Arc<PluginManager>,
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
        plugin_manager,
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
    use crate::model::Node;
    use crate::model::frame::color::Color;
    use crate::model::frame::entity::FrameContent;
    use crate::model::project::Composite;
    use crate::model::project::Project;
    use crate::model::property::{Property, PropertyMap, PropertyValue, Vec2};

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
            crate::plugin::entity_converter::VideoEntityConverterPlugin::new(),
        ));
        manager.register_entity_converter_plugin(Arc::new(
            crate::plugin::entity_converter::ImageEntityConverterPlugin::new(),
        ));
        manager.register_entity_converter_plugin(Arc::new(
            crate::plugin::entity_converter::TextEntityConverterPlugin::new(),
        ));
        manager.register_entity_converter_plugin(Arc::new(
            crate::plugin::entity_converter::ShapeEntityConverterPlugin::new(),
        ));
        manager.register_entity_converter_plugin(Arc::new(
            crate::plugin::entity_converter::SkSLEntityConverterPlugin::new(),
        ));
        manager
    }

    // Updated helper to creating dummy Layer with Image content
    fn create_dummy_layer() -> Layer {
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

        // Mock media content
        let content = LayerContent::Media(crate::model::MediaContent {
            asset_id: uuid::Uuid::new_v4(), // Dummy asset ID
            stream_index: None,
        });

        Layer {
            id: uuid::Uuid::new_v4(),
            name: "Dummy Layer".to_string(),
            start_time: ordered_float::OrderedFloat(0.0),
            duration: ordered_float::OrderedFloat(10.0), // 300 frames at 30fps
            trim_in: ordered_float::OrderedFloat(0.0),
            time_stretch: ordered_float::OrderedFloat(1.0),
            content,
            properties: props,
            styles: Vec::new(),
            effects: Vec::new(),
            ui_position: [0.0, 0.0],
        }
    }

    fn setup_test_project() -> (Project, uuid::Uuid) {
        let mut project = Project::new("Test");

        // Composite::new returns (Composite, RootTrack)
        let (comp, root_track) = Composite::new("comp", 1920, 1080, 30.0, 10.0);

        // Add root track to project nodes
        project.add_node(crate::model::Node::Track(root_track));

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

        let layer = Layer {
            id: uuid::Uuid::new_v4(),
            name: "Text Layer".to_string(),
            start_time: ordered_float::OrderedFloat(0.0),
            duration: ordered_float::OrderedFloat(5.0),
            trim_in: ordered_float::OrderedFloat(0.0),
            time_stretch: ordered_float::OrderedFloat(1.0),
            content: LayerContent::Generator(GeneratorContent::Text {
                text: "Hello".to_string(),
                font: "Roboto".to_string(),
            }),
            properties: text_props,
            styles: Vec::new(),
            effects: Vec::new(),
            ui_position: [0.0, 0.0],
        };
        let clip_id = layer.id;
        project.add_node(Node::Layer(layer));
        project
            .get_track_mut(root_id)
            .unwrap()
            .children
            .push(clip_id);

        let plugin_manager = create_test_plugin_manager();
        let registry = plugin_manager.get_property_evaluators();

        let composition = &project.compositions[0];
        let evaluator = FrameEvaluator::new(
            &project,
            composition,
            Arc::clone(&registry),
            Arc::clone(&plugin_manager),
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

        let mut early = create_dummy_layer();
        early.start_time = ordered_float::OrderedFloat(0.0);
        early.duration = ordered_float::OrderedFloat(1.0); // 30 frames at 30 fps
        early.properties = props.clone();

        let mut late = create_dummy_layer();
        late.start_time = ordered_float::OrderedFloat(5.0); // 150 frames
        late.duration = ordered_float::OrderedFloat(1.0);
        late.properties = props;

        let early_id = early.id;
        let late_id = late.id;
        project.add_node(Node::Layer(early));
        project.add_node(Node::Layer(late));
        project
            .get_track_mut(root_id)
            .unwrap()
            .children
            .push(early_id);
        project
            .get_track_mut(root_id)
            .unwrap()
            .children
            .push(late_id);

        let plugin_manager = create_test_plugin_manager();
        let registry = plugin_manager.get_property_evaluators();

        let composition = &project.compositions[0];
        let evaluator = FrameEvaluator::new(
            &project,
            composition,
            Arc::clone(&registry),
            Arc::clone(&plugin_manager),
        );

        // Frame 15 -> 0.5 sec. Active (start 0, dur 1)
        let frame = evaluator.evaluate(15, 1.0, None);
        assert_eq!(frame.objects.len(), 1, "Only early entity should render");

        // Frame 165 -> 5.5 sec. Active (start 5, dur 1)
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

        let clip1 = create_dummy_layer();
        let clip2 = create_dummy_layer();
        let clip1_id = clip1.id;
        let clip2_id = clip2.id;

        // Create child track
        let child_track = crate::model::Track::new("Child Track");
        let child_track_id = child_track.id;
        project.add_node(Node::Track(child_track));

        // Add layers
        project.add_node(Node::Layer(clip1));
        project.add_node(Node::Layer(clip2));

        // Link hierarchy
        project
            .get_track_mut(root_id)
            .unwrap()
            .children
            .push(clip1_id);
        project
            .get_track_mut(root_id)
            .unwrap()
            .children
            .push(child_track_id);
        project
            .get_track_mut(child_track_id)
            .unwrap()
            .children
            .push(clip2_id);

        let plugin_manager = create_test_plugin_manager();
        let registry = plugin_manager.get_property_evaluators();

        let composition = &project.compositions[0];
        let evaluator = FrameEvaluator::new(
            &project,
            composition,
            Arc::clone(&registry),
            Arc::clone(&plugin_manager),
        );

        let frame = evaluator.evaluate(10, 1.0, None);
        assert_eq!(
            frame.objects.len(),
            2,
            "Should find clips from both parent and child tracks"
        );
    }
}
