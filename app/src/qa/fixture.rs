use super::QA_PORT_ENV;
use library::editor::ProjectService;
use library::model::frame::color::Color;
use library::model::project::{
    PortAddress, PortOwner, IMAGE_INPUT_PORT, IMAGE_OUTPUT_PORT, MERGE_IMAGES_PORT,
    SHAPE_INPUT_PORT, SHAPE_OUTPUT_PORT, TIME_PORT,
};
use library::model::property::{Property, PropertyValue, Vec2};
#[cfg(test)]
use library::model::NodeContent;
use library::model::{Clip, Composition, Node, Project, Track};
use library::plugin::PluginManager;
use ordered_float::OrderedFloat;
use std::sync::{Arc, RwLock};
use uuid::Uuid;

mod audio;

use audio::audio_node;

mod transform_preview;
mod waveform;

#[cfg(test)]
use transform_preview::{
    E2E_AMBIGUOUS_CLIP_ID, E2E_AMBIGUOUS_FILL_A_ID, E2E_AMBIGUOUS_FILL_B_ID,
    E2E_AMBIGUOUS_MERGE_ID, E2E_AMBIGUOUS_SHAPE_A_ID, E2E_AMBIGUOUS_SHAPE_B_ID,
    E2E_AMBIGUOUS_TRANSFORM_A_ID, E2E_AMBIGUOUS_TRANSFORM_B_ID,
};

