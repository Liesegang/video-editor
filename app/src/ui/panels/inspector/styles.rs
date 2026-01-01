use super::action_handler::{ActionContext, PropertyTarget};
use super::properties::{render_inspector_properties_grid, PropertyRenderContext};
use crate::action::HistoryManager;
use crate::state::context::EditorContext;

use egui::collapsing_header::CollapsingState;
use egui::Ui;
use library::model::project::style::StyleInstance;
use library::EditorService as ProjectService;
use uuid::Uuid;

pub fn render_styles_section(
    ui: &mut Ui,
    project_service: &mut ProjectService,
    history_manager: &mut HistoryManager,
    editor_context: &mut EditorContext,
    selected_entity_id: Uuid,
    current_time: f64,
    fps: f64,
    styles: &Vec<StyleInstance>,
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
                    let defs = plugin_manager.get_style_properties(&type_name);
                    let props =
                        library::model::project::property::PropertyMap::from_definitions(&defs);
                    let new_style = StyleInstance::new(&type_name, props);

                    let mut new_styles = styles.clone();
                    new_styles.push(new_style);

                    project_service
                        .update_track_clip_styles(selected_entity_id, new_styles)
                        .ok();

                    // No history push needed here? Original code did push.
                    // Wait, original code call history_manager.push_project_state.
                    // But I need access to history_manager. It is passed as argument.
                    // Let's use it.
                    let current_state = project_service.get_project().read().unwrap().clone();
                    history_manager.push_project_state(current_state);

                    *needs_refresh = true;
                    ui.close();
                }
            }
        });
    });

    let mut local_styles = styles.clone();
    let list_id = egui::Id::new(format!("styles_list_{}", selected_entity_id));

    crate::ui::widgets::collection_editor::CollectionEditor::new(
        list_id,
        &mut local_styles,
        |s| egui::Id::new(s.id),
        |ui, visual_index, style, handle, history_manager, project_service, needs_refresh| {
            let backend_index = styles
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
                // Process actions outside Grid closure
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
