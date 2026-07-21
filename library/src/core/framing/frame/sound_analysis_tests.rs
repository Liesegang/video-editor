use std::collections::HashSet;
use std::f64::consts::TAU;
use std::path::PathBuf;
use std::sync::{Arc, RwLock};

use ordered_float::OrderedFloat;

use super::FrameEvaluator;
use crate::cache::CacheManager;
use crate::core::audio::analysis::rms;
use crate::core::audio::mixer::render_owner_samples;
use crate::editor::project_service::ProjectManager;
use crate::framing::get_frame_from_project;
use crate::model::frame::color::Color;
use crate::model::frame::entity::{FrameGroup, FrameItem};
use crate::model::project::{
    ANALYSIS_HOP_MS_PROPERTY, ANALYSIS_SAMPLE_RATE_PROPERTY, ANALYSIS_WINDOW_MS_PROPERTY,
    AUDIO_OUTPUT_PORT, BAND_HIGH_HZ_PROPERTY, BAND_LOW_HZ_PROPERTY, Composition, EvalOutput,
    FMOD_DIVISOR_INPUT_PORT, FMOD_X_INPUT_PORT, IMAGE_INPUT_PORT, IMAGE_OUTPUT_PORT,
    NUMBER_RESULT_OUTPUT_PORT, NUMERIC_A_INPUT_PORT, NUMERIC_B_INPUT_PORT, NodeContainer,
    PortAddress, PortDirection, PortOwner, Project, ProjectConnection, SOUND_INPUT_PORT,
    SPECTRUM_INPUT_PORT, SPECTRUM_OUTPUT_PORT, TIME_PORT,
};
use crate::model::property::{Property, PropertyValue};
use crate::model::{Asset, AssetKind, Clip, MediaContent, Node, SoundAnalysisContent};
use crate::plugin::{PluginManager, property_port_key};

struct TestWave(PathBuf);

impl TestWave {
    fn sine(frequency: f64, amplitude: f64, sample_rate: u32, frames: usize) -> Self {
        Self::from_samples(
            sample_rate,
            (0..frames).map(|index| {
                amplitude * (TAU * frequency * index as f64 / f64::from(sample_rate)).sin()
            }),
        )
    }

    fn from_samples(sample_rate: u32, samples: impl IntoIterator<Item = f64>) -> Self {
        let samples = samples.into_iter().collect::<Vec<_>>();
        let path =
            std::env::temp_dir().join(format!("ruvie-sound-analysis-{}.wav", uuid::Uuid::new_v4()));
        let mut bytes = Vec::with_capacity(44 + samples.len() * 2);
        let data_len = u32::try_from(samples.len() * 2).unwrap();
        bytes.extend_from_slice(b"RIFF");
        bytes.extend_from_slice(&(36 + data_len).to_le_bytes());
        bytes.extend_from_slice(b"WAVEfmt ");
        bytes.extend_from_slice(&16_u32.to_le_bytes());
        bytes.extend_from_slice(&1_u16.to_le_bytes());
        bytes.extend_from_slice(&1_u16.to_le_bytes());
        bytes.extend_from_slice(&sample_rate.to_le_bytes());
        bytes.extend_from_slice(&(sample_rate * 2).to_le_bytes());
        bytes.extend_from_slice(&2_u16.to_le_bytes());
        bytes.extend_from_slice(&16_u16.to_le_bytes());
        bytes.extend_from_slice(b"data");
        bytes.extend_from_slice(&data_len.to_le_bytes());
        for sample in samples {
            let pcm = (sample.clamp(-1.0, 1.0) * f64::from(i16::MAX)).round() as i16;
            bytes.extend_from_slice(&pcm.to_le_bytes());
        }
        std::fs::write(&path, bytes).unwrap();
        Self(path)
    }

    fn path(&self) -> String {
        self.0.to_string_lossy().into_owned()
    }
}

impl Drop for TestWave {
    fn drop(&mut self) {
        if let Err(error) = std::fs::remove_file(&self.0) {
            log::warn!("failed to remove Sound analysis test wave: {error}");
        }
    }
}