pub const QA_FIXTURE_ENV: &str = "RUVIE_QA_FIXTURE";
pub const NODE_EDITOR_E2E_FIXTURE: &str = "node_editor_e2e";
pub const TRANSFORM_PREVIEW_E2E_FIXTURE: &str = "transform_preview_e2e";
pub const AUDIO_WAVEFORM_E2E_FIXTURE: &str = "audio_waveform_e2e";

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
pub const E2E_AUDIO_A_ID: Uuid = Uuid::from_u128(0x405);
pub const E2E_AUDIO_B_ID: Uuid = Uuid::from_u128(0x406);
pub const E2E_AUDIO_ASSET_A_ID: Uuid = Uuid::from_u128(0x701);
pub const E2E_AUDIO_ASSET_B_ID: Uuid = Uuid::from_u128(0x702);
pub const E2E_EFFECTOR_TRANSFORM_ID: Uuid = Uuid::from_u128(0x501);
pub const E2E_EFFECTOR_OPACITY_ID: Uuid = Uuid::from_u128(0x502);
pub const E2E_DECORATOR_BACKPLATE_ID: Uuid = Uuid::from_u128(0x503);
pub const E2E_BLUR_EFFECT_ID: Uuid = Uuid::from_u128(0x504);
pub const E2E_TEXT_TRANSFORM_ID: Uuid = Uuid::from_u128(0x505);
pub const E2E_SHAPE_TRANSFORM_ID: Uuid = Uuid::from_u128(0x506);
pub const E2E_TEXT_FILL_ID: Uuid = Uuid::from_u128(0x601);
pub const E2E_SHAPE_FILL_ID: Uuid = Uuid::from_u128(0x602);
pub const E2E_SHAPE_STROKE_ID: Uuid = Uuid::from_u128(0x603);
pub const E2E_SHAPE_MERGE_ID: Uuid = Uuid::from_u128(0x604);

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
    if !matches!(
        name,
        NODE_EDITOR_E2E_FIXTURE | TRANSFORM_PREVIEW_E2E_FIXTURE | AUDIO_WAVEFORM_E2E_FIXTURE
    ) {
        return Err(format!("unknown {QA_FIXTURE_ENV} value {name:?}"));
    }
    let include_transform_ambiguity = name == TRANSFORM_PREVIEW_E2E_FIXTURE;
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
    if name == AUDIO_WAVEFORM_E2E_FIXTURE {
        return waveform::install(&mut project, &factory);
    }

    project.name = "RuViE QA E2E".to_string();

    let (mut composition, _) = Composition::new("QA Composition", 640, 360, 30.0, 20.0);
    composition.id = E2E_COMPOSITION_ID;
    composition.track_ids = vec![E2E_TRACK_A_ID, E2E_TRACK_B_ID];
    composition.ui_position = [0.0, 0.0];
    composition.ui_size = [3300.0, 1500.0];

    let mut track_a = Track::new("QA Track A");
    track_a.id = E2E_TRACK_A_ID;
    track_a.clip_ids = vec![E2E_CLIP_A1_ID, E2E_CLIP_A2_ID];
    track_a.ui_position = [120.0, 100.0];
    track_a.ui_size = [3050.0, 600.0];

    let mut track_b = Track::new("QA Track B");
    track_b.id = E2E_TRACK_B_ID;
    track_b.clip_ids = vec![E2E_CLIP_B1_ID];
    track_b.ui_position = [120.0, 780.0];
    track_b.ui_size = [1800.0, 520.0];

    let mut clip_a1 = Clip::new("QA Clip A1", 1.0, 4.0);
    clip_a1.id = E2E_CLIP_A1_ID;
    clip_a1.node_ids = vec![E2E_AUDIO_A_ID, E2E_AUDIO_B_ID, E2E_SOLID_ID, E2E_MERGE_ID];
    clip_a1.output_node_id = Some(E2E_MERGE_ID);
    clip_a1.ui_position = [2250.0, 180.0];
    clip_a1.ui_size = [750.0, 440.0];

    let mut clip_a2 = Clip::new("QA Clip A2", 1.0, 8.0);
    clip_a2.id = E2E_CLIP_A2_ID;
    clip_a2.node_ids = vec![
        E2E_AUX_A_ID,
        E2E_TEXT_TRANSFORM_ID,
        E2E_EFFECTOR_TRANSFORM_ID,
        E2E_EFFECTOR_OPACITY_ID,
        E2E_DECORATOR_BACKPLATE_ID,
        E2E_TEXT_FILL_ID,
        E2E_BLUR_EFFECT_ID,
    ];
    clip_a2.output_node_id = Some(E2E_BLUR_EFFECT_ID);
    clip_a2.ui_position = [250.0, 180.0];
    clip_a2.ui_size = [1900.0, 380.0];

    let mut clip_b1 = Clip::new("QA Clip B1", 1.0, 8.0);
    clip_b1.id = E2E_CLIP_B1_ID;
    clip_b1.node_ids = vec![
        E2E_AUX_B_ID,
        E2E_SHAPE_TRANSFORM_ID,
        E2E_SHAPE_FILL_ID,
        E2E_SHAPE_STROKE_ID,
        E2E_SHAPE_MERGE_ID,
    ];
    clip_b1.output_node_id = Some(E2E_SHAPE_MERGE_ID);
    clip_b1.ui_position = [250.0, 860.0];
    clip_b1.ui_size = [1600.0, 380.0];

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
        [2350.0, 390.0],
    )?;
    solid.set_property(
        "opacity".to_string(),
        Property::constant(PropertyValue::Number(OrderedFloat(100.0))),
    )?;
    let mut merge = Node::new_merge("QA Merge");
    merge.id = E2E_MERGE_ID;
    merge.ui_position = [2670.0, 390.0];
    let (audio_asset_a, audio_a) = audio_node(
        &factory,
        E2E_AUDIO_ASSET_A_ID,
        E2E_AUDIO_A_ID,
        "QA Audio A",
        "test_data/e2e_media/tone.mp3",
        [2320.0, 230.0],
    )?;
    let (audio_asset_b, audio_b) = audio_node(
        &factory,
        E2E_AUDIO_ASSET_B_ID,
        E2E_AUDIO_B_ID,
        "QA Audio B",
        "test_data/test_sound2.mp3",
        [2600.0, 230.0],
    )?;

    let text = text_node(&factory, E2E_AUX_A_ID, [350.0, 300.0])?;
    let text_transform = root_transform_node(
        plugin_manager,
        E2E_TEXT_TRANSFORM_ID,
        "QA Text Transform",
        [320.0, 180.0],
        [0.0, 0.0],
        [600.0, 300.0],
    )?;
    let mut transform = operation_node(
        plugin_manager.create_effector_operation_node("transform"),
        E2E_EFFECTOR_TRANSFORM_ID,
        "QA Transform Modulation",
        [850.0, 300.0],
    )?;
    for (name, value) in [
        ("tx", 0.0),
        ("ty", 0.0),
        ("scale_x", 1.0),
        ("scale_y", 1.0),
        ("rotation", 0.0),
    ] {
        transform.set_property(
            name.to_string(),
            Property::constant(PropertyValue::Number(OrderedFloat(value))),
        )?;
    }
    let mut opacity = operation_node(
        plugin_manager.create_effector_operation_node("opacity"),
        E2E_EFFECTOR_OPACITY_ID,
        "QA Opacity Modulation",
        [1100.0, 300.0],
    )?;
    opacity.set_property(
        "opacity".to_string(),
        Property::constant(PropertyValue::Number(OrderedFloat(100.0))),
    )?;
    let mut backplate = operation_node(
        plugin_manager.create_decorator_operation_node("backplate"),
        E2E_DECORATOR_BACKPLATE_ID,
        "QA Backplate",
        [1350.0, 300.0],
    )?;
    backplate.set_property(
        "target".to_string(),
        Property::constant(PropertyValue::String("Block".to_string())),
    )?;
    backplate.set_property(
        "shape".to_string(),
        Property::constant(PropertyValue::String("RoundRect".to_string())),
    )?;
    backplate.set_property(
        "color".to_string(),
        Property::constant(PropertyValue::Color(Color {
            r: 20,
            g: 20,
            b: 20,
            a: 210,
        })),
    )?;
    backplate.set_property(
        "padding".to_string(),
        Property::constant(PropertyValue::Number(OrderedFloat(8.0))),
    )?;
    backplate.set_property(
        "radius".to_string(),
        Property::constant(PropertyValue::Number(OrderedFloat(6.0))),
    )?;
    let mut text_fill = operation_node(
        plugin_manager.create_style_operation_node("fill"),
        E2E_TEXT_FILL_ID,
        "QA Text Fill",
        [1600.0, 300.0],
    )?;
    text_fill.set_property(
        "color".to_string(),
        Property::constant(PropertyValue::Color(Color {
            r: 250,
            g: 245,
            b: 90,
            a: 255,
        })),
    )?;
    text_fill.set_property(
        "opacity".to_string(),
        Property::constant(PropertyValue::Number(OrderedFloat(1.0))),
    )?;
    let blur = operation_node(
        plugin_manager.create_effect_operation_node("blur"),
        E2E_BLUR_EFFECT_ID,
        "QA Blur",
        [1850.0, 300.0],
    )?;

    let shape = shape_node(&factory, E2E_AUX_B_ID, [350.0, 980.0])?;
    let shape_transform = root_transform_node(
        plugin_manager,
        E2E_SHAPE_TRANSFORM_ID,
        "QA Shape Transform",
        [320.0, 180.0],
        [80.0, 45.0],
        [620.0, 980.0],
    )?;
    let mut shape_fill = operation_node(
        plugin_manager.create_style_operation_node("fill"),
        E2E_SHAPE_FILL_ID,
        "QA Shape Fill",
        [900.0, 900.0],
    )?;
    shape_fill.set_property(
        "color".to_string(),
        Property::constant(PropertyValue::Color(Color {
            r: 54,
            g: 209,
            b: 122,
            a: 255,
        })),
    )?;
    shape_fill.set_property(
        "opacity".to_string(),
        Property::constant(PropertyValue::Number(OrderedFloat(1.0))),
    )?;
    let mut shape_stroke = operation_node(
        plugin_manager.create_style_operation_node("stroke"),
        E2E_SHAPE_STROKE_ID,
        "QA Shape Stroke",
        [900.0, 1060.0],
    )?;
    shape_stroke.set_property(
        "color".to_string(),
        Property::constant(PropertyValue::Color(Color {
            r: 255,
            g: 255,
            b: 255,
            a: 255,
        })),
    )?;
    shape_stroke.set_property(
        "opacity".to_string(),
        Property::constant(PropertyValue::Number(OrderedFloat(1.0))),
    )?;
    shape_stroke.set_property(
        "width".to_string(),
        Property::constant(PropertyValue::Number(OrderedFloat(4.0))),
    )?;
    let mut shape_merge = Node::new_merge("QA Shape Merge");
    shape_merge.id = E2E_SHAPE_MERGE_ID;
    shape_merge.ui_position = [1250.0, 980.0];

    project.add_track(track_a);
    project.add_track(track_b);
    project.add_clip(clip_a1);
    project.add_clip(clip_a2);
    project.add_clip(clip_b1);
    project.assets.push(audio_asset_a);
    project.assets.push(audio_asset_b);
    project.add_node(audio_a);
    project.add_node(audio_b);
    project.add_node(solid);
    project.add_node(merge);
    project.add_node(text);
    project.add_node(text_transform);
    project.add_node(transform);
    project.add_node(opacity);
    project.add_node(backplate);
    project.add_node(text_fill);
    project.add_node(blur);
    project.add_node(shape);
    project.add_node(shape_transform);
    project.add_node(shape_fill);
    project.add_node(shape_stroke);
    project.add_node(shape_merge);
    project.add_composition(composition);

    for (source_owner, source_port, target_node, target_port) in [
        (
            PortOwner::Node(E2E_AUX_A_ID),
            SHAPE_OUTPUT_PORT,
            E2E_TEXT_TRANSFORM_ID,
            SHAPE_INPUT_PORT,
        ),
        (
            PortOwner::Node(E2E_TEXT_TRANSFORM_ID),
            SHAPE_OUTPUT_PORT,
            E2E_EFFECTOR_TRANSFORM_ID,
            SHAPE_INPUT_PORT,
        ),
        (
            PortOwner::Node(E2E_EFFECTOR_TRANSFORM_ID),
            SHAPE_OUTPUT_PORT,
            E2E_EFFECTOR_OPACITY_ID,
            SHAPE_INPUT_PORT,
        ),
        (
            PortOwner::Node(E2E_EFFECTOR_OPACITY_ID),
            SHAPE_OUTPUT_PORT,
            E2E_DECORATOR_BACKPLATE_ID,
            SHAPE_INPUT_PORT,
        ),
        (
            PortOwner::Node(E2E_DECORATOR_BACKPLATE_ID),
            SHAPE_OUTPUT_PORT,
            E2E_TEXT_FILL_ID,
            SHAPE_INPUT_PORT,
        ),
        (
            PortOwner::Node(E2E_TEXT_FILL_ID),
            IMAGE_OUTPUT_PORT,
            E2E_BLUR_EFFECT_ID,
            IMAGE_INPUT_PORT,
        ),
        (
            PortOwner::Node(E2E_AUX_B_ID),
            SHAPE_OUTPUT_PORT,
            E2E_SHAPE_TRANSFORM_ID,
            SHAPE_INPUT_PORT,
        ),
        (
            PortOwner::Node(E2E_SHAPE_TRANSFORM_ID),
            SHAPE_OUTPUT_PORT,
            E2E_SHAPE_FILL_ID,
            SHAPE_INPUT_PORT,
        ),
        (
            PortOwner::Node(E2E_SHAPE_TRANSFORM_ID),
            SHAPE_OUTPUT_PORT,
            E2E_SHAPE_STROKE_ID,
            SHAPE_INPUT_PORT,
        ),
        (
            PortOwner::Node(E2E_SHAPE_FILL_ID),
            IMAGE_OUTPUT_PORT,
            E2E_SHAPE_MERGE_ID,
            MERGE_IMAGES_PORT,
        ),
        (
            PortOwner::Node(E2E_SHAPE_STROKE_ID),
            IMAGE_OUTPUT_PORT,
            E2E_SHAPE_MERGE_ID,
            MERGE_IMAGES_PORT,
        ),
        (
            PortOwner::Node(E2E_SOLID_ID),
            IMAGE_OUTPUT_PORT,
            E2E_MERGE_ID,
            MERGE_IMAGES_PORT,
        ),
        (
            PortOwner::Clip(E2E_CLIP_A2_ID),
            IMAGE_OUTPUT_PORT,
            E2E_MERGE_ID,
            MERGE_IMAGES_PORT,
        ),
        (
            PortOwner::Clip(E2E_CLIP_B1_ID),
            IMAGE_OUTPUT_PORT,
            E2E_MERGE_ID,
            MERGE_IMAGES_PORT,
        ),
    ] {
        project
            .connect_ports(
                PortAddress::new(source_owner, source_port),
                PortAddress::new(PortOwner::Node(target_node), target_port),
            )
            .map_err(|error| format!("cannot connect QA content graph: {error}"))?;
    }
    for (container, node) in [
        (E2E_CLIP_A1_ID, E2E_AUDIO_A_ID),
        (E2E_CLIP_A1_ID, E2E_AUDIO_B_ID),
        (E2E_CLIP_A1_ID, E2E_SOLID_ID),
        (E2E_CLIP_A2_ID, E2E_AUX_A_ID),
        (E2E_CLIP_A2_ID, E2E_TEXT_TRANSFORM_ID),
        (E2E_CLIP_A2_ID, E2E_EFFECTOR_TRANSFORM_ID),
        (E2E_CLIP_A2_ID, E2E_EFFECTOR_OPACITY_ID),
        (E2E_CLIP_A2_ID, E2E_DECORATOR_BACKPLATE_ID),
        (E2E_CLIP_A2_ID, E2E_TEXT_FILL_ID),
        (E2E_CLIP_A2_ID, E2E_BLUR_EFFECT_ID),
        (E2E_CLIP_B1_ID, E2E_AUX_B_ID),
        (E2E_CLIP_B1_ID, E2E_SHAPE_TRANSFORM_ID),
        (E2E_CLIP_B1_ID, E2E_SHAPE_FILL_ID),
        (E2E_CLIP_B1_ID, E2E_SHAPE_STROKE_ID),
        (E2E_CLIP_B1_ID, E2E_SHAPE_MERGE_ID),
    ] {
        project
            .connect_ports(
                PortAddress::new(PortOwner::Clip(container), TIME_PORT),
                PortAddress::new(PortOwner::Node(node), TIME_PORT),
            )
            .map_err(|error| format!("cannot connect QA time metadata: {error}"))?;
    }

    if include_transform_ambiguity {
        transform_preview::install(&mut project, &factory, plugin_manager)?;
    }

    let connection_errors = project.validate_connections();
    if !connection_errors.is_empty() {
        return Err(format!(
            "QA fixture has invalid graph connections: {}",
            connection_errors
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
                .join("; ")
        ));
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
    node.set_property(
        "size".to_string(),
        Property::constant(PropertyValue::Number(OrderedFloat(64.0))),
    )?;
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
    Ok(node)
}

