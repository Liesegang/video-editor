use super::*;

use library::model::project::{
    Composition, PortAddress, PortOwner, ProjectConnection, IMAGE_OUTPUT_PORT, MERGE_IMAGES_PORT,
};
use library::model::Node;

use crate::ui::panels::node_editor::{
    apply_auto_layout, compute_auto_layout, container_rect, estimated_node_rect,
    estimated_node_size, nested_content_rect, AutoLayoutScope, AUTO_LAYOUT_CLIP_TOP,
    AUTO_LAYOUT_COMPOSITION_BOTTOM, AUTO_LAYOUT_COMPOSITION_RIGHT,
};

struct Fixture {
    project: Project,
    composition_id: Uuid,
    source: Uuid,
    middle: Uuid,
    sink: Uuid,
    rects: HashMap<Uuid, egui::Rect>,
}

struct NestedFixture {
    project: Project,
    composition_id: Uuid,
    track_id: Uuid,
    clip_id: Uuid,
    source: Uuid,
    sink: Uuid,
    rects: HashMap<Uuid, egui::Rect>,
}

impl NestedFixture {
    fn new() -> Result<Self, Box<dyn std::error::Error>> {
        let (mut project, composition_id, track_id, clip_id, source, sink) =
            crate::ui::panels::node_editor::test_fixture::fixture();
        let plan = compute_auto_layout(&project, composition_id, AutoLayoutScope::All)
            .ok_or("nested fixture could not compute its baseline layout")?;
        apply_auto_layout(&mut project, composition_id, &plan);

        let clip = project
            .get_clip(clip_id)
            .ok_or("nested fixture Clip is missing")?;
        let content = nested_content_rect(
            container_rect(clip.ui_position, clip.ui_size),
            AUTO_LAYOUT_CLIP_TOP,
        );
        let source_size = estimated_node_size(&project, source);
        let sink_size = estimated_node_size(&project, sink);
        let y = content.top();
        project
            .get_node_mut(source)
            .ok_or("nested fixture source is missing")?
            .ui_position = [content.right() - source_size.x, y];
        project
            .get_node_mut(sink)
            .ok_or("nested fixture sink is missing")?
            .ui_position = [content.left(), y];
        if sink_size.y > content.height() {
            return Err("nested fixture Clip is too short for its sink Node".into());
        }
        if container_hierarchy_needs_reflow(&project, composition_id) {
            return Err("nested fixture baseline violates container hierarchy".into());
        }
        let rects = rendered_rects(&project, &[source, sink])?;
        Ok(Self {
            project,
            composition_id,
            track_id,
            clip_id,
            source,
            sink,
            rects,
        })
    }
}

fn rendered_rects(
    project: &Project,
    node_ids: &[Uuid],
) -> Result<HashMap<Uuid, egui::Rect>, Box<dyn std::error::Error>> {
    node_ids
        .iter()
        .copied()
        .map(|node_id| {
            estimated_node_rect(project, node_id)
                .map(|rect| (node_id, rect))
                .ok_or_else(|| format!("Node {node_id} has no estimated rectangle").into())
        })
        .collect()
}

fn prepare_horizontal_commit(
    project: &Project,
    composition_id: Uuid,
    anchor: Uuid,
    rects: &HashMap<Uuid, egui::Rect>,
    state: &mut NodeEditorState,
    history: &HistoryManager,
) -> Result<DirectionalLayoutCommit, Box<dyn std::error::Error>> {
    handle_directional_layout_outputs(
        project,
        composition_id,
        &[],
        rects,
        &[output(intent(
            LayoutSwipePhase::Start,
            anchor,
            egui::pos2(400.0, 300.0),
            None,
            egui::Modifiers::NONE,
        ))],
        state,
        history,
    );
    handle_directional_layout_outputs(
        project,
        composition_id,
        &[],
        rects,
        &[output(intent(
            LayoutSwipePhase::Commit,
            anchor,
            egui::pos2(620.0, 300.0),
            Some(LayoutSwipeAxis::Horizontal),
            egui::Modifiers::NONE,
        ))],
        state,
        history,
    )
    .commit
    .ok_or_else(|| "activated release did not prepare a directional commit".into())
}

fn container_geometry(project: &Project, owner: NodeContainer) -> Option<([f32; 2], [f32; 2])> {
    match owner {
        NodeContainer::Composition(id) => project
            .get_composition(id)
            .map(|container| (container.ui_position, container.ui_size)),
        NodeContainer::Track(id) => project
            .get_track(id)
            .map(|container| (container.ui_position, container.ui_size)),
        NodeContainer::Clip(id) => project
            .get_clip(id)
            .map(|container| (container.ui_position, container.ui_size)),
    }
}

