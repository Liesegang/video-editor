use eframe::egui::{self, Align2, Color32, ComboBox, DragValue, TextEdit, Window};
use library::animation::EasingFunction;
use library::model::project::project::Project;
use library::service::project_service::ProjectService;
use std::sync::{Arc, RwLock};

use crate::action::HistoryManager;
use crate::state::context::EditorContext;

pub fn show_keyframe_dialog(
    ctx: &egui::Context,
    editor_context: &mut EditorContext,
    history_manager: &mut HistoryManager,
    project_service: &mut ProjectService,
    _project: &Arc<RwLock<Project>>,
) {
    let mut open = editor_context.keyframe_dialog.is_open;
    let mut should_close = false;
    let mut should_update = false;
    let mut should_push_history = false;

    Window::new("Edit Keyframe")
        .open(&mut open)
        .anchor(Align2::CENTER_CENTER, egui::Vec2::ZERO)
        .collapsible(false)
        .resizable(true)
        .show(ctx, |ui| {
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
                            if ui
                                .selectable_value(
                                    &mut state.easing,
                                    EasingFunction::Linear,
                                    "Linear",
                                )
                                .clicked()
                            {
                                should_update = true;
                                should_push_history = true;
                            }
                            ui.separator();
                            ui.label("Sine");
                            if ui
                                .selectable_value(
                                    &mut state.easing,
                                    EasingFunction::EaseInSine,
                                    "Ease In Sine",
                                )
                                .clicked()
                            {
                                should_update = true;
                                should_push_history = true;
                            }
                            if ui
                                .selectable_value(
                                    &mut state.easing,
                                    EasingFunction::EaseOutSine,
                                    "Ease Out Sine",
                                )
                                .clicked()
                            {
                                should_update = true;
                                should_push_history = true;
                            }
                            if ui
                                .selectable_value(
                                    &mut state.easing,
                                    EasingFunction::EaseInOutSine,
                                    "Ease In Out Sine",
                                )
                                .clicked()
                            {
                                should_update = true;
                                should_push_history = true;
                            }
                            ui.separator();
                            ui.label("Quad");
                            if ui
                                .selectable_value(
                                    &mut state.easing,
                                    EasingFunction::EaseInQuad,
                                    "Ease In Quad",
                                )
                                .clicked()
                            {
                                should_update = true;
                                should_push_history = true;
                            }
                            if ui
                                .selectable_value(
                                    &mut state.easing,
                                    EasingFunction::EaseOutQuad,
                                    "Ease Out Quad",
                                )
                                .clicked()
                            {
                                should_update = true;
                                should_push_history = true;
                            }
                            if ui
                                .selectable_value(
                                    &mut state.easing,
                                    EasingFunction::EaseInOutQuad,
                                    "Ease In Out Quad",
                                )
                                .clicked()
                            {
                                should_update = true;
                                should_push_history = true;
                            }
                            ui.separator();
                            ui.label("Back");
                            if ui
                                .selectable_value(
                                    &mut state.easing,
                                    EasingFunction::EaseInBack { c1: 1.70158 },
                                    "Ease In Back",
                                )
                                .clicked()
                            {
                                should_update = true;
                                should_push_history = true;
                            }
                            if ui
                                .selectable_value(
                                    &mut state.easing,
                                    EasingFunction::EaseOutBack { c1: 1.70158 },
                                    "Ease Out Back",
                                )
                                .clicked()
                            {
                                should_update = true;
                                should_push_history = true;
                            }
                            if ui
                                .selectable_value(
                                    &mut state.easing,
                                    EasingFunction::EaseInOutBack { c1: 1.70158 },
                                    "Ease In Out Back",
                                )
                                .clicked()
                            {
                                should_update = true;
                                should_push_history = true;
                            }
                            ui.separator();
                            ui.label("Elastic");
                            if ui
                                .selectable_value(
                                    &mut state.easing,
                                    EasingFunction::EaseInElastic { period: 3.0 },
                                    "Ease In Elastic",
                                )
                                .clicked()
                            {
                                should_update = true;
                                should_push_history = true;
                            }
                            if ui
                                .selectable_value(
                                    &mut state.easing,
                                    EasingFunction::EaseOutElastic { period: 3.0 },
                                    "Ease Out Elastic",
                                )
                                .clicked()
                            {
                                should_update = true;
                                should_push_history = true;
                            }
                            if ui
                                .selectable_value(
                                    &mut state.easing,
                                    EasingFunction::EaseInOutElastic { period: 4.5 },
                                    "Ease In Out Elastic",
                                )
                                .clicked()
                            {
                                should_update = true;
                                should_push_history = true;
                            }
                            ui.separator();
                            ui.label("Bounce");
                            if ui
                                .selectable_value(
                                    &mut state.easing,
                                    EasingFunction::EaseInBounce {
                                        n1: 7.5625,
                                        d1: 2.75,
                                    },
                                    "Ease In Bounce",
                                )
                                .clicked()
                            {
                                should_update = true;
                                should_push_history = true;
                            }
                            if ui
                                .selectable_value(
                                    &mut state.easing,
                                    EasingFunction::EaseOutBounce {
                                        n1: 7.5625,
                                        d1: 2.75,
                                    },
                                    "Ease Out Bounce",
                                )
                                .clicked()
                            {
                                should_update = true;
                                should_push_history = true;
                            }
                            if ui
                                .selectable_value(
                                    &mut state.easing,
                                    EasingFunction::EaseInOutBounce {
                                        n1: 7.5625,
                                        d1: 2.75,
                                    },
                                    "Ease In Out Bounce",
                                )
                                .clicked()
                            {
                                should_update = true;
                                should_push_history = true;
                            }
                            ui.separator();
                            if ui
                                .selectable_label(
                                    matches!(state.easing, EasingFunction::Expression { .. }),
                                    "Expression",
                                )
                                .clicked()
                            {
                                // Preserve text if already expression, otherwise default
                                if !matches!(state.easing, EasingFunction::Expression { .. }) {
                                    state.easing = EasingFunction::Expression {
                                        text: "t".to_string(),
                                    };
                                    should_update = true;
                                    should_push_history = true;
                                }
                            }
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

            ui.add_space(10.0);
            ui.horizontal(|ui| {
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

                    let new_value = PropertyValue::Number(OrderedFloat(state.value));

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

                    let result = if let Some((eff_idx, prop_key)) = parse_key(&state.property_name)
                    {
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
                    } else {
                        project_service.update_keyframe(
                            comp_id,
                            track_id,
                            entity_id,
                            &state.property_name,
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