fn root_transform_node(
    plugin_manager: &PluginManager,
    id: Uuid,
    name: &str,
    position: [f64; 2],
    anchor: [f64; 2],
    ui_position: [f32; 2],
) -> Result<Node, String> {
    let mut node = operation_node(
        plugin_manager.create_shape_transform_operation_node(),
        id,
        name,
        ui_position,
    )?;
    for (key, value) in [
        (
            "position",
            PropertyValue::Vec2(Vec2 {
                x: OrderedFloat(position[0]),
                y: OrderedFloat(position[1]),
            }),
        ),
        (
            "anchor",
            PropertyValue::Vec2(Vec2 {
                x: OrderedFloat(anchor[0]),
                y: OrderedFloat(anchor[1]),
            }),
        ),
    ] {
        node.set_property(key.to_string(), Property::constant(value))?;
    }
    Ok(node)
}

fn operation_node<E: std::fmt::Display>(
    result: Result<Node, E>,
    id: Uuid,
    name: &str,
    ui_position: [f32; 2],
) -> Result<Node, String> {
    let mut node = result.map_err(|error| format!("cannot create QA {name}: {error}"))?;
    node.id = id;
    node.name = name.to_string();
    node.ui_position = ui_position;
    Ok(node)
}

#[cfg(test)]
mod tests {
    use super::*;
    use library::model::project::asset::AssetKind;

