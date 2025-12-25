use eframe::egui::{self, Color32, ComboBox, DragValue, TextEdit};
use library::animation::EasingFunction;
use library::model::project::project::Project;
use library::EditorService;
use std::sync::{Arc, RwLock};

use crate::action::HistoryManager;
use crate::state::context::EditorContext;

pub fn show_keyframe_dialog(
    ctx: &egui::Context,
    editor_context: &mut EditorContext,
    history_manager: &mut HistoryManager,
    project_service: &mut EditorService,
    project: &Arc<RwLock<Project>>,
) {
    let mut open = editor_context.keyframe_dialog.is_open;
    let mut should_close = false;
    let mut should_update = false;
    let mut should_push_history = false;

    crate::ui::widgets::modal::Modal::new("Edit Keyframe")
        .open(&mut open)
        .collapsible(false)
        .resizable(true)
        .show(ctx, |ui| {
            if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                should_close = true;
            }

            let state = &mut editor_context.keyframe_dialog;

            // Sanitize values to prevent panics
            if !state.time.is_finite() {
                state.time = 0.0;
            }
            if !state.value.is_finite() {
                state.value = 0.0;
            }

            egui::Grid::new("keyframe_grid")
                .num_columns(2)
                .spacing([10.0, 10.0])
                .show(ui, |ui| {
                    ui.label("Time:");
                    let time_response = ui.add(
                        DragValue::new(&mut state.time)
                            .speed(0.01)
                            .suffix(" s")
                            .range(0.0..=f64::MAX),
                    );
                    if time_response.changed() {
                        should_update = true;
                    }
                    if time_response.drag_stopped() {
                        should_push_history = true;
                    }
                    ui.end_row();

                    ui.label("Value:");
                    let val_response = ui.add(DragValue::new(&mut state.value).speed(0.1)); // Range is implicitly infinite, but value is sanitized
                    if val_response.changed() {
                        should_update = true;
                    }
                    if val_response.drag_stopped() {
                        should_push_history = true;
                    }
                    ui.end_row();

                    ui.label("Easing:");
                    let current_variant_name = match state.easing {
                        EasingFunction::Linear => "Linear",
                        EasingFunction::Constant => "Constant",
                        EasingFunction::Expression { .. } => "Expression",
                        // Sine
                        EasingFunction::EaseInSine => "Ease In Sine",
                        EasingFunction::EaseOutSine => "Ease Out Sine",
                        EasingFunction::EaseInOutSine => "Ease In Out Sine",
                        // Quad
                        EasingFunction::EaseInQuad => "Ease In Quad",
                        EasingFunction::EaseOutQuad => "Ease Out Quad",
                        EasingFunction::EaseInOutQuad => "Ease In Out Quad",
                        // Cubic
                        EasingFunction::EaseInCubic => "Ease In Cubic",
                        EasingFunction::EaseOutCubic => "Ease Out Cubic",
                        EasingFunction::EaseInOutCubic => "Ease In Out Cubic",
                        // Quart
                        EasingFunction::EaseInQuart => "Ease In Quart",
                        EasingFunction::EaseOutQuart => "Ease Out Quart",
                        EasingFunction::EaseInOutQuart => "Ease In Out Quart",
                        // Quint
                        EasingFunction::EaseInQuint => "Ease In Quint",
                        EasingFunction::EaseOutQuint => "Ease Out Quint",
                        EasingFunction::EaseInOutQuint => "Ease In Out Quint",
                        // Expo
                        EasingFunction::EaseInExpo => "Ease In Expo",
                        EasingFunction::EaseOutExpo => "Ease Out Expo",
                        EasingFunction::EaseInOutExpo => "Ease In Out Expo",
                        // Circ
                        EasingFunction::EaseInCirc => "Ease In Circ",
                        EasingFunction::EaseOutCirc => "Ease Out Circ",
                        EasingFunction::EaseInOutCirc => "Ease In Out Circ",
                        // Back
                        EasingFunction::EaseInBack { .. } => "Ease In Back",
                        EasingFunction::EaseOutBack { .. } => "Ease Out Back",
                        EasingFunction::EaseInOutBack { .. } => "Ease In Out Back",
                        // Elastic
                        EasingFunction::EaseInElastic { .. } => "Ease In Elastic",
                        EasingFunction::EaseOutElastic { .. } => "Ease Out Elastic",
                        EasingFunction::EaseInOutElastic { .. } => "Ease In Out Elastic",
                        // Bounce
                        EasingFunction::EaseInBounce { .. } => "Ease In Bounce",
                        EasingFunction::EaseOutBounce { .. } => "Ease Out Bounce",
                        EasingFunction::EaseInOutBounce { .. } => "Ease In Out Bounce",

                        _ => "Custom",
                    };

                    ComboBox::from_id_salt("easing_selector")
                        .selected_text(current_variant_name)
                        .show_ui(ui, |ui| {
                            let current_easing = state.easing.clone();
                            crate::ui::easing_menus::show_easing_menu(
                                ui,
                                Some(&current_easing),
                                |easing| {
                                    state.easing = easing;
                                    should_update = true;
                                    should_push_history = true;
                                },
                            );
                        });
                    ui.end_row();
                });

            // Parameter Editor
            match &mut state.easing {
                EasingFunction::EaseInBack { c1 }
                | EasingFunction::EaseOutBack { c1 }
                | EasingFunction::EaseInOutBack { c1 } => {
                    // Sanitize c1
                    if !c1.is_finite() {
                        *c1 = 1.70158;
                    }

                    ui.separator();
                    ui.horizontal(|ui| {
                        ui.label("Overshoot (c1):");
                        let c1_res = ui.add(DragValue::new(c1).speed(0.01));
                        if c1_res.changed() {
                            should_update = true;
                        }
                        if c1_res.drag_stopped() {
                            should_push_history = true;
                        }
                    });
                }
                EasingFunction::EaseInElastic { period }
                | EasingFunction::EaseOutElastic { period }
                | EasingFunction::EaseInOutElastic { period } => {
                    // Sanitize period? Range prevents bad values from UI, but init might be bad.
                    if !period.is_finite() {
                        *period = 3.0;
                    }

                    ui.separator();
                    ui.horizontal(|ui| {
                        ui.label("Period:");
                        let period_res =
                            ui.add(DragValue::new(period).speed(0.01).range(0.1..=100.0));
                        if period_res.changed() {
                            should_update = true;
                        }
                        if period_res.drag_stopped() {
                            should_push_history = true;
                        }
                    });
                }
                EasingFunction::EaseInBounce { n1, d1 }
                | EasingFunction::EaseOutBounce { n1, d1 }
                | EasingFunction::EaseInOutBounce { n1, d1 } => {
                    if !n1.is_finite() {
                        *n1 = 7.5625;
                    }
                    if !d1.is_finite() {
                        *d1 = 2.75;
                    }

                    ui.separator();
                    ui.horizontal(|ui| {
                        ui.label("Amplitude (n1):");
                        let n1_res = ui.add(DragValue::new(n1).speed(0.01));
                        if n1_res.changed() {
                            should_update = true;
                        }
                        if n1_res.drag_stopped() {
                            should_push_history = true;
                        }

                        ui.add_space(10.0);
                        ui.label("Duration Factor (d1):");
                        let d1_res = ui.add(DragValue::new(d1).speed(0.01));
                        if d1_res.changed() {
                            should_update = true;
                        }
                        if d1_res.drag_stopped() {
                            should_push_history = true;
                        }
                    });
                }
                EasingFunction::Expression { text } => {
                    ui.separator();
                    ui.label("Expression (Python):");
                    let response = ui.add(
                        TextEdit::multiline(text)
                            .code_editor()
                            .desired_rows(3)
                            .lock_focus(true)
                            .text_color(Color32::LIGHT_GRAY),
                    );
                    if response.changed() {
                        should_update = true;
                    }
                    if response.lost_focus() {
                        should_push_history = true;
                    } // Push only when done editing expression

                    ui.label(
                        egui::RichText::new("Variables: t (0.0 to 1.0)")
                            .size(10.0)
                            .weak(),
                    );
                }
                _ => {}
            }

            super::dialog_footer(ui, |ui| {
                if ui.button("Close").clicked() {
                    should_close = true;
                }
            });

            if should_update {
                let comp_id = match editor_context.selection.composition_id {
                    Some(id) => id,
                    None => return,
                };

                if let (Some(track_id), Some(entity_id)) = (state.track_id, state.entity_id) {
                    let new_time = state.time;
                    use library::model::project::property::PropertyValue;
                    use ordered_float::OrderedFloat;

                    // Handle suffix logic
                    let (base_name, component_suffix) = if state.property_name.ends_with(".x") {
                        (
                            state.property_name.trim_end_matches(".x"),
                            Some(crate::ui::panels::graph_editor::PropertyComponent::X),
                        )
                    } else if state.property_name.ends_with(".y") {
                        (
                            state.property_name.trim_end_matches(".y"),
                            Some(crate::ui::panels::graph_editor::PropertyComponent::Y),
                        )
                    } else {
                        (state.property_name.as_str(), None)
                    };

                    // Helper to parse key
                    let parse_key = |key: &str| -> Option<(usize, String)> {
                        if key.starts_with("effect:") {
                            let parts: Vec<&str> = key.splitn(3, ':').collect();
                            if parts.len() == 3 {
                                if let Ok(idx) = parts[1].parse::<usize>() {
                                    return Some((idx, parts[2].to_string()));
                                }
                            }
                        }
                        None
                    };

                    let parse_style_key = |key: &str| -> Option<(usize, String)> {
                        if key.starts_with("style:") {
                            let parts: Vec<&str> = key.splitn(3, ':').collect();
                            if parts.len() == 3 {
                                if let Ok(idx) = parts[1].parse::<usize>() {
                                    return Some((idx, parts[2].to_string()));
                                }
                            }
                        }
                        None
                    };

                    // Fetch current value to merge components if needed
                    let mut current_pv = None;
                    if let Ok(proj) = project.read() {
                        if let Some(comp) = proj.compositions.iter().find(|c| c.id == comp_id) {
                            if let Some(track) = comp.get_track(track_id) {
                                if let Some(clip) = track.clips().find(|c| c.id == entity_id) {
                                    if let Some((eff_idx, prop_key)) = parse_key(base_name) {
                                        if let Some(effect) = clip.effects.get(eff_idx) {
                                            if let Some(prop) = effect.properties.get(&prop_key) {
                                                if let Some(kf) =
                                                    prop.keyframes().get(state.keyframe_index)
                                                {
                                                    // Use index directly, assuming sorted
                                                    current_pv = Some(kf.value.clone());
                                                }
                                            }
                                        }
                                    } else if let Some((style_idx, prop_key)) =
                                        parse_style_key(base_name)
                                    {
                                        if let Some(style) = clip.styles.get(style_idx) {
                                            if let Some(prop) = style.properties.get(&prop_key) {
                                                if let Some(kf) =
                                                    prop.keyframes().get(state.keyframe_index)
                                                {
                                                    current_pv = Some(kf.value.clone());
                                                }
                                            }
                                        }
                                    } else {
                                        if let Some(prop) = clip.properties.get(base_name) {
                                            if let Some(kf) =
                                                prop.keyframes().get(state.keyframe_index)
                                            {
                                                current_pv = Some(kf.value.clone());
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }

                    // Construct new PropertyValue
                    let new_value = if let Some(PropertyValue::Vec2(old_vec)) = current_pv {
                        match component_suffix {
                            Some(crate::ui::panels::graph_editor::PropertyComponent::X) => {
                                PropertyValue::Vec2(library::model::project::property::Vec2 {
                                    x: OrderedFloat(state.value),
                                    y: old_vec.y,
                                })
                            }
                            Some(crate::ui::panels::graph_editor::PropertyComponent::Y) => {
                                PropertyValue::Vec2(library::model::project::property::Vec2 {
                                    x: old_vec.x,
                                    y: OrderedFloat(state.value),
                                })
                            }
                            _ => PropertyValue::Number(OrderedFloat(state.value)),
                        }
                    } else {
                        PropertyValue::Number(OrderedFloat(state.value))
                    };

                    let result = if let Some((eff_idx, prop_key)) = parse_key(base_name) {
                        project_service.update_effect_keyframe_by_index(
                            comp_id,
                            track_id,
                            entity_id,
                            eff_idx,
                            &prop_key,
                            state.keyframe_index,
                            Some(new_time),
                            Some(new_value),
                            Some(state.easing.clone()),
                        )
                    } else if let Some((style_idx, prop_key)) = parse_style_key(base_name) {
                        project_service.update_style_keyframe_by_index(
                            comp_id,
                            track_id,
                            entity_id,
                            style_idx,
                            &prop_key,
                            state.keyframe_index,
                            Some(new_time),
                            Some(new_value),
                            Some(state.easing.clone()),
                        )
                    } else {
                        project_service.update_keyframe(
                            comp_id,
                            track_id,
                            entity_id,
                            base_name,
                            state.keyframe_index,
                            Some(new_time),
                            Some(new_value),
                            Some(state.easing.clone()),
                        )
                    };

                    if let Err(_e) = result {
                        // Only show error on final interaction to avoid spamming?
                        // Or show generic error.
                        // editor_context.interaction.active_modal_error = Some(format!("Failed to update keyframe: {}", e));
                    }
                }
            }

            if should_push_history {
                if let Ok(proj_read) = project_service.get_project().read() {
                    history_manager.push_project_state(proj_read.clone());
                }
            }
        });

    editor_context.keyframe_dialog.is_open = open && !should_close;
}
