use super::list_table::{list_entry, TABLE_COLUMNS, TABLE_WIDTH};
use super::*;
#[test]
fn table_contract_keeps_all_required_columns() {
    assert_eq!(
        TABLE_COLUMNS.map(|(name, _)| name),
        ["Name", "Kind", "Size", "FPS", "Duration"]
    );
    assert_eq!(
        TABLE_COLUMNS.map(|(_, width)| width).iter().sum::<f32>(),
        TABLE_WIDTH
    );
}
#[test]
fn media_metadata_keeps_size_fps_and_duration() {
    let mut asset = Asset::new("Video", "video.mov", AssetKind::Video);
    asset.width = Some(3_840);
    asset.height = Some(2_160);
    asset.fps = Some(23.976);
    asset.duration = Some(65.0);
    let entry = LibraryEntry::Media(&asset);
    assert_eq!(entry.size(), "3840 x 2160");
    assert_eq!(entry.fps(), "23.976");
    assert_eq!(entry.duration(), "1:05");
    assert_eq!(
        entry.list_metadata(),
        "Video | 3840 x 2160 | 23.976 fps | 1:05"
    );
}
#[test]
fn list_rows_allocate_distinct_vertical_slots() {
    let context = egui::Context::default();
    let asset = Asset::new("Image", "image.png", AssetKind::Image);
    let project = AuthoringProject::new(
        "test",
        1920,
        1080,
        library::model::authoring::RationalRate::new(30, 1).unwrap(),
        library::model::authoring::MediaTime::new(10, 1).unwrap(),
    )
    .unwrap();
    let state = AuthoringUiState::new(project.root_timeline_id);
    let mut rows = Vec::new();
    drop(context.run(
        egui::RawInput {
            screen_rect: Some(Rect::from_min_size(
                egui::Pos2::ZERO,
                Vec2::new(500.0, 300.0),
            )),
            ..Default::default()
        },
        |context| {
            egui::CentralPanel::default().show(context, |ui| {
                for index in 0..3 {
                    rows.push(list_entry(ui, LibraryEntry::Media(&asset), index, &state).rect);
                }
            });
        },
    ));
    assert!(rows.windows(2).all(|rows| rows[0].bottom() < rows[1].top()));
}

#[test]
fn transition_modules_are_not_presented_as_draggable_node_clips() {
    use library::model::authoring::{
        ModuleDefinition, ModuleDefinitionSharing, ModuleTemplateOrigin, TransitionMediaType,
    };

    let (node_clip, _) = ModuleDefinition::new_project_image("Reusable Node Clip");
    let (transition, _) = ModuleDefinition::new_transition(
        "Reusable Transition",
        ModuleDefinitionSharing::ReusableTemplate(ModuleTemplateOrigin::Project),
        TransitionMediaType::Image,
    )
    .unwrap();
    let (private, _) =
        ModuleDefinition::new_image("Private Node Clip", ModuleDefinitionSharing::Private);

    assert!(is_node_clip_definition(&node_clip));
    assert!(!is_node_clip_definition(&transition));
    assert!(!is_node_clip_definition(&private));
}

#[test]
fn particle_system_source_has_beginner_facing_copy_and_one_stable_drag_contract() {
    let project = AuthoringProject::new(
        "test",
        1920,
        1080,
        library::model::authoring::RationalRate::new(30, 1).unwrap(),
        library::model::authoring::MediaTime::new(10, 1).unwrap(),
    )
    .unwrap();
    let state = AuthoringUiState::new(project.root_timeline_id);
    let entry = LibraryEntry::NewParticleNodeClip;

    assert_eq!(entry.qa_id(), "assets.particle_node_clip_source");
    assert_eq!(entry.name(), "Particle System");
    assert_eq!(entry.kind(), "Particle System");
    assert_eq!(entry.list_metadata(), "Procedural particle generator");
    let hover_text = entry.hover_text(&state);
    assert!(hover_text.contains("Drag the Particle System to the Timeline"));
    for implementation_term in ["Node", "GPU", "private", "Private"] {
        assert!(
            !entry.name().contains(implementation_term)
                && !entry.kind().contains(implementation_term)
                && !entry.list_metadata().contains(implementation_term)
                && !hover_text.contains(implementation_term),
            "beginner-facing Particle copy exposed {implementation_term:?}"
        );
    }
    assert!(entry.draggable(&state));
    assert_eq!(
        library_drag_payload(entry),
        AuthoringLibraryDrag::NewParticleNodeClip
    );

    let metadata = entry_qa_metadata(entry, &state, true, 1);
    assert_eq!(metadata["kind"], "particle_system");
    assert_eq!(metadata["creation_kind"], "particle_node_clip");
    assert_eq!(metadata["private_definition"], true);
    assert_eq!(metadata["draggable_to_timeline"], true);
}
