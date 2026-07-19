use super::action_handler::{ActionContext, PropertyTarget};
use super::properties::{render_inspector_properties_grid, PropertyRenderContext};
use crate::action::HistoryManager;
use crate::state::context::EditorContext;

use egui::collapsing_header::CollapsingState;
use egui::Ui;
use library::model::style::StyleInstance;
use library::EditorService as ProjectService;
use library::PropertyOwner;
use uuid::Uuid;

#[allow(
    clippy::too_many_arguments,
    reason = "style rendering coordinates editor state, model services, timing, and refresh state"
)]
pub fn render_styles_section(
    ui: &mut Ui,
    project_service: &mut ProjectService,
    history_manager: &mut HistoryManager,
    editor_context: &mut EditorContext,
    node_id: Uuid,
    current_time: f64,
    fps: f64,
    styles: &[StyleInstance],
    needs_refresh: &mut bool,
) {
    ui.add_space(10.0);
    ui.heading("Styles");
    ui.separator();

    // Add buttons
    // Add buttons
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
                    if let Err(e) = project_service.add_style(node_id, &type_name) {
                        log::error!("Failed to add style: {}", e);
                    } else {
                        match project_service.get_project().read() {
                            Ok(project) => {
                                history_manager.push_project_state(project.clone());
                                *needs_refresh = true;
                            }
                            Err(error) => log::error!(
                                "Failed to capture history after adding a style: {error}"
                            ),
                        }
                    }
                    ui.close();
                }
            }
        });
    });

    let mut local_styles = styles.to_vec();
    let list_id = egui::Id::new(format!("styles_list_{node_id}"));

    crate::ui::widgets::collection_editor::CollectionEditor::new(
        list_id,
        &mut local_styles,
        |s| egui::Id::new(s.id),
        |ui, _visual_index, style, handle, history_manager, project_service, needs_refresh| {
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

            let (toggle_response, header_response, _) = header_res.body(|ui| {
                let defs = project_service
                    .get_plugin_manager()
                    .get_style_properties(&style.style_type);

                let context = PropertyRenderContext {
                    available_fonts: &editor_context.available_fonts,
                    in_grid: true,
                    current_time,
                    qa_scope: format!("node:{node_id}.style:{}", style.id),
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
                // Process actions outside Grid closure
                let style_props = style.properties.clone();
                let mut ctx = ActionContext::new(
                    project_service,
                    history_manager,
                    PropertyOwner::Node(node_id),
                    current_time,
                );
                if ctx.handle_actions(pending_actions, PropertyTarget::Style(style.id), |n| {
                    style_props.get(n).cloned()
                }) {
                    *needs_refresh = true;
                }
            });
            crate::qa::register_component_with_metadata(
                format!("inspector.style.node:{node_id}:{}", style.id),
                "inspector_style_item",
                toggle_response.rect,
                true,
                Some(serde_json::json!({
                    "node_id": node_id,
                    "instance_id": style.id,
                    "type": style.style_type,
                    "header_rect": {
                        "min": {
                            "x": header_response.response.rect.min.x,
                            "y": header_response.response.rect.min.y,
                        },
                        "max": {
                            "x": header_response.response.rect.max.x,
                            "y": header_response.response.rect.max.y,
                        },
                    },
                })),
            );

            remove_clicked
        },
        |new_styles, project_service| project_service.update_node_styles(node_id, new_styles),
    )
    .show(ui, history_manager, project_service, needs_refresh);
}