fn add_to_clip(project: &mut Project, clip_id: uuid::Uuid, node: Node) -> uuid::Uuid {
    let node_id = node.id;
    project.add_node(node);
    project
        .attach_node_to_container(NodeContainer::Clip(clip_id), node_id)
        .unwrap();
    node_id
}

fn constant_number(value: f64) -> Property {
    Property::constant(PropertyValue::Number(OrderedFloat(value)))
}

fn connect_number_setting(
    project: &mut Project,
    clip_id: uuid::Uuid,
    target_node_id: uuid::Uuid,
    target_property: &str,
    value_at_half_second: f64,
) -> uuid::Uuid {
    let mut source = Node::new_add(&format!("{target_property} wire"));
    source
        .set_property(
            NUMERIC_B_INPUT_PORT.to_string(),
            constant_number(value_at_half_second - 0.5),
        )
        .unwrap();
    let source_id = add_to_clip(project, clip_id, source);
    project
        .connect_ports(
            PortAddress::new(PortOwner::Clip(clip_id), TIME_PORT),
            PortAddress::new(PortOwner::Node(source_id), NUMERIC_A_INPUT_PORT),
        )
        .unwrap();
    project
        .connect_ports(
            PortAddress::new(PortOwner::Node(source_id), NUMBER_RESULT_OUTPUT_PORT),
            PortAddress::new(PortOwner::Node(target_node_id), target_property),
        )
        .unwrap();
    source_id
}

fn find_group(items: &[FrameItem], source_id: uuid::Uuid) -> Option<&FrameGroup> {
    items.iter().find_map(|item| match item {
        FrameItem::Group(group) if group.source_id == source_id => Some(group),
        FrameItem::Group(group) => find_group(&group.items, source_id),
        FrameItem::Object(_) => None,
    })
}

