//! Executable proof for installing a third-party plugin after this host binary
//! has already been built. See `scripts/test-runtime-plugin.sh`.

use std::collections::{BTreeSet, HashMap};
use std::path::PathBuf;
use std::sync::{Arc, RwLock};

use anyhow::{Context, bail};
use library::cache::CacheManager;
use library::core::ensemble::decorators::{BackplateShape, BackplateTarget};
use library::core::ensemble::types::DecoratorConfig;
use library::framing::get_frame_from_project;
use library::model::frame::draw_type::{CapType, DrawStyle, JoinType};
use library::model::frame::entity::{FrameContent, FrameItem};
use library::model::project::{
    EvalOutput, NodeContainer, PortAddress, PortOwner, ProjectConnection, SHAPE_INPUT_PORT,
    SHAPE_OUTPUT_PORT,
};
use library::model::property::{Property, PropertyMap, PropertyValue};
use library::model::{Clip, Composition, GeneratorContent, Node, NodeContent, Project};
use library::plugin::native_plugin_api::{
    DECORATOR_CATEGORY, PROPERTY_CATEGORY, PropertyValueV1, STYLE_CATEGORY,
};
use library::plugin::{EvaluationContext, FrameEvaluationContext, PluginManager};
use library::rendering::renderer::RenderOutput;
use library::{RenderService, SkiaRenderer};

const FILL_COMPONENT_ID: &str = "runtime_fill_style";
const STROKE_COMPONENT_ID: &str = "runtime_stroke_style";
const BACKPLATE_COMPONENT_ID: &str = "runtime_backplate_decorator";

fn main() -> anyhow::Result<()> {
    let bundle_path = std::env::args_os()
        .nth(1)
        .map(PathBuf::from)
        .context("usage: runtime_plugin_probe <bundle-directory>")?;
    let manager = Arc::new(PluginManager::default());

    let report = manager.rescan_runtime_plugin_path(&bundle_path);
    if !report.failures.is_empty() {
        bail!("runtime scan failed: {:?}", report.failures);
    }
    if report.loaded_bundles.len() != 1 || report.registered_components.len() != 4 {
        bail!("unexpected first scan report: {report:?}");
    }

    let descriptors = manager.get_runtime_plugin_descriptors();
    let component = descriptors
        .iter()
        .flat_map(|bundle| &bundle.descriptor.components)
        .find(|component| component.category == PROPERTY_CATEGORY)
        .context("bundle descriptor has no integrated property component")?;
    let component_id = component.id.clone();
    if !matches!(
        component.output_default.as_ref(),
        Some(PropertyValueV1::Number { value }) if value.abs() < f64::EPSILON
    ) {
        bail!("property descriptor has no explicit numeric fail-safe default");
    }

    let property = manager.create_property_instance(&component_id)?;
    let descriptor_property_names = component
        .properties
        .iter()
        .map(|definition| definition.name.as_str())
        .collect::<BTreeSet<_>>();
    let instance_property_names = property
        .properties
        .keys()
        .map(String::as_str)
        .collect::<BTreeSet<_>>();
    if descriptor_property_names != instance_property_names {
        bail!("descriptor-backed factory did not materialize every property");
    }

    let evaluators = manager.get_property_evaluators();
    let sibling_properties = PropertyMap::new();
    let context = EvaluationContext {
        property_map: &sibling_properties,
        fps: 30.0,
    };
    let complete_output = evaluators.evaluate(&property, 0.25, &context);
    let sparse = Property {
        evaluator: component_id.clone(),
        properties: HashMap::new(),
    };
    let recovered_output = evaluators.evaluate(&sparse, 0.25, &context);
    if complete_output != recovered_output {
        bail!("sparse property did not evaluate with descriptor defaults");
    }
    if !matches!(
        complete_output,
        PropertyValue::Number(value) if (-1.0..=1.0).contains(&value.into_inner())
    ) {
        bail!("runtime property returned an invalid value");
    }

    verify_unknown_property_is_safe(&manager, &context)?;
    verify_config_operations(&manager, &descriptors)?;

    let second = manager.rescan_runtime_plugin_path(&bundle_path);
    if !second.failures.is_empty() || second.already_loaded_bundles.len() != 1 {
        bail!("runtime rescan was not idempotent: {second:?}");
    }

    println!(
        "runtime plugin proof passed: component={}, properties={}, prebuilt_host=true",
        component_id,
        descriptor_property_names.len()
    );
    Ok(())
}

