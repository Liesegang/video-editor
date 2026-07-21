pub mod actions;
pub mod drawing;
pub mod utils;

use actions::*;
pub use utils::PropertyComponent;
use utils::*;

use egui::{Color32, Sense, Ui, Vec2};
use library::model::project::{NodeContainer, Project};
use library::model::property::{Property, PropertyDefinition, PropertyMap};
use library::model::NodeContent;
use library::EditorService;
use std::collections::HashSet;
use std::sync::{Arc, RwLock};
use uuid::Uuid;

use crate::action::HistoryManager;
use crate::command::CommandRegistry;
use crate::state::context::EditorContext;
use crate::state::context_types::SelectionTarget;

use crate::command::CommandId;
use crate::ui::viewport::{ViewportController, ViewportInputPolicy, ViewportState, ZoomPolicy};
use pan_zoom_ui::{AxisMask, CanvasState, NavigationConfig};

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

fn append_property_map<'a>(
    output: &mut Vec<(String, &'a Property, &'a PropertyMap, PropertyComponent)>,
    properties: &'a PropertyMap,
    definitions: &[PropertyDefinition],
) {
    let mut known_names = HashSet::with_capacity(definitions.len());
    for definition in definitions {
        if !known_names.insert(definition.name()) {
            continue;
        }
        let Some(property) = properties.get(definition.name()) else {
            continue;
        };
        for component in numeric_property_components(Some(definition), property) {
            output.push((
                graph_property_name(definition.name(), component),
                property,
                properties,
                component,
            ));
        }
    }

    let mut extras = properties
        .iter()
        .filter(|(property_key, _)| !known_names.contains(property_key.as_str()))
        .collect::<Vec<_>>();
    extras.sort_by_key(|(property_key, _)| property_key.as_str());
    for (property_key, property) in extras {
        for component in numeric_property_components(None, property) {
            output.push((
                graph_property_name(property_key, component),
                property,
                properties,
                component,
            ));
        }
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

pub fn graph_editor_panel(
    ui: &mut Ui,
    editor_context: &mut EditorContext,
    history_manager: &mut HistoryManager,
    project_service: &mut EditorService,
    project: &Arc<RwLock<Project>>,
    registry: &CommandRegistry,
) {
    let graph_owner = editor_context
        .active_composition_id
        .zip(graph_node_selection(editor_context.selection.primary()))
        .and_then(|(composition_id, node_id)| {
            project.read().ok().and_then(|project| {
                node_belongs_to_composition(&project, node_id, composition_id).then_some(node_id)
            })
        });
    finish_graph_drag_if_owner_changed(graph_owner, editor_context, project, history_manager);

    let Some(comp_id) = editor_context.active_composition_id else {
        ui.label("No composition selected.");
        return;
    };
    let Some(entity_id) = graph_owner else {
        ui.label("Select a Node to edit its keyframes.");
        return;
    };
    if editor_context.graph_editor.active_entity_id != Some(entity_id) {
        actions::finish_pending_move(editor_context, project, history_manager);
    }
    if editor_context.graph_editor.begin_entity(entity_id) {
        editor_context.interaction.selected_keyframe = None;
        editor_context.interaction.editing_keyframe = None;
    }

    let property_definitions =
        exact_node_property_definitions(project_service, project, comp_id, entity_id);
    let mut actions = Vec::new();

    {
        let proj_read = if let Ok(p) = project.read() {
            p
        } else {
            return;
        };

        let composition = if let Some(c) = proj_read.compositions.iter().find(|c| c.id == comp_id) {
            c
        } else {
            return;
        };

        let entity = if let Some(e) = proj_read.get_node(entity_id) {
            e
        } else {
            return;
        };

        let mut properties_to_plot: Vec<(String, &Property, &PropertyMap, PropertyComponent)> =
            Vec::new();
        append_property_map(
            &mut properties_to_plot,
            entity.properties(),
            &property_definitions,
        );

        // Capture clip range for visualization
        let containing_clip = proj_read
            .find_parent_clip(entity.id)
            .and_then(|clip_id| proj_read.get_clip(clip_id));
        let valid_time_range = {
            let start = containing_clip
                .map(|clip| clip.start_time.into_inner())
                .unwrap_or(0.0);
            let duration = containing_clip
                .map(|clip| clip.duration.into_inner())
                .unwrap_or(composition.duration);
            Some((start, start + duration))
        };
        if properties_to_plot.is_empty() {
            ui.label("No animatable properties found.");
            return;
        }

        if editor_context.graph_editor.visible_properties.is_empty() {
            for (name, _, _, _) in &properties_to_plot {
                editor_context
                    .graph_editor
                    .visible_properties
                    .insert(name.clone());
            }
        }

        {
            let sidebar_width = 200.0;
            egui::SidePanel::left("graph_sidebar")
                .resizable(true)
                .default_width(sidebar_width)
                .show_inside(ui, |ui| {
                    ui.heading("Properties");
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

                        for (index, (name, _, _, _)) in properties_to_plot.iter().enumerate() {
                            let color = PROPERTY_COLORS[index % PROPERTY_COLORS.len()];
                            let mut is_visible = editor_context
                                .graph_editor
                                .visible_properties
                                .contains(name);

                            ui.horizontal(|ui| {
                                let (rect, _response) =
                                    ui.allocate_exact_size(Vec2::splat(12.0), Sense::hover());
                                ui.painter().circle_filled(rect.center(), 5.0, color);

                                let visibility = ui.checkbox(&mut is_visible, name);
                                crate::qa::register_component_with_metadata(
                                    format!("graph.property_visibility:{name}"),
                                    "graph_property_visibility",
                                    visibility.rect,
                                    visibility.enabled(),
                                    Some(serde_json::json!({
                                        "property": name,
                                        "visible": is_visible,
                                        "entity_id": entity_id,
                                    })),
                                );
                                if visibility.changed() {
                                    if is_visible {
                                        editor_context
                                            .graph_editor
                                            .visible_properties
                                            .insert(name.clone());
                                    } else {
                                        editor_context.graph_editor.visible_properties.remove(name);
                                    }
                                }
                            });
                        }
                    });
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
                        "entity_id": entity_id,
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
                        "entity_id": entity_id,
                        "pixels_per_second": pixels_per_second,
                    })),
                );

                let (_base_response, painter) =
                    ui.allocate_painter(available_rect.size(), Sense::hover());

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

                let mut controller =
                    ViewportController::new(ui, ui.id().with("graph"), hand_tool_key)
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

                let time_mapper =
                    containing_clip.map_or_else(TimeMapper::identity, TimeMapper::from_clip);

                drawing::draw_properties(
                    ui,
                    &painter,
                    &graph_response,
                    &transform,
                    &time_mapper,
                    &properties_to_plot,
                    entity_id,
                    editor_context,
                    project_service,
                    &mut actions,
                    composition.fps,
                    (composition.width, composition.height),
                );

                drawing::draw_playhead(
                    &painter,
                    &transform,
                    ruler_rect,
                    editor_context.timeline.current_time as f64,
                );
            });
        }
    }

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
            entity_id,
            project_service,
            project,
            editor_context,
            history_manager,
        );
    }
}

