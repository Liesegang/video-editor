use std::error::Error;
use std::io;
use std::sync::{Arc, RwLock};

use library::EditorService;
use library::cache::CacheManager;
use library::model::project::{NodeContainer, Project};
use library::model::property::{PropertyValue, Vec2};
use library::plugin::PluginManager;
use ordered_float::OrderedFloat;

use super::*;
use crate::state::context_types::SelectionTarget;

type TestResult<T = ()> = Result<T, Box<dyn Error>>;

struct Fixture {
    service: EditorService,
    project: Arc<RwLock<Project>>,
    composition_id: uuid::Uuid,
    clip_id: uuid::Uuid,
}

fn fixture() -> TestResult<Fixture> {
    let project = Arc::new(RwLock::new(Project::new("semantic Inspector")));
    let service = EditorService::new(
        Arc::clone(&project),
        Arc::new(PluginManager::default()),
        Arc::new(CacheManager::new()),
    )?;
    let composition_id = service.add_composition("Comp", 1920, 1080, 30.0, 10.0)?;
    let track_id = service.add_track(composition_id, "Track")?;
    let bundle = service.create_shape_clip(0.0, 5.0, 1920, 1080)?;
    let clip_id = service.add_clip_to_track(composition_id, track_id, bundle, None)?;
    Ok(Fixture {
        service,
        project,
        composition_id,
        clip_id,
    })
}

fn position(x: f64, y: f64) -> PropertyValue {
    PropertyValue::Vec2(Vec2 {
        x: OrderedFloat(x),
        y: OrderedFloat(y),
    })
}

fn snapshot(project: &Arc<RwLock<Project>>) -> Result<Project, io::Error> {
    project
        .read()
        .map(|project| project.clone())
        .map_err(|_| io::Error::other("Project read lock poisoned"))
}

#[test]
fn semantic_property_edit_keeps_clip_identity_and_commits_once() -> TestResult {
    let mut fixture = fixture()?;
    let mut editor_context = EditorContext::new(fixture.composition_id);
    editor_context.select_target(SelectionTarget::Clip(fixture.clip_id));
    let selected_before = editor_context.selection.primary();
    let mut history = HistoryManager::new();
    history.push_project_state(snapshot(&fixture.project)?);
    let initial_depth = history.undo_depth();

    let mut actions = SemanticPropertyActions::new(
        &mut fixture.service,
        &mut history,
        SemanticPropertyOwner::SemanticContainer(NodeContainer::Clip(fixture.clip_id)),
        1.25,
    );
    let errors = actions.handle(
        vec![
            PropertyAction::Update("position".to_string(), position(321.0, 123.0)),
            PropertyAction::Commit,
        ],
        |_| None,
    );

    assert!(errors.is_empty());
    assert!(actions.changed);
    assert_eq!(history.undo_depth(), initial_depth + 1);
    assert_eq!(editor_context.selection.primary(), selected_before);
    let projection = fixture
        .service
        .semantic_container_property_projection(NodeContainer::Clip(fixture.clip_id))?;
    assert_eq!(
        projection
            .properties()
            .get("position")
            .and_then(|property| property.value()),
        Some(&position(321.0, 123.0)),
    );
    Ok(())
}

#[test]
fn failed_semantic_edit_is_fail_closed_without_history_or_selection_change() -> TestResult {
    let mut fixture = fixture()?;
    let missing_clip = uuid::Uuid::new_v4();
    let mut editor_context = EditorContext::new(fixture.composition_id);
    editor_context.select_target(SelectionTarget::Clip(fixture.clip_id));
    let selected_before = editor_context.selection.primary();
    let project_before = snapshot(&fixture.project)?;
    let mut history = HistoryManager::new();
    history.push_project_state(project_before.clone());
    let initial_depth = history.undo_depth();

    let mut actions = SemanticPropertyActions::new(
        &mut fixture.service,
        &mut history,
        SemanticPropertyOwner::SemanticContainer(NodeContainer::Clip(missing_clip)),
        0.0,
    );
    let errors = actions.handle(
        vec![PropertyAction::Update(
            "position".to_string(),
            position(5.0, 6.0),
        )],
        |_| None,
    );

    assert_eq!(errors.len(), 1);
    assert!(!actions.changed);
    assert_eq!(history.undo_depth(), initial_depth);
    assert_eq!(editor_context.selection.primary(), selected_before);
    assert_eq!(snapshot(&fixture.project)?, project_before);
    Ok(())
}

#[test]
fn constant_only_properties_keep_value_editing_without_authoring_modes() {
    assert_eq!(
        property_capabilities(
            &SemanticPropertyAccess::Editable,
            SemanticAnimationSupport::ConstantOnly,
        ),
        (true, false),
    );
    assert_eq!(
        property_capabilities(
            &SemanticPropertyAccess::Wired {
                source: library::model::project::PortAddress::new(
                    library::model::project::PortOwner::Clip(uuid::Uuid::new_v4()),
                    "time",
                ),
            },
            SemanticAnimationSupport::ConstantOnly,
        ),
        (false, false),
    );
}