struct CompositionDirectPair {
    source: Uuid,
    sink: Uuid,
    rects: HashMap<Uuid, egui::Rect>,
}

fn add_composition_direct_pair(
    fixture: &mut NestedFixture,
    source_value: u128,
    sink_value: u128,
) -> Result<CompositionDirectPair, Box<dyn std::error::Error>> {
    let owner = NodeContainer::Composition(fixture.composition_id);
    let track_rect = fixture
        .project
        .get_track(fixture.track_id)
        .map(|track| container_rect(track.ui_position, track.ui_size))
        .ok_or("nested fixture Track is missing")?;
    let y = track_rect.bottom() + AUTO_LAYOUT_ROW_GAP * 2.0;
    let source = add_node(
        &mut fixture.project,
        owner,
        source_value,
        "Composition direct source",
        [track_rect.left(), y],
    )?;
    let source_width = estimated_node_size(&fixture.project, source).x;
    let sink = add_node(
        &mut fixture.project,
        owner,
        sink_value,
        "Composition direct sink",
        [track_rect.left() + source_width + AUTO_LAYOUT_COLUMN_GAP, y],
    )?;
    connect(&mut fixture.project, source, sink, 0);

    let pair_rect = estimated_node_rect(&fixture.project, source)
        .ok_or("Composition direct source has no estimated rectangle")?
        .union(
            estimated_node_rect(&fixture.project, sink)
                .ok_or("Composition direct sink has no estimated rectangle")?,
        );
    let composition = fixture
        .project
        .get_composition_mut(fixture.composition_id)
        .ok_or("nested fixture Composition is missing")?;
    composition.ui_size[0] = composition.ui_size[0]
        .max(pair_rect.right() - composition.ui_position[0] + AUTO_LAYOUT_COMPOSITION_RIGHT);
    composition.ui_size[1] = composition.ui_size[1]
        .max(pair_rect.bottom() - composition.ui_position[1] + AUTO_LAYOUT_COMPOSITION_BOTTOM);
    let rects = rendered_rects(&fixture.project, &[source, sink])?;
    Ok(CompositionDirectPair {
        source,
        sink,
        rects,
    })
}

impl Fixture {
    fn new() -> Result<Self, Box<dyn std::error::Error>> {
        let mut project = Project::new("layout swipe adapter");
        let (composition, track) = Composition::new("Main", 1920, 1080, 30.0, 10.0);
        let composition_id = composition.id;
        project.add_track(track)?;
        project.add_composition(composition)?;
        let owner = NodeContainer::Composition(composition_id);
        let source = add_node(&mut project, owner, 0x7101, "Source", [80.0, 80.0])?;
        let middle = add_node(&mut project, owner, 0x7102, "Middle", [90.0, 430.0])?;
        let sink = add_node(&mut project, owner, 0x7103, "Sink", [100.0, 760.0])?;
        connect(&mut project, source, middle, 0);
        connect(&mut project, middle, sink, 0);
        let rects = HashMap::from([
            (
                source,
                egui::Rect::from_min_size(egui::pos2(77.0, 76.0), egui::vec2(120.0, 84.0)),
            ),
            (
                middle,
                egui::Rect::from_min_size(egui::pos2(87.0, 426.0), egui::vec2(180.0, 116.0)),
            ),
        ]);
        Ok(Self {
            project,
            composition_id,
            source,
            middle,
            sink,
            rects,
        })
    }
}

fn add_node(
    project: &mut Project,
    owner: NodeContainer,
    id: u128,
    name: &str,
    position: [f32; 2],
) -> Result<Uuid, Box<dyn std::error::Error>> {
    let id = Uuid::from_u128(id);
    let mut node = Node::new_merge(name);
    node.id = id;
    node.ui_position = position;
    project.add_node(node);
    project.attach_node_to_container(owner, id)?;
    Ok(id)
}

fn connect(project: &mut Project, from: Uuid, to: Uuid, order: i64) {
    let connection = ProjectConnection::new(
        PortAddress::new(PortOwner::Node(from), IMAGE_OUTPUT_PORT),
        PortAddress::new(PortOwner::Node(to), MERGE_IMAGES_PORT),
        order,
    );
    project.connections.push(connection);
}