#[test]
fn sound_analysis_drives_numeric_property_wire_and_preserves_spectrum_identity() {
    const SAMPLE_RATE: u32 = 48_000;
    let wave = TestWave::sine(1_000.0, 0.5, SAMPLE_RATE, SAMPLE_RATE as usize);
    let mut project = Project::new("sound analysis property wire");
    let (composition, track) = Composition::new("main", 32, 32, 30.0, 1.0);
    let composition_id = composition.id;
    let track_id = track.id;
    project.add_track(track).unwrap();
    project.add_composition(composition).unwrap();
    let clip = Clip::new("tone", 0.0, 1.0);
    let clip_id = clip.id;
    project.add_clip(clip);
    project.attach_clip_to_track(track_id, clip_id).unwrap();

    let mut asset = Asset::new("tone", &wave.path(), AssetKind::Audio);
    asset.duration = Some(1.0);
    let media = Node::from_media_converter(
        "tone",
        MediaContent {
            asset_id: asset.id,
            stream_index: None,
            audio_stream_index: None,
        },
        &[],
        asset.path.clone(),
    )
    .unwrap();
    project.assets.push(asset);
    let media_id = add_to_clip(&mut project, clip_id, media);
    project
        .set_audio_output_node(NodeContainer::Clip(clip_id), Some(media_id))
        .unwrap();

    let rms_id = add_to_clip(
        &mut project,
        clip_id,
        Node::new_sound_analysis("RMS", SoundAnalysisContent::Rms),
    );
    let peak_id = add_to_clip(
        &mut project,
        clip_id,
        Node::new_sound_analysis("Peak", SoundAnalysisContent::Peak),
    );
    let spectrum_id = add_to_clip(
        &mut project,
        clip_id,
        Node::new_sound_analysis("FFT", SoundAnalysisContent::Spectrum),
    );
    let mut band = Node::new_sound_analysis("Band", SoundAnalysisContent::BandEnergy);
    band.set_property(BAND_LOW_HZ_PROPERTY.to_string(), constant_number(950.0))
        .unwrap();
    band.set_property(BAND_HIGH_HZ_PROPERTY.to_string(), constant_number(1_050.0))
        .unwrap();
    let band_id = add_to_clip(&mut project, clip_id, band);
    let add_id = add_to_clip(&mut project, clip_id, Node::new_add("Time + RMS"));
    let window_source_id = connect_number_setting(
        &mut project,
        clip_id,
        rms_id,
        ANALYSIS_WINDOW_MS_PROPERTY,
        50.0,
    );
    connect_number_setting(
        &mut project,
        clip_id,
        rms_id,
        ANALYSIS_HOP_MS_PROPERTY,
        10.0,
    );
    connect_number_setting(
        &mut project,
        clip_id,
        rms_id,
        ANALYSIS_SAMPLE_RATE_PROPERTY,
        f64::from(SAMPLE_RATE),
    );
    connect_number_setting(&mut project, clip_id, band_id, BAND_LOW_HZ_PROPERTY, 950.0);
    connect_number_setting(
        &mut project,
        clip_id,
        band_id,
        BAND_HIGH_HZ_PROPERTY,
        1_050.0,
    );

    for analysis_id in [rms_id, peak_id, spectrum_id] {
        project
            .connect_ports(
                PortAddress::new(PortOwner::Node(media_id), AUDIO_OUTPUT_PORT),
                PortAddress::new(PortOwner::Node(analysis_id), SOUND_INPUT_PORT),
            )
            .unwrap();
    }
    let spectrum_connection_id = project
        .connect_ports(
            PortAddress::new(PortOwner::Node(spectrum_id), SPECTRUM_OUTPUT_PORT),
            PortAddress::new(PortOwner::Node(band_id), SPECTRUM_INPUT_PORT),
        )
        .unwrap();
    project
        .connect_ports(
            PortAddress::new(PortOwner::Clip(clip_id), TIME_PORT),
            PortAddress::new(PortOwner::Node(add_id), NUMERIC_A_INPUT_PORT),
        )
        .unwrap();
    let property_connection_id = project
        .connect_ports(
            PortAddress::new(PortOwner::Node(rms_id), NUMBER_RESULT_OUTPUT_PORT),
            PortAddress::new(PortOwner::Node(add_id), NUMERIC_B_INPUT_PORT),
        )
        .unwrap();

    assert!(project.validate_connections().is_empty());
    let spectrum_connection = project
        .connections
        .iter()
        .find(|connection| connection.id == spectrum_connection_id)
        .unwrap();
    assert_eq!(
        spectrum_connection.from,
        PortAddress::new(PortOwner::Node(spectrum_id), SPECTRUM_OUTPUT_PORT)
    );
    assert_eq!(
        spectrum_connection.to,
        PortAddress::new(PortOwner::Node(band_id), SPECTRUM_INPUT_PORT)
    );
    assert_eq!(
        project
            .port_definition(&spectrum_connection.from, PortDirection::Output)
            .unwrap()
            .data_type,
        crate::model::project::PortDataType::Spectrum
    );
    let property_connection = project
        .connections
        .iter()
        .find(|connection| connection.id == property_connection_id)
        .unwrap();
    assert_eq!(
        property_connection.to,
        PortAddress::new(PortOwner::Node(add_id), NUMERIC_B_INPUT_PORT),
        "the analysis result must remain a canonical wire into the real numeric property input"
    );

    let plugin_manager = Arc::new(PluginManager::default());
    let evaluator = FrameEvaluator::new(
        &project,
        project.get_composition(composition_id).unwrap(),
        plugin_manager.get_property_evaluators(),
        plugin_manager.as_ref(),
    );
    let evaluate = |node_id| {
        evaluator
            .resolve_metadata_value(
                &PortAddress::new(PortOwner::Node(node_id), NUMBER_RESULT_OUTPUT_PORT),
                0.5,
                &mut HashSet::new(),
            )
            .unwrap()
    };
    let EvalOutput::Produced(PropertyValue::Number(sum)) = evaluate(add_id) else {
        panic!("RMS property wire must produce the downstream Add output");
    };
    assert!((sum.into_inner() - (0.5 + 0.5 / 2.0_f64.sqrt())).abs() < 2.0e-3);
    let EvalOutput::Produced(PropertyValue::Number(peak)) = evaluate(peak_id) else {
        panic!("Peak must produce a Number");
    };
    assert!((peak.into_inner() - 0.5).abs() < 2.0e-3);
    let EvalOutput::Produced(PropertyValue::Number(energy)) = evaluate(band_id) else {
        panic!("FFT -> Band Energy must produce a Number");
    };
    assert!(energy.into_inner() > 0.45);

    let window_target = PortAddress::new(PortOwner::Node(rms_id), ANALYSIS_WINDOW_MS_PROPERTY);
    for malformed_source in [
        PortAddress::new(PortOwner::Node(media_id), AUDIO_OUTPUT_PORT),
        PortAddress::new(PortOwner::Node(rms_id), NUMBER_RESULT_OUTPUT_PORT),
    ] {
        let mut malformed = project.clone();
        malformed
            .connections
            .retain(|connection| connection.to != window_target);
        malformed.connections.push(ProjectConnection::new(
            malformed_source,
            window_target.clone(),
            0,
        ));
        let malformed_evaluator = FrameEvaluator::new(
            &malformed,
            malformed.get_composition(composition_id).unwrap(),
            plugin_manager.get_property_evaluators(),
            plugin_manager.as_ref(),
        );
        assert!(
            malformed_evaluator
                .resolve_metadata_value(
                    &PortAddress::new(PortOwner::Node(rms_id), NUMBER_RESULT_OUTPUT_PORT),
                    0.5,
                    &mut HashSet::new(),
                )
                .is_err(),
            "malformed typed or cyclic analysis setting wires must fail closed"
        );
    }

    project.get_node_mut(window_source_id).unwrap().enabled = false;
    let evaluator = FrameEvaluator::new(
        &project,
        project.get_composition(composition_id).unwrap(),
        plugin_manager.get_property_evaluators(),
        plugin_manager.as_ref(),
    );
    assert_eq!(
        evaluator
            .resolve_metadata_value(
                &PortAddress::new(PortOwner::Node(rms_id), NUMBER_RESULT_OUTPUT_PORT),
                0.5,
                &mut HashSet::new(),
            )
            .unwrap(),
        EvalOutput::NoOutput,
        "a connected analysis setting that produces NoOutput must not fall back to authored data"
    );

    project.get_node_mut(window_source_id).unwrap().enabled = true;
    project.get_node_mut(rms_id).unwrap().enabled = false;
    let evaluator = FrameEvaluator::new(
        &project,
        project.get_composition(composition_id).unwrap(),
        plugin_manager.get_property_evaluators(),
        plugin_manager.as_ref(),
    );
    assert_eq!(
        evaluator
            .resolve_metadata_value(
                &PortAddress::new(PortOwner::Node(add_id), NUMBER_RESULT_OUTPUT_PORT),
                0.5,
                &mut HashSet::new(),
            )
            .unwrap(),
        EvalOutput::NoOutput,
        "disabled Sound analysis must propagate NoOutput through its property wire"
    );
    assert_eq!(
        evaluator
            .resolve_metadata_value(
                &PortAddress::new(PortOwner::Node(peak_id), NUMBER_RESULT_OUTPUT_PORT),
                1.0,
                &mut HashSet::new(),
            )
            .unwrap(),
        EvalOutput::NoOutput,
        "the Clip's half-open range must gate Sound analysis"
    );
}

