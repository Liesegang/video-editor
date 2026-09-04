use super::*;
use crate::test_support::generator_node;
use crate::ui::panels::node_editor::ensure_structural_merge_layout;
use library::editor::project_service::GeneratorNodeRequest;
use library::model::Clip;

#[test]
fn dense_text_vertical_distribute_rejects_before_overlapping_sibling_clip()
-> Result<(), Box<dyn std::error::Error>> {
    let mut fixture = NestedFixture::new()?;
    let owner = NodeContainer::Clip(fixture.clip_id);
    let clip_rect = fixture
        .project
        .get_clip(fixture.clip_id)
        .map(|clip| container_rect(clip.ui_position, clip.ui_size))
        .ok_or("dense Text fixture Clip is missing")?;
    let content = nested_content_rect(clip_rect, AUTO_LAYOUT_CLIP_TOP);
    let mut sibling = Clip::new("Sibling below dense Text", 0.0, 1.0);
    sibling.ui_position = [clip_rect.left(), clip_rect.bottom() + AUTO_LAYOUT_ROW_GAP];
    sibling.ui_size = [clip_rect.width(), 220.0];
    let sibling_id = sibling.id;
    let sibling_rect = container_rect(sibling.ui_position, sibling.ui_size);
    fixture.project.add_clip(sibling);
    fixture
        .project
        .attach_clip_to_track(fixture.track_id, sibling_id)?;
    grow_container_hierarchy_to_rect_all_edges(
        &mut fixture.project,
        NodeContainer::Track(fixture.track_id),
        sibling_rect,
    );
    assert!(!container_hierarchy_needs_reflow(
        &fixture.project,
        fixture.composition_id
    ));

    let mut previous = fixture.source;
    for index in 0..4_u128 {
        let mut text = generator_node(
            &format!("Dense Text {index}"),
            GeneratorNodeRequest::Text {
                text: format!("Dense Text {index}"),
                font: "Arial".to_string(),
            },
        );
        text.id = Uuid::from_u128(0x7_400 + index);
        text.ui_position = [content.left(), content.top()];
        let text_id = text.id;
        fixture.project.add_node(text);
        fixture.project.attach_node_to_container(owner, text_id)?;
        connect(&mut fixture.project, previous, text_id, index as i64);
        previous = text_id;
    }

    let before = fixture.project.clone();
    let before_bytes = serde_json::to_vec(&before)?;
    let mut state = NodeEditorState::default();
    let mut history = HistoryManager::new();
    history.push_project_state(fixture.project.clone());
    let history_before = (history.undo_depth(), history.redo_depth());
    let modifiers = egui::Modifiers {
        alt: true,
        ..egui::Modifiers::NONE
    };
    handle_directional_layout_outputs(
        &fixture.project,
        fixture.composition_id,
        &[],
        &fixture.rects,
        &[output(intent(
            LayoutSwipePhase::Start,
            fixture.source,
            egui::pos2(400.0, 300.0),
            None,
            modifiers,
        ))],
        &mut state,
        &mut history,
    );
    let update = handle_directional_layout_outputs(
        &fixture.project,
        fixture.composition_id,
        &[],
        &fixture.rects,
        &[output(intent(
            LayoutSwipePhase::Update,
            fixture.source,
            egui::pos2(402.0, 500.0),
            Some(LayoutSwipeAxis::Vertical),
            modifiers,
        ))],
        &mut state,
        &mut history,
    );
    assert!(update.commit.is_none());
    let active = state
        .directional_layout_swipe
        .as_ref()
        .ok_or("dense Text update unexpectedly ended the gesture")?;
    assert!(!active.preview_positions.is_empty());
    assert_eq!(fixture.project, before);
    assert_eq!((history.undo_depth(), history.redo_depth()), history_before);

    let prepared = handle_directional_layout_outputs(
        &fixture.project,
        fixture.composition_id,
        &[],
        &fixture.rects,
        &[output(intent(
            LayoutSwipePhase::Commit,
            fixture.source,
            egui::pos2(402.0, 500.0),
            Some(LayoutSwipeAxis::Vertical),
            modifiers,
        ))],
        &mut state,
        &mut history,
    )
    .commit
    .ok_or("dense Text commit was not prepared")?;
    assert_eq!(prepared.gesture.preview_positions, prepared.positions);
    let mut project = fixture.project;
    let result = apply_directional_layout_commit(&mut project, &mut state, &mut history, prepared);
    assert!(!result.changed);
    assert_eq!(project, before);
    assert_eq!(serde_json::to_vec(&project)?, before_bytes);
    assert_eq!((history.undo_depth(), history.redo_depth()), history_before);
    let rejected = state
        .last_directional_layout_swipe
        .as_ref()
        .ok_or("dense Text commit rejection diagnostics are missing")?;
    assert_eq!(rejected.outcome, DirectionalLayoutGestureOutcome::Rejected);
    assert!(rejected.moved_node_ids.is_empty());
    assert!(
        rejected
            .reason
            .as_deref()
            .is_some_and(|reason| reason.contains("container hierarchy violation")),
        "unexpected dense Text rejection: {:?}",
        rejected.reason
    );
    Ok(())
}

