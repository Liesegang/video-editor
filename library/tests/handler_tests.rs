//! Integration tests for track/clip creation workflow.
//!
//! Verifies the full flow: add track → create clip → add to track → verify graph nodes.

use std::sync::{Arc, RwLock};

use library::plugin::PluginManager;
use library::project::clip::TrackClipKind;
use library::project::node::Node;
use library::project::project::{Composition, Project};

use library::service::handlers::clip_factory::ClipFactory;
use library::service::handlers::clip_handler::ClipHandler;
use library::service::handlers::track_handler::TrackHandler;

/// Helper: create a PluginManager (still needed for setup_clip_graph_nodes).
fn make_plugin_manager() -> PluginManager {
    PluginManager::default()
}

/// Helper: create a Project with one composition and its root track.
fn setup_project() -> (Arc<RwLock<Project>>, uuid::Uuid, uuid::Uuid) {
    let mut project = Project::new("Test Project");
    let (comp, root_track) = Composition::new("Main Composition", 1920, 1080, 30.0, 60.0);
    let comp_id = comp.id;
    let root_track_id = comp.root_track_id;
    project.add_node(Node::Track(root_track));
    project.add_composition(comp);
    (Arc::new(RwLock::new(project)), comp_id, root_track_id)
}

#[test]
fn test_add_track_to_composition() {
    // トラックをコンポジションに追加できることを確認
    let (project, comp_id, root_track_id) = setup_project();

    let track_id = TrackHandler::add_track(&project, comp_id, "New Track").unwrap();

    let proj = project.read().unwrap();
    // トラックが nodes レジストリに存在する
    let track = proj
        .get_track(track_id)
        .expect("Track should exist in nodes");
    assert_eq!(track.name, "New Track");
    // ルートトラックの子として追加されている
    let root_track = proj.get_track(root_track_id).unwrap();
    assert!(root_track.child_ids.contains(&track_id));
}

#[test]
fn test_add_text_clip_creates_full_graph() {
    // テキストクリップ追加時にレイヤー + fill + transform + 正しい接続が作成される
    let (project, comp_id, _root_track_id) = setup_project();
    let plugin_manager = make_plugin_manager();

    // 1. トラックを追加
    let track_id = TrackHandler::add_track(&project, comp_id, "Video Track").unwrap();

    // 2. テキストクリップを作成
    let text_clip = ClipFactory::create_text_clip("Hello", 0, 90, 30.0);
    let clip_kind = text_clip.kind.clone();

    // 3. クリップをトラックに追加
    let clip_id =
        ClipHandler::add_clip_to_track(&project, comp_id, track_id, text_clip, 0, 90, None)
            .unwrap();

    // 4. グラフノードをセットアップ (レイヤー + fill + transform + 接続)
    ClipHandler::setup_clip_graph_nodes(&project, &plugin_manager, track_id, clip_id, &clip_kind)
        .unwrap();

    let proj = project.read().unwrap();

    // クリップが存在する
    assert!(proj.get_clip(clip_id).is_some(), "Clip should exist");

    // トラックの子にレイヤーが追加されている (クリップは直接トラックの子ではない)
    let track = proj.get_track(track_id).unwrap();
    assert_eq!(
        track.child_ids.len(),
        1,
        "Track should have 1 child (layer)"
    );
    let layer_id = track.child_ids[0];

    // レイヤーはトラック (サブトラック) として存在
    let layer = proj
        .get_track(layer_id)
        .expect("Layer sub-track should exist");
    assert_eq!(layer.name, "Layer");

    // レイヤーの子にクリップがある
    assert!(
        layer.child_ids.contains(&clip_id),
        "Layer should contain the clip"
    );

    // レイヤー内にグラフノード (fill, transform) がある
    let graph_nodes_in_layer: Vec<_> = layer
        .child_ids
        .iter()
        .filter(|id| proj.get_graph_node(**id).is_some())
        .collect();
    assert_eq!(
        graph_nodes_in_layer.len(),
        2,
        "Layer should have 2 graph nodes (fill + transform)"
    );

    // fill ノードと transform ノードを特定
    let fill_id = layer
        .child_ids
        .iter()
        .find(|id| {
            proj.get_graph_node(**id)
                .map_or(false, |n| n.type_id.contains("fill"))
        })
        .expect("Fill node should exist in layer");

    let transform_id = layer
        .child_ids
        .iter()
        .find(|id| {
            proj.get_graph_node(**id)
                .map_or(false, |n| n.type_id.contains("transform"))
        })
        .expect("Transform node should exist in layer");

    // 接続を検証: clip.shape_out → fill.shape_in
    let has_shape_connection = proj.connections.iter().any(|c| {
        c.from.node_id == clip_id
            && c.from.pin_name == "shape_out"
            && c.to.node_id == *fill_id
            && c.to.pin_name == "shape_in"
    });
    assert!(
        has_shape_connection,
        "clip.shape_out → fill.shape_in connection should exist"
    );

    // 接続を検証: fill.image_out → transform.image_in
    let has_fill_transform = proj.connections.iter().any(|c| {
        c.from.node_id == *fill_id
            && c.from.pin_name == "image_out"
            && c.to.node_id == *transform_id
            && c.to.pin_name == "image_in"
    });
    assert!(
        has_fill_transform,
        "fill.image_out → transform.image_in connection should exist"
    );

    // 接続を検証: transform.image_out → layer.image_out
    let has_container_output = proj.connections.iter().any(|c| {
        c.from.node_id == *transform_id
            && c.from.pin_name == "image_out"
            && c.to.node_id == layer_id
            && c.to.pin_name == "image_out"
    });
    assert!(
        has_container_output,
        "transform.image_out → layer.image_out connection should exist"
    );
}