fn verify_config_operations(
    manager: &Arc<PluginManager>,
    descriptors: &[library::plugin::RuntimePluginDescriptor],
) -> anyhow::Result<()> {
    let components = descriptors
        .iter()
        .flat_map(|bundle| &bundle.descriptor.components)
        .collect::<Vec<_>>();
    let fill_component = components
        .iter()
        .copied()
        .find(|component| component.category == STYLE_CATEGORY && component.id == FILL_COMPONENT_ID)
        .context("bundle descriptor has no runtime Fill component")?;
    let stroke_component = components
        .iter()
        .copied()
        .find(|component| {
            component.category == STYLE_CATEGORY && component.id == STROKE_COMPONENT_ID
        })
        .context("bundle descriptor has no runtime Stroke component")?;
    let backplate_component = components
        .iter()
        .copied()
        .find(|component| {
            component.category == DECORATOR_CATEGORY && component.id == BACKPLATE_COMPONENT_ID
        })
        .context("bundle descriptor has no runtime Backplate component")?;

    let fill = manager.create_style_operation_node(FILL_COMPONENT_ID)?;
    let stroke = manager.create_style_operation_node(STROKE_COMPONENT_ID)?;
    let backplate = manager.create_decorator_operation_node(BACKPLATE_COMPONENT_ID)?;
    verify_all_defaults(fill_component, &fill)?;
    verify_all_defaults(stroke_component, &stroke)?;
    verify_all_defaults(backplate_component, &backplate)?;
    verify_hard_minimum(stroke_component, "width")?;
    verify_hard_minimum(stroke_component, "miter")?;
    verify_soft_minimum(stroke_component, "offset")?;
    verify_soft_minimum(stroke_component, "dash_offset")?;
    verify_hard_minimum(backplate_component, "corner_radius")?;

    let (composition, track) = Composition::new("Runtime config proof", 640, 360, 30.0, 1.0);
    let composition_id = composition.id;
    let mut project = Project::new("Runtime config proof");
    project.add_track(track);
    project.add_composition(composition);
    let composition = project
        .get_composition(composition_id)
        .context("test composition disappeared")?;
    let property_evaluators = manager.get_property_evaluators();
    let context = FrameEvaluationContext {
        project: &project,
        composition,
        property_evaluators: &property_evaluators,
        plugin_manager: manager,
        resolved_inputs: None,
    };

    let EvalOutput::Produced(fill_config) = manager.evaluate_style_operation(
        &context,
        FILL_COMPONENT_ID,
        fill.id,
        &fill.properties,
        0.25,
    ) else {
        bail!("runtime Fill adapter produced NoOutput")
    };
    if fill_config.id != fill.id
        || fill_config.style
            != (DrawStyle::Fill {
                color: library::model::frame::color::Color {
                    r: 255,
                    g: 128,
                    b: 32,
                    a: 255,
                },
                offset: 2.0,
            })
    {
        bail!("runtime Fill adapter returned the wrong host config")
    }

    let EvalOutput::Produced(stroke_config) = manager.evaluate_style_operation(
        &context,
        STROKE_COMPONENT_ID,
        stroke.id,
        &stroke.properties,
        0.25,
    ) else {
        bail!("runtime Stroke adapter produced NoOutput")
    };
    if stroke_config.id != stroke.id
        || stroke_config.style
            != (DrawStyle::Stroke {
                color: library::model::frame::color::Color {
                    r: 32,
                    g: 128,
                    b: 255,
                    a: 255,
                },
                width: 3.0,
                offset: 0.0,
                cap: CapType::Round,
                join: JoinType::Miter,
                miter: 4.0,
                dash_array: vec![3.0, 2.0],
                dash_offset: 1.0,
            })
    {
        bail!("runtime Stroke adapter returned the wrong host config")
    }

    let EvalOutput::Produced(decorator_config) = manager.evaluate_decorator_operation(
        &context,
        BACKPLATE_COMPONENT_ID,
        backplate.id,
        &backplate.properties,
        0.25,
    ) else {
        bail!("runtime Backplate adapter produced NoOutput")
    };
    if decorator_config
        != (DecoratorConfig::Backplate {
            target: BackplateTarget::Block,
            shape: BackplateShape::RoundedRect,
            color: library::model::frame::color::Color {
                r: 0,
                g: 0,
                b: 0,
                a: 192,
            },
            padding: (4.0, 6.0, 4.0, 6.0),
            corner_radius: 3.0,
        })
    {
        bail!("runtime Backplate adapter returned the wrong host config")
    }

    verify_runtime_config_graph(manager)?;

    let mut unavailable = fill;
    let unavailable_component_id = {
        let library::model::NodeContent::PluginOperation(operation) = &mut unavailable.content
        else {
            bail!("runtime Style factory returned non-operation content")
        };
        operation.component_id = "unavailable.vendor.style".to_string();
        operation.component_id.clone()
    };
    let json = serde_json::to_string(&unavailable)?;
    let preserved: Node = serde_json::from_str(&json)?;
    if preserved != unavailable {
        bail!("uninstalled runtime Style operation was not lossless")
    }
    if !matches!(
        manager.evaluate_style_operation(
            &context,
            &unavailable_component_id,
            unavailable.id,
            &unavailable.properties,
            0.25,
        ),
        EvalOutput::NoOutput
    ) {
        bail!("uninstalled runtime Style operation did not produce NoOutput")
    }

    Ok(())
}

