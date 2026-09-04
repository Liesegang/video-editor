//! Authoring-native fixtures for the loopback QA surface.
//!
//! Fixtures are built exclusively through [`TimelineEditorService`]. They do
//! not create or synchronize a legacy graph-backed Project. UUIDs may vary,
//! so every externally useful identity is returned in [`TimelineFirstE2eFixtureInfo`].

use std::collections::HashMap;
use std::path::Path;

use ordered_float::OrderedFloat;
use serde::Serialize;

use crate::error::LibraryError;
use crate::model::authoring::{
    AttachmentId, AttachmentOwner, AttachmentStage, AuthoringProject, MediaTime, ModuleDefinition,
    ModuleDefinitionId, ModuleDefinitionSharing, ModuleGraph, ModuleInstanceId, ModuleInterface,
    ModulePortAddress, PublishedMediaOutput, PublishedMediaOutputId, PublishedParameter,
    PublishedParameterId, RationalRate, SourceRef, TimelineId, TimelineInterval, TimelineItemId,
    TimelineTrackId,
};
use crate::model::frame::color::Color;
use crate::model::project::{IMAGE_OUTPUT_PORT, PortDataType};
use crate::model::property::{ColorValue, PropertyValue, Vec2};
use crate::plugin::PluginManager;

use super::{
    AuthoringPropertyOwner, ModuleItemPlacement, ModuleNodeRequest, TimelineEditorService,
};

pub const TIMELINE_FIRST_E2E_FIXTURE: &str = "timeline_first_e2e";
pub const TIMELINE_FIRST_E2E_IMAGE: &str = "rgba.png";

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct TimelineFirstE2EFixtureInfo {
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
}

#[derive(Clone)]
pub struct TimelineFirstE2EFixture {
    pub service: TimelineEditorService,
    pub info: TimelineFirstE2EFixtureInfo,
}

/// Builds the fixture from an e2e media directory (normally
/// `test_data/e2e_media`). The returned service has revision zero and empty
/// undo/redo history; setup commands are not user-visible edits.
pub fn build_timeline_first_e2e_fixture(
    e2e_media_directory: &Path,
    plugins: &PluginManager,
) -> Result<TimelineFirstE2EFixture, LibraryError> {
    let project = AuthoringProject::new(
        "Timeline-first QA",
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

    let image_path = e2e_media_directory.join(TIMELINE_FIRST_E2E_IMAGE);
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
            text: "Timeline-first QA".to_string(),
        },
        interval(1, 7)?,
        2,
    )?;
    service.set_authored_property_constant(
        AuthoringPropertyOwner::Item(text_item_id),
        "position".to_string(),
        vec2_value(120.0, 180.0),
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

    let clean_project = service.snapshot()?;
    let service = TimelineEditorService::new((*clean_project).clone())?;
    let info = TimelineFirstE2EFixtureInfo {
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
    };
    Ok(TimelineFirstE2EFixture { service, info })
}

fn solid_module_definition(
    service: &TimelineEditorService,
    plugins: &PluginManager,
) -> Result<
    (
        ModuleDefinition,
        PublishedMediaOutputId,
        PublishedParameterId,
    ),
    LibraryError,
> {
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
    let output_id = PublishedMediaOutputId::new();
    let parameter_id = PublishedParameterId::new();
    Ok((
        ModuleDefinition {
            id: ModuleDefinitionId::new(),
            name: "QA Solid Module".to_string(),
            sharing: ModuleDefinitionSharing::Private,
            graph: ModuleGraph {
                nodes: HashMap::from([(node_id, node)]),
                connections: Vec::new(),
            },
            interface: ModuleInterface {
                parameters: vec![PublishedParameter {
                    id: parameter_id,
                    name: "Color".to_string(),
                    data_type: PortDataType::Color,
                    default_value: default_color,
                    target: ModulePortAddress {
                        node_id,
                        port: "color".to_string(),
                    },
                }],
                media_outputs: vec![PublishedMediaOutput {
                    id: output_id,
                    name: "Image".to_string(),
                    data_type: PortDataType::Image,
                    source: ModulePortAddress {
                        node_id,
                        port: IMAGE_OUTPUT_PORT.to_string(),
                    },
                }],
                ..ModuleInterface::default()
            },
            topology_revision: 1,
            interface_version: 1,
        },
        output_id,
        parameter_id,
    ))
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

    use crate::model::authoring::{AttachmentProcessor, ModuleDefinitionSharing, ProjectRevision};

    use super::*;

    #[test]
    fn timeline_first_fixture_is_valid_and_exposes_all_qa_targets() {
        let media_directory =
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../test_data/e2e_media");
        let fixture = build_timeline_first_e2e_fixture(&media_directory, &PluginManager::default())
            .expect("timeline-first fixture");
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
                && Path::new(&asset.path).ends_with(TIMELINE_FIRST_E2E_IMAGE)
        }));

        let text = project
            .items
            .get(&fixture.info.text_item_id)
            .expect("text item");
        assert!(matches!(
            &text.source,
            SourceRef::Text { text } if text == "Timeline-first QA"
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

        serde_json::to_string(&fixture.info).expect("serializable fixture info");
    }
}
