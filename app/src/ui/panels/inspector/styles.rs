use super::action_handler::{ActionContext, PropertyTarget};
use super::graph_items::{
    collect_graph_nodes, render_chain_add_button, render_graph_node_item, ChainConfig,
};
use super::properties::{render_inspector_properties_grid, PropertyRenderContext};
use crate::action::HistoryManager;
use crate::state::context::EditorContext;

use egui::collapsing_header::CollapsingState;
use egui::Ui;
use library::model::project::graph_analysis;
use library::model::project::style::StyleInstance;
use library::EditorService as ProjectService;
use std::sync::{Arc, RwLock};
use uuid::Uuid;

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

    let graph_styles = collect_graph_nodes(
        project,
        project_service,
        selected_entity_id,
        graph_analysis::get_associated_styles,
    );

    let has_graph_styles = !graph_styles.is_empty();

    // Add button
    ui.horizontal(|ui| {
        render_chain_add_button(
            ui,
            project_service,
            history_manager,
            project,
            track_id,
            selected_entity_id,
            &ChainConfig::STYLE,
            |pm| pm.get_available_styles(),
            |pm, name| {
                pm.get_style_plugin(name)
                    .map(|p| p.name())
                    .unwrap_or_else(|| name.to_string())
            },
            needs_refresh,
        );
    });

    let context = PropertyRenderContext {
        available_fonts: &editor_context.available_fonts,
        in_grid: true,
        current_time,
    };

    // Render graph-based styles
    if has_graph_styles {
        for style in &graph_styles {
            render_graph_node_item(
                ui,
                project_service,
                history_manager,
                selected_entity_id,
                style,
                current_time,
                fps,
                &context,
                needs_refresh,
                "graph_style",
                false,
            );
        }
    } else if !styles.is_empty() {
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
