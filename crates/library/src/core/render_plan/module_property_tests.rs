//! End-to-end property coverage for the bounded Module runtime.

use std::collections::HashMap;
use std::sync::Arc;

use ordered_float::OrderedFloat;

use super::{RenderPlanCompiler, evaluate_render_plan_frame};
use crate::editor::TimelineEditorService;
use crate::editor::project_service::{GeneratorNodeRequest, test_generator_node};
use crate::model::BlendMode;
use crate::model::authoring::{
    AuthoringProject, MediaTime, ModuleConnection, ModuleConnectionId, ModuleDefinition,
    ModuleDefinitionSharing, ModuleInstance, ModuleInstanceId, ModuleInvocation, ModulePortAddress,
    RationalRate, SourceRef, TimeMap, TimelineInterval, TimelineItem, TimelineItemId,
};
use crate::model::frame::color::Color;
use crate::model::frame::entity::{FrameContent, FrameGroup, FrameGroupKind, FrameItem};
use crate::model::project::{
    IMAGE_INPUT_PORT, IMAGE_OUTPUT_PORT, SHAPE_INPUT_PORT, SHAPE_OUTPUT_PORT,
};
use crate::model::property::{Property, PropertyValue};
use crate::plugin::{
    EvaluationContext, Plugin, PluginManager, PropertyEvaluationError, PropertyEvaluator,
    PropertyPlugin,
};

fn seconds(value: i64) -> MediaTime {
    MediaTime::new(value, 1).expect("whole seconds")
}

fn connection(
    from_node: uuid::Uuid,
    from_port: &str,
    to_node: uuid::Uuid,
    to_port: &str,
) -> ModuleConnection {
    ModuleConnection {
        id: ModuleConnectionId::new(),
        from: ModulePortAddress {
            node_id: from_node,
            port: from_port.to_string(),
        },
        to: ModulePortAddress {
            node_id: to_node,
            port: to_port.to_string(),
        },
        order: 0,
        blend_mode: BlendMode::Normal,
    }
}

fn project_with_module(
    mut definition: ModuleDefinition,
    output_id: crate::model::authoring::ModuleOutputId,
) -> (AuthoringProject, ModuleInstanceId) {
    definition.topology_revision += 1;
    definition.validate().expect("complete Module definition");
    let definition_id = definition.id;
    let mut project = AuthoringProject::new(
        "Module property runtime",
        320,
        180,
        RationalRate::new(30, 1).unwrap(),
        seconds(4),
    )
    .unwrap();
    let track_id = project.timelines[&project.root_timeline_id].track_order[0];
    let instance_id = ModuleInstanceId::new();
    project.module_definitions.insert(definition_id, definition);
    project.module_instances.insert(
        instance_id,
        ModuleInstance {
            id: instance_id,
            definition_id,
            parameter_overrides: HashMap::new(),
        },
    );
    let item_id = TimelineItemId::new();
    project.items.insert(
        item_id,
        TimelineItem {
            id: item_id,
            track_id,
            name: "Node Clip".to_string(),
            source: SourceRef::Module(ModuleInvocation {
                instance_id,
                output_id,
                input_bindings: HashMap::new(),
                automation_tracks: HashMap::new(),
            }),
            interval: TimelineInterval::new(seconds(0), seconds(4)).unwrap(),
            time_map: TimeMap::default(),
            layer: 0,
            parent: None,
            blend_mode: BlendMode::Normal,
            authored_properties: Default::default(),
        },
    );
    project.validate().expect("valid Node Clip project");
    (project, instance_id)
}

fn find_text(items: &[FrameItem]) -> Option<(&str, f64)> {
    for item in items {
        match item {
            FrameItem::Object(object) => {
                if let FrameContent::Text { text, size, .. } = &object.content {
                    return Some((text, *size));
                }
            }
            FrameItem::Group(group) => {
                if let Some(found) = find_text(&group.items) {
                    return Some(found);
                }
            }
            FrameItem::Transition(transition) => {
                if let Some(found) = find_text(std::slice::from_ref(&transition.from.item))
                    .or_else(|| find_text(std::slice::from_ref(&transition.to.item)))
                {
                    return Some(found);
                }
            }
        }
    }
    None
}

fn find_group(
    items: &[FrameItem],
    source_id: uuid::Uuid,
    kind: FrameGroupKind,
) -> Option<&FrameGroup> {
    for item in items {
        if let FrameItem::Group(group) = item {
            if group.source_id == source_id && group.kind == kind {
                return Some(group);
            }
            if let Some(found) = find_group(&group.items, source_id, kind) {
                return Some(found);
            }
        }
    }
    None
}

struct TimeOffsetEvaluator;

impl PropertyEvaluator for TimeOffsetEvaluator {
    fn evaluate(
        &self,
        property: &Property,
        time: f64,
        _context: &EvaluationContext,
    ) -> Result<PropertyValue, PropertyEvaluationError> {
        let base = property
            .value()
            .and_then(|value| value.get_as::<f64>())
            .ok_or_else(|| PropertyEvaluationError::new("time-offset-test", "missing number"))?;
        Ok(PropertyValue::Number(OrderedFloat(base + time)))
    }
}

struct TimeOffsetPlugin;

impl Plugin for TimeOffsetPlugin {
    fn id(&self) -> &str {
        "time-offset-test"
    }

    fn name(&self) -> String {
        "Time offset test".to_string()
    }

    fn category(&self) -> String {
        "Tests".to_string()
    }

    fn version(&self) -> (u32, u32, u32) {
        (0, 1, 0)
    }
}

impl PropertyPlugin for TimeOffsetPlugin {
    fn get_evaluator_instance(&self) -> Arc<dyn PropertyEvaluator> {
        Arc::new(TimeOffsetEvaluator)
    }
}