#[test]
fn exact_vertical_distribute_can_grow_owner_hierarchy_and_undo_once()
-> Result<(), Box<dyn std::error::Error>> {
    let fixture = NestedFixture::new()?;
    let before = fixture.project.clone();
    let clip_before = container_geometry(&fixture.project, NodeContainer::Clip(fixture.clip_id))
        .ok_or("nested fixture Clip geometry is missing")?;
    let track_before = container_geometry(&fixture.project, NodeContainer::Track(fixture.track_id))
        .ok_or("nested fixture Track geometry is missing")?;
    let composition_before = container_geometry(
        &fixture.project,
        NodeContainer::Composition(fixture.composition_id),
    )
    .ok_or("nested fixture Composition geometry is missing")?;
    let mut project = fixture.project;
    let mut state = NodeEditorState::default();
    let mut history = HistoryManager::new();
    history.push_project_state(project.clone());
    let history_before = (history.undo_depth(), history.redo_depth());
    let modifiers = egui::Modifiers {
        alt: true,
        ..egui::Modifiers::NONE
    };

    handle_directional_layout_outputs(
        &project,
        fixture.composition_id,
        &[],
        &fixture.rects,
        &[output(intent(
            LayoutSwipePhase::Start,
            fixture.source,
            egui::pos2(400.0, 300.0),
            None,
            modifiers,
        ))],
        &mut state,
        &mut history,
    );
    let update = handle_directional_layout_outputs(
        &project,
        fixture.composition_id,
        &[],
        &fixture.rects,
        &[output(intent(
            LayoutSwipePhase::Update,
            fixture.source,
            egui::pos2(402.0, 500.0),
            Some(LayoutSwipeAxis::Vertical),
            modifiers,
        ))],
        &mut state,
        &mut history,
    );
    assert!(update.commit.is_none());
    let preview = state
        .directional_layout_swipe
        .as_ref()
        .ok_or("exact vertical distribute rejected before preview")?;
    let sink_preview = preview
        .preview_positions
        .get(&fixture.sink)
        .copied()
        .ok_or("exact vertical distribute omitted its downstream sink")?;
    let clip_bottom = clip_before.0[1] + clip_before.1[1];
    let sink_height = estimated_node_size(&project, fixture.sink).y;
    assert!(sink_preview[1] + sink_height > clip_bottom);
    assert_eq!(project, before);
    assert_eq!((history.undo_depth(), history.redo_depth()), history_before);

    let prepared = handle_directional_layout_outputs(
        &project,
        fixture.composition_id,
        &[],
        &fixture.rects,
        &[output(intent(
            LayoutSwipePhase::Commit,
            fixture.source,
            egui::pos2(402.0, 500.0),
            Some(LayoutSwipeAxis::Vertical),
            modifiers,
        ))],
        &mut state,
        &mut history,
    )
    .commit
    .ok_or("exact vertical distribute did not prepare its commit")?;
    assert_eq!(prepared.gesture.preview_positions, prepared.positions);
    let result = apply_directional_layout_commit(&mut project, &mut state, &mut history, prepared);
    assert!(
        result.changed,
        "safe exact growth rejected: {:?}",
        state
            .last_directional_layout_swipe
            .as_ref()
            .and_then(|execution| execution.reason.as_deref())
    );
    assert_eq!(history.undo_depth(), history_before.0 + 1);
    assert_eq!(history.redo_depth(), 0);
    assert!(
        container_geometry(&project, NodeContainer::Clip(fixture.clip_id))
            .is_some_and(|geometry| geometry.1[1] > clip_before.1[1])
    );
    assert!(
        container_geometry(&project, NodeContainer::Track(fixture.track_id))
            .is_some_and(|geometry| geometry.1[1] > track_before.1[1])
    );
    assert!(
        container_geometry(&project, NodeContainer::Composition(fixture.composition_id))
            .is_some_and(|geometry| geometry.1[1] > composition_before.1[1])
    );
    assert!(!container_hierarchy_needs_reflow(
        &project,
        fixture.composition_id
    ));
    assert_eq!(
        history
            .undo(&project)
            .ok_or("exact vertical distribute commit was not undoable")?,
        before
    );
    Ok(())
}