#[test]
fn analysis_window_uses_composition_time_before_clip_stretch_and_explicit_fmod() {
    const SAMPLE_RATE: u32 = 8_000;
    let wave = TestWave::from_samples(
        SAMPLE_RATE,
        (0..SAMPLE_RATE).map(|index| f64::from(index) / f64::from(SAMPLE_RATE)),
    );
    let mut project = Project::new("retimed sound analysis window");
    let (composition, track) = Composition::new("main", 32, 32, 30.0, 2.0);
    let composition_id = composition.id;
    let track_id = track.id;
    project.add_track(track).unwrap();
    project.add_composition(composition).unwrap();

    let mut clip = Clip::new("retimed", 0.2, 1.4);
    clip.trim_in = OrderedFloat(0.1);
    clip.time_stretch = OrderedFloat(2.0);
    let clip_id = clip.id;
    project.add_clip(clip);
    project.attach_clip_to_track(track_id, clip_id).unwrap();

    let mut asset = Asset::new("ramp", &wave.path(), AssetKind::Audio);
    asset.duration = Some(1.0);
    let media = Node::from_media_converter(
        "ramp",
        MediaContent {
            asset_id: asset.id,
            stream_index: None,
            audio_stream_index: None,
        },
        &[],
        asset.path.clone(),
    )
    .unwrap();
    project.assets.push(asset);
    let media_id = add_to_clip(&mut project, clip_id, media);
    project
        .set_audio_output_node(NodeContainer::Clip(clip_id), Some(media_id))
        .unwrap();

    let mut modulo = Node::new_fmod("loop source time");
    modulo
        .set_property(FMOD_DIVISOR_INPUT_PORT.to_string(), constant_number(0.5))
        .unwrap();
    let modulo_id = add_to_clip(&mut project, clip_id, modulo);
    let mut analysis = Node::new_sound_analysis("RMS", SoundAnalysisContent::Rms);
    for (key, value) in [
        (ANALYSIS_WINDOW_MS_PROPERTY, 100.0),
        (ANALYSIS_HOP_MS_PROPERTY, 200.0),
        (ANALYSIS_SAMPLE_RATE_PROPERTY, f64::from(SAMPLE_RATE)),
    ] {
        analysis
            .set_property(key.to_string(), constant_number(value))
            .unwrap();
    }
    let analysis_id = add_to_clip(&mut project, clip_id, analysis);
    project
        .connect_ports(
            PortAddress::new(PortOwner::Clip(clip_id), TIME_PORT),
            PortAddress::new(PortOwner::Node(modulo_id), FMOD_X_INPUT_PORT),
        )
        .unwrap();
    project
        .connect_ports(
            PortAddress::new(PortOwner::Node(modulo_id), NUMBER_RESULT_OUTPUT_PORT),
            PortAddress::new(PortOwner::Node(media_id), TIME_PORT),
        )
        .unwrap();
    project
        .connect_ports(
            PortAddress::new(PortOwner::Node(media_id), AUDIO_OUTPUT_PORT),
            PortAddress::new(PortOwner::Node(analysis_id), SOUND_INPUT_PORT),
        )
        .unwrap();
    assert!(project.validate_connections().is_empty());

    let plugins = Arc::new(PluginManager::default());
    let evaluator = FrameEvaluator::new(
        &project,
        project.get_composition(composition_id).unwrap(),
        plugins.get_property_evaluators(),
        plugins.as_ref(),
    );
    let EvalOutput::Produced(PropertyValue::Number(observed)) = evaluator
        .resolve_metadata_value(
            &PortAddress::new(PortOwner::Node(analysis_id), NUMBER_RESULT_OUTPUT_PORT),
            0.63,
            &mut HashSet::new(),
        )
        .unwrap()
    else {
        panic!("retimed RMS must produce a Number");
    };

    let composition = project.get_composition(composition_id).unwrap();
    let expected_samples = render_owner_samples(
        &project,
        composition,
        &PortAddress::new(PortOwner::Node(media_id), AUDIO_OUTPUT_PORT),
        &CacheManager::new(),
        4_400,
        800,
        SAMPLE_RATE,
        0.6,
        plugins.as_ref(),
    )
    .unwrap();
    let old_local_time_samples = render_owner_samples(
        &project,
        composition,
        &PortAddress::new(PortOwner::Node(media_id), AUDIO_OUTPUT_PORT),
        &CacheManager::new(),
        3_360,
        800,
        SAMPLE_RATE,
        0.47,
        plugins.as_ref(),
    )
    .unwrap();
    let expected = rms(&expected_samples);
    let old_local_time_result = rms(&old_local_time_samples);
    assert!((observed.into_inner() - expected).abs() < 1.0e-9);
    assert!(
        (expected - old_local_time_result).abs() > 0.03,
        "the fixture must distinguish global-window quantization from the old local-time bug"
    );
}

