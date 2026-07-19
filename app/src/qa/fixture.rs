use super::QA_PORT_ENV;
use library::editor::ProjectService;
use library::model::ensemble::{DecoratorInstance, EffectorInstance};
use library::model::frame::color::Color;
use library::model::project::{
    PortAddress, PortOwner, IMAGE_OUTPUT_PORT, MERGE_IMAGES_PORT, TIME_PORT,
};
use library::model::property::{Property, PropertyMap, PropertyValue, Vec2};
use library::model::{Clip, Composition, Node, NodeContent, Project, Track};
use library::plugin::PluginManager;
use ordered_float::OrderedFloat;
use std::sync::{Arc, RwLock};
use uuid::Uuid;

pub const QA_FIXTURE_ENV: &str = "RUVIE_QA_FIXTURE";
pub const NODE_EDITOR_E2E_FIXTURE: &str = "node_editor_e2e";

pub const E2E_COMPOSITION_ID: Uuid = Uuid::from_u128(0x100);
pub const E2E_TRACK_A_ID: Uuid = Uuid::from_u128(0x201);
pub const E2E_TRACK_B_ID: Uuid = Uuid::from_u128(0x202);
pub const E2E_CLIP_A1_ID: Uuid = Uuid::from_u128(0x301);
pub const E2E_CLIP_A2_ID: Uuid = Uuid::from_u128(0x302);
pub const E2E_CLIP_B1_ID: Uuid = Uuid::from_u128(0x303);
pub const E2E_SOLID_ID: Uuid = Uuid::from_u128(0x401);
pub const E2E_MERGE_ID: Uuid = Uuid::from_u128(0x402);
pub const E2E_AUX_A_ID: Uuid = Uuid::from_u128(0x403);
pub const E2E_AUX_B_ID: Uuid = Uuid::from_u128(0x404);
pub const E2E_EFFECTOR_TRANSFORM_ID: Uuid = Uuid::from_u128(0x501);
pub const E2E_EFFECTOR_OPACITY_ID: Uuid = Uuid::from_u128(0x502);
pub const E2E_DECORATOR_BACKPLATE_ID: Uuid = Uuid::from_u128(0x503);
pub const E2E_BLUR_EFFECT_ID: Uuid = Uuid::from_u128(0x504);
pub const E2E_TEXT_FILL_ID: Uuid = Uuid::from_u128(0x601);
pub const E2E_SHAPE_FILL_ID: Uuid = Uuid::from_u128(0x602);
pub const E2E_SHAPE_STROKE_ID: Uuid = Uuid::from_u128(0x603);

#[derive(Clone, Debug)]
pub struct FixtureInfo {
    pub composition_id: Uuid,
    pub expanded_tracks: Vec<Uuid>,
}

/// Install a deterministic fixture only when both the loopback QA bridge and
/// an explicit fixture name are enabled.  The supplied Arc is the same shared
/// Project later used by EditorService and every editor view.
pub fn install_from_env(
    project: &Arc<RwLock<Project>>,
    plugin_manager: &Arc<PluginManager>,
) -> Result<Option<FixtureInfo>, String> {
    if std::env::var_os(QA_PORT_ENV).is_none() {
        return Ok(None);
    }
    let name = match std::env::var(QA_FIXTURE_ENV) {
        Ok(name) => name,
        Err(std::env::VarError::NotPresent) => return Ok(None),
        Err(std::env::VarError::NotUnicode(_)) => {
            return Err(format!("{QA_FIXTURE_ENV} is not valid Unicode"));
        }
    };
    install_named(project, &name, plugin_manager).map(Some)
}