#[test]
fn clip_layout_growth_propagates_through_track_and_composition_in_one_undo_step()
-> Result<(), Box<dyn std::error::Error>> {
    let fixture = NestedFixture::new()?;
    let before = fixture.project.clone();
    let mut project = fixture.project;
    let mut state = NodeEditorState::default();
    let mut history = HistoryManager::new();
    history.push_project_state(project.clone());
    let history_before = (history.undo_depth(), history.redo_depth());
    let mut commit = prepare_horizontal_commit(
        &project,
        fixture.composition_id,
        fixture.source,
        &fixture.rects,
        &mut state,
        &mut history,
    )?;
    let composition_bottom = project
        .get_composition(fixture.composition_id)
        .map(|composition| composition.ui_position[1] + composition.ui_size[1])
        .ok_or("nested fixture Composition is missing")?;
    let sink_x = project
        .get_node(fixture.sink)
        .map(|node| node.ui_position[0])
        .ok_or("nested fixture sink is missing")?;
    commit
        .positions
        .insert(fixture.sink, [sink_x, composition_bottom + 800.0]);

    let clip_before = container_geometry(&project, NodeContainer::Clip(fixture.clip_id))
        .ok_or("nested fixture Clip geometry is missing")?;
    let track_before = container_geometry(&project, NodeContainer::Track(fixture.track_id))
        .ok_or("nested fixture Track geometry is missing")?;
    let composition_before =
        container_geometry(&project, NodeContainer::Composition(fixture.composition_id))
            .ok_or("nested fixture Composition geometry is missing")?;
    let result = apply_directional_layout_commit(&mut project, &mut state, &mut history, commit);

    assert!(
        result.changed,
        "growth commit rejected: {:?}",
        state
            .last_directional_layout_swipe
            .as_ref()
            .and_then(|execution| execution.reason.as_deref())
    );
    assert_eq!(history.undo_depth(), history_before.0 + 1);
    assert_eq!(history.redo_depth(), 0);
    let clip_after = container_geometry(&project, NodeContainer::Clip(fixture.clip_id))
        .ok_or("committed Clip geometry is missing")?;
    let track_after = container_geometry(&project, NodeContainer::Track(fixture.track_id))
        .ok_or("committed Track geometry is missing")?;
    let composition_after =
        container_geometry(&project, NodeContainer::Composition(fixture.composition_id))
            .ok_or("committed Composition geometry is missing")?;
    assert!(clip_after.1[1] > clip_before.1[1]);
    assert!(track_after.1[1] > track_before.1[1]);
    assert!(composition_after.1[1] > composition_before.1[1]);
    assert!(!container_hierarchy_needs_reflow(
        &project,
        fixture.composition_id
    ));
    let restored = history
        .undo(&project)
        .ok_or("nested container layout commit was not undoable")?;
    assert_eq!(restored, before);
    Ok(())
}