fn graph_node_selection(target: Option<SelectionTarget>) -> Option<uuid::Uuid> {
    target.and_then(SelectionTarget::node_id)
}

fn finish_graph_drag_if_owner_changed(
    graph_owner: Option<uuid::Uuid>,
    editor_context: &mut EditorContext,
    project: &Arc<RwLock<Project>>,
    history_manager: &mut HistoryManager,
) -> bool {
    if editor_context
        .graph_editor
        .keyframe_drag
        .as_ref()
        .is_some_and(|drag| graph_owner != Some(drag.entity_id))
    {
        return actions::finish_pending_move(editor_context, project, history_manager);
    }
    false
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
        append_property_map, finish_graph_drag_if_owner_changed, graph_navigation_config,
        graph_node_selection, GraphViewportState, HistoryManager, SelectionTarget,
    };
    use crate::state::context::EditorContext;
    use crate::state::context_types::GraphKeyframeDragState;
    use crate::ui::viewport::ViewportController;
    use library::model::project::Project;
    use library::model::property::{
        KeyframeId, Property, PropertyDefinition, PropertyMap, PropertyUiType, PropertyValue, Vec3,
        Vec4,
    };
    use ordered_float::OrderedFloat;
    use std::sync::{Arc, RwLock};
    use uuid::Uuid;

    const VIEWPORT: egui::Rect =
        egui::Rect::from_min_max(egui::pos2(20.0, 30.0), egui::pos2(420.0, 230.0));

    #[test]
    fn property_rows_keep_definition_order_then_sort_numeric_persisted_extras() {
        let number = |value| PropertyValue::Number(OrderedFloat(value));
        let vec3 = |x, y, z| {
            PropertyValue::Vec3(Vec3 {
                x: OrderedFloat(x),
                y: OrderedFloat(y),
                z: OrderedFloat(z),
            })
        };
        let vec4 = |x, y, z, w| {
            PropertyValue::Vec4(Vec4 {
                x: OrderedFloat(x),
                y: OrderedFloat(y),
                z: OrderedFloat(z),
                w: OrderedFloat(w),
            })
        };
        let definitions = vec![
            PropertyDefinition::new(
                "later",
                PropertyUiType::vec3(""),
                "Later",
                vec3(0.0, 0.0, 0.0),
            ),
            PropertyDefinition::new(
                "first",
                PropertyUiType::Float {
                    min: -10.0,
                    max: 10.0,
                    step: 0.1,
                    suffix: String::new(),
                    min_hard_limit: false,
                    max_hard_limit: false,
                },
                "First",
                number(0.0),
            ),
            PropertyDefinition::new(
                "first",
                PropertyUiType::vec4(""),
                "Duplicate First",
                vec4(0.0, 0.0, 0.0, 0.0),
            ),
        ];
        let mut properties = PropertyMap::new();
        properties.set("first".to_string(), Property::constant(number(1.0)));
        properties.set("later".to_string(), Property::constant(vec3(2.0, 3.0, 4.0)));
        properties.set("zeta".to_string(), Property::constant(number(5.0)));
        properties.set(
            "orphan_expression".to_string(),
            Property::expression("value + time".to_string(), vec4(6.0, 7.0, 8.0, 9.0)),
        );

        let mut rows = Vec::new();
        append_property_map(&mut rows, &properties, &definitions);
        let names = rows
            .iter()
            .map(|(name, _, _, _)| name.as_str())
            .collect::<Vec<_>>();
        assert_eq!(
            names,
            [
                "node:later.x",
                "node:later.y",
                "node:later.z",
                "node:first",
                "node:orphan_expression.x",
                "node:orphan_expression.y",
                "node:orphan_expression.z",
                "node:orphan_expression.w",
                "node:zeta",
            ]
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
            entity_id: node_id,
            anchor: ("node:opacity".to_string(), keyframe_id),
            origins: Vec::new(),
            changed: true,
        });
        context.select_target(SelectionTarget::Clip(node_id));
        let mut history = HistoryManager::new();
        history.push_project_state(original.clone());

        assert!(finish_graph_drag_if_owner_changed(
            graph_node_selection(context.selection.primary()),
            &mut context,
            &project,
            &mut history,
        ));
        assert_eq!(history.undo_depth(), 2);
        assert_eq!(history.undo(&edited), Some(original));
        assert!(context.graph_editor.keyframe_drag.is_none());
    }
}
