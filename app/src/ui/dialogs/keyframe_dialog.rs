use eframe::egui::{self, Align2, Color32, ComboBox, DragValue, TextEdit, Ui, Window};
use library::animation::EasingFunction;
use library::service::project_service::ProjectService;
use library::model::project::project::Project;
use std::sync::{Arc, RwLock};

use crate::state::context::EditorContext;

pub fn show_keyframe_dialog(
    ctx: &egui::Context,
    editor_context: &mut EditorContext,
    project_service: &mut ProjectService,
    _project: &Arc<RwLock<Project>>,
) {
    let mut open = editor_context.keyframe_dialog.is_open;
    let mut should_close = false;

    Window::new("Edit Keyframe")
        .open(&mut open)
        .anchor(Align2::CENTER_CENTER, egui::Vec2::ZERO)
        .collapsible(false)
        .resizable(true)
        .show(ctx, |ui| {
            let state = &mut editor_context.keyframe_dialog;

            egui::Grid::new("keyframe_grid")
                .num_columns(2)
                .spacing([10.0, 10.0])
                .show(ui, |ui| {
                    ui.label("Time:");
                    ui.add(DragValue::new(&mut state.time).speed(0.01).suffix(" s"));
                    ui.end_row();

                    ui.label("Value:");
                    ui.add(DragValue::new(&mut state.value).speed(0.1));
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
                            ui.selectable_value(&mut state.easing, EasingFunction::Linear, "Linear");
                            ui.separator();
                            ui.label("Sine");
                            ui.selectable_value(&mut state.easing, EasingFunction::EaseInSine, "Ease In Sine");
                            ui.selectable_value(&mut state.easing, EasingFunction::EaseOutSine, "Ease Out Sine");
                            ui.selectable_value(&mut state.easing, EasingFunction::EaseInOutSine, "Ease In Out Sine");
                             ui.separator();
                            ui.label("Quad");
                            ui.selectable_value(&mut state.easing, EasingFunction::EaseInQuad, "Ease In Quad");
                            ui.selectable_value(&mut state.easing, EasingFunction::EaseOutQuad, "Ease Out Quad");
                            ui.selectable_value(&mut state.easing, EasingFunction::EaseInOutQuad, "Ease In Out Quad");
                            ui.separator();
                            ui.label("Back");
                            ui.selectable_value(&mut state.easing, EasingFunction::EaseInBack { c1: 1.70158 }, "Ease In Back");
                            ui.selectable_value(&mut state.easing, EasingFunction::EaseOutBack { c1: 1.70158 }, "Ease Out Back");
                            ui.selectable_value(&mut state.easing, EasingFunction::EaseInOutBack { c1: 1.70158 }, "Ease In Out Back");
                            ui.separator();
                            ui.label("Elastic");
                            ui.selectable_value(&mut state.easing, EasingFunction::EaseInElastic { period: 3.0 }, "Ease In Elastic");
                            ui.selectable_value(&mut state.easing, EasingFunction::EaseOutElastic { period: 3.0 }, "Ease Out Elastic");
                            ui.selectable_value(&mut state.easing, EasingFunction::EaseInOutElastic { period: 4.5 }, "Ease In Out Elastic");
                            ui.separator();
                            ui.label("Bounce");
                            ui.selectable_value(&mut state.easing, EasingFunction::EaseInBounce { n1: 7.5625, d1: 2.75 }, "Ease In Bounce");
                            ui.selectable_value(&mut state.easing, EasingFunction::EaseOutBounce { n1: 7.5625, d1: 2.75 }, "Ease Out Bounce");
                            ui.selectable_value(&mut state.easing, EasingFunction::EaseInOutBounce { n1: 7.5625, d1: 2.75 }, "Ease In Out Bounce");
                            
                            ui.separator();
                            if ui.selectable_label(matches!(state.easing, EasingFunction::Expression{..}), "Expression").clicked() {
                                // Preserve text if already expression, otherwise default
                                if !matches!(state.easing, EasingFunction::Expression{..}) {
                                    state.easing = EasingFunction::Expression{ text: "t".to_string() };
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
                    ui.separator();
                    ui.horizontal(|ui| {
                        ui.label("Overshoot (c1):");
                        ui.add(DragValue::new(c1).speed(0.01));
                    });
                }
                EasingFunction::EaseInElastic { period }
                | EasingFunction::EaseOutElastic { period }
                | EasingFunction::EaseInOutElastic { period } => {
                    ui.separator();
                     ui.horizontal(|ui| {
                        ui.label("Period:");
                        ui.add(DragValue::new(period).speed(0.01).range(0.1..=100.0));
                    });
                }
                EasingFunction::EaseInBounce { n1, d1 }
                | EasingFunction::EaseOutBounce { n1, d1 }
                | EasingFunction::EaseInOutBounce { n1, d1 } => {
                    ui.separator();
                    ui.horizontal(|ui| {
                         ui.label("Amplitude (n1):");
                        ui.add(DragValue::new(n1).speed(0.01));
                        ui.add_space(10.0);
                        ui.label("Duration Factor (d1):");
                        ui.add(DragValue::new(d1).speed(0.01));
                    });
                }
                 EasingFunction::Expression { text } => {
                    ui.separator();
                    ui.label("Expression (Python):");
                    ui.add(
                        TextEdit::multiline(text)
                            .code_editor()
                            .desired_rows(3)
                            .lock_focus(true)
                            .text_color(Color32::LIGHT_GRAY)
                    );

                    ui.label(egui::RichText::new("Variables: t (0.0 to 1.0)").size(10.0).weak());
                }
                _ => {}
            }

            ui.add_space(10.0);
            ui.horizontal(|ui| {
                 if ui.button("Apply").clicked() {
                     let comp_id = match editor_context.selection.composition_id {
                         Some(id) => id,
                         None => {
                             // Should be impossible if editing keyframe, but fail safe
                             return; 
                         }
                     };
                     
                    if let (Some(track_id), Some(entity_id)) = (state.track_id, state.entity_id) {
                         // ProjectService uses internal lock, but update_keyframe takes &mut self.
                         // However, the signature I saw takes &mut self and locks internally?
                         // Checked signature: pub fn update_keyframe(&mut self, ...)
                         // Yes, it takes &mut self.
                         
                         let new_time = state.time;
                         use library::model::project::property::PropertyValue;
                         use ordered_float::OrderedFloat;
                         
                         let new_value = PropertyValue::Number(OrderedFloat(state.value));
                         
                         if let Err(e) = project_service.update_keyframe(
                             comp_id,
                             track_id, 
                             entity_id,
                             &state.property_name,
                             state.keyframe_index,
                             Some(new_time),
                             Some(new_value),
                             Some(state.easing.clone())
                         ) {
                             editor_context.interaction.active_modal_error = Some(format!("Failed to update keyframe: {}", e));
                         }
                    }
                     should_close = true; 
                 }
                if ui.button("Cancel").clicked() {
                    should_close = true;
                }
            });
        });

    editor_context.keyframe_dialog.is_open = open && !should_close;
}