#[test]
fn composition_direct_layout_grows_only_composition_and_preserves_child_track()
-> Result<(), Box<dyn std::error::Error>> {
    let mut fixture = NestedFixture::new()?;
    let owner = NodeContainer::Composition(fixture.composition_id);
    let mut source = Node::new_merge("Composition direct source");
    source.id = Uuid::from_u128(0x7201);
    let source_id = source.id;
    let mut sink = Node::new_merge("Composition direct sink");
    sink.id = Uuid::from_u128(0x7202);
    let sink_id = sink.id;
    let track_rect = fixture
        .project
        .get_track(fixture.track_id)
        .map(|track| container_rect(track.ui_position, track.ui_size))
        .ok_or("nested fixture Track is missing")?;
    let source_size = estimated_node_size(&fixture.project, source_id);
    let direct_y = track_rect.bottom() + AUTO_LAYOUT_ROW_GAP;
    source.ui_position = [track_rect.left(), direct_y];
    sink.ui_position = [
        track_rect.left() + source_size.x + AUTO_LAYOUT_COLUMN_GAP,
        direct_y,
    ];
    fixture.project.add_node(source);
    fixture.project.add_node(sink);
    fixture.project.attach_node_to_container(owner, source_id)?;
    fixture.project.attach_node_to_container(owner, sink_id)?;
    connect(&mut fixture.project, source_id, sink_id, 0);
    let sink_rect = estimated_node_rect(&fixture.project, sink_id)
        .ok_or("Composition direct sink has no estimated rectangle")?;
    let composition = fixture
        .project
        .get_composition_mut(fixture.composition_id)
        .ok_or("nested fixture Composition is missing")?;
    composition.ui_size[0] = composition.ui_size[0]
        .max(sink_rect.right() - composition.ui_position[0] + AUTO_LAYOUT_COMPOSITION_RIGHT);
    composition.ui_size[1] = composition.ui_size[1]
        .max(sink_rect.bottom() - composition.ui_position[1] + AUTO_LAYOUT_COMPOSITION_BOTTOM);
    if container_hierarchy_needs_reflow(&fixture.project, fixture.composition_id) {
        return Err("Composition direct fixture violates container hierarchy".into());
    }

    let before = fixture.project.clone();
    let track_before = container_geometry(&fixture.project, NodeContainer::Track(fixture.track_id))
        .ok_or("nested fixture Track geometry is missing")?;
    let composition_before = container_geometry(&fixture.project, owner)
        .ok_or("nested fixture Composition geometry is missing")?;
    let rects = rendered_rects(&fixture.project, &[source_id, sink_id])?;
    let mut project = fixture.project;
    let mut state = NodeEditorState::default();
    let mut history = HistoryManager::new();
    history.push_project_state(project.clone());
    let mut commit = prepare_horizontal_commit(
        &project,
        fixture.composition_id,
        source_id,
        &rects,
        &mut state,
        &mut history,
    )?;
    let composition_right = composition_before.0[0] + composition_before.1[0];
    commit
        .positions
        .insert(sink_id, [composition_right + 800.0, direct_y]);
    let result = apply_directional_layout_commit(&mut project, &mut state, &mut history, commit);

    assert!(result.changed);
    assert_eq!(history.undo_depth(), 2);
    assert_eq!(
        container_geometry(&project, NodeContainer::Track(fixture.track_id)),
        Some(track_before)
    );
    let composition_after =
        container_geometry(&project, owner).ok_or("committed Composition geometry is missing")?;
    assert!(composition_after.1[0] > composition_before.1[0]);
    assert!(!container_hierarchy_needs_reflow(
        &project,
        fixture.composition_id
    ));
    let restored = history
        .undo(&project)
        .ok_or("Composition direct layout commit was not undoable")?;
    assert_eq!(restored, before);
    Ok(())
}