fn verify_runtime_config_graph(manager: &Arc<PluginManager>) -> anyhow::Result<()> {
    let factory = library::editor::project_service::ProjectManager::new(
        Arc::new(RwLock::new(Project::new("runtime graph factory"))),
        Arc::clone(manager),
    );
    let mut graph = factory.create_text_graph("RUNTIME", "Arial", 320, 180)?;
    let text_id = graph
        .nodes
        .iter()
        .find(|node| matches!(node.content, NodeContent::Generator(GeneratorContent::Text)))
        .map(|node| node.id)
        .context("text graph has no explicit Shape source")?;
    let old_output = graph
        .output_node_id
        .context("text graph has no explicit Style output")?;
    let old_output_index = graph
        .nodes
        .iter()
        .position(|node| node.id == old_output)
        .context("text graph output Node disappeared")?;
    let mut fill = manager.create_style_operation_node(FILL_COMPONENT_ID)?;
    fill.ui_position = [720.0, 0.0];
    let fill_id = fill.id;
    graph.nodes[old_output_index] = fill;
    let mut backplate = manager.create_decorator_operation_node(BACKPLATE_COMPONENT_ID)?;
    backplate.ui_position = [360.0, 0.0];
    let backplate_id = backplate.id;
    graph.nodes.push(backplate);
    graph.connections = vec![
        ProjectConnection::new(
            PortAddress::new(PortOwner::Node(text_id), SHAPE_OUTPUT_PORT),
            PortAddress::new(PortOwner::Node(backplate_id), SHAPE_INPUT_PORT),
            0,
        ),
        ProjectConnection::new(
            PortAddress::new(PortOwner::Node(backplate_id), SHAPE_OUTPUT_PORT),
            PortAddress::new(PortOwner::Node(fill_id), SHAPE_INPUT_PORT),
            0,
        ),
    ];
    graph.output_node_id = Some(fill_id);

    let (mut composition, track) = Composition::new("Runtime graph", 320, 180, 30.0, 1.0);
    composition.background_color = library::model::frame::color::Color {
        r: 0,
        g: 0,
        b: 0,
        a: 0,
    };
    let track_id = track.id;
    let mut project = Project::new("Runtime graph");
    project.add_track(track);
    project.add_composition(composition);
    let clip = Clip::new("Runtime graph", 0.0, 1.0);
    let clip_id = clip.id;
    project.add_clip(clip);
    project.attach_clip_to_track(track_id, clip_id)?;
    project.insert_node_graph(NodeContainer::Clip(clip_id), graph)?;

    let property_evaluators = manager.get_property_evaluators();
    let frame = get_frame_from_project(&project, 0, 0, 1.0, None, &property_evaluators, manager)?;
    let content = first_frame_content(&frame.items)
        .context("explicit runtime config graph produced no FrameItem")?;
    let FrameContent::Text {
        styles,
        ensemble: Some(ensemble),
        ..
    } = content
    else {
        bail!("runtime config graph did not preserve Text shape metadata")
    };
    if !styles.iter().any(|style| {
        style.id == fill_id
            && matches!(
                &style.style,
                DrawStyle::Fill {
                    color: library::model::frame::color::Color {
                        r: 255,
                        g: 128,
                        b: 32,
                        a: 255
                    },
                    offset: 2.0
                }
            )
    }) {
        bail!("runtime Fill callback config did not reach the FrameItem")
    }
    if !ensemble.decorator_configs.iter().any(|decorator| {
        matches!(
            decorator,
            DecoratorConfig::Backplate {
                target: BackplateTarget::Block,
                shape: BackplateShape::RoundedRect,
                padding: (4.0, 6.0, 4.0, 6.0),
                corner_radius: 3.0,
                ..
            }
        )
    }) {
        bail!("runtime Backplate callback config did not reach the FrameItem")
    }

    let renderer = SkiaRenderer::new(
        frame.width as u32,
        frame.height as u32,
        frame.background_color.clone(),
        false,
        None,
        None,
    )?;
    let mut render_service =
        RenderService::new(renderer, Arc::clone(manager), Arc::new(CacheManager::new()));
    let RenderOutput::Image(image) = render_service.render_from_frame_info(&frame)? else {
        bail!("CPU runtime graph proof unexpectedly returned a texture")
    };
    if !image
        .data
        .chunks_exact(4)
        .any(|pixel| pixel[3] > 0 && (pixel[0] > 0 || pixel[1] > 0 || pixel[2] > 0))
    {
        bail!("runtime config graph produced no visible rendered pixels")
    }
    Ok(())
}