fn install_named(
    project: &Arc<RwLock<Project>>,
    name: &str,
    plugin_manager: &Arc<PluginManager>,
) -> Result<FixtureInfo, String> {
    if name != NODE_EDITOR_E2E_FIXTURE {
        return Err(format!("unknown {QA_FIXTURE_ENV} value {name:?}"));
    }
    let factory = ProjectService::new(Arc::clone(project), Arc::clone(plugin_manager));
    let mut project = project
        .write()
        .map_err(|error| format!("cannot install QA fixture: Project lock is poisoned: {error}"))?;
    if !project.compositions.is_empty()
        || !project.tracks.is_empty()
        || !project.clips.is_empty()
        || !project.nodes.is_empty()
    {
        return Err("QA fixture requires an empty shared Project".to_string());
    }

    project.name = "RuViE QA E2E".to_string();

    let (mut composition, _) = Composition::new("QA Composition", 640, 360, 30.0, 20.0);
    composition.id = E2E_COMPOSITION_ID;
    composition.track_ids = vec![E2E_TRACK_A_ID, E2E_TRACK_B_ID];
    composition.ui_position = [0.0, 0.0];
    composition.ui_size = [1320.0, 1000.0];

    let mut track_a = Track::new("QA Track A");
    track_a.id = E2E_TRACK_A_ID;
    track_a.clip_ids = vec![E2E_CLIP_A1_ID, E2E_CLIP_A2_ID];
    track_a.ui_position = [170.0, 130.0];
    track_a.ui_size = [980.0, 390.0];

    let mut track_b = Track::new("QA Track B");
    track_b.id = E2E_TRACK_B_ID;
    track_b.clip_ids = vec![E2E_CLIP_B1_ID];
    track_b.ui_position = [170.0, 590.0];
    track_b.ui_size = [980.0, 310.0];

    let mut clip_a1 = Clip::new("QA Clip A1", 1.0, 4.0);
    clip_a1.id = E2E_CLIP_A1_ID;
    clip_a1.node_ids = vec![E2E_SOLID_ID, E2E_MERGE_ID];
    clip_a1.output_node_id = Some(E2E_MERGE_ID);
    clip_a1.ui_position = [300.0, 215.0];
    clip_a1.ui_size = [700.0, 250.0];

    let mut clip_a2 = Clip::new("QA Clip A2", 1.0, 8.0);
    clip_a2.id = E2E_CLIP_A2_ID;
    clip_a2.node_ids = vec![E2E_AUX_A_ID];
    clip_a2.output_node_id = Some(E2E_AUX_A_ID);
    clip_a2.ui_position = [770.0, 215.0];
    clip_a2.ui_size = [300.0, 250.0];

    let mut clip_b1 = Clip::new("QA Clip B1", 1.0, 8.0);
    clip_b1.id = E2E_CLIP_B1_ID;
    clip_b1.node_ids = vec![E2E_AUX_B_ID];
    clip_b1.output_node_id = Some(E2E_AUX_B_ID);
    clip_b1.ui_position = [300.0, 665.0];
    clip_b1.ui_size = [600.0, 220.0];

    let mut solid = solid_node(
        &factory,
        E2E_SOLID_ID,
        "QA Solid",
        Color {
            r: 240,
            g: 40,
            b: 40,
            a: 255,
        },
        [480.0, 310.0],
    )?;
    solid.properties.set(
        "opacity".to_string(),
        Property::constant(PropertyValue::Number(OrderedFloat(100.0))),
    );
    let merge = Node {
        id: E2E_MERGE_ID,
        name: "QA Merge".to_string(),
        content: NodeContent::Merge,
        blend_mode: Default::default(),
        properties: Default::default(),
        styles: Vec::new(),
        effects: Vec::new(),
        effectors: Vec::new(),
        decorators: Vec::new(),
        ui_position: [760.0, 310.0],
    };
    let text = text_node(&factory, E2E_AUX_A_ID, [860.0, 310.0])?;
    let shape = shape_node(&factory, E2E_AUX_B_ID, [500.0, 760.0])?;

    project.add_track(track_a);
    project.add_track(track_b);
    project.add_clip(clip_a1);
    project.add_clip(clip_a2);
    project.add_clip(clip_b1);
    project.add_node(solid);
    project.add_node(merge);
    project.add_node(text);
    project.add_node(shape);
    project.add_composition(composition);

    for source in [E2E_SOLID_ID, E2E_AUX_A_ID, E2E_AUX_B_ID] {
        project
            .connect_ports(
                PortAddress::new(PortOwner::Node(source), IMAGE_OUTPUT_PORT),
                PortAddress::new(PortOwner::Node(E2E_MERGE_ID), MERGE_IMAGES_PORT),
            )
            .map_err(|error| format!("cannot connect QA image graph: {error}"))?;
    }
    for (container, node) in [
        (E2E_CLIP_A1_ID, E2E_SOLID_ID),
        (E2E_CLIP_A1_ID, E2E_MERGE_ID),
        (E2E_CLIP_A2_ID, E2E_AUX_A_ID),
        (E2E_CLIP_B1_ID, E2E_AUX_B_ID),
    ] {
        project
            .connect_ports(
                PortAddress::new(PortOwner::Clip(container), TIME_PORT),
                PortAddress::new(PortOwner::Node(node), TIME_PORT),
            )
            .map_err(|error| format!("cannot connect QA time metadata: {error}"))?;
    }

    drop(project);
    Ok(FixtureInfo {
        composition_id: E2E_COMPOSITION_ID,
        expanded_tracks: vec![E2E_TRACK_A_ID, E2E_TRACK_B_ID],
    })
}