#[test]
fn contained_layout_does_not_resize_any_container() -> Result<(), Box<dyn std::error::Error>> {
    let fixture = NestedFixture::new()?;
    let mut project = fixture.project;
    let clip_before = container_geometry(&project, NodeContainer::Clip(fixture.clip_id))
        .ok_or("nested fixture Clip geometry is missing")?;
    let track_before = container_geometry(&project, NodeContainer::Track(fixture.track_id))
        .ok_or("nested fixture Track geometry is missing")?;
    let composition_before =
        container_geometry(&project, NodeContainer::Composition(fixture.composition_id))
            .ok_or("nested fixture Composition geometry is missing")?;
    let sink_before = project
        .get_node(fixture.sink)
        .map(|node| node.ui_position)
        .ok_or("nested fixture sink is missing")?;
    let mut state = NodeEditorState::default();
    let mut history = HistoryManager::new();
    history.push_project_state(project.clone());
    let mut commit = prepare_horizontal_commit(
        &project,
        fixture.composition_id,
        fixture.source,
        &fixture.rects,
        &mut state,
        &mut history,
    )?;
    commit
        .positions
        .insert(fixture.sink, [sink_before[0] + 1.0, sink_before[1]]);
    let result = apply_directional_layout_commit(&mut project, &mut state, &mut history, commit);

    assert!(result.changed);
    assert_eq!(
        container_geometry(&project, NodeContainer::Clip(fixture.clip_id)),
        Some(clip_before)
    );
    assert_eq!(
        container_geometry(&project, NodeContainer::Track(fixture.track_id)),
        Some(track_before)
    );
    assert_eq!(
        container_geometry(&project, NodeContainer::Composition(fixture.composition_id)),
        Some(composition_before)
    );
    Ok(())
}

#[test]
fn unrelated_legacy_hierarchy_violation_does_not_block_safe_layout()
-> Result<(), Box<dyn std::error::Error>> {
    let mut fixture = NestedFixture::new()?;
    let track_right = fixture
        .project
        .get_track(fixture.track_id)
        .map(|track| track.ui_position[0] + track.ui_size[0])
        .ok_or("nested fixture Track is missing")?;
    let composition = fixture
        .project
        .get_composition_mut(fixture.composition_id)
        .ok_or("nested fixture Composition is missing")?;
    composition.ui_size[0] =
        track_right - composition.ui_position[0] + AUTO_LAYOUT_COMPOSITION_RIGHT - 24.0;
    assert!(container_hierarchy_needs_reflow(
        &fixture.project,
        fixture.composition_id
    ));

    let before = fixture.project.clone();
    let sink_before = before
        .get_node(fixture.sink)
        .ok_or("nested fixture sink is missing")?
        .ui_position;
    let mut project = fixture.project;
    let mut state = NodeEditorState::default();
    let mut history = HistoryManager::new();
    history.push_project_state(project.clone());
    let mut commit = prepare_horizontal_commit(
        &project,
        fixture.composition_id,
        fixture.source,
        &fixture.rects,
        &mut state,
        &mut history,
    )?;
    commit
        .positions
        .insert(fixture.sink, [sink_before[0] + 1.0, sink_before[1]]);

    let result = apply_directional_layout_commit(&mut project, &mut state, &mut history, commit);
    assert!(result.changed);
    assert_eq!(history.undo_depth(), 2);
    assert_eq!(
        project
            .get_node(fixture.sink)
            .ok_or("committed sink is missing")?
            .ui_position,
        [sink_before[0] + 1.0, sink_before[1]]
    );
    assert!(container_hierarchy_needs_reflow(
        &project,
        fixture.composition_id
    ));
    assert_eq!(
        history
            .undo(&project)
            .ok_or("safe legacy-baseline layout was not undoable")?,
        before
    );
    Ok(())
}

