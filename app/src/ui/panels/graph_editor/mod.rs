pub mod actions;
pub mod drawing;
pub(crate) mod mutation;
pub mod projection;
pub mod utils;

use actions::*;
pub use utils::PropertyComponent;
use utils::*;

use egui::{Color32, Sense, Ui, Vec2};
use library::editor::project_service::SemanticPropertyAccess;
use library::model::project::{NodeContainer, Project};
use library::model::property::PropertyDefinition;
use library::model::NodeContent;
use library::EditorService;
use std::sync::{Arc, RwLock};
use uuid::Uuid;

use crate::action::HistoryManager;
use crate::command::CommandRegistry;
use crate::state::context::EditorContext;
use crate::state::context_types::SelectionTarget;

use crate::command::CommandId;
use crate::ui::viewport::{ViewportController, ViewportInputPolicy, ViewportState, ZoomPolicy};
use pan_zoom_ui::{AxisMask, CanvasState, NavigationConfig};
use projection::{container_for_selection, GraphPropertyProjection};

fn graph_navigation_config() -> NavigationConfig {
    NavigationConfig {
        input_policy: ViewportInputPolicy::AxisModifiers,
        zoom_policy: ZoomPolicy::IndependentXY,
        pan_axes: AxisMask::BOTH,
        zoom_axes: AxisMask::BOTH,
        ..Default::default()
    }
}

struct GraphViewportState<'a> {
    pan: &'a mut Vec2,
    zoom_x: &'a mut f32,
    zoom_y: &'a mut f32,
}

impl<'a> ViewportState for GraphViewportState<'a> {
    fn canvas_state(&self) -> CanvasState {
        CanvasState::new(*self.pan, Vec2::new(*self.zoom_x, *self.zoom_y))
    }

    fn set_canvas_state(&mut self, state: CanvasState) {
        *self.pan = state.pan;
        *self.zoom_x = state.zoom.x;
        *self.zoom_y = state.zoom.y;
    }
}

fn exact_node_property_definitions(
    project_service: &EditorService,
    project: &Arc<RwLock<Project>>,
    composition_id: Uuid,
    node_id: Uuid,
) -> Vec<PropertyDefinition> {
    let canonical = project.read().ok().and_then(|project| {
        let node = project.get_node(node_id)?;
        match node.content() {
            NodeContent::Value(value) => Some(value.property_definitions().to_vec()),
            NodeContent::PluginOperation(operation) => project_service
                .get_plugin_manager()
                .operation_descriptor(
                    &operation.category,
                    &operation.component_id,
                    &operation.operation,
                )
                .ok()
                .map(|descriptor| descriptor.properties().to_vec()),
            _ => None,
        }
    });
    canonical.unwrap_or_else(|| {
        let track_id = project
            .read()
            .ok()
            .and_then(|project| match project.find_node_container(node_id) {
                Some(NodeContainer::Clip(clip_id)) => project.find_track_for_clip(clip_id),
                Some(NodeContainer::Track(track_id)) => Some(track_id),
                Some(NodeContainer::Composition(_)) | None => None,
            })
            .unwrap_or_else(Uuid::nil);
        project_service.get_property_definitions(composition_id, track_id, node_id)
    })
}

fn graph_property_projection(
    project_service: &EditorService,
    project: &Arc<RwLock<Project>>,
    composition_id: Uuid,
    target: SelectionTarget,
) -> Result<GraphPropertyProjection, String> {
    match target {
        SelectionTarget::Node(node_id) => {
            let definitions =
                exact_node_property_definitions(project_service, project, composition_id, node_id);
            let project = project.read().map_err(|error| error.to_string())?;
            let node = project
                .get_node(node_id)
                .ok_or_else(|| format!("Graph Node {node_id} does not exist"))?;
            Ok(GraphPropertyProjection::exact_node(
                &project,
                node,
                &definitions,
            ))
        }
        SelectionTarget::Clip(_) | SelectionTarget::Track(_) | SelectionTarget::Composition(_) => {
            let container = container_for_selection(target)
                .ok_or_else(|| "Graph selection is not a container".to_string())?;
            let stack = project_service
                .semantic_container_property_stack(container)
                .map_err(|error| error.to_string())?;
            let project = project.read().map_err(|error| error.to_string())?;
            Ok(GraphPropertyProjection::semantic(&project, &stack))
        }
    }
}

