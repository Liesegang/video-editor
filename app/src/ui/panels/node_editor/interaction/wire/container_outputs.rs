use eframe::egui;
use library::model::project::{
    ContainerAudioSourceKind, ContainerImageSourceKind, PortAddress, PortDataType, PortOwner,
};
use library::model::Project;
use std::collections::HashMap;

use super::render::register_edge_component;
use crate::ui::panels::node_editor::{
    container_output_binding_port, container_output_port, container_output_type_key, pin_color,
    qa_container_key, EdgeComponent, OverviewWirePainter, RenderedEdge, RenderedEdgeKind,
    RenderedPortKey,
};

#[derive(Clone, Copy)]
enum ProjectedSourceKind {
    OutputBinding,
    DerivedChild,
}

pub(super) fn register_container_output_edges(
    project: &Project,
    owner: PortOwner,
    ports: &HashMap<RenderedPortKey, egui::Rect>,
    canvas_clip: egui::Rect,
    overview: Option<OverviewWirePainter<'_>>,
) -> Vec<RenderedEdge> {
    let mut rendered = Vec::new();
    register_typed_sources(
        owner,
        PortDataType::Image,
        project
            .container_image_sources(owner)
            .into_iter()
            .map(|source| {
                let kind = match source.kind {
                    ContainerImageSourceKind::OutputBinding => ProjectedSourceKind::OutputBinding,
                    ContainerImageSourceKind::DerivedChild => ProjectedSourceKind::DerivedChild,
                };
                (source.source, kind)
            }),
        ports,
        canvas_clip,
        overview,
        &mut rendered,
    );
    register_typed_sources(
        owner,
        PortDataType::Audio,
        project
            .container_audio_sources(owner)
            .into_iter()
            .map(|source| {
                let kind = match source.kind {
                    ContainerAudioSourceKind::OutputBinding => ProjectedSourceKind::OutputBinding,
                    ContainerAudioSourceKind::DerivedChild => ProjectedSourceKind::DerivedChild,
                };
                (source.source, kind)
            }),
        ports,
        canvas_clip,
        overview,
        &mut rendered,
    );
    rendered
}

