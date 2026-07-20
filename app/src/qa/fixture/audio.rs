//! Deterministic audio sources used only by the Node Editor HTTP QA fixture.

use library::editor::project_service::MediaNodeRequest;
use library::editor::ProjectService;
use library::model::node::Node;
use library::model::project::asset::{Asset, AssetKind};
use uuid::Uuid;

pub(super) fn audio_node(
    factory: &ProjectService,
    asset_id: Uuid,
    node_id: Uuid,
    name: &str,
    path: &str,
    ui_position: [f32; 2],
) -> Result<(Asset, Node), String> {
    let mut asset = Asset::new(name, path, AssetKind::Audio);
    asset.id = asset_id;
    let mut node = factory
        .create_media_node(
            name,
            MediaNodeRequest::Audio {
                asset_id,
                file_path: path.to_string(),
                audio_stream_index: None,
            },
            640,
            360,
            1,
            1,
        )
        .map_err(|error| format!("cannot create QA Audio through factory: {error}"))?;
    node.id = node_id;
    node.ui_position = ui_position;
    Ok((asset, node))
}
