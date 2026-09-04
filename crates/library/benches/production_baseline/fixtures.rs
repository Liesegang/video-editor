use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use library::model::BlendMode;
use library::model::authoring::{
    AuthoringProject, MediaTime, ModuleDefinition, ModuleDefinitionId, ModuleInstance,
    ModuleInstanceId, ModuleInvocation, ProjectDocument, RationalRate, SourceRef, TimeMap,
    TimelineId, TimelineInterval, TimelineItem, TimelineItemId, TimelineTrackId,
};
use library::model::frame::color::Color;
use library::model::project::property::PropertyMap;
use serde::Serialize;
use sha2::{Digest, Sha256};
use tempfile::TempDir;
use uuid::Uuid;

use crate::BenchResult;

const FIXTURE_SPEC: &str = concat!(
    "ruvie-production-baseline-v1\n",
    "canvas=320x180@30fps,duration=600s\n",
    "timeline-item-workloads=100,1000,10000\n",
    "project-load-items=1000\n",
    "preview-items=4\n",
    "shared-module-definitions=1\n",
    "shared-module-instances=1000\n",
    "audio=test_data/e2e_media/tone.mp3\n",
    "audio-window=4800-frames@48000Hz-stereo\n",
    "consecutive-preview-frames=30\n",
);

#[derive(Clone, Debug, Serialize)]
pub struct FixtureMetadata {
    pub name: &'static str,
    pub generator_version: u32,
    pub sha256: String,
    pub load_document_sha256: String,
    pub audio_media_sha256: String,
    pub workloads: Vec<FixtureWorkload>,
}

#[derive(Clone, Debug, Serialize)]
pub struct FixtureWorkload {
    pub name: &'static str,
    pub timeline_items: usize,
    pub module_definitions: usize,
    pub module_instances: usize,
}

pub struct FixtureSet {
    pub preview_project: AuthoringProject,
    pub preview_item_id: TimelineItemId,
    pub items_100: AuthoringProject,
    pub items_1_000: AuthoringProject,
    pub items_10_000: AuthoringProject,
    pub shared_module_1_000: AuthoringProject,
    pub load_project_path: PathBuf,
    pub audio_media_directory: PathBuf,
    metadata: FixtureMetadata,
    _temporary_directory: TempDir,
}

impl FixtureSet {
    pub fn build(repository_root: &Path) -> BenchResult<Self> {
        let (preview_project, _, preview_item_id) = solid_project(4, 1)?;
        let (items_100, _, _) = solid_project(100, 2)?;
        let (items_1_000, _, _) = solid_project(1_000, 3)?;
        let (items_10_000, _, _) = solid_project(10_000, 4)?;
        let shared_module_1_000 = shared_module_project(1_000, 5)?;

        let load_document = canonical_project_json(&items_1_000)?;
        let load_document_sha256 = sha256_hex(load_document.as_bytes());
        let audio_media_directory = repository_root.join("test_data/e2e_media");
        let audio_source = fs::read(audio_media_directory.join("tone.mp3"))?;
        let audio_media_sha256 = sha256_hex(&audio_source);
        let mut fixture_identity = FIXTURE_SPEC.as_bytes().to_vec();
        fixture_identity.extend_from_slice(load_document_sha256.as_bytes());
        fixture_identity.extend_from_slice(audio_media_sha256.as_bytes());

        let temporary_directory = tempfile::tempdir()?;
        let load_project_path = temporary_directory.path().join("load-project.ruvie");
        fs::write(&load_project_path, load_document)?;
        let metadata = FixtureMetadata {
            name: "production-baseline-v1",
            generator_version: 1,
            sha256: sha256_hex(&fixture_identity),
            load_document_sha256,
            audio_media_sha256,
            workloads: vec![
                workload("timeline-items-100", 100, 0, 0),
                workload("timeline-items-1000", 1_000, 0, 0),
                workload("timeline-items-10000", 10_000, 0, 0),
                workload("shared-module-1000", 1_000, 1, 1_000),
                workload("preview", 4, 0, 0),
            ],
        };
        Ok(Self {
            preview_project,
            preview_item_id,
            items_100,
            items_1_000,
            items_10_000,
            shared_module_1_000,
            load_project_path,
            audio_media_directory,
            metadata,
            _temporary_directory: temporary_directory,
        })
    }

    pub fn metadata(&self) -> &FixtureMetadata {
        &self.metadata
    }
}

fn workload(
    name: &'static str,
    timeline_items: usize,
    module_definitions: usize,
    module_instances: usize,
) -> FixtureWorkload {
    FixtureWorkload {
        name,
        timeline_items,
        module_definitions,
        module_instances,
    }
}