fn register_typed_sources(
    owner: PortOwner,
    data_type: PortDataType,
    sources: impl Iterator<Item = (PortOwner, ProjectedSourceKind)>,
    ports: &HashMap<RenderedPortKey, egui::Rect>,
    canvas_clip: egui::Rect,
    overview: Option<OverviewWirePainter<'_>>,
    rendered: &mut Vec<RenderedEdge>,
) {
    let (Some(output_port), Some(binding_port), Some(type_key)) = (
        container_output_port(data_type),
        container_output_binding_port(data_type),
        container_output_type_key(data_type),
    ) else {
        return;
    };
    let sink = PortAddress::new(owner, binding_port);
    for (source, source_kind) in sources {
        let from = PortAddress::new(source, output_port);
        let source_key = qa_container_key(source);
        let (id, kind) = match source_kind {
            ProjectedSourceKind::OutputBinding => {
                let PortOwner::Node(node_id) = source else {
                    continue;
                };
                (
                    format!(
                        "node_editor.edge.output_binding:{}:{type_key}:{node_id}",
                        qa_container_key(owner),
                    ),
                    RenderedEdgeKind::OutputBinding {
                        owner,
                        node_id,
                        data_type,
                    },
                )
            }
            ProjectedSourceKind::DerivedChild => (
                format!(
                    "node_editor.edge.derived:{}:{type_key}:{source_key}",
                    qa_container_key(owner)
                ),
                RenderedEdgeKind::DerivedOutput {
                    owner,
                    source,
                    data_type,
                },
            ),
        };
        if let Some(edge) = register_edge_component(
            EdgeComponent {
                id,
                kind,
                from: &from,
                to: &sink,
                wire_color: pin_color(data_type),
                authored_order: None,
                back_to_front_index: None,
                layer_count: None,
                physical_merge_target: false,
                authored_blend_mode: None,
                authored_blend_available: false,
            },
            ports,
            canvas_clip,
            overview,
        ) {
            rendered.push(edge);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::media_node_for_canvas;
    use crate::ui::panels::node_editor::{AUDIO_OUTPUT_BINDING_PORT, IMAGE_OUTPUT_BINDING_PORT};
    use library::editor::project_service::MediaNodeRequest;
    use library::model::asset::{Asset, AssetKind};
    use library::model::project::{
        NodeContainer, PortDirection, AUDIO_OUTPUT_PORT, IMAGE_OUTPUT_PORT,
    };
    use library::model::{Clip, Composition};

    fn routed_video_project() -> (Project, PortOwner, PortOwner, PortOwner, uuid::Uuid) {
        let mut project = Project::new("render typed container wires");
        let (composition, track) = Composition::new("Main", 64, 64, 24.0, 2.0);
        let composition_owner = PortOwner::Composition(composition.id);
        let track_owner = PortOwner::Track(track.id);
        let track_id = track.id;
        project.add_track(track);
        project.add_composition(composition);
        let clip = Clip::new("Video", 0.0, 2.0);
        let clip_owner = PortOwner::Clip(clip.id);
        let clip_id = clip.id;
        project.add_clip(clip);
        project.attach_clip_to_track(track_id, clip_id).unwrap();

        let asset = Asset::new("Video", "/fixture/video.mp4", AssetKind::Video);
        let node = media_node_for_canvas(
            "Video",
            MediaNodeRequest::Video {
                asset_id: asset.id,
                file_path: asset.path.clone(),
                stream_index: None,
                audio_stream_index: None,
            },
            64,
            64,
            64,
            64,
        );
        let node_id = node.id;
        project.assets.push(asset);
        project.add_node(node);
        project
            .attach_node_to_container(NodeContainer::Clip(clip_id), node_id)
            .unwrap();
        project
            .set_output_node(NodeContainer::Clip(clip_id), Some(node_id))
            .unwrap();
        project
            .set_audio_output_node(NodeContainer::Clip(clip_id), Some(node_id))
            .unwrap();
        (project, composition_owner, track_owner, clip_owner, node_id)
    }

    fn insert_port(
        ports: &mut HashMap<RenderedPortKey, egui::Rect>,
        address: PortAddress,
        direction: PortDirection,
        center: egui::Pos2,
    ) {
        ports.insert(
            RenderedPortKey {
                address,
                direction,
                connection_id: None,
            },
            egui::Rect::from_center_size(center, egui::vec2(12.0, 12.0)),
        );
    }

    #[test]
    fn rendered_container_edges_include_bound_and_derived_audio_independently() {
        let (project, composition, track, clip, node_id) = routed_video_project();
        let mut ports = HashMap::new();
        for (index, (owner, source)) in [
            (clip, PortOwner::Node(node_id)),
            (track, clip),
            (composition, track),
        ]
        .into_iter()
        .enumerate()
        {
            for (row, (output, binding)) in [
                (IMAGE_OUTPUT_PORT, IMAGE_OUTPUT_BINDING_PORT),
                (AUDIO_OUTPUT_PORT, AUDIO_OUTPUT_BINDING_PORT),
            ]
            .into_iter()
            .enumerate()
            {
                let y = 40.0 + index as f32 * 80.0 + row as f32 * 24.0;
                insert_port(
                    &mut ports,
                    PortAddress::new(source, output),
                    PortDirection::Output,
                    egui::pos2(100.0, y),
                );
                insert_port(
                    &mut ports,
                    PortAddress::new(owner, binding),
                    PortDirection::Input,
                    egui::pos2(400.0, y),
                );
            }
        }

        let canvas = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(600.0, 400.0));
        let edges = [clip, track, composition]
            .into_iter()
            .flat_map(|owner| {
                register_container_output_edges(&project, owner, &ports, canvas, None)
            })
            .collect::<Vec<_>>();
        assert_eq!(edges.len(), 6);
        for data_type in [PortDataType::Image, PortDataType::Audio] {
            assert!(edges.iter().any(|edge| matches!(
                edge.kind,
                RenderedEdgeKind::OutputBinding {
                    owner,
                    node_id: bound,
                    data_type: edge_type,
                } if owner == clip && bound == node_id && edge_type == data_type
            )));
            assert!(edges.iter().any(|edge| matches!(
                edge.kind,
                RenderedEdgeKind::DerivedOutput {
                    owner,
                    source,
                    data_type: edge_type,
                } if owner == track && source == clip && edge_type == data_type
            )));
            assert!(edges.iter().any(|edge| matches!(
                edge.kind,
                RenderedEdgeKind::DerivedOutput {
                    owner,
                    source,
                    data_type: edge_type,
                } if owner == composition && source == track && edge_type == data_type
            )));
        }
    }
}
