use super::action_handler::{ActionContext, PropertyTarget};
use super::properties::{render_inspector_properties_grid, PropertyRenderContext};
use crate::action::HistoryManager;
use crate::state::context::EditorContext;

use egui::collapsing_header::CollapsingState;
use egui::Ui;
use library::EditorService as ProjectService;
use uuid::Uuid;

pub fn render_effects_section(
    ui: &mut Ui,
    project_service: &mut ProjectService,
    history_manager: &mut HistoryManager,
    editor_context: &mut EditorContext,
    selected_entity_id: Uuid,
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

        crate::ui::widgets::searchable_context_menu::show_searchable_menu(
            ui,
            "add_effect_menu",
            &items,
            |effect_id| {
                project_service
                    .add_effect_to_clip(selected_entity_id, &effect_id)
                    .ok();

                let current_state = project_service.with_project(|p| p.clone());
                history_manager.push_project_state(current_state);

                *needs_refresh = true;
            },
        );
    });

    let track_clip_ref =
        project_service.with_project(|proj| proj.get_clip(selected_entity_id).cloned());

    if let Some(track_clip) = track_clip_ref {
        let effects = track_clip.effects.clone();

        let mut local_effects = effects.clone();
        let list_id = egui::Id::new(format!("effects_{}", selected_entity_id));

        crate::ui::widgets::collection_editor::CollectionEditor::new(
            list_id,
            &mut local_effects,
            |e| egui::Id::new(e.id),
            |ui, visual_index, effect, handle, history_manager, project_service, needs_refresh| {
                let effect_index = effects
                    .iter()
                    .position(|e| e.id == effect.id)
                    .unwrap_or(visual_index);
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
                header_res.body(|ui| {
                    let defs = project_service
                        .get_plugin_manager()
                        .get_effect_properties(&effect.effect_type);

                    let context = PropertyRenderContext {
                        available_fonts: &editor_context.available_fonts,
                        in_grid: true,
                        current_time,
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
                    let mut ctx = ActionContext::new(
                        project_service,
                        history_manager,
                        selected_entity_id,
                        current_time,
                    );
                    if ctx.handle_actions(
                        pending_actions,
                        PropertyTarget::Effect(effect_index),
                        |n| effect_props.get(n).cloned(),
                    ) {
                        *needs_refresh = true;
                    }
                });

                remove_clicked
            },
            |new_effects, project_service| {
                project_service.update_track_clip_effects(selected_entity_id, new_effects)
            },
        )
        .show(ui, history_manager, project_service, needs_refresh);
    }
}