#[test]
fn test_add_shape_clip_creates_fill_chain() {
    // シェイプクリップも同様の fill チェーンが作成される
    let (project, comp_id, _) = setup_project();
    let plugin_manager = make_plugin_manager();

    let track_id = TrackHandler::add_track(&project, comp_id, "Shape Track").unwrap();
    let shape_clip = ClipFactory::create_shape_clip(0, 60, 30.0);
    let clip_kind = shape_clip.kind.clone();
    assert_eq!(clip_kind, TrackClipKind::Shape);

    let clip_id =
        ClipHandler::add_clip_to_track(&project, comp_id, track_id, shape_clip, 0, 60, None)
            .unwrap();
    ClipHandler::setup_clip_graph_nodes(&project, &plugin_manager, track_id, clip_id, &clip_kind)
        .unwrap();

    let proj = project.read().unwrap();
    let track = proj.get_track(track_id).unwrap();
    let layer_id = track.child_ids[0];

    // shape_out → fill.shape_in 接続がある
    let has_shape_out = proj
        .connections
        .iter()
        .any(|c| c.from.node_id == clip_id && c.from.pin_name == "shape_out");
    assert!(has_shape_out, "Shape clip should have shape_out connection");

    // container output がある
    let has_container = proj
        .connections
        .iter()
        .any(|c| c.to.node_id == layer_id && c.to.pin_name == "image_out");
    assert!(
        has_container,
        "Layer should have container output connection"
    );
}

#[test]
fn test_add_image_clip_creates_direct_image_chain() {
    // Image クリップは fill なしで直接 image_out → transform の接続
    let (project, comp_id, _) = setup_project();
    let plugin_manager = make_plugin_manager();

    let track_id = TrackHandler::add_track(&project, comp_id, "Image Track").unwrap();
    let image_clip = ClipFactory::create_image_clip(None, "/path/to/image.png", 0, 90, 30.0);
    let clip_kind = image_clip.kind.clone();

    let clip_id =
        ClipHandler::add_clip_to_track(&project, comp_id, track_id, image_clip, 0, 90, None)
            .unwrap();
    ClipHandler::setup_clip_graph_nodes(&project, &plugin_manager, track_id, clip_id, &clip_kind)
        .unwrap();

    let proj = project.read().unwrap();
    let track = proj.get_track(track_id).unwrap();
    let layer_id = track.child_ids[0];
    let layer = proj.get_track(layer_id).unwrap();

    // Image クリップは fill ノードを持たない (transform のみ)
    let graph_nodes: Vec<_> = layer
        .child_ids
        .iter()
        .filter(|id| proj.get_graph_node(**id).is_some())
        .collect();
    assert_eq!(
        graph_nodes.len(),
        1,
        "Image layer should have only 1 graph node (transform)"
    );

    // clip.image_out → transform.image_in
    let has_image_connection = proj
        .connections
        .iter()
        .any(|c| c.from.node_id == clip_id && c.from.pin_name == "image_out");
    assert!(
        has_image_connection,
        "Image clip should have image_out connection"
    );

    // fill ノードが存在しない
    let has_fill = layer.child_ids.iter().any(|id| {
        proj.get_graph_node(*id)
            .map_or(false, |n| n.type_id.contains("fill"))
    });
    assert!(!has_fill, "Image layer should NOT have a fill node");
}