fn graph_selection_for_composition(
    project: &Project,
    target: Option<SelectionTarget>,
    composition_id: Uuid,
) -> Option<SelectionTarget> {
    let target = target?;
    let belongs = match target {
        SelectionTarget::Node(node_id) => {
            node_belongs_to_composition(project, node_id, composition_id)
        }
        SelectionTarget::Clip(clip_id) => {
            project.get_clip(clip_id).is_some()
                && project
                    .find_track_for_clip(clip_id)
                    .is_some_and(|track_id| {
                        project.find_composition_for_track(track_id) == Some(composition_id)
                    })
        }
        SelectionTarget::Track(track_id) => {
            project.get_track(track_id).is_some()
                && project.find_composition_for_track(track_id) == Some(composition_id)
        }
        SelectionTarget::Composition(id) => {
            id == composition_id && project.get_composition(id).is_some()
        }
    };
    belongs.then_some(target)
}

fn graph_valid_time_range(
    project: &Project,
    target: SelectionTarget,
    composition_duration: f64,
) -> Option<(f64, f64)> {
    let clip_id = match target {
        SelectionTarget::Node(node_id) => project.find_parent_clip(node_id),
        SelectionTarget::Clip(clip_id) => Some(clip_id),
        SelectionTarget::Track(_) | SelectionTarget::Composition(_) => None,
    };
    let Some(clip_id) = clip_id else {
        return Some((0.0, composition_duration));
    };
    project.get_clip(clip_id).map(|clip| {
        let start = clip.start_time.into_inner();
        (start, start + clip.duration.into_inner())
    })
}