fn intent(
    phase: LayoutSwipePhase,
    anchor: Uuid,
    current: egui::Pos2,
    axis: Option<LayoutSwipeAxis>,
    modifiers: egui::Modifiers,
) -> LayoutSwipeIntent<Uuid> {
    LayoutSwipeIntent {
        phase,
        anchor,
        start: egui::pos2(400.0, 300.0),
        current,
        axis,
        modifiers,
        transform: egui::emath::TSTransform::from_scaling(1.25),
    }
}

fn output(intent: LayoutSwipeIntent<Uuid>) -> SurfaceOutput {
    EditorOutput::LayoutSwipe(intent)
}

#[path = "swipe_containment_tests.rs"]
mod containment;

#[test]
fn start_and_update_only_change_sparse_display_projection() -> Result<(), Box<dyn std::error::Error>>
{
    let fixture = Fixture::new()?;
    let before = fixture.project.clone();
    let bytes_before = serde_json::to_vec(&fixture.project)?;
    let mut state = NodeEditorState::default();
    let mut history = HistoryManager::new();
    history.push_project_state(fixture.project.clone());
    let undo_before = history.undo_depth();
    let redo_before = history.redo_depth();

    let started = handle_directional_layout_outputs(
        &fixture.project,
        fixture.composition_id,
        &[],
        &fixture.rects,
        &[output(intent(
            LayoutSwipePhase::Start,
            fixture.source,
            egui::pos2(400.0, 300.0),
            None,
            egui::Modifiers::NONE,
        ))],
        &mut state,
        &history,
    );
    assert!(started.owns_pointer);
    assert!(started.commit.is_none());
    assert_eq!(history.undo_depth(), undo_before);
    assert_eq!(history.redo_depth(), redo_before);
    assert_eq!(fixture.project, before);

    let active = state
        .directional_layout_swipe
        .as_ref()
        .ok_or("Start did not establish host gesture state")?;
    assert!(active.frozen_geometry[&fixture.source].measured);
    assert!(active.frozen_geometry[&fixture.middle].measured);
    assert!(!active.frozen_geometry[&fixture.sink].measured);
    assert_eq!(active.frozen_geometry.len(), fixture.project.nodes.len());

    let updated = handle_directional_layout_outputs(
        &fixture.project,
        fixture.composition_id,
        &[],
        &fixture.rects,
        &[output(intent(
            LayoutSwipePhase::Update,
            fixture.source,
            egui::pos2(580.0, 304.0),
            Some(LayoutSwipeAxis::Horizontal),
            egui::Modifiers::NONE,
        ))],
        &mut state,
        &history,
    );
    assert!(updated.owns_pointer);
    assert!(updated.request_repaint);
    assert_eq!(fixture.project, before);
    assert_eq!(serde_json::to_vec(&fixture.project)?, bytes_before);
    assert_eq!(history.undo_depth(), undo_before);
    assert_eq!(history.redo_depth(), redo_before);
    let active = state
        .directional_layout_swipe
        .as_ref()
        .ok_or("Update unexpectedly removed host gesture state")?;
    assert_eq!(
        active.direction,
        Some(DirectionalLayoutGestureDirection::Downstream)
    );
    assert_eq!(active.mode, DirectionalLayoutGestureMode::Layout);
    assert!(active.preview_positions.contains_key(&fixture.middle));
    assert!(active.preview_positions.contains_key(&fixture.sink));
    assert!(!active.preview_positions.contains_key(&fixture.source));
    Ok(())
}

#[test]
fn commit_is_one_atomic_history_entry_and_undo_restores_positions(
) -> Result<(), Box<dyn std::error::Error>> {
    let fixture = Fixture::new()?;
    let before = fixture.project.clone();
    let mut project = fixture.project;
    let mut state = NodeEditorState::default();
    let mut history = HistoryManager::new();
    history.push_project_state(project.clone());
    let undo_before = history.undo_depth();
    let start = output(intent(
        LayoutSwipePhase::Start,
        fixture.source,
        egui::pos2(400.0, 300.0),
        None,
        egui::Modifiers::NONE,
    ));
    handle_directional_layout_outputs(
        &project,
        fixture.composition_id,
        &[],
        &fixture.rects,
        &[start],
        &mut state,
        &history,
    );
    let commit_frame = handle_directional_layout_outputs(
        &project,
        fixture.composition_id,
        &[],
        &fixture.rects,
        &[output(intent(
            LayoutSwipePhase::Commit,
            fixture.source,
            egui::pos2(620.0, 302.0),
            Some(LayoutSwipeAxis::Horizontal),
            egui::Modifiers::NONE,
        ))],
        &mut state,
        &history,
    );
    assert_eq!(project, before, "Commit intent must still be read-only");
    assert_eq!(history.undo_depth(), undo_before);
    let commit = commit_frame
        .commit
        .ok_or("activated release did not prepare an atomic commit")?;
    let result = apply_directional_layout_commit(&mut project, &mut state, &mut history, commit);
    assert!(
        result.changed,
        "commit rejected: {:?}",
        state
            .last_directional_layout_swipe
            .as_ref()
            .and_then(|execution| execution.reason.as_deref())
    );
    assert_eq!(history.undo_depth(), undo_before + 1);
    assert_eq!(history.redo_depth(), 0);
    let execution = state
        .last_directional_layout_swipe
        .as_ref()
        .ok_or("commit did not record diagnostics")?;
    assert_eq!(
        execution.outcome,
        DirectionalLayoutGestureOutcome::Committed
    );
    assert_eq!(execution.history_undo_before, undo_before);
    assert_eq!(execution.history_undo_after, undo_before + 1);
    assert!(!execution.moved_node_ids.is_empty());

    let restored = history
        .undo(&project)
        .ok_or("directional layout commit was not undoable")?;
    assert_eq!(restored, before);
    Ok(())
}