fn solid_node(
    factory: &ProjectService,
    id: Uuid,
    name: &str,
    color: Color,
    ui_position: [f32; 2],
) -> Result<Node, String> {
    let mut node = factory
        .create_solid_node(color, 640, 360)
        .map_err(|error| format!("cannot create QA Solid through factory: {error}"))?;
    node.id = id;
    node.name = name.to_string();
    node.ui_position = ui_position;
    Ok(node)
}

fn text_node(factory: &ProjectService, id: Uuid, ui_position: [f32; 2]) -> Result<Node, String> {
    let mut node = factory
        .create_text_node("QA Text", "Arial", 640, 360)
        .map_err(|error| format!("cannot create QA Text through factory: {error}"))?;
    node.id = id;
    node.name = "QA Text".to_string();
    node.ui_position = ui_position;
    node.properties.set(
        "position".to_string(),
        Property::constant(PropertyValue::Vec2(Vec2 {
            x: OrderedFloat(320.0),
            y: OrderedFloat(180.0),
        })),
    );
    node.properties.set(
        "opacity".to_string(),
        Property::constant(PropertyValue::Number(OrderedFloat(100.0))),
    );
    node.properties.set(
        "size".to_string(),
        Property::constant(PropertyValue::Number(OrderedFloat(64.0))),
    );
    let fill = node
        .styles
        .iter_mut()
        .find(|style| style.style_type == "fill")
        .ok_or_else(|| "QA Text factory did not materialize a fill style".to_string())?;
    fill.id = E2E_TEXT_FILL_ID;
    fill.properties.set(
        "color".to_string(),
        Property::constant(PropertyValue::Color(Color {
            r: 250,
            g: 245,
            b: 90,
            a: 255,
        })),
    );

    let plugin_manager = factory.get_plugin_manager();
    let mut transform_properties =
        PropertyMap::from_definitions(&plugin_manager.get_effector_properties("transform"));
    for (name, value) in [
        ("tx", 0.0),
        ("ty", 0.0),
        ("scale_x", 1.0),
        ("scale_y", 1.0),
        ("rotation", 0.0),
    ] {
        transform_properties.set(
            name.to_string(),
            Property::constant(PropertyValue::Number(OrderedFloat(value))),
        );
    }
    let mut transform = EffectorInstance::new("transform", transform_properties);
    transform.id = E2E_EFFECTOR_TRANSFORM_ID;

    let mut opacity_properties =
        PropertyMap::from_definitions(&plugin_manager.get_effector_properties("opacity"));
    opacity_properties.set(
        "opacity".to_string(),
        Property::constant(PropertyValue::Number(OrderedFloat(100.0))),
    );
    let mut opacity = EffectorInstance::new("opacity", opacity_properties);
    opacity.id = E2E_EFFECTOR_OPACITY_ID;
    node.effectors = vec![transform, opacity];

    let mut decorator_properties =
        PropertyMap::from_definitions(&plugin_manager.get_decorator_properties("backplate"));
    decorator_properties.set(
        "target".to_string(),
        Property::constant(PropertyValue::String("Block".to_string())),
    );
    decorator_properties.set(
        "shape".to_string(),
        Property::constant(PropertyValue::String("RoundRect".to_string())),
    );
    decorator_properties.set(
        "color".to_string(),
        Property::constant(PropertyValue::Color(Color {
            r: 20,
            g: 20,
            b: 20,
            a: 210,
        })),
    );
    decorator_properties.set(
        "padding".to_string(),
        Property::constant(PropertyValue::Number(OrderedFloat(8.0))),
    );
    decorator_properties.set(
        "radius".to_string(),
        Property::constant(PropertyValue::Number(OrderedFloat(6.0))),
    );
    let mut backplate = DecoratorInstance::new("backplate", decorator_properties);
    backplate.id = E2E_DECORATOR_BACKPLATE_ID;
    node.decorators = vec![backplate];

    let mut blur = plugin_manager
        .get_default_effect_config("blur")
        .ok_or_else(|| "QA fixture could not materialize the blur Effect defaults".to_string())?;
    blur.id = E2E_BLUR_EFFECT_ID;
    node.effects = vec![blur];
    Ok(node)
}