fn draw_property_sidebar(
    ui: &mut Ui,
    projection: &GraphPropertyProjection,
    editor_context: &mut EditorContext,
) {
    ui.heading("Properties");
    for diagnostic in &projection.diagnostics {
        ui.colored_label(Color32::LIGHT_RED, diagnostic);
    }
    ui.separator();
    egui::ScrollArea::vertical().show(ui, |ui| {
        const PROPERTY_COLORS: [Color32; 7] = [
            Color32::RED,
            Color32::GREEN,
            Color32::BLUE,
            Color32::YELLOW,
            Color32::CYAN,
            Color32::MAGENTA,
            Color32::ORANGE,
        ];
        let mut row_index = 0usize;
        for section in &projection.sections {
            let section_label = ui.strong(&section.label);
            crate::qa::register_component_with_metadata(
                format!("graph.section:{}", section.stable_id),
                "graph_property_section",
                section_label.rect,
                false,
                Some(serde_json::json!({
                    "target": projection.target,
                    "section_id": section.stable_id,
                    "group": format!("{:?}", section.group),
                    "owner": format!("{:?}", section.owner),
                    "node_id": section.node_id,
                    "diagnostics": section.diagnostics,
                })),
            );
            for diagnostic in &section.diagnostics {
                ui.small(egui::RichText::new(diagnostic).color(Color32::LIGHT_RED));
            }
            for row in &section.rows {
                let color = PROPERTY_COLORS[row_index % PROPERTY_COLORS.len()];
                row_index += 1;
                let mut is_visible = editor_context
                    .graph_editor
                    .visible_properties
                    .contains(&row.stable_id);
                ui.horizontal(|ui| {
                    let (rect, _) = ui.allocate_exact_size(Vec2::splat(12.0), Sense::hover());
                    ui.painter().circle_filled(
                        rect.center(),
                        5.0,
                        if row.is_plottable() {
                            color
                        } else {
                            Color32::DARK_GRAY
                        },
                    );
                    let visibility = ui.add_enabled(
                        row.is_plottable(),
                        egui::Checkbox::new(&mut is_visible, &row.label),
                    );
                    let visibility = if let Some(status) = row.access_label() {
                        visibility.on_hover_text(status)
                    } else if row.component.is_none() {
                        visibility.on_hover_text("This property is not numeric and is not plotted")
                    } else {
                        visibility
                    };
                    crate::qa::register_component_with_metadata(
                        format!("graph.property:{}", row.stable_id),
                        "graph_property_visibility",
                        visibility.rect,
                        row.is_plottable(),
                        Some(serde_json::json!({
                            "target": projection.target,
                            "property": row.stable_id,
                            "property_key": row.property_key,
                            "label": row.label,
                            "visible": is_visible,
                            "plotted": row.is_plottable(),
                            "editable": row.is_editable(),
                            "component": row.component.map(|component| format!("{component:?}")),
                            "owner": format!("{:?}", row.owner),
                            "access": property_access_metadata(&row.access),
                            "animation": format!("{:?}", row.animation),
                            "definition": row.definition.as_ref().map(|definition| serde_json::json!({
                                "name": definition.name(),
                                "label": definition.label(),
                                "ui_type": format!("{:?}", definition.ui_type()),
                            })),
                        })),
                    );
                    if visibility.changed() {
                        if is_visible {
                            editor_context
                                .graph_editor
                                .visible_properties
                                .insert(row.stable_id.clone());
                        } else {
                            editor_context
                                .graph_editor
                                .visible_properties
                                .remove(&row.stable_id);
                        }
                    }
                });
            }
            ui.add_space(6.0);
        }
        if projection.sections.is_empty() {
            ui.label("No properties found.");
        }
    });
}

fn property_access_metadata(access: &SemanticPropertyAccess) -> serde_json::Value {
    match access {
        SemanticPropertyAccess::Editable => serde_json::json!({"kind": "editable"}),
        SemanticPropertyAccess::Wired { source } => {
            serde_json::json!({"kind": "wired", "source": source})
        }
        SemanticPropertyAccess::ReadOnly {
            reason,
            related_nodes,
        } => serde_json::json!({
            "kind": "read_only",
            "reason": reason,
            "related_nodes": related_nodes,
        }),
    }
}