#[test]
fn releasing_a_before_pointer_cancels_without_project_or_history_change(
) -> Result<(), Box<dyn std::error::Error>> {
    let fixture = Fixture::new()?;
    let before = fixture.project.clone();
    let mut state = NodeEditorState::default();
    let mut history = HistoryManager::new();
    history.push_project_state(fixture.project.clone());
    let history_before = (history.undo_depth(), history.redo_depth());
    for swipe in [
        intent(
            LayoutSwipePhase::Start,
            fixture.source,
            egui::pos2(400.0, 300.0),
            None,
            egui::Modifiers::NONE,
        ),
        intent(
            LayoutSwipePhase::Update,
            fixture.source,
            egui::pos2(580.0, 300.0),
            Some(LayoutSwipeAxis::Horizontal),
            egui::Modifiers::NONE,
        ),
        intent(
            LayoutSwipePhase::Cancel,
            fixture.source,
            egui::pos2(580.0, 300.0),
            Some(LayoutSwipeAxis::Horizontal),
            egui::Modifiers::NONE,
        ),
    ] {
        handle_directional_layout_outputs(
            &fixture.project,
            fixture.composition_id,
            &[],
            &fixture.rects,
            &[output(swipe)],
            &mut state,
            &history,
        );
    }
    assert!(state.directional_layout_swipe.is_none());
    assert!(state.directional_layout_release_guard);
    assert_eq!(fixture.project, before);
    assert_eq!((history.undo_depth(), history.redo_depth()), history_before);
    let execution = state
        .last_directional_layout_swipe
        .as_ref()
        .ok_or("cancel did not record diagnostics")?;
    assert_eq!(
        execution.outcome,
        DirectionalLayoutGestureOutcome::Cancelled
    );
    assert!(execution.moved_node_ids.is_empty());

    // The still-down physical pointer keeps ownership after an A-first
    // cancellation. Its release-only frame is also guarded until every
    // competing interaction has observed that release.
    assert!(!recover_directional_layout_release_guard(
        &mut state, false, true, false
    ));
    let guarded = handle_directional_layout_outputs(
        &fixture.project,
        fixture.composition_id,
        &[],
        &fixture.rects,
        &[],
        &mut state,
        &history,
    );
    assert!(guarded.owns_pointer);
    assert!(!recover_directional_layout_release_guard(
        &mut state, false, false, true
    ));
    assert!(finish_directional_layout_release_guard(&mut state, true));
    assert!(!state.directional_layout_release_guard);
    Ok(())
}

#[test]
fn focus_loss_without_pointer_release_recovers_on_the_next_stable_frame(
) -> Result<(), Box<dyn std::error::Error>> {
    let fixture = Fixture::new()?;
    let before = fixture.project.clone();
    let mut state = NodeEditorState::default();
    let mut history = HistoryManager::new();
    history.push_project_state(fixture.project.clone());
    let history_before = (history.undo_depth(), history.redo_depth());
    for swipe in [
        intent(
            LayoutSwipePhase::Start,
            fixture.source,
            egui::pos2(400.0, 300.0),
            None,
            egui::Modifiers::NONE,
        ),
        intent(
            LayoutSwipePhase::Update,
            fixture.source,
            egui::pos2(580.0, 300.0),
            Some(LayoutSwipeAxis::Horizontal),
            egui::Modifiers::NONE,
        ),
        intent(
            LayoutSwipePhase::Cancel,
            fixture.source,
            egui::pos2(580.0, 300.0),
            Some(LayoutSwipeAxis::Horizontal),
            egui::Modifiers::NONE,
        ),
    ] {
        handle_directional_layout_outputs(
            &fixture.project,
            fixture.composition_id,
            &[],
            &fixture.rects,
            &[output(swipe)],
            &mut state,
            &history,
        );
    }
    assert!(state.directional_layout_release_guard);

    // No release arrives after PointerGone. The repaint following Cancel is
    // the first stable frame and proves there is no old physical drag left.
    assert!(recover_directional_layout_release_guard(
        &mut state, false, false, false
    ));
    let normal_input = handle_directional_layout_outputs(
        &fixture.project,
        fixture.composition_id,
        &[],
        &fixture.rects,
        &[],
        &mut state,
        &history,
    );
    assert!(!normal_input.owns_pointer);
    assert_eq!(fixture.project, before);
    assert_eq!((history.undo_depth(), history.redo_depth()), history_before);
    Ok(())
}

