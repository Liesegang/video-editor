use super::QA_PORT_ENV;
use library::editor::ProjectService;
#[cfg(test)]
use library::model::NodeContent;
use library::model::frame::color::Color;
use library::model::project::{
    BACKGROUND_SHAPE_INPUT_PORT, IMAGE_INPUT_PORT, IMAGE_OUTPUT_PORT, MERGE_IMAGES_PORT,
    PortAddress, PortOwner, SHAPE_INPUT_PORT, SHAPE_OUTPUT_PORT, TIME_PORT,
};
use library::model::property::{Property, PropertyValue};
use library::model::{Clip, Composition, Node, Project, Track};
use library::plugin::PluginManager;
use ordered_float::OrderedFloat;
use std::sync::{Arc, RwLock};
use uuid::Uuid;

mod audio;

use audio::audio_node;

mod color_operations;
mod composition_drop;
mod nodes;
mod transform_preview;
mod waveform;

use nodes::{operation_node, root_transform_node};

#[cfg(test)]
use transform_preview::{
    E2E_AMBIGUOUS_CLIP_ID, E2E_AMBIGUOUS_FILL_A_ID, E2E_AMBIGUOUS_FILL_B_ID,
    E2E_AMBIGUOUS_MERGE_ID, E2E_AMBIGUOUS_SHAPE_A_ID, E2E_AMBIGUOUS_SHAPE_B_ID,
    E2E_AMBIGUOUS_TRANSFORM_A_ID, E2E_AMBIGUOUS_TRANSFORM_B_ID,
};