pub fn graph_editor_panel(
    ui: &mut Ui,
    editor_context: &mut EditorContext,
    history_manager: &mut HistoryManager,
    project_service: &mut EditorService,
    project: &Arc<RwLock<Project>>,
    registry: &CommandRegistry,
) {
    let Some(comp_id) = editor_context.active_composition_id else {
        ui.label("No composition selected.");
        return;
    };
    let graph_target = project.read().ok().and_then(|project| {
        graph_selection_for_composition(&project, editor_context.selection.primary(), comp_id)
    });
    finish_graph_drag_if_owner_changed(graph_target, editor_context, project, history_manager);
    let Some(target) = graph_target else {
        ui.label("Select a Node, Clip, Track, or Composition to inspect its properties.");
        return;
    };
    if editor_context.graph_editor.active_target != Some(target) {
        actions::finish_pending_move(editor_context, project, history_manager);
    }
    if editor_context.graph_editor.begin_target(target) {
        editor_context.interaction.selected_keyframe = None;
        editor_context.interaction.editing_keyframe = None;
    }

    let projection = match graph_property_projection(project_service, project, comp_id, target) {
        Ok(projection) => projection,
        Err(error) => {
            let response = ui.colored_label(
                Color32::LIGHT_RED,
                format!("Cannot resolve Graph properties: {error}"),
            );
            crate::qa::register_component_with_metadata(
                "graph.projection_error",
                "graph_diagnostic",
                response.rect,
                false,
                Some(serde_json::json!({
                    "target": target,
                    "error": error,
                })),
            );
            return;
        }
    };
    let property_rows = projection.rows().cloned().collect::<Vec<_>>();
    editor_context.graph_editor.sync_properties(
        property_rows
            .iter()
            .filter(|row| row.is_plottable())
            .map(|row| row.stable_id.clone()),
    );
    let mut actions = Vec::new();
    let (composition_fps, composition_resolution, valid_time_range) = {
        let Ok(project) = project.read() else {
            return;
        };
        let Some(composition) = project.get_composition(comp_id) else {
            return;
        };
        (
            composition.fps,
            (composition.width, composition.height),
            graph_valid_time_range(&project, target, composition.duration),
        )
    };

    egui::SidePanel::left("graph_sidebar")
        .resizable(true)
        .default_width(240.0)
        .show_inside(ui, |ui| {
            draw_property_sidebar(ui, &projection, editor_context);
        });

    egui::CentralPanel::default().show_inside(ui, |ui| {
        let pixels_per_second = editor_context.graph_editor.zoom_x;
        let pixels_per_unit = editor_context.graph_editor.zoom_y;

        let ruler_height = 24.0;
        let available_rect = ui.available_rect_before_wrap();

        let mut ruler_rect = available_rect;
        ruler_rect.max.y = ruler_rect.min.y + ruler_height;

        let mut graph_rect = available_rect;
        graph_rect.min.y += ruler_height;

        crate::qa::register_component_with_metadata(
            "graph.canvas",
            "graph_canvas",
            graph_rect,
            true,
            Some(serde_json::json!({
                "target": target,
                "entity_id": target.node_id(),
                "pan": {
                    "x": editor_context.graph_editor.pan.x,
                    "y": editor_context.graph_editor.pan.y,
                },
                "zoom_x": pixels_per_second,
                "zoom_y": pixels_per_unit,
            })),
        );
        crate::qa::register_component_with_metadata(
            "graph.ruler",
            "graph_ruler",
            ruler_rect,
            true,
            Some(serde_json::json!({
                "target": target,
                "entity_id": target.node_id(),
                "pixels_per_second": pixels_per_second,
            })),
        );

        let (_base_response, painter) = ui.allocate_painter(available_rect.size(), Sense::hover());

        let ruler_response =
            ui.interact(ruler_rect, ui.id().with("ruler"), Sense::click_and_drag());

        let mut state = GraphViewportState {
            pan: &mut editor_context.graph_editor.pan,
            zoom_x: &mut editor_context.graph_editor.zoom_x,
            zoom_y: &mut editor_context.graph_editor.zoom_y,
        };

        let hand_tool_key = registry
            .commands
            .iter()
            .find(|c| c.id == CommandId::HandTool)
            .and_then(|c| c.shortcut)
            .map(|(_, k)| k);

        let mut controller = ViewportController::new(ui, ui.id().with("graph"), hand_tool_key)
            .with_config(graph_navigation_config())
            .with_screen_origin(egui::pos2(graph_rect.min.x, graph_rect.center().y));

        let (_, graph_response) = controller.interact_with_rect(
            graph_rect,
            &mut state,
            &mut editor_context.interaction.handled_hand_tool_drag,
        );

        let transform = GraphTransform::new(
            graph_rect,
            editor_context.graph_editor.pan,
            editor_context.graph_editor.zoom_x,
            editor_context.graph_editor.zoom_y,
        );

        drawing::draw_background(&painter, &transform, ruler_rect, valid_time_range);
        drawing::draw_grid(&painter, &transform, ruler_rect);

        if ruler_response.dragged() || ruler_response.clicked() {
            if let Some(pos) = ruler_response.interact_pointer_pos() {
                let (t, _) = transform.screen_to_graph(pos);
                editor_context.timeline.current_time = t.max(0.0) as f32;
            }
        }

        drawing::draw_properties(
            ui,
            &painter,
            &graph_response,
            &transform,
            &property_rows,
            target,
            true,
            editor_context,
            project_service,
            &mut actions,
            composition_fps,
            composition_resolution,
        );

        drawing::draw_playhead(
            &painter,
            &transform,
            ruler_rect,
            editor_context.timeline.current_time as f64,
        );
    });

    if editor_context.graph_editor.keyframe_drag.is_some()
        && ui.input(|input| input.pointer.any_released())
        && !actions
            .iter()
            .any(|action| matches!(action, Action::FinishMove))
    {
        actions.push(Action::FinishMove);
    }

    for action in actions {
        actions::process_action(
            action,
            comp_id,
            project_service,
            project,
            editor_context,
            history_manager,
        );
    }
}

