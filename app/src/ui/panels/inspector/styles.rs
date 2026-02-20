use super::action_handler::{ActionContext, PropertyTarget};
use super::properties::{render_inspector_properties_grid, PropertyRenderContext};
use crate::action::HistoryManager;
use crate::state::context::EditorContext;

use egui::collapsing_header::CollapsingState;
use egui::Ui;
use library::model::project::connection::PinId;
use library::model::project::graph_analysis;
use library::model::project::property::PropertyMap;
use library::model::project::style::StyleInstance;
use library::EditorService as ProjectService;
use std::sync::{Arc, RwLock};
use uuid::Uuid;

/// Lightweight info about a graph-based style for UI display.
struct GraphStyleInfo {
    node_id: Uuid,
    type_id: String,
    display_name: String,
    properties: PropertyMap,
}

pub fn render_styles_section(
    ui: &mut Ui,
    project_service: &mut ProjectService,
    history_manager: &mut HistoryManager,
    editor_context: &mut EditorContext,
    selected_entity_id: Uuid,
    track_id: Uuid,
    current_time: f64,
    fps: f64,
    styles: &Vec<StyleInstance>,
    project: &Arc<RwLock<library::model::project::project::Project>>,
    needs_refresh: &mut bool,
) {
    ui.add_space(10.0);
    ui.heading("Styles");
    ui.separator();

    // Collect graph-based styles
    let graph_styles: Vec<GraphStyleInfo> = if let Ok(proj) = project.read() {
        let style_ids = graph_analysis::get_associated_styles(&proj, selected_entity_id);
        style_ids
            .into_iter()
            .filter_map(|node_id| {
                let node = proj.get_graph_node(node_id)?;
                let type_id = node.type_id.clone();
                let display_name = project_service
                    .get_plugin_manager()
                    .get_node_type(&type_id)
                    .map(|def| def.display_name.clone())
                    .unwrap_or_else(|| type_id.clone());
                Some(GraphStyleInfo {
                    node_id,
                    type_id,
                    display_name,
                    properties: node.properties.clone(),
                })
            })
            .collect()
    } else {
        Vec::new()
    };

    let has_graph_styles = !graph_styles.is_empty();

    // Add button
    ui.horizontal(|ui| {
        use super::properties::render_add_button;
        render_add_button(ui, |ui| {
            let plugin_manager = project_service.get_plugin_manager();
            for type_name in plugin_manager.get_available_styles() {
                let label = plugin_manager
                    .get_style_plugin(&type_name)
                    .map(|p| p.name())
                    .unwrap_or_else(|| type_name.clone());

                if ui.button(label).clicked() {
                    // Add as graph node, inserting into the style chain
                    let graph_type_id = format!("style.{}", type_name);
                    match project_service.add_graph_node(track_id, &graph_type_id) {
                        Ok(new_node_id) => {
                            let clip_style_pin = PinId::new(selected_entity_id, "style_in");

                            // Check if clip's style_in already has a connection
                            let existing_conn = project.read().ok().and_then(|proj| {
                                graph_analysis::get_input_connection(&proj, &clip_style_pin)
                                    .map(|c| (c.id, c.from.clone()))
                            });

                            if let Some((conn_id, prev_from)) = existing_conn {
                                // Chain: disconnect old, connect old→new.style_in, new→clip
                                let _ = project_service.remove_graph_connection(conn_id);
                                let _ = project_service.add_graph_connection(
                                    prev_from,
                                    PinId::new(new_node_id, "style_in"),
                                );
                            }

                            // Connect new node's output to clip's style input
                            let from = PinId::new(new_node_id, "style_out");
                            if let Err(e) =
                                project_service.add_graph_connection(from, clip_style_pin)
                            {
                                log::error!("Failed to connect style: {}", e);
                            }
                            let current_state = project_service.with_project(|p| p.clone());
                            history_manager.push_project_state(current_state);
                            *needs_refresh = true;
                        }
                        Err(e) => {
                            log::error!("Failed to add style graph node: {}", e);
                        }
                    }
                    ui.close();
                }
            }
        });
    });

    // Render graph-based styles
    if has_graph_styles {
        for style in &graph_styles {
            render_graph_style_item(
                ui,
                project_service,
                history_manager,
                editor_context,
                selected_entity_id,
                style,
                current_time,
                fps,
                needs_refresh,
            );
        }
    } else if !styles.is_empty() {
        // Legacy: render embedded styles
        render_embedded_styles(
            ui,
            project_service,
            history_manager,
            editor_context,
            selected_entity_id,
            styles,
            current_time,
            fps,
            needs_refresh,
        );
    }
}