#[test]
fn new_child_overlap_rejects_whole_commit_on_an_invalidity_free_baseline()
-> Result<(), Box<dyn std::error::Error>> {
    let mut fixture = NestedFixture::new()?;
    let pair = add_composition_direct_pair(&mut fixture, 0x7301, 0x7302)?;
    let source = pair.source;
    let sink = pair.sink;
    assert!(!container_hierarchy_needs_reflow(
        &fixture.project,
        fixture.composition_id
    ));
    let before = fixture.project.clone();
    let track_rect = fixture
        .project
        .get_track(fixture.track_id)
        .map(|track| container_rect(track.ui_position, track.ui_size))
        .ok_or("nested fixture Track is missing")?;
    let mut project = fixture.project;
    let mut state = NodeEditorState::default();
    let mut history = HistoryManager::new();
    history.push_project_state(project.clone());
    let history_before = (history.undo_depth(), history.redo_depth());
    let mut commit = prepare_horizontal_commit(
        &project,
        fixture.composition_id,
        source,
        &pair.rects,
        &mut state,
        &mut history,
    )?;
    commit
        .positions
        .insert(sink, [track_rect.left() + 40.0, track_rect.top() + 80.0]);

    let result = apply_directional_layout_commit(&mut project, &mut state, &mut history, commit);
    assert!(!result.changed);
    assert_eq!(project, before);
    assert_eq!((history.undo_depth(), history.redo_depth()), history_before);
    let execution = state
        .last_directional_layout_swipe
        .as_ref()
        .ok_or("overlap rejection diagnostics are missing")?;
    assert_eq!(execution.outcome, DirectionalLayoutGestureOutcome::Rejected);
    assert!(execution.moved_node_ids.is_empty());
    Ok(())
}

#[test]
fn unrelated_sibling_violation_cannot_mask_a_new_overlap() -> Result<(), Box<dyn std::error::Error>>
{
    let mut fixture = NestedFixture::new()?;
    let target_clip_rect = fixture
        .project
        .get_clip(fixture.clip_id)
        .map(|clip| container_rect(clip.ui_position, clip.ui_size))
        .ok_or("target Clip is missing")?;

    let mut legacy_clip = Clip::new("Legacy invalid sibling", 0.0, 1.0);
    legacy_clip.ui_position = [
        target_clip_rect.left(),
        target_clip_rect.bottom() + AUTO_LAYOUT_ROW_GAP * 3.0,
    ];
    legacy_clip.ui_size = [420.0, 260.0];
    let legacy_clip_id = legacy_clip.id;
    let legacy_clip_rect = container_rect(legacy_clip.ui_position, legacy_clip.ui_size);
    fixture.project.add_clip(legacy_clip);
    fixture
        .project
        .attach_clip_to_track(fixture.track_id, legacy_clip_id)?;
    grow_container_hierarchy_to_rect_all_edges(
        &mut fixture.project,
        NodeContainer::Track(fixture.track_id),
        legacy_clip_rect,
    );
    let legacy_node = add_node(
        &mut fixture.project,
        NodeContainer::Clip(legacy_clip_id),
        0x7331,
        "Legacy escaped Node",
        [
            legacy_clip_rect.right() + 30.0,
            legacy_clip_rect.top() + 80.0,
        ],
    )?;
    assert!(fixture.project.get_node(legacy_node).is_some());

    let track_right = fixture
        .project
        .get_track(fixture.track_id)
        .map(|track| track.ui_position[0] + track.ui_size[0])
        .ok_or("target Track is missing")?;
    let source = add_node(
        &mut fixture.project,
        NodeContainer::Track(fixture.track_id),
        0x7332,
        "Track direct source",
        [track_right + 100.0, target_clip_rect.top()],
    )?;
    let source_width = estimated_node_size(&fixture.project, source).x;
    let sink = add_node(
        &mut fixture.project,
        NodeContainer::Track(fixture.track_id),
        0x7333,
        "Track direct sink",
        [
            track_right + 100.0 + source_width + AUTO_LAYOUT_COLUMN_GAP,
            target_clip_rect.top(),
        ],
    )?;
    connect(&mut fixture.project, source, sink, 0);
    let direct_bounds = estimated_node_rect(&fixture.project, source)
        .ok_or("Track direct source has no rectangle")?
        .union(
            estimated_node_rect(&fixture.project, sink)
                .ok_or("Track direct sink has no rectangle")?,
        );
    grow_container_hierarchy_to_rect_all_edges(
        &mut fixture.project,
        NodeContainer::Track(fixture.track_id),
        direct_bounds,
    );
    assert!(container_hierarchy_needs_reflow(
        &fixture.project,
        fixture.composition_id
    ));

    let rendered = rendered_rects(&fixture.project, &[source, sink])?;
    let before = fixture.project.clone();
    let mut project = fixture.project;
    let mut state = NodeEditorState::default();
    let mut history = HistoryManager::new();
    history.push_project_state(project.clone());
    let history_before = (history.undo_depth(), history.redo_depth());
    let mut commit = prepare_horizontal_commit(
        &project,
        fixture.composition_id,
        source,
        &rendered,
        &mut state,
        &mut history,
    )?;
    commit.positions.insert(
        sink,
        [
            target_clip_rect.left() + 60.0,
            target_clip_rect.top() + 100.0,
        ],
    );

    let result = apply_directional_layout_commit(&mut project, &mut state, &mut history, commit);
    assert!(!result.changed);
    assert_eq!(project, before);
    assert_eq!((history.undo_depth(), history.redo_depth()), history_before);
    assert_eq!(
        state
            .last_directional_layout_swipe
            .as_ref()
            .ok_or("masked-overlap rejection diagnostics are missing")?
            .outcome,
        DirectionalLayoutGestureOutcome::Rejected
    );
    Ok(())
}