    fn installed_fixture() -> (Arc<RwLock<Project>>, Arc<PluginManager>, FixtureInfo) {
        let project = Arc::new(RwLock::new(Project::new("empty")));
        let plugin_manager = Arc::new(PluginManager::default());
        let info = install_named(&project, NODE_EDITOR_E2E_FIXTURE, &plugin_manager).unwrap();
        (project, plugin_manager, info)
    }

    fn installed_transform_fixture() -> (Arc<RwLock<Project>>, Arc<PluginManager>, FixtureInfo) {
        let project = Arc::new(RwLock::new(Project::new("empty")));
        let plugin_manager = Arc::new(PluginManager::default());
        let info = install_named(&project, TRANSFORM_PREVIEW_E2E_FIXTURE, &plugin_manager).unwrap();
        (project, plugin_manager, info)
    }

    fn assert_connection(
        project: &Project,
        from_owner: PortOwner,
        from_port: &str,
        to_owner: PortOwner,
        to_port: &str,
        order: i64,
    ) {
        let matching = project
            .connections
            .iter()
            .filter(|connection| {
                connection.from == PortAddress::new(from_owner, from_port)
                    && connection.to == PortAddress::new(to_owner, to_port)
            })
            .collect::<Vec<_>>();
        assert_eq!(matching.len(), 1, "missing or duplicate fixture wire");
        assert_eq!(matching[0].order, order);
    }

