use std::collections::HashSet;
use std::f64::consts::TAU;
use std::path::PathBuf;
use std::sync::Arc;

use ordered_float::OrderedFloat;

use super::FrameEvaluator;
use crate::model::project::{
    AUDIO_OUTPUT_PORT, BAND_HIGH_HZ_PROPERTY, BAND_LOW_HZ_PROPERTY, Composition, EvalOutput,
    NUMBER_RESULT_OUTPUT_PORT, NUMERIC_A_INPUT_PORT, NUMERIC_B_INPUT_PORT, NodeContainer,
    PortAddress, PortDirection, PortOwner, Project, SOUND_INPUT_PORT, SPECTRUM_INPUT_PORT,
    SPECTRUM_OUTPUT_PORT, TIME_PORT,
};
use crate::model::property::{Property, PropertyValue};
use crate::model::{Asset, AssetKind, Clip, MediaContent, Node, SoundAnalysisContent};
use crate::plugin::PluginManager;

struct TestWave(PathBuf);

impl TestWave {
    fn sine(frequency: f64, amplitude: f64, sample_rate: u32, frames: usize) -> Self {
        let path =
            std::env::temp_dir().join(format!("ruvie-sound-analysis-{}.wav", uuid::Uuid::new_v4()));
        let mut bytes = Vec::with_capacity(44 + frames * 2);
        let data_len = u32::try_from(frames * 2).unwrap();
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
        for index in 0..frames {
            let sample =
                amplitude * (TAU * frequency * index as f64 / f64::from(sample_rate)).sin();
            let pcm = (sample * f64::from(i16::MAX)).round() as i16;
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
