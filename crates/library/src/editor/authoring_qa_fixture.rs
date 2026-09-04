//! Authoring-native fixtures for the loopback QA surface.
//!
//! Fixtures are built exclusively through [`TimelineEditorService`]. They do
//! not create or synchronize a legacy graph-backed Project. UUIDs may vary,
//! so every externally useful identity is returned in [`AuthoringE2eFixtureInfo`].

use std::collections::HashMap;
use std::path::Path;

use ordered_float::OrderedFloat;
use serde::Serialize;

use crate::error::LibraryError;
use crate::model::asset::AssetKind;
use crate::model::authoring::{
    AttachmentId, AttachmentOwner, AttachmentStage, AuthoringProject, CompositionInstance,
    DurationPolicy, MediaTime, ModuleConnection, ModuleConnectionId, ModuleDefinition,
    ModuleDefinitionId, ModuleDefinitionSharing, ModuleInstanceId, ModuleOutputId,
    ModulePortAddress, ModuleTemplateOrigin, PublishedMediaInput, PublishedMediaInputId,
    PublishedParameter, PublishedParameterId, RationalRate, ShapeKind, ShapeSource, SourceRef,
    TimelineId, TimelineInterval, TimelineItemId, TimelineTrackId, TransitionMediaType,
};
use crate::model::frame::color::Color;
use crate::model::node::Node;
use crate::model::path::{FillRule, PathContour, PathPoint, PathSegment, PathValue};
use crate::model::project::{IMAGE_INPUT_PORT, IMAGE_OUTPUT_PORT, MERGE_IMAGES_PORT, PortDataType};
use crate::model::property::{ColorValue, PropertyValue, Vec2};
use crate::plugin::PluginManager;

use super::{
    AuthoringPropertyOwner, ModuleItemPlacement, ModuleNodeRequest, TimelineEditorService,
};

pub const AUTHORING_E2E_FIXTURE: &str = "authoring_e2e";
pub const AUTHORING_AUDIO_E2E_FIXTURE: &str = "authoring_audio_e2e";
pub const AUTHORING_PATH_E2E_FIXTURE: &str = "authoring_path_e2e";
pub const AUTHORING_E2E_IMAGE: &str = "rgba.png";
pub const AUTHORING_E2E_AUDIO: &str = "tone.mp3";
pub const AUTHORING_E2E_VIDEO: &str = "h264_24.mp4";

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct AuthoringE2eFixtureInfo {
    pub timeline_id: TimelineId,
    pub primary_track_id: TimelineTrackId,
    pub image_asset_id: uuid::Uuid,
    pub image_item_id: TimelineItemId,
    pub overlapping_item_id: TimelineItemId,
    pub text_item_id: TimelineItemId,
    pub text_keyframe_ids: Vec<crate::model::property::KeyframeId>,
    pub node_clip_item_id: TimelineItemId,
    pub module_definition_id: ModuleDefinitionId,
    pub module_instance_id: ModuleInstanceId,
    pub module_parameter_id: PublishedParameterId,
    pub module_keyframe_ids: Vec<crate::model::property::KeyframeId>,
    pub effect_attachment_ids: Vec<AttachmentId>,
    pub required_input_transition_definition_id: ModuleDefinitionId,
    pub required_input_transition_media_input_id: PublishedMediaInputId,
}

