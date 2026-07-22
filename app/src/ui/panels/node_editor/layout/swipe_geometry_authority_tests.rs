use std::collections::{BTreeMap, HashMap};

use super::*;
use crate::ui::panels::node_editor::AUTO_LAYOUT_COLUMN_GAP;

const START: egui::Pos2 = egui::pos2(400.0, 300.0);
const END: egui::Pos2 = egui::pos2(620.0, 300.0);

fn intent_at_scale(
    phase: LayoutSwipePhase,
    anchor: Uuid,
    axis: Option<LayoutSwipeAxis>,
    scale: f32,
) -> LayoutSwipeIntent<Uuid> {
    LayoutSwipeIntent {
        phase,
        anchor,
        start: START,
        current: if phase == LayoutSwipePhase::Start {
            START
        } else {
            END
        },
        axis,
        modifiers: egui::Modifiers::NONE,
        transform: egui::emath::TSTransform::from_scaling(scale),
    }
}

fn downstream_preview(
    fixture: &Fixture,
    rendered: &HashMap<Uuid, egui::Rect>,
    scale: f32,
) -> Result<BTreeMap<Uuid, [f32; 2]>, Box<dyn std::error::Error>> {
    let mut state = NodeEditorState::default();
    let mut history = HistoryManager::new();
    history.push_project_state(fixture.project.clone());
    handle_directional_layout_outputs(
        &fixture.project,
        fixture.composition_id,
        &[],
        rendered,
        &[output(intent_at_scale(
            LayoutSwipePhase::Start,
            fixture.source,
            None,
            scale,
        ))],
        &mut state,
        &mut history,
    );
    handle_directional_layout_outputs(
        &fixture.project,
        fixture.composition_id,
        &[],
        rendered,
        &[output(intent_at_scale(
            LayoutSwipePhase::Update,
            fixture.source,
            Some(LayoutSwipeAxis::Horizontal),
            scale,
        ))],
        &mut state,
        &mut history,
    );
    Ok(state
        .directional_layout_swipe
        .as_ref()
        .ok_or("layout preview was not retained")?
        .preview_positions
        .clone())
}

fn assert_close(actual: f32, expected: f32) {
    assert!(
        (actual - expected).abs() <= 0.001,
        "expected {expected}, got {actual}"
    );
}