#[test]
fn shift_alt_and_negative_vertical_swipe_are_frozen_at_start(
) -> Result<(), Box<dyn std::error::Error>> {
    let fixture = Fixture::new()?;
    let mut state = NodeEditorState::default();
    let history = HistoryManager::new();
    let modifiers = egui::Modifiers {
        alt: true,
        shift: true,
        ..egui::Modifiers::NONE
    };
    handle_directional_layout_outputs(
        &fixture.project,
        fixture.composition_id,
        &[],
        &fixture.rects,
        &[output(intent(
            LayoutSwipePhase::Start,
            fixture.sink,
            egui::pos2(400.0, 300.0),
            None,
            modifiers,
        ))],
        &mut state,
        &history,
    );
    handle_directional_layout_outputs(
        &fixture.project,
        fixture.composition_id,
        &[],
        &fixture.rects,
        &[output(intent(
            LayoutSwipePhase::Update,
            fixture.sink,
            egui::pos2(396.0, 120.0),
            Some(LayoutSwipeAxis::Vertical),
            modifiers,
        ))],
        &mut state,
        &history,
    );
    let active = state
        .directional_layout_swipe
        .as_ref()
        .ok_or("modified gesture did not remain active")?;
    assert_eq!(
        active.mode,
        DirectionalLayoutGestureMode::AlignAndDistribute
    );
    assert_eq!(
        active.direction,
        Some(DirectionalLayoutGestureDirection::Upstream)
    );
    assert_eq!(active.axis, Some(LayoutSwipeAxis::Vertical));
    Ok(())
}

#[test]
fn concurrent_project_change_rejects_whole_commit_without_partial_layout(
) -> Result<(), Box<dyn std::error::Error>> {
    let fixture = Fixture::new()?;
    let mut project = fixture.project;
    let mut state = NodeEditorState::default();
    let mut history = HistoryManager::new();
    history.push_project_state(project.clone());
    let history_before = history.undo_depth();
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
            egui::Modifiers::NONE,
        ))],
        &mut state,
        &history,
    );
    let frame = handle_directional_layout_outputs(
        &project,
        fixture.composition_id,
        &[],
        &fixture.rects,
        &[output(intent(
            LayoutSwipePhase::Commit,
            fixture.source,
            egui::pos2(620.0, 300.0),
            Some(LayoutSwipeAxis::Horizontal),
            egui::Modifiers::NONE,
        ))],
        &mut state,
        &history,
    );
    let commit = frame.commit.ok_or("commit plan was not prepared")?;
    let middle_before = project
        .get_node(fixture.middle)
        .ok_or("middle Node missing before concurrent mutation")?
        .ui_position;
    project
        .get_node_mut(fixture.source)
        .ok_or("source Node missing before concurrent mutation")?
        .name = "Concurrent rename".to_string();
    let concurrent_project = project.clone();
    let history_state_before = (history.undo_depth(), history.redo_depth());
    let result = apply_directional_layout_commit(&mut project, &mut state, &mut history, commit);
    assert!(!result.changed);
    assert_eq!(project, concurrent_project);
    assert_eq!(
        (history.undo_depth(), history.redo_depth()),
        history_state_before
    );
    assert_eq!(history.undo_depth(), history_before);
    assert_eq!(
        project
            .get_node(fixture.middle)
            .ok_or("middle Node missing after rejection")?
            .ui_position,
        middle_before
    );
    assert_eq!(
        state
            .last_directional_layout_swipe
            .as_ref()
            .ok_or("rejection diagnostics missing")?
            .outcome,
        DirectionalLayoutGestureOutcome::Rejected
    );
    Ok(())
}