fn shape_node(factory: &ProjectService, id: Uuid, ui_position: [f32; 2]) -> Result<Node, String> {
    let path = "M 0 0 H 160 V 90 H 0 Z".to_string();
    let mut node = factory
        .create_shape_node(&path, 640, 360, 160, 90)
        .map_err(|error| format!("cannot create QA Shape through factory: {error}"))?;
    node.id = id;
    node.name = "QA Shape".to_string();
    node.ui_position = ui_position;
    node.properties.set(
        "opacity".to_string(),
        Property::constant(PropertyValue::Number(OrderedFloat(100.0))),
    );
    let fill = node
        .styles
        .iter_mut()
        .find(|style| style.style_type == "fill")
        .ok_or_else(|| "QA Shape factory did not materialize a fill style".to_string())?;
    fill.id = E2E_SHAPE_FILL_ID;
    fill.properties.set(
        "color".to_string(),
        Property::constant(PropertyValue::Color(Color {
            r: 54,
            g: 209,
            b: 122,
            a: 255,
        })),
    );
    let stroke = node
        .styles
        .iter_mut()
        .find(|style| style.style_type == "stroke")
        .ok_or_else(|| "QA Shape factory did not materialize a stroke style".to_string())?;
    stroke.id = E2E_SHAPE_STROKE_ID;
    stroke.properties.set(
        "color".to_string(),
        Property::constant(PropertyValue::Color(Color {
            r: 255,
            g: 255,
            b: 255,
            a: 255,
        })),
    );
    stroke.properties.set(
        "width".to_string(),
        Property::constant(PropertyValue::Number(OrderedFloat(4.0))),
    );
    Ok(node)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fixture_populates_the_supplied_shared_project_once() {
        let project = Arc::new(RwLock::new(Project::new("empty")));
        let plugin_manager = Arc::new(PluginManager::default());
        let info = install_named(&project, NODE_EDITOR_E2E_FIXTURE, &plugin_manager).unwrap();
        let read = project.read().unwrap();
        assert_eq!(info.composition_id, E2E_COMPOSITION_ID);
        assert_eq!(read.compositions[0].track_ids, info.expanded_tracks);
        assert_eq!(read.get_clip(E2E_CLIP_A1_ID).unwrap().node_ids.len(), 2);
        let text = read.get_node(E2E_AUX_A_ID).unwrap();
        assert!(matches!(
            text.content,
            NodeContent::Generator(library::model::GeneratorContent::Text)
        ));
        assert_eq!(text.effectors.len(), 2);
        assert_eq!(text.decorators.len(), 1);
        assert_eq!(text.effects.len(), 1);
        assert_eq!(text.effects[0].id, E2E_BLUR_EFFECT_ID);
        assert_eq!(text.styles.len(), 1);
        for (kind, properties, definitions) in [
            (
                "transform Effector",
                &text.effectors[0].properties,
                plugin_manager.get_effector_properties("transform"),
            ),
            (
                "opacity Effector",
                &text.effectors[1].properties,
                plugin_manager.get_effector_properties("opacity"),
            ),
            (
                "backplate Decorator",
                &text.decorators[0].properties,
                plugin_manager.get_decorator_properties("backplate"),
            ),
            (
                "blur Effect",
                &text.effects[0].properties,
                plugin_manager.get_effect_properties("blur"),
            ),
        ] {
            for definition in definitions {
                assert!(
                    properties.get(definition.name()).is_some(),
                    "{kind} is missing {}",
                    definition.name()
                );
            }
        }
        assert!(matches!(
            read.get_node(E2E_AUX_B_ID).unwrap().content,
            NodeContent::Generator(library::model::GeneratorContent::Shape)
        ));
        assert_eq!(read.get_node(E2E_AUX_B_ID).unwrap().styles.len(), 2);
        assert_eq!(read.connections.len(), 7);
        assert!(read.validate_connections().is_empty());
        drop(read);
        assert!(install_named(&project, NODE_EDITOR_E2E_FIXTURE, &plugin_manager).is_err());
    }
}