pub const QA_FIXTURE_ENV: &str = "RUVIE_QA_FIXTURE";
pub const NODE_EDITOR_E2E_FIXTURE: &str = "node_editor_e2e";
pub const NODE_INSPECTOR_E2E_FIXTURE: &str = "node_inspector_e2e";
pub const TRANSFORM_PREVIEW_E2E_FIXTURE: &str = "transform_preview_e2e";
pub const AUDIO_WAVEFORM_E2E_FIXTURE: &str = "audio_waveform_e2e";
pub const COMPOSITION_DROP_E2E_FIXTURE: &str = "composition_drop_e2e";
pub const COLOR_OPERATIONS_E2E_FIXTURE: &str = "color_operations_e2e";

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
pub const E2E_BACKPLATE_SHAPE_ID: Uuid = Uuid::from_u128(0x407);
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
pub const E2E_BACKPLATE_FILL_ID: Uuid = Uuid::from_u128(0x605);
pub const E2E_TEXT_MERGE_ID: Uuid = Uuid::from_u128(0x606);
pub const E2E_INSPECTOR_VECTOR_ID: Uuid = Uuid::from_u128(0x608);

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
        NODE_EDITOR_E2E_FIXTURE
            | NODE_INSPECTOR_E2E_FIXTURE
            | TRANSFORM_PREVIEW_E2E_FIXTURE
            | AUDIO_WAVEFORM_E2E_FIXTURE
            | COMPOSITION_DROP_E2E_FIXTURE
            | COLOR_OPERATIONS_E2E_FIXTURE
    ) {
        return Err(format!("unknown {QA_FIXTURE_ENV} value {name:?}"));
    }
    let include_inspector_probe = name == NODE_INSPECTOR_E2E_FIXTURE;
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
    if name == COMPOSITION_DROP_E2E_FIXTURE {
        return composition_drop::install(&mut project);
    }
    if name == COLOR_OPERATIONS_E2E_FIXTURE {
        return color_operations::install(&mut project);
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
        E2E_BACKPLATE_SHAPE_ID,
        E2E_DECORATOR_BACKPLATE_ID,
        E2E_TEXT_FILL_ID,
        E2E_BACKPLATE_FILL_ID,
        E2E_TEXT_MERGE_ID,
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

    let solid = solid_node(
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
        "padding".to_string(),
        Property::constant(PropertyValue::Number(OrderedFloat(8.0))),
    )?;
    let mut backplate_shape = shape_node(&factory, E2E_BACKPLATE_SHAPE_ID, [1100.0, 500.0])?;
    backplate_shape.name = "QA Backplate Shape".to_string();
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
    let mut backplate_fill = operation_node(
        plugin_manager.create_style_operation_node("fill"),
        E2E_BACKPLATE_FILL_ID,
        "QA Backplate Fill",
        [1600.0, 500.0],
    )?;
    backplate_fill.set_property(
        "color".to_string(),
        Property::constant(PropertyValue::Color(Color {
            r: 20,
            g: 20,
            b: 20,
            a: 210,
        })),
    )?;
    backplate_fill.set_property(
        "opacity".to_string(),
        Property::constant(PropertyValue::Number(OrderedFloat(1.0))),
    )?;
    let mut text_merge = Node::new_merge("QA Text Merge");
    text_merge.id = E2E_TEXT_MERGE_ID;
    text_merge.ui_position = [1850.0, 400.0];
    let blur = operation_node(
        plugin_manager.create_effect_operation_node("blur"),
        E2E_BLUR_EFFECT_ID,
        "QA Blur",
        [2100.0, 400.0],
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

    project
        .add_track(track_a)
        .map_err(|error| format!("cannot insert primary QA Track: {error}"))?;
    project
        .add_track(track_b)
        .map_err(|error| format!("cannot insert secondary QA Track: {error}"))?;
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
    project.add_node(backplate_shape);
    project.add_node(backplate);
    project.add_node(text_fill);
    project.add_node(backplate_fill);
    project.add_node(text_merge);
    project.add_node(blur);
    project.add_node(shape);
    project.add_node(shape_transform);
    project.add_node(shape_fill);
    project.add_node(shape_stroke);
    project.add_node(shape_merge);
    if include_inspector_probe {
        composition.node_ids.push(E2E_INSPECTOR_VECTOR_ID);
        project.add_node(inspector_vector_probe_node()?);
    }
    project
        .add_composition(composition)
        .map_err(|error| format!("cannot insert QA Composition: {error}"))?;

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
            PortOwner::Node(E2E_BACKPLATE_SHAPE_ID),
            SHAPE_OUTPUT_PORT,
            E2E_DECORATOR_BACKPLATE_ID,
            BACKGROUND_SHAPE_INPUT_PORT,
        ),
        (
            PortOwner::Node(E2E_DECORATOR_BACKPLATE_ID),
            SHAPE_OUTPUT_PORT,
            E2E_BACKPLATE_FILL_ID,
            SHAPE_INPUT_PORT,
        ),
        (
            PortOwner::Node(E2E_EFFECTOR_OPACITY_ID),
            SHAPE_OUTPUT_PORT,
            E2E_TEXT_FILL_ID,
            SHAPE_INPUT_PORT,
        ),
        (
            PortOwner::Node(E2E_BACKPLATE_FILL_ID),
            IMAGE_OUTPUT_PORT,
            E2E_TEXT_MERGE_ID,
            MERGE_IMAGES_PORT,
        ),
        (
            PortOwner::Node(E2E_TEXT_FILL_ID),
            IMAGE_OUTPUT_PORT,
            E2E_TEXT_MERGE_ID,
            MERGE_IMAGES_PORT,
        ),
        (
            PortOwner::Node(E2E_TEXT_MERGE_ID),
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
        (E2E_CLIP_A2_ID, E2E_BACKPLATE_SHAPE_ID),
        (E2E_CLIP_A2_ID, E2E_DECORATOR_BACKPLATE_ID),
        (E2E_CLIP_A2_ID, E2E_TEXT_FILL_ID),
        (E2E_CLIP_A2_ID, E2E_BACKPLATE_FILL_ID),
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

/// A disconnected, unresolved Plugin operation is intentional here: it proves
/// both editor surfaces can edit persisted Vec4 state without consulting
/// plugin code. This is the same late-bound contract used by real third-party
/// Nodes when their plugin is not installed in the current process.
fn inspector_vector_probe_node() -> Result<Node, String> {
    serde_json::from_value(serde_json::json!({
        "id": E2E_INSPECTOR_VECTOR_ID,
        "name": "QA Vec4 Inspector Probe",
        "content": {
            "type": "PluginOperation",
            "data": {
                "category": "qa",
                "component_id": "vec4-probe",
                "operation": "qa.vec4-probe.v1",
                "declared_ports": [
                    {
                        "key": "property:vector",
                        "label": "Vector",
                        "direction": "Input",
                        "data_type": "Vec4",
                        "side": "Left",
                        "multiplicity": "Single",
                        "exposure": "Graph"
                    },
                    {
                        "key": "result",
                        "label": "Result",
                        "direction": "Output",
                        "data_type": "Vec4",
                        "side": "Right",
                        "multiplicity": "Single",
                        "exposure": "Graph"
                    }
                ]
            }
        },
        "enabled": true,
        "blend_mode": "Normal",
        "properties": {
            "vector": {
                "type": "constant",
                "properties": {"value": {"x": 1.0, "y": 2.0, "z": 3.0, "w": 4.0}}
            }
        },
        "ui_position": [2200.0, 720.0],
        "ui_size": [360.0, 160.0],
        "ui_collapsed": false
    }))
    .map_err(|error| format!("cannot create QA Vec4 Inspector probe: {error}"))
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

#[cfg(test)]
mod tests;