    fn assert_operation(
        project: &Project,
        plugin_manager: &PluginManager,
        node_id: Uuid,
        category: &str,
        component_id: &str,
    ) {
        let node = project.get_node(node_id).unwrap();
        let NodeContent::PluginOperation(operation) = node.content() else {
            panic!("{node_id} must be a PluginOperation Node");
        };
        assert_eq!(operation.category, category);
        assert_eq!(operation.component_id, component_id);
        let descriptor = plugin_manager
            .operation_descriptor(
                &operation.category,
                &operation.component_id,
                &operation.operation,
            )
            .unwrap();
        assert_eq!(
            operation.declared_ports.as_slice(),
            descriptor.declared_ports()
        );
        for definition in descriptor.properties() {
            assert!(
                node.properties().get(definition.name()).is_some(),
                "{} is missing {}",
                node.name,
                definition.name()
            );
        }
    }

    #[test]
    fn fixture_uses_explicit_operation_nodes_and_output_bindings() {
        let (project, plugin_manager, info) = installed_fixture();
        let read = project.read().unwrap();
        assert_eq!(info.composition_id, E2E_COMPOSITION_ID);
        let composition = &read.compositions[0];
        assert_eq!(composition.track_ids, info.expanded_tracks);
        assert!(composition.node_ids.is_empty());
        assert!(composition.output_node_id.is_none());
        assert_eq!(
            read.get_track(E2E_TRACK_A_ID).unwrap().clip_ids,
            vec![E2E_CLIP_A1_ID, E2E_CLIP_A2_ID]
        );
        assert_eq!(
            read.get_track(E2E_TRACK_B_ID).unwrap().clip_ids,
            vec![E2E_CLIP_B1_ID]
        );

        let clip_a1 = read.get_clip(E2E_CLIP_A1_ID).unwrap();
        assert_eq!(
            clip_a1.node_ids,
            vec![E2E_AUDIO_A_ID, E2E_AUDIO_B_ID, E2E_SOLID_ID, E2E_MERGE_ID,]
        );
        assert_eq!(clip_a1.output_node_id, Some(E2E_MERGE_ID));
        assert!(clip_a1.audio_output_node_id.is_none());
        let clip_a2 = read.get_clip(E2E_CLIP_A2_ID).unwrap();
        assert_eq!(
            clip_a2.node_ids,
            vec![
                E2E_AUX_A_ID,
                E2E_TEXT_TRANSFORM_ID,
                E2E_EFFECTOR_TRANSFORM_ID,
                E2E_EFFECTOR_OPACITY_ID,
                E2E_DECORATOR_BACKPLATE_ID,
                E2E_TEXT_FILL_ID,
                E2E_BLUR_EFFECT_ID,
            ]
        );
        assert_eq!(clip_a2.output_node_id, Some(E2E_BLUR_EFFECT_ID));
        let clip_b1 = read.get_clip(E2E_CLIP_B1_ID).unwrap();
        assert_eq!(
            clip_b1.node_ids,
            vec![
                E2E_AUX_B_ID,
                E2E_SHAPE_TRANSFORM_ID,
                E2E_SHAPE_FILL_ID,
                E2E_SHAPE_STROKE_ID,
                E2E_SHAPE_MERGE_ID,
            ]
        );
        assert_eq!(clip_b1.output_node_id, Some(E2E_SHAPE_MERGE_ID));

        for (node_id, asset_id) in [
            (E2E_AUDIO_A_ID, E2E_AUDIO_ASSET_A_ID),
            (E2E_AUDIO_B_ID, E2E_AUDIO_ASSET_B_ID),
        ] {
            let NodeContent::Media(media) = read.get_node(node_id).unwrap().content() else {
                panic!("{node_id} must be a Media Node");
            };
            assert_eq!(media.asset_id, asset_id);
            assert_eq!(
                read.assets
                    .iter()
                    .find(|asset| asset.id == asset_id)
                    .unwrap()
                    .kind,
                AssetKind::Audio
            );
        }

        let text = read.get_node(E2E_AUX_A_ID).unwrap();
        assert!(matches!(
            text.content(),
            NodeContent::Generator(library::model::GeneratorContent::Text)
        ));
        assert!(matches!(
            read.get_node(E2E_AUX_B_ID).unwrap().content(),
            NodeContent::Generator(library::model::GeneratorContent::Shape)
        ));
        for content_id in [E2E_AUX_A_ID, E2E_AUX_B_ID] {
            let content = read.get_node(content_id).unwrap();
            for property in ["position", "rotation", "scale", "anchor", "opacity"] {
                assert!(
                    content.properties().get(property).is_none(),
                    "{} must not duplicate {property} ownership",
                    content.name
                );
            }
        }
        for transform_id in [E2E_TEXT_TRANSFORM_ID, E2E_SHAPE_TRANSFORM_ID] {
            let transform = read.get_node(transform_id).unwrap();
            for property in ["position", "rotation", "scale", "anchor"] {
                assert!(
                    transform.properties().get(property).is_some(),
                    "{} must own {property}",
                    transform.name
                );
            }
        }
        for style_id in [E2E_TEXT_FILL_ID, E2E_SHAPE_FILL_ID, E2E_SHAPE_STROKE_ID] {
            let style = read.get_node(style_id).unwrap();
            assert!(
                style.properties().get("opacity").is_some(),
                "{} must own opacity",
                style.name
            );
        }

        for (node_id, category, component_id) in [
            (E2E_TEXT_TRANSFORM_ID, "transform", "transform"),
            (E2E_SHAPE_TRANSFORM_ID, "transform", "transform"),
            (E2E_EFFECTOR_TRANSFORM_ID, "effector", "transform"),
            (E2E_EFFECTOR_OPACITY_ID, "effector", "opacity"),
            (E2E_DECORATOR_BACKPLATE_ID, "decorator", "backplate"),
            (E2E_BLUR_EFFECT_ID, "effect", "blur"),
            (E2E_TEXT_FILL_ID, "style", "fill"),
            (E2E_SHAPE_FILL_ID, "style", "fill"),
            (E2E_SHAPE_STROKE_ID, "style", "stroke"),
        ] {
            assert_operation(&read, &plugin_manager, node_id, category, component_id);
        }

        assert_eq!(read.nodes.len(), 16);
        for track in read.tracks.values() {
            assert!(track.output_node_id.is_none());
            assert!(track.node_ids.is_empty());
        }
        assert!(read.validate_connections().is_empty());
        assert!(read.validate_containment().is_empty());
        drop(read);
        assert!(install_named(&project, NODE_EDITOR_E2E_FIXTURE, &plugin_manager).is_err());
    }