#[test]
fn detailed_measured_geometry_unions_estimate_and_commit_matches_preview_and_undo(
) -> Result<(), Box<dyn std::error::Error>> {
    let fixture = Fixture::new()?;
    let before = fixture.project.clone();
    let source_position = before
        .get_node(fixture.source)
        .ok_or("source missing")?
        .ui_position;
    let middle_position = before
        .get_node(fixture.middle)
        .ok_or("middle missing")?
        .ui_position;
    let middle_render_offset = egui::vec2(-23.0, -11.0);
    let rendered = HashMap::from([
        (
            fixture.source,
            egui::Rect::from_min_size(
                egui::pos2(source_position[0] - 17.0, source_position[1] - 13.0),
                egui::vec2(90.0, 70.0),
            ),
        ),
        (
            fixture.middle,
            egui::Rect::from_min_size(
                egui::pos2(
                    middle_position[0] + middle_render_offset.x,
                    middle_position[1] + middle_render_offset.y,
                ),
                egui::vec2(110.0, 76.0),
            ),
        ),
    ]);
    let mut project = fixture.project;
    let mut state = NodeEditorState::default();
    let mut history = HistoryManager::new();
    history.push_project_state(project.clone());
    let history_before = (history.undo_depth(), history.redo_depth());

    handle_directional_layout_outputs(
        &project,
        fixture.composition_id,
        &[],
        &rendered,
        &[output(intent_at_scale(
            LayoutSwipePhase::Start,
            fixture.source,
            None,
            1.0,
        ))],
        &mut state,
        &mut history,
    );
    handle_directional_layout_outputs(
        &project,
        fixture.composition_id,
        &[],
        &rendered,
        &[output(intent_at_scale(
            LayoutSwipePhase::Update,
            fixture.source,
            Some(LayoutSwipeAxis::Horizontal),
            1.0,
        ))],
        &mut state,
        &mut history,
    );
    let preview = state
        .directional_layout_swipe
        .as_ref()
        .ok_or("detailed layout preview missing")?
        .preview_positions
        .clone();
    let expected_middle_x = source_position[0]
        + estimated_node_size(&project, fixture.source).x
        + AUTO_LAYOUT_COLUMN_GAP
        - middle_render_offset.x;
    assert_close(preview[&fixture.middle][0], expected_middle_x);

    let frame = handle_directional_layout_outputs(
        &project,
        fixture.composition_id,
        &[],
        &rendered,
        &[output(intent_at_scale(
            LayoutSwipePhase::Commit,
            fixture.source,
            Some(LayoutSwipeAxis::Horizontal),
            1.0,
        ))],
        &mut state,
        &mut history,
    );
    let commit = frame.commit.ok_or("detailed layout commit missing")?;
    assert_eq!(commit.gesture.preview_positions, preview);
    assert_eq!(commit.positions, preview);
    assert_eq!(project, before, "preparation must not mutate Project");
    assert_eq!((history.undo_depth(), history.redo_depth()), history_before);

    let result = apply_directional_layout_commit(&mut project, &mut state, &mut history, commit);
    assert!(
        result.changed,
        "commit rejected: {:?}",
        state
            .last_directional_layout_swipe
            .as_ref()
            .and_then(|execution| execution.reason.as_deref())
    );
    for (node_id, position) in &preview {
        assert_eq!(
            project
                .get_node(*node_id)
                .ok_or("committed Node missing")?
                .ui_position,
            *position,
            "Project position diverged from preview for {node_id}"
        );
    }
    assert_eq!(history.undo_depth(), history_before.0 + 1);
    assert_eq!(history.redo_depth(), 0);
    assert_eq!(history.undo(&project).ok_or("layout Undo missing")?, before);
    assert_eq!(history.undo_depth(), history_before.0);
    assert_eq!(history.redo_depth(), history_before.1 + 1);
    Ok(())
}

#[test]
fn overview_ignores_screen_stable_outline_and_is_zoom_stable(
) -> Result<(), Box<dyn std::error::Error>> {
    let fixture = Fixture::new()?;
    let preview_at = |scale: f32| {
        let rendered = fixture
            .project
            .nodes
            .iter()
            .map(|(node_id, node)| {
                let inflation = 2.0 / scale;
                let size = estimated_node_size(&fixture.project, *node_id);
                (
                    *node_id,
                    egui::Rect::from_min_max(
                        egui::pos2(
                            node.ui_position[0] - inflation,
                            node.ui_position[1] - inflation,
                        ),
                        egui::pos2(
                            node.ui_position[0] + size.x + inflation,
                            node.ui_position[1] + size.y + inflation,
                        ),
                    ),
                )
            })
            .collect::<HashMap<_, _>>();
        downstream_preview(&fixture, &rendered, scale)
    };

    let minimum_zoom_preview = preview_at(0.0065)?;
    let overview_preview = preview_at(0.10)?;
    assert_eq!(minimum_zoom_preview, overview_preview);
    let source_position = fixture
        .project
        .get_node(fixture.source)
        .ok_or("source missing")?
        .ui_position;
    assert_close(
        overview_preview[&fixture.middle][0],
        source_position[0]
            + estimated_node_size(&fixture.project, fixture.source).x
            + AUTO_LAYOUT_COLUMN_GAP,
    );
    Ok(())
}

#[test]
fn unmeasured_nodes_fall_back_to_conservative_estimated_geometry(
) -> Result<(), Box<dyn std::error::Error>> {
    let fixture = Fixture::new()?;
    let preview = downstream_preview(&fixture, &HashMap::new(), 1.0)?;
    let source_position = fixture
        .project
        .get_node(fixture.source)
        .ok_or("source missing")?
        .ui_position;
    assert_close(
        preview[&fixture.middle][0],
        source_position[0]
            + estimated_node_size(&fixture.project, fixture.source).x
            + AUTO_LAYOUT_COLUMN_GAP,
    );
    Ok(())
}