#[test]
fn test_remove_clip_cleans_up_graph() {
    // クリップ削除時にレイヤー・グラフノード・接続が全て削除される
    let (project, comp_id, _) = setup_project();
    let plugin_manager = make_plugin_manager();

    let track_id = TrackHandler::add_track(&project, comp_id, "Track").unwrap();
    let text_clip = ClipFactory::create_text_clip("Test", 0, 30, 30.0);
    let clip_kind = text_clip.kind.clone();
    let clip_id =
        ClipHandler::add_clip_to_track(&project, comp_id, track_id, text_clip, 0, 30, None)
            .unwrap();
    ClipHandler::setup_clip_graph_nodes(&project, &plugin_manager, track_id, clip_id, &clip_kind)
        .unwrap();

    // 削除前: ノードと接続が存在する
    {
        let proj = project.read().unwrap();
        assert!(proj.get_clip(clip_id).is_some());
        assert!(!proj.connections.is_empty());
    }

    // レイヤー ID を取得 (クリップの親)
    let layer_id = {
        let proj = project.read().unwrap();
        let track = proj.get_track(track_id).unwrap();
        track.child_ids[0]
    };

    // クリップをレイヤーから削除
    ClipHandler::remove_clip_from_track(&project, layer_id, clip_id).unwrap();

    // 削除後: クリップとその接続が消えている
    let proj = project.read().unwrap();
    assert!(proj.get_clip(clip_id).is_none(), "Clip should be removed");

    // clip_id に関連する接続は全て消えている
    let clip_connections = proj
        .connections
        .iter()
        .filter(|c| c.from.node_id == clip_id || c.to.node_id == clip_id)
        .count();
    assert_eq!(
        clip_connections, 0,
        "All clip connections should be removed"
    );
}

// ==================== Clip-to-track validation tests ====================

#[test]
fn test_add_clip_rejects_nonexistent_composition() {
    // 存在しないコンポジションIDを指定した場合、クリップ追加は失敗すべき
    let (project, _comp_id, _root_track_id) = setup_project();

    // 正規のトラックを追加
    let track_id = TrackHandler::add_track(&project, _comp_id, "Track").unwrap();

    // テキストクリップを作成
    let clip = ClipFactory::create_text_clip("Hello", 0, 90, 30.0);

    // 存在しないコンポジションIDで追加を試みる
    let fake_comp_id = uuid::Uuid::new_v4();
    let result =
        ClipHandler::add_clip_to_track(&project, fake_comp_id, track_id, clip, 0, 90, None);

    assert!(
        result.is_err(),
        "Adding a clip with a non-existent composition_id should fail"
    );
}

#[test]
fn test_add_clip_rejects_track_not_in_composition() {
    // 別のコンポジションに属するトラックにクリップを追加しようとした場合、失敗すべき
    let (project, comp_a_id, _root_a) = setup_project();

    // 2つ目のコンポジションを作成
    let (comp_b, root_track_b) = Composition::new("Comp B", 1920, 1080, 30.0, 60.0);
    let comp_b_id = comp_b.id;
    {
        let mut proj = project.write().unwrap();
        proj.add_node(Node::Track(root_track_b));
        proj.add_composition(comp_b);
    }

    // コンポジションBにトラックを追加
    let track_in_b = TrackHandler::add_track(&project, comp_b_id, "Track in B").unwrap();

    // テキストクリップを作成
    let clip = ClipFactory::create_text_clip("Hello", 0, 90, 30.0);

    // コンポジションAのIDだが、コンポジションBのトラックを指定 → 失敗すべき
    let result = ClipHandler::add_clip_to_track(&project, comp_a_id, track_in_b, clip, 0, 90, None);

    assert!(
        result.is_err(),
        "Adding a clip to a track that belongs to a different composition should fail"
    );

    // クリップがノードレジストリに追加されていないことを確認
    let proj = project.read().unwrap();
    assert_eq!(
        proj.all_clips().count(),
        0,
        "No clips should exist after failed add"
    );
}