#[derive(Clone)]
pub struct AuthoringE2eFixture {
    pub service: TimelineEditorService,
    pub info: AuthoringE2eFixtureInfo,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct AuthoringAudioE2eFixtureInfo {
    pub timeline_id: TimelineId,
    pub audio_asset_id: uuid::Uuid,
    pub audio_item_id: TimelineItemId,
    pub node_audio_item_id: TimelineItemId,
    pub video_asset_id: uuid::Uuid,
    pub video_item_id: TimelineItemId,
    pub shape_item_id: TimelineItemId,
    pub composition_item_id: TimelineItemId,
}

#[derive(Clone)]
pub struct AuthoringAudioE2eFixture {
    pub service: TimelineEditorService,
    pub info: AuthoringAudioE2eFixtureInfo,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct AuthoringPathE2eFixtureInfo {
    pub timeline_id: TimelineId,
    pub path_item_id: TimelineItemId,
}

#[derive(Clone)]
pub struct AuthoringPathE2eFixture {
    pub service: TimelineEditorService,
    pub info: AuthoringPathE2eFixtureInfo,
}

/// Builds the fixture from an e2e media directory (normally
/// `test_data/e2e_media`). The returned service has revision zero and empty
/// undo/redo history; setup commands are not user-visible edits.
pub fn build_authoring_e2e_fixture(
    e2e_media_directory: &Path,
    plugins: &PluginManager,
) -> Result<AuthoringE2eFixture, LibraryError> {
    let project = AuthoringProject::new(
        "Authoring QA",
        640,
        360,
        RationalRate::new(30, 1).map_err(LibraryError::Validation)?,
        media_time(12, 1)?,
    )
    .map_err(LibraryError::Validation)?;
    let timeline_id = project.root_timeline_id;
    let primary_track_id = project
        .timelines
        .get(&timeline_id)
        .and_then(|timeline| timeline.track_order.first())
        .copied()
        .ok_or_else(|| LibraryError::Validation("QA Project has no primary Track".to_string()))?;
    let service = TimelineEditorService::new(project)?;

    let image_path = e2e_media_directory.join(AUTHORING_E2E_IMAGE);
    let (asset_ids, _) = service.import_file(&image_path, plugins)?;
    let image_asset_id = asset_ids
        .first()
        .copied()
        .ok_or_else(|| LibraryError::Validation("QA image import returned no Asset".to_string()))?;
    let (image_item_id, _) = service.add_item(
        primary_track_id,
        "QA Image".to_string(),
        SourceRef::Asset {
            asset_id: image_asset_id,
        },
        interval(0, 6)?,
        0,
    )?;
    let (overlapping_item_id, _) = service.add_item(
        primary_track_id,
        "QA Overlap".to_string(),
        SourceRef::Solid {
            color: rgba8(32, 96, 220, 180),
        },
        interval(2, 5)?,
        1,
    )?;
    let (text_item_id, _) = service.add_item(
        primary_track_id,
        "QA Text".to_string(),
        SourceRef::Text {
            text: "Authoring QA".to_string(),
            ensemble_operations: Vec::new(),
        },
        interval(1, 7)?,
        2,
    )?;
    service.set_authored_property_constant(
        AuthoringPropertyOwner::Item(text_item_id),
        "position".to_string(),
        vec2_value(120.0, 180.0),
    )?;
    // Timeline Dope Sheet QA uses this authored value to prove that a plain
    // constant remains Inspector-only until it becomes a real keyframe track.
    service.set_authored_property_constant(
        AuthoringPropertyOwner::Item(text_item_id),
        "opacity".to_string(),
        PropertyValue::from(1.0),
    )?;
    let (text_key_a, _) = service.upsert_authored_property_keyframe(
        AuthoringPropertyOwner::Item(text_item_id),
        "position".to_string(),
        MediaTime::zero(),
        vec2_value(120.0, 180.0),
        None,
    )?;
    let (text_key_b, _) = service.upsert_authored_property_keyframe(
        AuthoringPropertyOwner::Item(text_item_id),
        "position".to_string(),
        media_time(2, 1)?,
        vec2_value(420.0, 180.0),
        None,
    )?;

    let (definition, output_id, parameter_id) = solid_module_definition(&service, plugins)?;
    let module_definition_id = definition.id;
    let (node_clip_item_id, module_instance_id, _) = service.create_private_module_item(
        definition,
        ModuleItemPlacement {
            track_id: primary_track_id,
            name: "QA Node Clip".to_string(),
            output_id,
            interval: interval(4, 5)?,
            layer: 3,
            parameter_overrides: HashMap::new(),
            input_bindings: HashMap::new(),
        },
    )?;
    let module_color_a = color_value(rgba8(240, 72, 80, 255));
    let module_color_b = color_value(rgba8(80, 220, 160, 255));
    service.set_module_parameter(module_instance_id, parameter_id, module_color_a.clone())?;
    let (module_key_a, _) = service.upsert_module_parameter_keyframe(
        node_clip_item_id,
        parameter_id,
        MediaTime::zero(),
        module_color_a,
        None,
    )?;
    let (module_key_b, _) = service.upsert_module_parameter_keyframe(
        node_clip_item_id,
        parameter_id,
        media_time(2, 1)?,
        module_color_b,
        None,
    )?;

    let (blur_attachment_id, _) = service.add_builtin_effect_by_id(
        plugins,
        AttachmentOwner::Item {
            item_id: text_item_id,
        },
        AttachmentStage::ItemPostTransform,
        "blur",
    )?;
    let (tile_attachment_id, _) = service.add_builtin_effect_by_id(
        plugins,
        AttachmentOwner::Item {
            item_id: text_item_id,
        },
        AttachmentStage::ItemPostTransform,
        "tile",
    )?;
    // Keep the fixture's Tile attachment visually neutral at startup. Its
    // descriptor defaults sample only the top-left 100x100 region, while QA
    // Text is positioned lower on the canvas, which otherwise erases the Text
    // before any Inspector interaction can be verified in Preview.
    service.set_builtin_effect_parameter(
        tile_attachment_id,
        "width",
        PropertyValue::from(640.0),
    )?;
    service.set_builtin_effect_parameter(
        tile_attachment_id,
        "height",
        PropertyValue::from(360.0),
    )?;

    let (required_transition, required_transition_input_id) =
        required_input_transition_definition()?;
    let required_transition_definition_id = required_transition.id;
    service.add_module_definition(required_transition)?;

    let clean_project = service.snapshot()?;
    let service = TimelineEditorService::new((*clean_project).clone())?;
    let info = AuthoringE2eFixtureInfo {
        timeline_id,
        primary_track_id,
        image_asset_id,
        image_item_id,
        overlapping_item_id,
        text_item_id,
        text_keyframe_ids: vec![text_key_a, text_key_b],
        node_clip_item_id,
        module_definition_id,
        module_instance_id,
        module_parameter_id: parameter_id,
        module_keyframe_ids: vec![module_key_a, module_key_b],
        effect_attachment_ids: vec![blur_attachment_id, tile_attachment_id],
        required_input_transition_definition_id: required_transition_definition_id,
        required_input_transition_media_input_id: required_transition_input_id,
    };
    Ok(AuthoringE2eFixture { service, info })
}

/// Adds a real decoded Audio placement without changing the general-purpose
/// authoring fixture's stable item/asset counts.
pub fn build_authoring_audio_e2e_fixture(
    e2e_media_directory: &Path,
    plugins: &PluginManager,
) -> Result<AuthoringAudioE2eFixture, LibraryError> {
    let base = build_authoring_e2e_fixture(e2e_media_directory, plugins)?;
    let audio_path = e2e_media_directory.join(AUTHORING_E2E_AUDIO);
    let (asset_ids, _) = base.service.import_file(&audio_path, plugins)?;
    let project = base.service.snapshot()?;
    let audio_asset_id = asset_ids
        .into_iter()
        .find(|asset_id| {
            project
                .assets
                .iter()
                .any(|asset| asset.id == *asset_id && asset.kind == AssetKind::Audio)
        })
        .ok_or_else(|| {
            LibraryError::Validation("QA audio import returned no Audio Asset".to_string())
        })?;
    let (audio_item_id, _) = base.service.add_item(
        base.info.primary_track_id,
        "QA Audio".to_string(),
        SourceRef::Asset {
            asset_id: audio_asset_id,
        },
        // Keep a plain Asset placement for Timeline waveform QA, but start it
        // after the playback probe. The independently converted Node Audio
        // item below is therefore the only audible source at time zero.
        interval(2, 1)?,
        4,
    )?;
    let (node_audio_item_id, _) = base.service.add_item(
        base.info.primary_track_id,
        "QA Node Audio".to_string(),
        SourceRef::Asset {
            asset_id: audio_asset_id,
        },
        interval(0, 1)?,
        5,
    )?;
    base.service
        .convert_source_to_node_clip(plugins, node_audio_item_id)?;
    let video_path = e2e_media_directory.join(AUTHORING_E2E_VIDEO);
    let (video_asset_ids, _) = base.service.import_file(&video_path, plugins)?;
    let project = base.service.snapshot()?;
    let video_asset_id = video_asset_ids
        .into_iter()
        .find(|asset_id| {
            project
                .assets
                .iter()
                .any(|asset| asset.id == *asset_id && asset.kind == AssetKind::Video)
        })
        .ok_or_else(|| {
            LibraryError::Validation("QA video import returned no Video Asset".to_string())
        })?;
    let (video_item_id, _) = base.service.add_item(
        base.info.primary_track_id,
        "QA Video".to_string(),
        SourceRef::Asset {
            asset_id: video_asset_id,
        },
        interval(6, 3)?,
        6,
    )?;
    let (shape_item_id, _) = base.service.add_item(
        base.info.primary_track_id,
        "QA Rectangle".to_string(),
        SourceRef::Shape {
            shape: ShapeSource {
                shape_kind: ShapeKind::Rectangle,
                parameters: HashMap::new(),
            },
        },
        interval(0, 3)?,
        7,
    )?;
    let (nested_timeline_id, _, _) = base.service.add_timeline(
        "QA Nested".to_string(),
        640,
        360,
        RationalRate::new(30, 1).map_err(LibraryError::Validation)?,
        media_time(3, 1)?,
    )?;
    let (composition_item_id, _) = base.service.add_item(
        base.info.primary_track_id,
        "QA Composition".to_string(),
        SourceRef::Composition(CompositionInstance {
            timeline_id: nested_timeline_id,
            duration_policy: DurationPolicy::Fixed,
            parameter_overrides: HashMap::new(),
            transition_module_overrides: Vec::new(),
        }),
        interval(8, 3)?,
        8,
    )?;

    let clean_project = base.service.snapshot()?;
    let service = TimelineEditorService::new((*clean_project).clone())?;
    Ok(AuthoringAudioE2eFixture {
        service,
        info: AuthoringAudioE2eFixtureInfo {
            timeline_id: base.info.timeline_id,
            audio_asset_id,
            audio_item_id,
            node_audio_item_id,
            video_asset_id,
            video_item_id,
            shape_item_id,
            composition_item_id,
        },
    })
}

/// Adds one canonical Path source without changing the general-purpose QA
/// fixture used by unrelated Preview and Timeline suites.
pub fn build_authoring_path_e2e_fixture(
    e2e_media_directory: &Path,
    plugins: &PluginManager,
) -> Result<AuthoringPathE2eFixture, LibraryError> {
    let base = build_authoring_e2e_fixture(e2e_media_directory, plugins)?;
    let path = PathValue::new(
        FillRule::NonZero,
        vec![PathContour::new(
            // Keep a non-zero local origin so native QA catches Gizmos that
            // substitute width/height from (0, 0) for the painted Path bounds.
            PathPoint::new(120.0, 80.0),
            vec![
                PathSegment::line(PathPoint::new(280.0, 80.0)),
                PathSegment::line(PathPoint::new(280.0, 170.0)),
                PathSegment::line(PathPoint::new(120.0, 170.0)),
            ],
            true,
        )],
    )
    .map_err(|error| LibraryError::Validation(error.to_string()))?;
    let (path_item_id, _) = base.service.add_item(
        base.info.primary_track_id,
        "QA Path".to_string(),
        SourceRef::Shape {
            shape: ShapeSource {
                shape_kind: ShapeKind::Path,
                parameters: HashMap::from([
                    ("path".to_string(), PropertyValue::Path(path)),
                    ("width".to_string(), PropertyValue::from(160.0)),
                    ("height".to_string(), PropertyValue::from(90.0)),
                    ("color".to_string(), color_value(rgba8(255, 185, 45, 255))),
                ]),
            },
        },
        interval(0, 3)?,
        4,
    )?;
    base.service.set_authored_property_constant(
        AuthoringPropertyOwner::Item(path_item_id),
        "position".to_string(),
        vec2_value(360.0, 230.0),
    )?;

    let clean_project = base.service.snapshot()?;
    let service = TimelineEditorService::new((*clean_project).clone())?;
    Ok(AuthoringPathE2eFixture {
        service,
        info: AuthoringPathE2eFixtureInfo {
            timeline_id: base.info.timeline_id,
            path_item_id,
        },
    })
}

fn solid_module_definition(
    service: &TimelineEditorService,
    plugins: &PluginManager,
) -> Result<(ModuleDefinition, ModuleOutputId, PublishedParameterId), LibraryError> {
    let node = service.create_module_node(
        plugins,
        ModuleNodeRequest::Solid {
            color: rgba8(240, 72, 80, 255),
        },
        640,
        360,
    )?;
    let node_id = node.id;
    let default_color = node
        .properties()
        .get_constant_value("color")
        .cloned()
        .ok_or_else(|| LibraryError::Validation("Solid Node has no default color".to_string()))?;
    let parameter_id = PublishedParameterId::new();
    let (mut definition, output_id) =
        ModuleDefinition::new_image("QA Solid Module", ModuleDefinitionSharing::Private);
    let output_target = definition
        .output(output_id)
        .ok_or_else(|| LibraryError::Validation("Solid Module has no output terminal".to_string()))?
        .target(crate::model::project::PortDataType::Image)
        .ok_or_else(|| {
            LibraryError::Validation("Solid Module Output has no Image input".to_string())
        })?;
    definition.graph.nodes.insert(node_id, node);
    definition.graph.connections.push(ModuleConnection {
        id: ModuleConnectionId::new(),
        from: ModulePortAddress {
            node_id,
            port: IMAGE_OUTPUT_PORT.to_string(),
        },
        to: output_target,
        order: 0,
        blend_mode: crate::model::BlendMode::Normal,
    });
    definition.interface.parameters.push(PublishedParameter {
        id: parameter_id,
        name: "Color".to_string(),
        data_type: PortDataType::Color,
        default_value: default_color,
        target: ModulePortAddress {
            node_id,
            port: "color".to_string(),
        },
    });
    definition.topology_revision = 2;
    Ok((definition, output_id, parameter_id))
}

/// A reusable Transition fixture whose visible result comes from one required
/// public Image input. Native UI QA uses it to prove that choosing a template,
/// resolving the public input and creating the instance is one atomic edit.
fn required_input_transition_definition()
-> Result<(ModuleDefinition, PublishedMediaInputId), LibraryError> {
    let (mut definition, contract) = ModuleDefinition::new_transition(
        "QA Required Input Transition",
        ModuleDefinitionSharing::ReusableTemplate(ModuleTemplateOrigin::Project),
        TransitionMediaType::Image,
    )
    .map_err(LibraryError::Validation)?;
    let output_target = definition
        .output(contract.output_id)
        .and_then(|output| output.target(PortDataType::Image))
        .ok_or_else(|| {
            LibraryError::Validation(
                "QA Transition Module has no protected Image output".to_string(),
            )
        })?;
    let output_connection = definition
        .graph
        .connections
        .iter()
        .position(|connection| connection.to == output_target)
        .map(|index| definition.graph.connections.remove(index))
        .ok_or_else(|| {
            LibraryError::Validation(
                "QA Transition Module has no starter output connection".to_string(),
            )
        })?;
    debug_assert_eq!(output_connection.to.port, IMAGE_INPUT_PORT);

    let input_target = Node::new_merge("Required Image Input");
    let input_node_id = input_target.id;
    definition.graph.nodes.insert(input_node_id, input_target);
    definition.graph.connections.push(ModuleConnection {
        id: ModuleConnectionId::new(),
        from: ModulePortAddress {
            node_id: input_node_id,
            port: IMAGE_OUTPUT_PORT.to_string(),
        },
        to: output_connection.to,
        order: 0,
        blend_mode: crate::model::BlendMode::Normal,
    });
    let input_id = PublishedMediaInputId::new();
    definition.interface.media_inputs.push(PublishedMediaInput {
        id: input_id,
        name: "External Image".to_string(),
        data_type: PortDataType::Image,
        target: ModulePortAddress {
            node_id: input_node_id,
            port: MERGE_IMAGES_PORT.to_string(),
        },
        required: true,
        primary: false,
    });
    definition.topology_revision += 1;
    definition.interface_version += 1;
    definition.validate().map_err(LibraryError::Validation)?;
    Ok((definition, input_id))
}

fn media_time(value: i64, timescale: u32) -> Result<MediaTime, LibraryError> {
    MediaTime::new(value, timescale).map_err(LibraryError::Validation)
}

fn interval(start: i64, duration: i64) -> Result<TimelineInterval, LibraryError> {
    TimelineInterval::new(media_time(start, 1)?, media_time(duration, 1)?)
        .map_err(LibraryError::Validation)
}

fn vec2_value(x: f64, y: f64) -> PropertyValue {
    PropertyValue::Vec2(Vec2 {
        x: OrderedFloat(x),
        y: OrderedFloat(y),
    })
}

fn color_value(color: Color) -> PropertyValue {
    PropertyValue::ColorValue(ColorValue::from_straight_srgba8(&color))
}

fn rgba8(r: u8, g: u8, b: u8, a: u8) -> Color {
    Color { r, g, b, a }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use crate::core::audio::authoring::AuthoringAudioMixer;
    use crate::core::cache::CacheManager;
    use crate::model::authoring::{AttachmentProcessor, ModuleDefinitionSharing, ProjectRevision};

    use super::*;

    #[test]
    fn authoring_fixture_is_valid_and_exposes_all_qa_targets() {
        let media_directory =
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../test_data/e2e_media");
        let fixture = build_authoring_e2e_fixture(&media_directory, &PluginManager::default())
            .expect("authoring fixture");
        let project = fixture.service.snapshot().expect("fixture snapshot");

        project.validate().expect("valid fixture Project");
        assert_eq!(
            fixture.service.revision().expect("fixture revision"),
            ProjectRevision::initial()
        );
        assert!(!fixture.service.can_undo().expect("undo state"));
        assert!(!fixture.service.can_redo().expect("redo state"));

        let timeline = project
            .timelines
            .get(&fixture.info.timeline_id)
            .expect("fixture Timeline");
        assert_eq!(timeline.track_order, vec![fixture.info.primary_track_id]);
        let track_items = project
            .items
            .values()
            .filter(|item| item.track_id == fixture.info.primary_track_id)
            .collect::<Vec<_>>();
        assert_eq!(track_items.len(), 4);

        let image = project
            .items
            .get(&fixture.info.image_item_id)
            .expect("image item");
        let overlap = project
            .items
            .get(&fixture.info.overlapping_item_id)
            .expect("overlap item");
        assert!(matches!(
            image.source,
            SourceRef::Asset { asset_id } if asset_id == fixture.info.image_asset_id
        ));
        assert!(matches!(overlap.source, SourceRef::Solid { .. }));
        let overlap_time = media_time(4, 1).expect("overlap time");
        assert!(
            image
                .interval
                .contains(overlap_time)
                .expect("image interval")
        );
        assert!(
            overlap
                .interval
                .contains(overlap_time)
                .expect("overlap interval")
        );
        assert!(project.assets.iter().any(|asset| {
            asset.id == fixture.info.image_asset_id
                && Path::new(&asset.path).ends_with(AUTHORING_E2E_IMAGE)
        }));

        let text = project
            .items
            .get(&fixture.info.text_item_id)
            .expect("text item");
        assert!(matches!(
            &text.source,
            SourceRef::Text { text, .. } if text == "Authoring QA"
        ));
        let authored_keys = text
            .authored_properties
            .get("position")
            .expect("position Property")
            .keyframes()
            .into_iter()
            .map(|keyframe| keyframe.id)
            .collect::<Vec<_>>();
        assert_eq!(authored_keys, fixture.info.text_keyframe_ids);

        let node_clip = project
            .items
            .get(&fixture.info.node_clip_item_id)
            .expect("Node Clip item");
        let SourceRef::Module(invocation) = &node_clip.source else {
            panic!("fixture Node Clip must use SourceRef::Module");
        };
        assert_eq!(invocation.instance_id, fixture.info.module_instance_id);
        let module_keys = invocation
            .automation_tracks
            .get(&fixture.info.module_parameter_id)
            .expect("module automation")
            .keyframes
            .iter()
            .map(|keyframe| keyframe.id)
            .collect::<Vec<_>>();
        assert_eq!(module_keys, fixture.info.module_keyframe_ids);
        let instance = project
            .module_instances
            .get(&fixture.info.module_instance_id)
            .expect("module instance");
        assert_eq!(instance.definition_id, fixture.info.module_definition_id);
        assert_eq!(
            project
                .module_definitions
                .get(&fixture.info.module_definition_id)
                .expect("module definition")
                .sharing,
            ModuleDefinitionSharing::Private
        );

        let effects = fixture
            .info
            .effect_attachment_ids
            .iter()
            .map(|id| project.attachments.get(id).expect("fixture effect"))
            .collect::<Vec<_>>();
        assert_eq!(effects.len(), 2);
        assert_eq!(effects[0].order, 0);
        assert_eq!(effects[1].order, 1);
        assert!(effects.iter().all(|attachment| {
            attachment.owner
                == AttachmentOwner::Item {
                    item_id: fixture.info.text_item_id,
                }
                && matches!(attachment.processor, AttachmentProcessor::BuiltinEffect(_))
        }));

        let transition_definition = project
            .module_definitions
            .get(&fixture.info.required_input_transition_definition_id)
            .expect("required-input Transition definition");
        assert!(matches!(
            transition_definition.sharing,
            ModuleDefinitionSharing::ReusableTemplate(ModuleTemplateOrigin::Project)
        ));
        assert!(
            transition_definition
                .interface
                .media_inputs
                .iter()
                .any(|input| {
                    input.id == fixture.info.required_input_transition_media_input_id
                        && input.required
                })
        );

        serde_json::to_string(&fixture.info).expect("serializable fixture info");
    }

    #[test]
    fn audio_fixture_has_a_clean_revision_and_audible_route() {
        let media_directory =
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../test_data/e2e_media");
        let fixture =
            build_authoring_audio_e2e_fixture(&media_directory, &PluginManager::default())
                .expect("authoring audio fixture");
        let project = fixture.service.snapshot().expect("fixture snapshot");

        project.validate().expect("valid audio fixture Project");
        assert_eq!(
            fixture.service.revision().expect("fixture revision"),
            ProjectRevision::initial()
        );
        assert_eq!(project.assets.len(), 3);
        assert_eq!(project.timelines.len(), 2);
        assert_eq!(
            project
                .items
                .values()
                .filter(|item| item.track_id
                    == project.timelines[&fixture.info.timeline_id].track_order[0])
                .count(),
            9
        );
        assert!(matches!(
            project.items[&fixture.info.audio_item_id].source,
            SourceRef::Asset { asset_id } if asset_id == fixture.info.audio_asset_id
        ));
        let SourceRef::Module(node_audio) = &project.items[&fixture.info.node_audio_item_id].source
        else {
            panic!("Node Audio fixture item must be a Module invocation");
        };
        let node_audio_instance = &project.module_instances[&node_audio.instance_id];
        let node_audio_definition = &project.module_definitions[&node_audio_instance.definition_id];
        let sound_target = node_audio_definition
            .output(node_audio.output_id)
            .and_then(|output| output.target(PortDataType::Audio))
            .expect("Node Audio Output must expose Sound");
        assert!(
            node_audio_definition
                .graph
                .connections
                .iter()
                .any(|connection| connection.to == sound_target)
        );
        assert!(matches!(
            project.items[&fixture.info.video_item_id].source,
            SourceRef::Asset { asset_id } if asset_id == fixture.info.video_asset_id
        ));
        assert!(matches!(
            project.items[&fixture.info.shape_item_id].source,
            SourceRef::Shape {
                shape: ShapeSource {
                    shape_kind: ShapeKind::Rectangle,
                    ..
                }
            }
        ));
        assert!(matches!(
            project.items[&fixture.info.composition_item_id].source,
            SourceRef::Composition(_)
        ));
        assert!(project.assets.iter().any(|asset| {
            asset.id == fixture.info.video_asset_id
                && asset.kind == AssetKind::Video
                && Path::new(&asset.path).ends_with(AUTHORING_E2E_VIDEO)
        }));
        let cache = CacheManager::with_audio_chunk_capacity(2);
        let mut mixer =
            AuthoringAudioMixer::new(project.as_ref(), &cache, fixture.info.timeline_id)
                .expect("fixture audio schedule");
        assert!(mixer.has_audio_routes());
        assert!(
            mixer
                .render_window(0, 4_800)
                .expect("fixture audio render")
                .into_iter()
                .any(|sample| sample.abs() > 0.001)
        );
    }

    #[test]
    fn path_fixture_owns_one_canonical_edit_target_and_clean_history() {
        let media_directory =
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../test_data/e2e_media");
        let fixture = build_authoring_path_e2e_fixture(&media_directory, &PluginManager::default())
            .expect("authoring Path fixture");
        let project = fixture.service.snapshot().expect("fixture snapshot");

        project.validate().expect("valid Path fixture Project");
        assert_eq!(
            fixture.service.revision().expect("fixture revision"),
            ProjectRevision::initial()
        );
        assert!(!fixture.service.can_undo().expect("clean Undo state"));
        let SourceRef::Shape { shape } = &project.items[&fixture.info.path_item_id].source else {
            panic!("Path fixture source");
        };
        assert_eq!(shape.shape_kind, ShapeKind::Path);
        assert!(matches!(
            shape.parameters.get("path"),
            Some(PropertyValue::Path(path)) if path.contours().len() == 1
        ));
    }
}