#[cfg(test)]
fn graph_node_selection(target: Option<SelectionTarget>) -> Option<uuid::Uuid> {
    target.and_then(SelectionTarget::node_id)
}

fn finish_graph_drag_if_owner_changed(
    graph_target: Option<SelectionTarget>,
    editor_context: &mut EditorContext,
    project: &Arc<RwLock<Project>>,
    history_manager: &mut HistoryManager,
) -> bool {
    let changed = if editor_context
        .graph_editor
        .keyframe_drag
        .as_ref()
        .is_some_and(|drag| graph_target != Some(drag.target))
    {
        actions::finish_pending_move(editor_context, project, history_manager)
    } else {
        false
    };
    // The panel returns before `begin_target` when selection is cleared. Do
    // not leave the previous owner's property visibility or keyframe
    // selection available to a later re-selection of that same typed owner.
    if graph_target.is_none() {
        editor_context.graph_editor.clear_target();
        editor_context.interaction.selected_keyframe = None;
        editor_context.interaction.editing_keyframe = None;
    }
    changed
}

fn node_belongs_to_composition(
    project: &Project,
    node_id: uuid::Uuid,
    comp_id: uuid::Uuid,
) -> bool {
    if project.get_node(node_id).is_none() {
        return false;
    }
    let Some(container) = project.find_node_container(node_id) else {
        return false;
    };
    match container {
        NodeContainer::Composition(id) => id == comp_id,
        NodeContainer::Track(track_id)
            if project.find_composition_for_track(track_id) == Some(comp_id) =>
        {
            true
        }
        NodeContainer::Clip(clip_id) => project
            .find_track_for_clip(clip_id)
            .is_some_and(|track_id| project.find_composition_for_track(track_id) == Some(comp_id)),
        NodeContainer::Track(_) => false,
    }
}

#[cfg(test)]
mod tests {
    use super::{
        finish_graph_drag_if_owner_changed, graph_navigation_config, graph_node_selection,
        graph_selection_for_composition, GraphViewportState, HistoryManager, SelectionTarget,
    };
    use crate::state::context::EditorContext;
    use crate::state::context_types::GraphKeyframeDragState;
    use crate::ui::viewport::ViewportController;
    use library::model::project::Project;
    use library::model::property::KeyframeId;
    use library::model::{Clip, Composition};
    use std::sync::{Arc, RwLock};
    use uuid::Uuid;

    const VIEWPORT: egui::Rect =
        egui::Rect::from_min_max(egui::pos2(20.0, 30.0), egui::pos2(420.0, 230.0));