#[test]
fn sound_rms_changes_a_real_image_transform_property_with_audio_level() {
    const SAMPLE_RATE: u32 = 8_000;
    let wave = TestWave::from_samples(
        SAMPLE_RATE,
        (0..SAMPLE_RATE).map(|index| {
            let amplitude = if index < SAMPLE_RATE / 2 { 0.1 } else { 0.8 };
            amplitude * (TAU * 1_000.0 * f64::from(index) / f64::from(SAMPLE_RATE)).sin()
        }),
    );
    let plugins = Arc::new(PluginManager::default());
    let factory = ProjectManager::new(
        Arc::new(RwLock::new(Project::new("sound-driven transform factory"))),
        plugins.clone(),
    );
    let mut project = Project::new("sound-driven transform");
    let (composition, track) = Composition::new("main", 64, 64, 20.0, 1.0);
    let track_id = track.id;
    project.add_track(track).unwrap();
    project.add_composition(composition).unwrap();
    let clip = Clip::new("sound and image", 0.0, 1.0);
    let clip_id = clip.id;
    project.add_clip(clip);
    project.attach_clip_to_track(track_id, clip_id).unwrap();

    let mut asset = Asset::new("two levels", &wave.path(), AssetKind::Audio);
    asset.duration = Some(1.0);
    let media = Node::from_media_converter(
        "two levels",
        MediaContent {
            asset_id: asset.id,
            stream_index: None,
            audio_stream_index: None,
        },
        &[],
        asset.path.clone(),
    )
    .unwrap();
    project.assets.push(asset);
    let media_id = add_to_clip(&mut project, clip_id, media);

    let mut analysis = Node::new_sound_analysis("RMS", SoundAnalysisContent::Rms);
    for (key, value) in [
        (ANALYSIS_WINDOW_MS_PROPERTY, 100.0),
        (ANALYSIS_HOP_MS_PROPERTY, 50.0),
        (ANALYSIS_SAMPLE_RATE_PROPERTY, f64::from(SAMPLE_RATE)),
    ] {
        analysis
            .set_property(key.to_string(), constant_number(value))
            .unwrap();
    }
    let analysis_id = add_to_clip(&mut project, clip_id, analysis);
    let mut amplify = Node::new_multiply("RMS x 100");
    amplify
        .set_property(NUMERIC_B_INPUT_PORT.to_string(), constant_number(100.0))
        .unwrap();
    let amplify_id = add_to_clip(&mut project, clip_id, amplify);
    let solid = factory.create_solid_node(Color::white(), 64, 64).unwrap();
    let solid_id = add_to_clip(&mut project, clip_id, solid);
    let transform = plugins.create_image_transform_operation_node().unwrap();
    let transform_id = add_to_clip(&mut project, clip_id, transform);

    for (from, to) in [
        (
            PortAddress::new(PortOwner::Node(media_id), AUDIO_OUTPUT_PORT),
            PortAddress::new(PortOwner::Node(analysis_id), SOUND_INPUT_PORT),
        ),
        (
            PortAddress::new(PortOwner::Node(analysis_id), NUMBER_RESULT_OUTPUT_PORT),
            PortAddress::new(PortOwner::Node(amplify_id), NUMERIC_A_INPUT_PORT),
        ),
        (
            PortAddress::new(PortOwner::Node(amplify_id), NUMBER_RESULT_OUTPUT_PORT),
            PortAddress::new(PortOwner::Node(transform_id), property_port_key("rotation")),
        ),
        (
            PortAddress::new(PortOwner::Node(solid_id), IMAGE_OUTPUT_PORT),
            PortAddress::new(PortOwner::Node(transform_id), IMAGE_INPUT_PORT),
        ),
    ] {
        project.connect_ports(from, to).unwrap();
    }
    project
        .set_audio_output_node(NodeContainer::Clip(clip_id), Some(media_id))
        .unwrap();
    project
        .set_output_node(NodeContainer::Clip(clip_id), Some(transform_id))
        .unwrap();
    assert!(project.validate_connections().is_empty());

    let evaluate_rotation = |frame_number| {
        let frame = get_frame_from_project(
            &project,
            0,
            frame_number,
            1.0,
            None,
            &plugins.get_property_evaluators(),
            &plugins,
        )
        .unwrap();
        find_group(&frame.items, transform_id)
            .expect("Image Transform must be present in the evaluated frame")
            .transform
            .rotation
    };
    let quiet_rotation = evaluate_rotation(5);
    let loud_rotation = evaluate_rotation(15);
    assert!(quiet_rotation > 6.0 && quiet_rotation < 8.0);
    assert!(loud_rotation > 55.0 && loud_rotation < 58.0);
    assert!(loud_rotation > quiet_rotation * 7.5);
}