    #[test]
    fn fixture_wires_shape_and_image_flow_with_stable_merge_order() {
        let (project, _plugin_manager, _info) = installed_fixture();
        let read = project.read().unwrap();

        for (from_node, to_node) in [
            (E2E_AUX_A_ID, E2E_TEXT_TRANSFORM_ID),
            (E2E_TEXT_TRANSFORM_ID, E2E_EFFECTOR_TRANSFORM_ID),
            (E2E_EFFECTOR_TRANSFORM_ID, E2E_EFFECTOR_OPACITY_ID),
            (E2E_EFFECTOR_OPACITY_ID, E2E_DECORATOR_BACKPLATE_ID),
            (E2E_DECORATOR_BACKPLATE_ID, E2E_TEXT_FILL_ID),
            (E2E_AUX_B_ID, E2E_SHAPE_TRANSFORM_ID),
            (E2E_SHAPE_TRANSFORM_ID, E2E_SHAPE_FILL_ID),
            (E2E_SHAPE_TRANSFORM_ID, E2E_SHAPE_STROKE_ID),
        ] {
            assert_connection(
                &read,
                PortOwner::Node(from_node),
                SHAPE_OUTPUT_PORT,
                PortOwner::Node(to_node),
                SHAPE_INPUT_PORT,
                0,
            );
        }
        assert_connection(
            &read,
            PortOwner::Node(E2E_TEXT_FILL_ID),
            IMAGE_OUTPUT_PORT,
            PortOwner::Node(E2E_BLUR_EFFECT_ID),
            IMAGE_INPUT_PORT,
            0,
        );
        for (source_node, order) in [(E2E_SHAPE_FILL_ID, 0), (E2E_SHAPE_STROKE_ID, 1)] {
            assert_connection(
                &read,
                PortOwner::Node(source_node),
                IMAGE_OUTPUT_PORT,
                PortOwner::Node(E2E_SHAPE_MERGE_ID),
                MERGE_IMAGES_PORT,
                order,
            );
        }
        for (source_owner, order) in [
            (PortOwner::Node(E2E_SOLID_ID), 0),
            (PortOwner::Clip(E2E_CLIP_A2_ID), 1),
            (PortOwner::Clip(E2E_CLIP_B1_ID), 2),
        ] {
            assert_connection(
                &read,
                source_owner,
                IMAGE_OUTPUT_PORT,
                PortOwner::Node(E2E_MERGE_ID),
                MERGE_IMAGES_PORT,
                order,
            );
        }

        for (clip_id, node_id) in [
            (E2E_CLIP_A1_ID, E2E_AUDIO_A_ID),
            (E2E_CLIP_A1_ID, E2E_AUDIO_B_ID),
            (E2E_CLIP_A1_ID, E2E_SOLID_ID),
            (E2E_CLIP_A2_ID, E2E_AUX_A_ID),
            (E2E_CLIP_A2_ID, E2E_TEXT_TRANSFORM_ID),
            (E2E_CLIP_A2_ID, E2E_EFFECTOR_TRANSFORM_ID),
            (E2E_CLIP_A2_ID, E2E_EFFECTOR_OPACITY_ID),
            (E2E_CLIP_A2_ID, E2E_DECORATOR_BACKPLATE_ID),
            (E2E_CLIP_A2_ID, E2E_TEXT_FILL_ID),
            (E2E_CLIP_A2_ID, E2E_BLUR_EFFECT_ID),
            (E2E_CLIP_B1_ID, E2E_AUX_B_ID),
            (E2E_CLIP_B1_ID, E2E_SHAPE_TRANSFORM_ID),
            (E2E_CLIP_B1_ID, E2E_SHAPE_FILL_ID),
            (E2E_CLIP_B1_ID, E2E_SHAPE_STROKE_ID),
            (E2E_CLIP_B1_ID, E2E_SHAPE_MERGE_ID),
        ] {
            assert_connection(
                &read,
                PortOwner::Clip(clip_id),
                TIME_PORT,
                PortOwner::Node(node_id),
                TIME_PORT,
                0,
            );
        }

        assert!(!read.connections.iter().any(|connection| {
            connection.to == PortAddress::new(PortOwner::Node(E2E_MERGE_ID), TIME_PORT)
        }));
        assert_eq!(read.connections.len(), 29);
        assert!(read.validate_connections().is_empty());
    }