    #[test]
    fn graph_selection_accepts_each_typed_target_only_in_the_active_composition() {
        let mut project = Project::new("typed Graph targets");
        let (composition, track) = Composition::new("main", 320, 180, 30.0, 2.0);
        let composition_id = composition.id;
        let track_id = track.id;
        project.add_track(track).expect("track insertion succeeds");
        project
            .add_composition(composition)
            .expect("composition insertion succeeds");
        let clip = Clip::new("clip", 0.0, 2.0);
        let clip_id = clip.id;
        project.add_clip(clip);
        project
            .attach_clip_to_track(track_id, clip_id)
            .expect("clip attachment succeeds");

        for target in [
            SelectionTarget::Clip(clip_id),
            SelectionTarget::Track(track_id),
            SelectionTarget::Composition(composition_id),
        ] {
            assert_eq!(
                graph_selection_for_composition(&project, Some(target), composition_id),
                Some(target)
            );
        }
        assert_eq!(
            graph_selection_for_composition(
                &project,
                Some(SelectionTarget::Composition(Uuid::new_v4())),
                composition_id,
            ),
            None
        );
    }

    fn run_graph_frame(
        context: &egui::Context,
        frame: usize,
        events: Vec<egui::Event>,
        modifiers: egui::Modifiers,
        pan: &mut egui::Vec2,
        zoom_x: &mut f32,
        zoom_y: &mut f32,
    ) {
        drop(context.run(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(500.0, 300.0),
                )),
                time: Some(frame as f64 / 60.0),
                modifiers,
                events,
                ..Default::default()
            },
            |context| {
                egui::CentralPanel::default().show(context, |ui| {
                    let mut state = GraphViewportState {
                        pan,
                        zoom_x,
                        zoom_y,
                    };
                    let mut handled = false;
                    ViewportController::new(
                        ui,
                        ui.make_persistent_id("graph-raw-navigation"),
                        None,
                    )
                    .with_config(graph_navigation_config())
                    .with_screen_origin(egui::pos2(VIEWPORT.min.x, VIEWPORT.center().y))
                    .interact_with_rect(VIEWPORT, &mut state, &mut handled);
                });
            },
        ));
    }

    #[test]
    fn graph_uses_shared_canvas_token_with_explicit_axis_grid_spacing() {
        let theme = super::drawing::graph_canvas_theme();
        let grid = super::drawing::graph_grid_config();
        let navigation = super::graph_navigation_config();

        assert_eq!(
            theme.canvas.background,
            pan_zoom_ui::CanvasTheme::default().background
        );
        assert_eq!(grid.minor_spacing, egui::vec2(0.1, 10.0));
        assert_eq!(grid.major_spacing, egui::vec2(0.5, 50.0));
        assert_eq!(
            navigation.zoom_policy,
            pan_zoom_ui::ZoomPolicy::IndependentXY
        );
        assert_eq!(navigation.zoom_axes, pan_zoom_ui::AxisMask::BOTH);
    }

    #[test]
    fn raw_events_preserve_graph_pan_sign_and_independent_zoom_axes() {
        let context = egui::Context::default();
        let pointer = egui::pos2(240.0, 120.0);
        let mut pan = egui::vec2(10.0, -5.0);
        let mut zoom_x = 2.0;
        let mut zoom_y = 3.0;
        run_graph_frame(
            &context,
            0,
            vec![egui::Event::PointerMoved(pointer)],
            egui::Modifiers::NONE,
            &mut pan,
            &mut zoom_x,
            &mut zoom_y,
        );
        run_graph_frame(
            &context,
            1,
            vec![egui::Event::MouseWheel {
                unit: egui::MouseWheelUnit::Point,
                delta: egui::vec2(4.0, -3.0),
                modifiers: egui::Modifiers::NONE,
            }],
            egui::Modifiers::NONE,
            &mut pan,
            &mut zoom_x,
            &mut zoom_y,
        );
        assert_eq!(pan, egui::vec2(14.0, -8.0));

        let command = egui::Modifiers::COMMAND;
        run_graph_frame(
            &context,
            2,
            vec![egui::Event::MouseWheel {
                unit: egui::MouseWheelUnit::Point,
                delta: egui::vec2(0.0, 4.0),
                modifiers: command,
            }],
            command,
            &mut pan,
            &mut zoom_x,
            &mut zoom_y,
        );
        assert!((zoom_x - 2.2).abs() < 1.0e-5);
        assert_eq!(zoom_y, 3.0);

        let command_shift = egui::Modifiers {
            shift: true,
            ..command
        };
        run_graph_frame(
            &context,
            3,
            vec![egui::Event::MouseWheel {
                unit: egui::MouseWheelUnit::Point,
                delta: egui::vec2(0.0, 4.0),
                modifiers: command_shift,
            }],
            command_shift,
            &mut pan,
            &mut zoom_x,
            &mut zoom_y,
        );
        assert!((zoom_x - 2.2).abs() < 1.0e-5);
        assert!((zoom_y - 3.3).abs() < 1.0e-5);
    }

    #[test]
    fn same_uuid_clip_target_is_not_accepted_as_graph_node() {
        let shared_id = Uuid::new_v4();

        assert_eq!(
            graph_node_selection(Some(SelectionTarget::Node(shared_id))),
            Some(shared_id)
        );
        assert_eq!(
            graph_node_selection(Some(SelectionTarget::Clip(shared_id))),
            None
        );
    }

    #[test]
    fn non_node_owner_finishes_changed_drag_before_panel_early_return() {
        let composition_id = Uuid::new_v4();
        let node_id = Uuid::new_v4();
        let keyframe_id = KeyframeId::new();
        let original = Project::new("before interrupted graph drag");
        let project = Arc::new(RwLock::new(original.clone()));
        project.write().unwrap().name = "after interrupted graph drag".to_string();
        let edited = project.read().unwrap().clone();
        let mut context = EditorContext::new(composition_id);
        context.graph_editor.keyframe_drag = Some(GraphKeyframeDragState {
            target: SelectionTarget::Node(node_id),
            anchor: ("node:opacity".to_string(), keyframe_id),
            origins: Vec::new(),
            changed: true,
        });
        context.select_target(SelectionTarget::Clip(node_id));
        let mut history = HistoryManager::new();
        history.push_project_state(original.clone());

        assert!(finish_graph_drag_if_owner_changed(
            context.selection.primary(),
            &mut context,
            &project,
            &mut history,
        ));
        assert_eq!(history.undo_depth(), 2);
        assert_eq!(history.undo(&edited), Some(original));
        assert!(context.graph_editor.keyframe_drag.is_none());
    }

    #[test]
    fn missing_owner_commits_changed_drag_and_prunes_graph_target_state() {
        let composition_id = Uuid::new_v4();
        let node_id = Uuid::new_v4();
        let keyframe_id = KeyframeId::new();
        let original = Project::new("before cleared graph selection");
        let project = Arc::new(RwLock::new(original.clone()));
        project.write().unwrap().name = "after cleared graph selection".to_string();
        let edited = project.read().unwrap().clone();
        let target = SelectionTarget::Node(node_id);
        let mut context = EditorContext::new(composition_id);
        assert!(context.graph_editor.begin_target(target));
        context
            .graph_editor
            .sync_properties(["node:opacity".to_string()]);
        context
            .graph_editor
            .selected_keyframes
            .insert(("node:opacity".to_string(), keyframe_id));
        context.graph_editor.keyframe_drag = Some(GraphKeyframeDragState {
            target,
            anchor: ("node:opacity".to_string(), keyframe_id),
            origins: Vec::new(),
            changed: true,
        });
        let mut history = HistoryManager::new();
        history.push_project_state(original.clone());

        assert!(finish_graph_drag_if_owner_changed(
            None,
            &mut context,
            &project,
            &mut history,
        ));

        assert_eq!(history.undo_depth(), 2);
        assert_eq!(history.undo(&edited), Some(original));
        assert_eq!(context.graph_editor.active_target, None);
        assert!(context.graph_editor.visible_properties.is_empty());
        assert!(context.graph_editor.known_properties.is_empty());
        assert!(context.graph_editor.selected_keyframes.is_empty());
        assert!(context.graph_editor.keyframe_drag.is_none());
    }
}