fn first_frame_content(items: &[FrameItem]) -> Option<&FrameContent> {
    items.iter().find_map(|item| match item {
        FrameItem::Object(object) => Some(&object.content),
        FrameItem::Group(group) => first_frame_content(&group.items),
    })
}

fn verify_all_defaults(
    component: &library::plugin::native_plugin_api::ComponentDescriptorV1,
    node: &Node,
) -> anyhow::Result<()> {
    let descriptor_names = component
        .properties
        .iter()
        .map(|property| property.name.as_str())
        .collect::<BTreeSet<_>>();
    let node_names = node
        .properties
        .iter()
        .map(|(name, _)| name.as_str())
        .collect::<BTreeSet<_>>();
    if descriptor_names != node_names {
        bail!(
            "descriptor-backed {} factory did not materialize every property",
            component.id
        );
    }
    Ok(())
}

fn verify_hard_minimum(
    component: &library::plugin::native_plugin_api::ComponentDescriptorV1,
    property_name: &str,
) -> anyhow::Result<()> {
    let property = component
        .properties
        .iter()
        .find(|property| property.name == property_name)
        .with_context(|| format!("{} has no {property_name} property", component.id))?;
    match property.ui {
        library::plugin::native_plugin_api::PropertyUiV1::Float {
            min: 0.0,
            min_hard_limit: true,
            ..
        } => Ok(()),
        _ => bail!(
            "{}.{} must authoritatively declare its non-negative hard minimum",
            component.id,
            property_name
        ),
    }
}

fn verify_soft_minimum(
    component: &library::plugin::native_plugin_api::ComponentDescriptorV1,
    property_name: &str,
) -> anyhow::Result<()> {
    let property = component
        .properties
        .iter()
        .find(|property| property.name == property_name)
        .with_context(|| format!("{} has no {property_name} property", component.id))?;
    match property.ui {
        library::plugin::native_plugin_api::PropertyUiV1::Float {
            min_hard_limit: false,
            ..
        } => Ok(()),
        _ => bail!(
            "{}.{} must keep its negative range soft",
            component.id,
            property_name
        ),
    }
}

fn verify_unknown_property_is_safe(
    manager: &PluginManager,
    context: &EvaluationContext,
) -> anyhow::Result<()> {
    let unknown = Property {
        evaluator: "unavailable.vendor.property".to_string(),
        properties: HashMap::from([(
            "vendor_private_value".to_string(),
            PropertyValue::String("keep exactly".to_string()),
        )]),
    };
    let json = serde_json::to_string(&unknown)?;
    let preserved: Property = serde_json::from_str(&json)?;
    if preserved != unknown {
        bail!("serialization changed unavailable property configuration");
    }

    // The registry's pre-existing unknown-evaluator fail-safe must not panic;
    // importantly, the authoritative Property above stays untouched.
    let evaluators = manager.get_property_evaluators();
    let _safe_value = evaluators.evaluate(&preserved, 0.0, context);
    if manager
        .invoke_runtime_plugin(
            PROPERTY_CATEGORY,
            &preserved.evaluator,
            "property.evaluate.v1",
            serde_json::json!({}),
        )
        .is_ok()
    {
        bail!("unknown runtime component unexpectedly invoked a plugin");
    }
    Ok(())
}