    #[test]
    fn transform_preview_fixture_has_two_independent_clip_spatial_roots() {
        let (project, plugin_manager, _info) = installed_transform_fixture();
        let read = project.read().unwrap();
        assert_eq!(
            read.get_track(E2E_TRACK_B_ID).unwrap().clip_ids,
            vec![E2E_CLIP_B1_ID, E2E_AMBIGUOUS_CLIP_ID]
        );
        let clip = read.get_clip(E2E_AMBIGUOUS_CLIP_ID).unwrap();
        assert_eq!(
            clip.node_ids,
            vec![
                E2E_AMBIGUOUS_SHAPE_A_ID,
                E2E_AMBIGUOUS_TRANSFORM_A_ID,
                E2E_AMBIGUOUS_FILL_A_ID,
                E2E_AMBIGUOUS_SHAPE_B_ID,
                E2E_AMBIGUOUS_TRANSFORM_B_ID,
                E2E_AMBIGUOUS_FILL_B_ID,
                E2E_AMBIGUOUS_MERGE_ID,
            ]
        );
        assert_eq!(clip.output_node_id, Some(E2E_AMBIGUOUS_MERGE_ID));
        for transform_id in [E2E_AMBIGUOUS_TRANSFORM_A_ID, E2E_AMBIGUOUS_TRANSFORM_B_ID] {
            assert_operation(
                &read,
                &plugin_manager,
                transform_id,
                "transform",
                "transform",
            );
        }
        for (shape, transform, fill) in [
            (
                E2E_AMBIGUOUS_SHAPE_A_ID,
                E2E_AMBIGUOUS_TRANSFORM_A_ID,
                E2E_AMBIGUOUS_FILL_A_ID,
            ),
            (
                E2E_AMBIGUOUS_SHAPE_B_ID,
                E2E_AMBIGUOUS_TRANSFORM_B_ID,
                E2E_AMBIGUOUS_FILL_B_ID,
            ),
        ] {
            assert_connection(
                &read,
                PortOwner::Node(shape),
                SHAPE_OUTPUT_PORT,
                PortOwner::Node(transform),
                SHAPE_INPUT_PORT,
                0,
            );
            assert_connection(
                &read,
                PortOwner::Node(transform),
                SHAPE_OUTPUT_PORT,
                PortOwner::Node(fill),
                SHAPE_INPUT_PORT,
                0,
            );
            assert_connection(
                &read,
                PortOwner::Node(fill),
                IMAGE_OUTPUT_PORT,
                PortOwner::Node(E2E_AMBIGUOUS_MERGE_ID),
                MERGE_IMAGES_PORT,
                if fill == E2E_AMBIGUOUS_FILL_A_ID {
                    0
                } else {
                    1
                },
            );
        }
        assert_connection(
            &read,
            PortOwner::Clip(E2E_AMBIGUOUS_CLIP_ID),
            IMAGE_OUTPUT_PORT,
            PortOwner::Node(E2E_MERGE_ID),
            MERGE_IMAGES_PORT,
            3,
        );
        assert!(read.validate_connections().is_empty());
        assert!(read.validate_containment().is_empty());
    }
}