fn render_graph_style_item(
    ui: &mut Ui,
    project_service: &mut ProjectService,
    history_manager: &mut HistoryManager,
    editor_context: &mut EditorContext,
    clip_id: Uuid,
    style: &GraphStyleInfo,
    current_time: f64,
    fps: f64,
    needs_refresh: &mut bool,
) {
    let id = ui.make_persistent_id(format!("graph_style_{}", style.node_id));
    let state = CollapsingState::load_with_default_open(ui.ctx(), id, false);

    let mut remove_clicked = false;
    let header_res = state.show_header(ui, |ui| {
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new(&style.display_name).strong());
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.button("X").clicked() {
                    remove_clicked = true;
                }
            });
        });
    });

    if remove_clicked {
        if let Err(e) = project_service.remove_graph_node(style.node_id) {
            log::error!("Failed to remove style node: {}", e);
        } else {
            let current_state = project_service.with_project(|p| p.clone());
            history_manager.push_project_state(current_state);
            *needs_refresh = true;
        }
    }

    header_res.body(|ui| {
        let defs = project_service
            .get_plugin_manager()
            .get_node_type(&style.type_id)
            .map(|def| def.default_properties.clone())
            .unwrap_or_default();

        let context = PropertyRenderContext {
            available_fonts: &editor_context.available_fonts,
            in_grid: true,
            current_time,
        };

        let pending_actions = render_inspector_properties_grid(
            ui,
            format!("graph_style_grid_{}", style.node_id),
            &style.properties,
            &defs,
            project_service,
            &context,
            fps,
        );

        let style_props = style.properties.clone();
        let mut ctx = ActionContext::new(project_service, history_manager, clip_id, current_time);
        if ctx.handle_actions(
            pending_actions,
            PropertyTarget::GraphNode(style.node_id),
            |n| style_props.get(n).cloned(),
        ) {
            *needs_refresh = true;
        }
    });
}

/// Legacy: render embedded StyleInstance items
fn render_embedded_styles(
    ui: &mut Ui,
    project_service: &mut ProjectService,
    history_manager: &mut HistoryManager,
    editor_context: &mut EditorContext,
    selected_entity_id: Uuid,
    styles: &[StyleInstance],
    current_time: f64,
    fps: f64,
    needs_refresh: &mut bool,
) {
    let styles_owned = styles.to_vec();
    let mut local_styles = styles_owned.clone();
    let list_id = egui::Id::new(format!("styles_list_{}", selected_entity_id));

    crate::ui::widgets::collection_editor::CollectionEditor::new(
        list_id,
        &mut local_styles,
        |s| egui::Id::new(s.id),
        |ui, visual_index, style, handle, history_manager, project_service, needs_refresh| {
            let backend_index = styles_owned
                .iter()
                .position(|s| s.id == style.id)
                .unwrap_or(visual_index);

            let id = ui.make_persistent_id(format!("style_{}", style.id));
            let state = CollapsingState::load_with_default_open(ui.ctx(), id, false);

            let mut remove_clicked = false;
            let header_res = state.show_header(ui, |ui| {
                ui.horizontal(|ui| {
                    handle.ui(ui, |ui| {
                        ui.label("::");
                    });
                    ui.label(
                        egui::RichText::new(
                            project_service
                                .get_plugin_manager()
                                .get_style_plugin(&style.style_type)
                                .map(|p| p.name())
                                .unwrap_or_else(|| style.style_type.clone().to_uppercase()),
                        )
                        .strong(),
                    );
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.button("X").clicked() {
                            remove_clicked = true;
                        }
                    });
                });
            });

            header_res.body(|ui| {
                let defs = project_service
                    .get_plugin_manager()
                    .get_style_properties(&style.style_type);

                let context = PropertyRenderContext {
                    available_fonts: &editor_context.available_fonts,
                    in_grid: true,
                    current_time,
                };

                let pending_actions = render_inspector_properties_grid(
                    ui,
                    format!("style_grid_{}", style.id),
                    &style.properties,
                    &defs,
                    project_service,
                    &context,
                    fps,
                );
                let style_props = style.properties.clone();
                let mut ctx = ActionContext::new(
                    project_service,
                    history_manager,
                    selected_entity_id,
                    current_time,
                );
                if ctx.handle_actions(pending_actions, PropertyTarget::Style(backend_index), |n| {
                    style_props.get(n).cloned()
                }) {
                    *needs_refresh = true;
                }
            });

            remove_clicked
        },
        |new_styles, project_service| {
            project_service.update_track_clip_styles(selected_entity_id, new_styles)
        },
    )
    .show(ui, history_manager, project_service, needs_refresh);
}