#[test]
fn worsening_the_same_legacy_overlap_rejects_atomically() -> Result<(), Box<dyn std::error::Error>>
{
    let mut fixture = NestedFixture::new()?;
    let pair = add_composition_direct_pair(&mut fixture, 0x7311, 0x7312)?;
    let source = pair.source;
    let sink = pair.sink;
    let track_rect = fixture
        .project
        .get_track(fixture.track_id)
        .map(|track| container_rect(track.ui_position, track.ui_size))
        .ok_or("nested fixture Track is missing")?;
    let sink_y = track_rect.top() + 100.0;
    fixture
        .project
        .get_node_mut(sink)
        .ok_or("Composition direct sink is missing")?
        .ui_position = [track_rect.right() - 4.0, sink_y];
    assert!(container_hierarchy_needs_reflow(
        &fixture.project,
        fixture.composition_id
    ));
    let rects = rendered_rects(&fixture.project, &[source, sink])?;
    let before = fixture.project.clone();
    let mut project = fixture.project;
    let mut state = NodeEditorState::default();
    let mut history = HistoryManager::new();
    history.push_project_state(project.clone());
    let history_before = (history.undo_depth(), history.redo_depth());
    let mut commit = prepare_horizontal_commit(
        &project,
        fixture.composition_id,
        source,
        &rects,
        &mut state,
        &mut history,
    )?;
    commit
        .positions
        .insert(sink, [track_rect.right() - 120.0, sink_y]);

    let result = apply_directional_layout_commit(&mut project, &mut state, &mut history, commit);
    assert!(!result.changed);
    assert_eq!(project, before);
    assert_eq!((history.undo_depth(), history.redo_depth()), history_before);
    assert_eq!(
        state
            .last_directional_layout_swipe
            .as_ref()
            .ok_or("worsened-overlap rejection diagnostics are missing")?
            .outcome,
        DirectionalLayoutGestureOutcome::Rejected
    );
    Ok(())
}

