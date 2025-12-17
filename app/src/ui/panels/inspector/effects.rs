use super::properties::{render_property_rows, PropertyRenderContext};
use crate::action::HistoryManager;
use crate::state::context::EditorContext;
use crate::ui::widgets::reorderable_list::ReorderableList;
use egui::collapsing_header::CollapsingState;
use egui::Ui;
use library::service::project_service::ProjectService;
use uuid::Uuid;

pub fn render_effects_section(
    ui: &mut Ui,
    project_service: &mut ProjectService,
    history_manager: &mut HistoryManager,
    editor_context: &mut EditorContext,
    comp_id: Uuid,
    track_id: Uuid,
    selected_entity_id: Uuid,
    current_time: f64,
    needs_refresh: &mut bool,
) {
    ui.add_space(10.0);
    ui.heading("Effects");
    ui.separator();

    ui.menu_button("Add Effect", |ui| {
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
                    .add_effect_to_clip(comp_id, track_id, selected_entity_id, &effect_id)
                    .ok();
                *needs_refresh = true;
            },
        );
    });

    let track_clip_ref = project_service
        .get_project()
        .read()
        .unwrap()
        .compositions
        .iter()
        .find(|c| c.id == comp_id)
        .and_then(|c| c.tracks.iter().find(|t| t.id == track_id))
        .and_then(|t| t.clips.iter().find(|c| c.id == selected_entity_id).cloned());

    if let Some(track_clip) = track_clip_ref {
        let mut effects = track_clip.effects.clone();

        let old_effects = effects.clone();
        let list_id = ui.make_persistent_id(format!("effects_{}", selected_entity_id));
        let mut needs_delete = None;

        ReorderableList::new(list_id, &mut effects)
            .show(ui, |ui, _visual_index, effect, handle| {
                let effect_index = track_clip.effects.iter().position(|e| e.id == effect.id).unwrap_or(_visual_index);
                let id = ui.make_persistent_id(format!("effect_{}", effect.id));
                let state = CollapsingState::load_with_default_open(ui.ctx(), id, false);
                
                // Render Header (with handle)
                let mut remove_clicked = false;
                let header_res = state.show_header(ui, |ui| {
                    ui.horizontal(|ui| {
                        handle.ui(ui, |ui| { ui.label("::"); });
                        ui.label(egui::RichText::new(&effect.effect_type).strong());
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                             if ui.button("X").clicked() {
                                 remove_clicked = true;
                             }
                        });
                    });
                });
                
                if remove_clicked {
                    needs_delete = Some(_visual_index);
                }

                // Render Body
                header_res.body(|ui| {
                    let defs = project_service
                        .get_plugin_manager()
                        .get_effect_properties(&effect.effect_type);

                    egui::Grid::new(format!("effect_grid_{}", effect.id))
                        .striped(true)
                        .show(ui, |ui| {
                            let actions = render_property_rows(
                                ui,
                                &defs,
                                |name| effect.properties.get(name).and_then(|p| Some(project_service.evaluate_property_value(p, &effect.properties, current_time))),
                                |name| effect.properties.get(name).cloned(),
                                &PropertyRenderContext { available_fonts: &editor_context.available_fonts, in_grid: true, current_time }
                            );

                            for action in actions {
                                match action {
                                    crate::ui::panels::inspector::properties::PropertyAction::Update(name, val) => {
                                        project_service.update_effect_property_or_keyframe(
                                             comp_id, track_id, selected_entity_id, effect_index, &name, current_time, val, None
                                        ).ok();
                                        *needs_refresh = true;
                                    }
                                    crate::ui::panels::inspector::properties::PropertyAction::Commit => {
                                         let current_state = project_service.get_project().read().unwrap().clone();
                                         history_manager.push_project_state(current_state);
                                    }
                                    crate::ui::panels::inspector::properties::PropertyAction::ToggleKeyframe(name, val) => {
                                         let mut keyframe_index_to_remove = None;
                                         if let Some(prop) = effect.properties.get(&name) {
                                             if prop.evaluator == "keyframe" {
                                                 if let Some(idx) = prop.keyframes().iter().position(|k| (k.time.into_inner() - current_time).abs() < 0.001) {
                                                     keyframe_index_to_remove = Some(idx);
                                                 }
                                             }
                                         }

                                         if let Some(index) = keyframe_index_to_remove {
                                              project_service.remove_effect_keyframe_by_index(
                                                  comp_id, track_id, selected_entity_id, effect_index, &name, index
                                              ).ok();
                                         } else {
                                              project_service.add_effect_keyframe(
                                                  comp_id, track_id, selected_entity_id, effect_index, &name, current_time, val, None
                                              ).ok();
                                         }
                                         *needs_refresh = true;
                                    }
                                }
                            }
                        });
            });
        });

        if let Some(idx) = needs_delete {
            effects.remove(idx);
        }

        // Sync reordering
        let ids: Vec<Uuid> = effects.iter().map(|e| e.id).collect();
        let old_ids: Vec<Uuid> = old_effects.iter().map(|e| e.id).collect();
        if ids != old_ids {
            // Update native order
            project_service
                .update_track_clip_effects(comp_id, track_id, selected_entity_id, effects)
                .ok();
            *needs_refresh = true;
        }
    }
}