#[test]
fn test_add_clip_rejects_orphan_track() {
    // どのコンポジションにも属さない孤立トラックにクリップを追加しようとした場合、失敗すべき
    let (project, comp_id, _root_track_id) = setup_project();

    // 孤立トラック (どのコンポジションのツリーにも属さない) を直接追加
    let orphan_track = library::project::track::TrackData::new("Orphan Track");
    let orphan_track_id = orphan_track.id;
    {
        let mut proj = project.write().unwrap();
        proj.add_node(Node::Track(orphan_track));
    }

    // テキストクリップを作成
    let clip = ClipFactory::create_text_clip("Hello", 0, 90, 30.0);

    // 孤立トラックに追加を試みる → 失敗すべき
    let result =
        ClipHandler::add_clip_to_track(&project, comp_id, orphan_track_id, clip, 0, 90, None);

    assert!(
        result.is_err(),
        "Adding a clip to an orphan track not in the composition tree should fail"
    );
}

// ==================== UI flow integration test ====================

/// UI操作を模擬した統合テスト:
///   1. 「Add Track」 (track_list.rs の右クリック → Add Track)
///   2. 「Add Sub-Track」 (track_list.rs の右クリック → Add Sub-Track)
///   3. 「Add Text Layer」 (context_menu.rs の右クリック → Add Text Layer)
///   4. ノード構造を検証
///   5. レンダリングに必要なグラフ接続を検証 (テキストが描画される前提条件)
#[test]
fn test_ui_flow_add_track_subtrack_text_clip() {
    let (project, comp_id, root_track_id) = setup_project();
    let plugin_manager = make_plugin_manager();

    // ──── Step 1: 「Add Track」ボタン押下 ────
    // UI: track_list.rs → DeferredTrackAction::AddTrack → project_service.add_track()
    let track_id = TrackHandler::add_track(&project, comp_id, "New Track").unwrap();

    // 検証: トラックがルートトラックの子として追加されている
    {
        let proj = project.read().unwrap();
        let root = proj.get_track(root_track_id).unwrap();
        assert!(
            root.child_ids.contains(&track_id),
            "New Track should be a child of root track"
        );
        let track = proj.get_track(track_id).unwrap();
        assert_eq!(track.name, "New Track");
    }

    // ──── Step 2: 「Add Sub-Track」コンテキストメニュー選択 ────
    // UI: track_list.rs → DeferredTrackAction::AddSubTrack → project_service.add_sub_track()
    let sub_track_id =
        TrackHandler::add_sub_track(&project, comp_id, track_id, "New Sub-Track").unwrap();

    // 検証: サブトラックが親トラックの子として追加されている
    {
        let proj = project.read().unwrap();
        let track = proj.get_track(track_id).unwrap();
        assert!(
            track.child_ids.contains(&sub_track_id),
            "Sub-Track should be a child of the parent track"
        );
        let sub_track = proj.get_track(sub_track_id).unwrap();
        assert_eq!(sub_track.name, "New Sub-Track");
    }

    // ──── Step 3: 「Add Text Layer」コンテキストメニュー選択 ────
    // UI: context_menu.rs → create_text_clip() + add_clip_to_best_track()
    let sample_text = "this is sample text";
    let fps: f64 = 30.0;
    let in_frame = 0u64;
    let duration_frames = (5.0_f64 * fps).round() as u64; // 5秒 = 150フレーム
    let out_frame = in_frame + duration_frames;

    let text_clip = ClipFactory::create_text_clip(sample_text, in_frame, out_frame, fps);
    let clip_kind = text_clip.kind.clone();
    assert_eq!(clip_kind, TrackClipKind::Text);

    // サブトラックにクリップを追加 (UI: add_clip_to_best_track がフラッタンから対象トラックを決定)
    let clip_id = ClipHandler::add_clip_to_track(
        &project,
        comp_id,
        sub_track_id,
        text_clip,
        in_frame,
        out_frame,
        None,
    )
    .unwrap();

    // グラフノードをセットアップ (UI: project_service.add_clip_to_track 内部で自動呼出)
    ClipHandler::setup_clip_graph_nodes(
        &project,
        &plugin_manager,
        sub_track_id,
        clip_id,
        &clip_kind,
    )
    .unwrap();

    // ──── Step 4: ノード構造を検証 ────
    let proj = project.read().unwrap();

    // 4a. クリップがノードレジストリに存在する
    let clip = proj.get_clip(clip_id).expect("Clip should exist in nodes");
    assert_eq!(clip.kind, TrackClipKind::Text);
    assert_eq!(clip.in_frame, in_frame);
    assert_eq!(clip.out_frame, out_frame);

    // 4b. テキストプロパティが設定されている
    let text_value = clip.properties.get_constant_value("text");
    assert!(text_value.is_some(), "Clip should have 'text' property");
    match text_value.unwrap() {
        library::project::property::PropertyValue::String(s) => {
            assert_eq!(s, sample_text, "Text property should match");
        }
        _ => panic!("'text' property should be a String"),
    }

    // 4c. サブトラックの子にレイヤー(サブトラック)が作成されている
    let sub_track = proj.get_track(sub_track_id).unwrap();
    assert_eq!(
        sub_track.child_ids.len(),
        1,
        "Sub-track should have 1 child (the layer)"
    );
    let layer_id = sub_track.child_ids[0];
    let layer = proj
        .get_track(layer_id)
        .expect("Layer sub-track should exist");
    assert_eq!(layer.name, "Layer");

    // 4d. レイヤーの子にクリップがある
    assert!(
        layer.child_ids.contains(&clip_id),
        "Layer should contain the clip"
    );

    // 4e. レイヤー内にグラフノードがある (fill + transform)
    let graph_nodes_in_layer: Vec<uuid::Uuid> = layer
        .child_ids
        .iter()
        .filter(|id| proj.get_graph_node(**id).is_some())
        .copied()
        .collect();
    assert_eq!(
        graph_nodes_in_layer.len(),
        2,
        "Layer should have 2 graph nodes (fill + transform)"
    );

    // ──── Step 5: レンダリングに必要なグラフ接続を検証 ────
    // テキストのレンダリングパイプライン:
    //   clip.shape_out → fill.shape_in → fill.image_out → transform.image_in → transform.image_out → layer.image_out

    // 5a. fill ノードと transform ノードを特定
    let fill_id = layer
        .child_ids
        .iter()
        .find(|id| {
            proj.get_graph_node(**id)
                .map_or(false, |n| n.type_id.contains("fill"))
        })
        .expect("Fill node should exist in layer");

    let transform_id = layer
        .child_ids
        .iter()
        .find(|id| {
            proj.get_graph_node(**id)
                .map_or(false, |n| n.type_id.contains("transform"))
        })
        .expect("Transform node should exist in layer");

    // 5b. clip.shape_out → fill.shape_in (テキストシェイプの受け渡し)
    let has_shape_to_fill = proj.connections.iter().any(|c| {
        c.from.node_id == clip_id
            && c.from.pin_name == "shape_out"
            && c.to.node_id == *fill_id
            && c.to.pin_name == "shape_in"
    });
    assert!(
        has_shape_to_fill,
        "clip.shape_out → fill.shape_in connection is required for text rendering"
    );

    // 5c. fill.image_out → transform.image_in (塗り結果の変換)
    let has_fill_to_transform = proj.connections.iter().any(|c| {
        c.from.node_id == *fill_id
            && c.from.pin_name == "image_out"
            && c.to.node_id == *transform_id
            && c.to.pin_name == "image_in"
    });
    assert!(
        has_fill_to_transform,
        "fill.image_out → transform.image_in connection is required for text rendering"
    );

    // 5d. transform.image_out → layer.image_out (レイヤー出力)
    let has_transform_to_layer = proj.connections.iter().any(|c| {
        c.from.node_id == *transform_id
            && c.from.pin_name == "image_out"
            && c.to.node_id == layer_id
            && c.to.pin_name == "image_out"
    });
    assert!(
        has_transform_to_layer,
        "transform.image_out → layer.image_out connection is required for rendering output"
    );

    // 5e. 全体のツリー構造を確認
    //   root_track → track ("New Track") → sub_track ("New Sub-Track") → layer ("Layer") → clip + graph_nodes
    let root = proj.get_track(root_track_id).unwrap();
    assert!(root.child_ids.contains(&track_id));
    let track = proj.get_track(track_id).unwrap();
    assert!(track.child_ids.contains(&sub_track_id));

    // 完全なレンダリングパイプラインが構築されていることを確認:
    // 3つの接続 = テキストが画面に描画される最低条件
    let total_connections = proj.connections.len();
    assert!(
        total_connections >= 3,
        "At least 3 connections needed for text rendering pipeline, got {}",
        total_connections
    );
}