#[test]
fn module_generator_uses_registered_property_evaluator_at_local_time() {
    let plugins = PluginManager::default();
    plugins.register_property_plugin(Arc::new(TimeOffsetPlugin));
    let mut text = test_generator_node(
        "Text",
        GeneratorNodeRequest::Text {
            text: "Effective".to_string(),
            font: "Arial".to_string(),
        },
    );
    text.set_property(
        "size".to_string(),
        Property {
            evaluator: "time-offset-test".to_string(),
            properties: HashMap::from([(
                "value".to_string(),
                PropertyValue::Number(OrderedFloat(100.0)),
            )]),
        },
    )
    .unwrap();
    let fill = plugins.create_style_operation_node("fill").unwrap();
    let (text_id, fill_id) = (text.id, fill.id);
    let (mut definition, output_id) =
        ModuleDefinition::new_image("Text property", ModuleDefinitionSharing::Private);
    let output = definition.output(output_id).unwrap();
    definition
        .graph
        .nodes
        .extend([(text_id, text), (fill_id, fill)]);
    definition.graph.connections.extend([
        connection(text_id, SHAPE_OUTPUT_PORT, fill_id, SHAPE_INPUT_PORT),
        connection(fill_id, IMAGE_OUTPUT_PORT, output.node_id, IMAGE_INPUT_PORT),
    ]);
    let (project, _) = project_with_module(definition, output_id);
    let plan = RenderPlanCompiler::compile(&project).unwrap();
    let frame = evaluate_render_plan_frame(&project, &plan, &plugins, 30, 1.0, None).unwrap();

    assert_eq!(find_text(&frame.items), Some(("Effective", 101.0)));
}

#[test]
fn module_effect_values_reach_the_frame_operation() {
    let plugins = PluginManager::default();
    let solid = test_generator_node(
        "Solid",
        GeneratorNodeRequest::Solid {
            color: Color::white(),
        },
    );
    let blur = plugins.create_effect_operation_node("blur").unwrap();
    let (solid_id, blur_id) = (solid.id, blur.id);
    let (mut definition, output_id) =
        ModuleDefinition::new_image("Effect properties", ModuleDefinitionSharing::Private);
    let output = definition.output(output_id).unwrap();
    definition
        .graph
        .nodes
        .extend([(solid_id, solid), (blur_id, blur)]);
    definition.graph.connections.extend([
        connection(solid_id, IMAGE_OUTPUT_PORT, blur_id, IMAGE_INPUT_PORT),
        connection(blur_id, IMAGE_OUTPUT_PORT, output.node_id, IMAGE_INPUT_PORT),
    ]);
    let (project, instance_id) = project_with_module(definition, output_id);
    let service = TimelineEditorService::new(project).unwrap();
    service
        .set_instance_module_node_property(
            instance_id,
            blur_id,
            "sigma_x".to_string(),
            Property::constant(PropertyValue::from(6.0)),
        )
        .unwrap();
    service
        .set_instance_module_node_property(
            instance_id,
            blur_id,
            "sigma_y".to_string(),
            Property::constant(PropertyValue::from(23.0)),
        )
        .unwrap();
    let project = service.snapshot().unwrap();
    let plan = RenderPlanCompiler::compile(&project).unwrap();
    let frame = evaluate_render_plan_frame(&project, &plan, &plugins, 30, 1.0, None).unwrap();
    let group = find_group(&frame.items, blur_id, FrameGroupKind::Effect).unwrap();
    let values = &group.effects[0].properties;

    assert_eq!(values["sigma_x"], PropertyValue::from(6.0));
    assert_eq!(values["sigma_y"], PropertyValue::from(23.0));
}

#[test]
fn module_transform_keeps_translate_and_scale_axes_independent() {
    let plugins = PluginManager::default();
    let solid = test_generator_node(
        "Solid",
        GeneratorNodeRequest::Solid {
            color: Color::white(),
        },
    );
    let transform = plugins.create_image_transform_operation_node().unwrap();
    let (solid_id, transform_id) = (solid.id, transform.id);
    let (mut definition, output_id) =
        ModuleDefinition::new_image("Transform axes", ModuleDefinitionSharing::Private);
    let output = definition.output(output_id).unwrap();
    definition
        .graph
        .nodes
        .extend([(solid_id, solid), (transform_id, transform)]);
    definition.graph.connections.extend([
        connection(solid_id, IMAGE_OUTPUT_PORT, transform_id, IMAGE_INPUT_PORT),
        connection(
            transform_id,
            IMAGE_OUTPUT_PORT,
            output.node_id,
            IMAGE_INPUT_PORT,
        ),
    ]);
    let (project, instance_id) = project_with_module(definition, output_id);
    let service = TimelineEditorService::new(project).unwrap();
    for (key, x, y) in [("position", 17.0, 29.0), ("scale", 50.0, 125.0)] {
        service
            .set_instance_module_node_property(
                instance_id,
                transform_id,
                key.to_string(),
                Property::constant(crate::plugin::transforms::vec2_value(x, y)),
            )
            .unwrap();
    }
    let project = service.snapshot().unwrap();
    let plan = RenderPlanCompiler::compile(&project).unwrap();
    let frame = evaluate_render_plan_frame(&project, &plan, &plugins, 0, 1.0, None).unwrap();
    let transform = &find_group(&frame.items, transform_id, FrameGroupKind::ImageTransform)
        .unwrap()
        .transform;

    assert_eq!((transform.position.x, transform.position.y), (17.0, 29.0));
    assert_eq!((transform.scale.x, transform.scale.y), (0.5, 1.25));
}
