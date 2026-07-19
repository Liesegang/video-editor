use super::action_handler::{ActionContext, PropertyTarget};
use super::properties::{render_inspector_properties_grid, PropertyRenderContext};
use crate::action::HistoryManager;
use crate::state::context::EditorContext;

use egui::collapsing_header::CollapsingState;
use egui::Ui;
use library::model::EffectConfig;
use library::EditorService as ProjectService;
use library::PropertyOwner;

#[allow(
    clippy::too_many_arguments,
    reason = "effect rendering coordinates editor state, model services, timing, and refresh state"
)]
pub fn render_effects_section(
    ui: &mut Ui,
    project_service: &mut ProjectService,
    history_manager: &mut HistoryManager,
    editor_context: &mut EditorContext,
    owner: PropertyOwner,
    effects: &[EffectConfig],
    current_time: f64,
    fps: f64,
    needs_refresh: &mut bool,
) {
    ui.add_space(10.0);
    ui.heading("Effects");
    ui.separator();

    use super::properties::render_add_button;
    render_add_button(ui, |ui| {
        let available_effects = project_service.get_plugin_manager().get_available_effects();
        let items: Vec<(String, Option<String>, String)> = available_effects
            .into_iter()
            .map(|(id, name, category)| (name, Some(category), id))
            .collect();
        let menu_id = format!("add_effect_menu:{owner:?}");

        crate::ui::widgets::searchable_context_menu::show_searchable_menu(
            ui,
            &menu_id,
            &items,
            |effect_id| match project_service.add_effect(owner, &effect_id) {
                Ok(()) => match project_service.get_project().read() {
                    Ok(project) => {
                        history_manager.push_project_state(project.clone());
                        *needs_refresh = true;
                    }
                    Err(error) => {
                        log::error!("Failed to capture history after adding an effect: {error}")
                    }
                },
                Err(error) => log::error!("Failed to add effect: {error}"),
            },
        );
    });

    let mut local_effects = effects.to_vec();
    let list_id = egui::Id::new(("effects", owner));

    crate::ui::widgets::collection_editor::CollectionEditor::new(
        list_id,
        &mut local_effects,
        |e| egui::Id::new(e.id),
        |ui, _visual_index, effect, handle, history_manager, project_service, needs_refresh| {
            let id = ui.make_persistent_id(format!("effect_{}", effect.id));
            let state = CollapsingState::load_with_default_open(ui.ctx(), id, false);

            // Render Header (with handle)
            let mut remove_clicked = false;
            let header_res = state.show_header(ui, |ui| {
                ui.horizontal(|ui| {
                    handle.ui(ui, |ui| {
                        ui.label("::");
                    });
                    ui.label(egui::RichText::new(&effect.effect_type).strong());
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.button("X").clicked() {
                            remove_clicked = true;
                        }
                    });
                });
            });

            // Render Body
            let (toggle_response, header_response, _) = header_res.body(|ui| {
                let defs = project_service
                    .get_plugin_manager()
                    .get_effect_properties(&effect.effect_type);

                let context = PropertyRenderContext {
                    available_fonts: &editor_context.available_fonts,
                    in_grid: true,
                    current_time,
                    qa_scope: format!(
                        "{}.effect:{}",
                        match owner {
                            PropertyOwner::Clip(id) => format!("clip:{id}"),
                            PropertyOwner::Node(id) => format!("node:{id}"),
                        },
                        effect.id
                    ),
                };

                let pending_actions = render_inspector_properties_grid(
                    ui,
                    format!("effect_grid_{}", effect.id),
                    &effect.properties,
                    &defs,
                    project_service,
                    &context,
                    fps,
                );
                // Process actions outside Grid closure
                let effect_props = effect.properties.clone();
                let mut ctx =
                    ActionContext::new(project_service, history_manager, owner, current_time);
                if ctx.handle_actions(pending_actions, PropertyTarget::Effect(effect.id), |n| {
                    effect_props.get(n).cloned()
                }) {
                    *needs_refresh = true;
                }
            });
            crate::qa::register_component_with_metadata(
                format!(
                    "inspector.effect.{}:{}",
                    match owner {
                        PropertyOwner::Clip(id) => format!("clip:{id}"),
                        PropertyOwner::Node(id) => format!("node:{id}"),
                    },
                    effect.id
                ),
                "inspector_effect_item",
                toggle_response.rect,
                true,
                Some(serde_json::json!({
                    "owner": format!("{owner:?}"),
                    "instance_id": effect.id,
                    "type": effect.effect_type,
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
        |new_effects, project_service| project_service.update_effects(owner, new_effects),
    )
    .show(ui, history_manager, project_service, needs_refresh);
}