fn solid_project(
    item_count: usize,
    namespace: u16,
) -> BenchResult<(AuthoringProject, TimelineTrackId, TimelineItemId)> {
    let duration = MediaTime::from_whole_seconds(600);
    let mut project = AuthoringProject::new(
        format!("Performance {item_count}"),
        320,
        180,
        RationalRate::new(30, 1)?,
        duration,
    )?;
    let track_id = stabilize_root_ids(&mut project, namespace)?;
    let mut first_item_id = None;
    for index in 0..item_count {
        let item_id = TimelineItemId::from_uuid(stable_uuid(namespace, 10_000 + index as u64));
        first_item_id.get_or_insert(item_id);
        project.items.insert(
            item_id,
            TimelineItem {
                id: item_id,
                track_id,
                name: format!("Solid {index}"),
                source: SourceRef::Solid {
                    color: Color {
                        r: ((index * 37) % 255) as u8,
                        g: ((index * 67) % 255) as u8,
                        b: ((index * 97) % 255) as u8,
                        a: 255,
                    },
                },
                interval: TimelineInterval::new(MediaTime::zero(), duration)?,
                time_map: TimeMap::default(),
                layer: index as i64,
                parent: None,
                blend_mode: BlendMode::Normal,
                authored_properties: PropertyMap::new(),
            },
        );
    }
    project.validate()?;
    Ok((
        project,
        track_id,
        first_item_id.unwrap_or_else(|| TimelineItemId::from_uuid(stable_uuid(namespace, 10_000))),
    ))
}

fn shared_module_project(instance_count: usize, namespace: u16) -> BenchResult<AuthoringProject> {
    let (mut project, track_id, _) = solid_project(0, namespace)?;
    let (mut definition, output_id) = ModuleDefinition::new_project_image("Shared Module");
    let definition_id = ModuleDefinitionId::from_uuid(stable_uuid(namespace, 20_000));
    definition.id = definition_id;
    project.module_definitions.insert(definition_id, definition);
    let duration = MediaTime::from_whole_seconds(600);
    for index in 0..instance_count {
        let instance_id =
            ModuleInstanceId::from_uuid(stable_uuid(namespace, 30_000 + index as u64));
        let item_id = TimelineItemId::from_uuid(stable_uuid(namespace, 40_000 + index as u64));
        project.module_instances.insert(
            instance_id,
            ModuleInstance {
                id: instance_id,
                definition_id,
                parameter_overrides: HashMap::new(),
            },
        );
        project.items.insert(
            item_id,
            TimelineItem {
                id: item_id,
                track_id,
                name: format!("Shared Module {index}"),
                source: SourceRef::Module(ModuleInvocation {
                    instance_id,
                    output_id,
                    input_bindings: HashMap::new(),
                    automation_tracks: HashMap::new(),
                }),
                interval: TimelineInterval::new(MediaTime::zero(), duration)?,
                time_map: TimeMap::default(),
                layer: index as i64,
                parent: None,
                blend_mode: BlendMode::Normal,
                authored_properties: PropertyMap::new(),
            },
        );
    }
    project.validate()?;
    Ok(project)
}

fn stabilize_root_ids(
    project: &mut AuthoringProject,
    namespace: u16,
) -> BenchResult<TimelineTrackId> {
    let old_timeline_id = project.root_timeline_id;
    let mut timeline = project
        .timelines
        .remove(&old_timeline_id)
        .ok_or("new Project has no root Timeline")?;
    let old_track_id = *timeline
        .track_order
        .first()
        .ok_or("new Project has no root Track")?;
    let mut track = project
        .tracks
        .remove(&old_track_id)
        .ok_or("new Project root Track is missing")?;
    let timeline_id = TimelineId::from_uuid(stable_uuid(namespace, 1));
    let track_id = TimelineTrackId::from_uuid(stable_uuid(namespace, 2));
    project.root_timeline_id = timeline_id;
    timeline.id = timeline_id;
    timeline.track_order = vec![track_id];
    track.id = track_id;
    track.timeline_id = timeline_id;
    project.timelines.insert(timeline_id, timeline);
    project.tracks.insert(track_id, track);
    Ok(track_id)
}

fn stable_uuid(namespace: u16, value: u64) -> Uuid {
    Uuid::from_u128(
        0x5255_5649_4500_0000_0000_0000_0000_0000_u128
            | (u128::from(namespace) << 64)
            | u128::from(value),
    )
}

fn canonical_project_json(project: &AuthoringProject) -> BenchResult<String> {
    let value = serde_json::to_value(ProjectDocument::new(project.clone()))?;
    Ok(serde_json::to_string_pretty(&value)?)
}

fn sha256_hex(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    digest.iter().map(|byte| format!("{byte:02x}")).collect()
}