#[test]
fn upstream_layout_keeps_structural_merge_anchor_and_preview_positions_exact()
-> Result<(), Box<dyn std::error::Error>> {
    let mut fixture = NestedFixture::new()?;
    let owner = NodeContainer::Track(fixture.track_id);
    let anchor = fixture
        .project
        .get_track(fixture.track_id)
        .ok_or("nested fixture Track is missing")?
        .structural_merge_node_id;
    let upstream = add_node(
        &mut fixture.project,
        owner,
        0x7321,
        "Track upstream source",
        [350.0, 180.0],
    )?;
    let middle = add_node(
        &mut fixture.project,
        owner,
        0x7322,
        "Track upstream middle",
        [680.0, 620.0],
    )?;
    connect(&mut fixture.project, upstream, middle, 0);
    connect(&mut fixture.project, middle, anchor, 1);
    let plan = compute_auto_layout(
        &fixture.project,
        fixture.composition_id,
        AutoLayoutScope::All,
    )
    .ok_or("structural anchor fixture could not compute layout")?;
    apply_auto_layout(&mut fixture.project, fixture.composition_id, &plan);

    let anchor_baseline = fixture
        .project
        .get_node(anchor)
        .ok_or("Track structural Merge is missing")?
        .ui_position;
    fixture
        .project
        .get_node_mut(anchor)
        .ok_or("Track structural Merge is missing")?
        .ui_position = [anchor_baseline[0], anchor_baseline[1] + 37.0];
    let anchor_position = fixture
        .project
        .get_node(anchor)
        .ok_or("Track structural Merge is missing")?
        .ui_position;
    let anchor_rect = estimated_node_rect(&fixture.project, anchor)
        .ok_or("Track structural Merge has no estimated rectangle")?;
    grow_container_hierarchy_to_rect_all_edges(&mut fixture.project, owner, anchor_rect);
    assert!(!container_hierarchy_needs_reflow(
        &fixture.project,
        fixture.composition_id
    ));
    let mut structurally_repaired = fixture.project.clone();
    assert!(ensure_structural_merge_layout(
        &mut structurally_repaired,
        owner
    ));
    assert_ne!(
        structurally_repaired
            .get_node(anchor)
            .ok_or("repaired structural Merge is missing")?
            .ui_position,
        anchor_position,
        "fixture must reproduce the collateral structural-repair movement"
    );

    let rendered = rendered_rects(&fixture.project, &[upstream, middle, anchor])?;
    let before = fixture.project.clone();
    let before_positions = before
        .nodes
        .iter()
        .map(|(node_id, node)| (*node_id, node.ui_position))
        .collect::<BTreeMap<_, _>>();
    let mut project = fixture.project;
    let mut state = NodeEditorState::default();
    let mut history = HistoryManager::new();
    history.push_project_state(project.clone());
    handle_directional_layout_outputs(
        &project,
        fixture.composition_id,
        &[],
        &rendered,
        &[output(intent(
            LayoutSwipePhase::Start,
            anchor,
            egui::pos2(400.0, 300.0),
            None,
            egui::Modifiers::NONE,
        ))],
        &mut state,
        &mut history,
    );
    let commit = handle_directional_layout_outputs(
        &project,
        fixture.composition_id,
        &[],
        &rendered,
        &[output(intent(
            LayoutSwipePhase::Commit,
            anchor,
            egui::pos2(180.0, 300.0),
            Some(LayoutSwipeAxis::Horizontal),
            egui::Modifiers::NONE,
        ))],
        &mut state,
        &mut history,
    )
    .commit
    .ok_or("upstream structural-anchor commit was not prepared")?;
    assert!(!commit.positions.is_empty());
    assert!(!commit.positions.contains_key(&anchor));
    assert_eq!(commit.gesture.preview_positions, commit.positions);
    let planned_positions = commit.positions.clone();
    let mut expected_moved = planned_positions
        .iter()
        .filter_map(|(node_id, position)| {
            (before_positions.get(node_id) != Some(position)).then_some(*node_id)
        })
        .collect::<Vec<_>>();
    expected_moved.sort_unstable();

    let result = apply_directional_layout_commit(&mut project, &mut state, &mut history, commit);
    assert!(result.changed);
    assert_eq!(history.undo_depth(), 2);
    for (node_id, before_position) in &before_positions {
        let actual = project
            .get_node(*node_id)
            .ok_or("Node disappeared during structural-anchor layout")?
            .ui_position;
        if let Some(planned) = planned_positions.get(node_id) {
            assert_eq!(
                actual, *planned,
                "commit diverged from preview for {node_id}"
            );
        } else {
            assert_eq!(
                actual, *before_position,
                "unplanned Node {node_id} moved as collateral layout"
            );
        }
    }
    assert_eq!(
        project
            .get_node(anchor)
            .ok_or("Track structural Merge disappeared")?
            .ui_position,
        anchor_position
    );
    assert_eq!(
        state
            .last_directional_layout_swipe
            .as_ref()
            .ok_or("structural-anchor diagnostics are missing")?
            .moved_node_ids,
        expected_moved
    );
    assert_eq!(
        history
            .undo(&project)
            .ok_or("structural-anchor layout was not undoable")?,
        before
    );
    Ok(())
}
