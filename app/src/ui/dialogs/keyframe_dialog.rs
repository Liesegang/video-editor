use eframe::egui::{self, Color32, ComboBox, DragValue, TextEdit};
use library::animation::EasingFunction;
use library::model::project::Project;
use library::model::property::{KeyframeUpdate, PropertyValue};
use library::EditorService;
use ordered_float::OrderedFloat;
use std::sync::{Arc, RwLock};

use crate::action::HistoryManager;
use crate::state::context::EditorContext;
use crate::state::context_types::{
    KeyframeDialogEditControl, KeyframeDialogState, KeyframeDialogValues, KeyframeValueComponent,
};
use crate::ui::panels::graph_editor::utils::time_mapper_for_owner;

struct PreparedKeyframeDialogUpdate {
    owner: library::PropertyOwner,
    property_key: String,
    keyframe_id: library::model::property::KeyframeId,
    update: KeyframeUpdate,
}

fn prepare_keyframe_dialog_update(
    project: &Project,
    state: &KeyframeDialogState,
) -> Option<PreparedKeyframeDialogUpdate> {
    let owner = state.owner?;
    let keyframe_id = state.keyframe_id?;
    let current_value = match owner {
        library::PropertyOwner::Node(node_id) => {
            project.get_node(node_id).map(|node| node.properties())
        }
        library::PropertyOwner::Clip(clip_id) => {
            project.get_clip(clip_id).map(|clip| &clip.properties)
        }
    }
    .and_then(|properties| properties.get(&state.property_key))
    .and_then(|property| property.keyframe_by_id(keyframe_id))
    .map(|keyframe| keyframe.value);
    let value = if let Some(PropertyValue::Vec2(old)) = current_value {
        match state.component {
            KeyframeValueComponent::X => PropertyValue::Vec2(library::model::property::Vec2 {
                x: OrderedFloat(state.value),
                y: old.y,
            }),
            KeyframeValueComponent::Y => PropertyValue::Vec2(library::model::property::Vec2 {
                x: old.x,
                y: OrderedFloat(state.value),
            }),
            KeyframeValueComponent::Scalar => PropertyValue::Number(OrderedFloat(state.value)),
        }
    } else {
        PropertyValue::Number(OrderedFloat(state.value))
    };
    Some(PreparedKeyframeDialogUpdate {
        owner,
        property_key: state.property_key.clone(),
        keyframe_id,
        update: KeyframeUpdate {
            time: Some(time_mapper_for_owner(project, owner).to_source_time(state.time)),
            value: Some(value),
            easing: Some(state.easing.clone()),
        },
    })
}

fn response_finished(ui: &egui::Ui, response: &egui::Response) -> bool {
    response.drag_stopped()
        || response.lost_focus()
        || (response.has_focus() && ui.input(|input| input.key_pressed(egui::Key::Enter)))
}

fn flush_keyframe_dialog_transaction(
    state: &mut KeyframeDialogState,
    history_manager: &mut HistoryManager,
    project_service: &EditorService,
) -> bool {
    if !state.transaction.dirty {
        state.transaction.active_control = None;
        return false;
    }
    let snapshot = match project_service.get_project().read() {
        Ok(project) => project.clone(),
        Err(error) => {
            log::error!("Failed to capture Keyframe dialog history: {error}");
            return false;
        }
    };
    history_manager.push_project_state(snapshot);
    state.transaction.baseline = Some(state.values());
    state.transaction.active_control = None;
    state.transaction.dirty = false;
    true
}

fn apply_keyframe_dialog_change(
    state: &mut KeyframeDialogState,
    control: KeyframeDialogEditControl,
    frame_baseline: KeyframeDialogValues,
    history_manager: &mut HistoryManager,
    project_service: &EditorService,
    project: &Arc<RwLock<Project>>,
) -> bool {
    if state
        .transaction
        .active_control
        .is_some_and(|active| active != control)
    {
        flush_keyframe_dialog_transaction(state, history_manager, project_service);
        state.transaction.baseline = Some(frame_baseline);
    } else if state.transaction.baseline.is_none() {
        state.transaction.baseline = Some(frame_baseline);
    }
    state.transaction.active_control = Some(control);

    let prepared = project
        .read()
        .ok()
        .and_then(|project| prepare_keyframe_dialog_update(&project, state));
    let Some(prepared) = prepared else {
        log::error!("Failed to prepare Keyframe dialog update");
        return false;
    };
    match project_service.update_keyframe_by_id(
        prepared.owner,
        &prepared.property_key,
        prepared.keyframe_id,
        prepared.update,
    ) {
        Ok(()) => {
            let current = state.values();
            state.transaction.dirty = state
                .transaction
                .baseline
                .as_ref()
                .is_none_or(|baseline| !baseline.matches(&current));
            true
        }
        Err(error) => {
            log::error!(
                "Failed to update keyframe {}: {error}",
                prepared.keyframe_id
            );
            false
        }
    }
}

pub fn show_keyframe_dialog(
    ctx: &egui::Context,
    editor_context: &mut EditorContext,
    history_manager: &mut HistoryManager,
    project_service: &mut EditorService,
    project: &Arc<RwLock<Project>>,
) {
    let mut open = editor_context.keyframe_dialog.is_open;
    let mut should_close = false;

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
            let frame_baseline = state.values();
            let mut changed_control = None;
            let mut finished_controls = Vec::new();

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
                    crate::qa::register_component_with_metadata(
                        "keyframe_dialog.time",
                        "keyframe_dialog_control",
                        time_response.rect,
                        time_response.enabled(),
                        Some(serde_json::json!({
                            "global_time": state.time,
                            "entity_id": state.entity_id,
                            "property": state.property_key,
                        })),
                    );
                    if time_response.changed() {
                        changed_control = Some(KeyframeDialogEditControl::Time);
                    }
                    if response_finished(ui, &time_response) {
                        finished_controls.push(KeyframeDialogEditControl::Time);
                    }
                    ui.end_row();

                    ui.label("Value:");
                    let val_response = ui.add(DragValue::new(&mut state.value).speed(0.1)); // Range is implicitly infinite, but value is sanitized
                    crate::qa::register_component_with_metadata(
                        "keyframe_dialog.value",
                        "keyframe_dialog_control",
                        val_response.rect,
                        val_response.enabled(),
                        Some(serde_json::json!({
                            "value": state.value,
                            "component": format!("{:?}", state.component),
                        })),
                    );
                    if val_response.changed() {
                        changed_control = Some(KeyframeDialogEditControl::Value);
                    }
                    if response_finished(ui, &val_response) {
                        finished_controls.push(KeyframeDialogEditControl::Value);
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

                    let easing_response = ComboBox::from_id_salt("easing_selector")
                        .selected_text(current_variant_name)
                        .show_ui(ui, |ui| {
                            let current_easing = state.easing.clone();
                            crate::ui::easing_menus::show_easing_menu(
                                ui,
                                Some(&current_easing),
                                None,
                                |easing| {
                                    state.easing = easing;
                                    changed_control = Some(KeyframeDialogEditControl::Easing);
                                    finished_controls.push(KeyframeDialogEditControl::Easing);
                                },
                            );
                        });
                    crate::qa::register_component(
                        "keyframe_dialog.easing",
                        "keyframe_dialog_control",
                        easing_response.response.rect,
                    );
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
                            changed_control = Some(KeyframeDialogEditControl::Overshoot);
                        }
                        if response_finished(ui, &c1_res) {
                            finished_controls.push(KeyframeDialogEditControl::Overshoot);
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
                            changed_control = Some(KeyframeDialogEditControl::Period);
                        }
                        if response_finished(ui, &period_res) {
                            finished_controls.push(KeyframeDialogEditControl::Period);
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
                            changed_control = Some(KeyframeDialogEditControl::BounceAmplitude);
                        }
                        if response_finished(ui, &n1_res) {
                            finished_controls.push(KeyframeDialogEditControl::BounceAmplitude);
                        }

                        ui.add_space(10.0);
                        ui.label("Duration Factor (d1):");
                        let d1_res = ui.add(DragValue::new(d1).speed(0.01));
                        if d1_res.changed() {
                            changed_control = Some(KeyframeDialogEditControl::BounceDuration);
                        }
                        if response_finished(ui, &d1_res) {
                            finished_controls.push(KeyframeDialogEditControl::BounceDuration);
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
                        changed_control = Some(KeyframeDialogEditControl::Expression);
                    }
                    if response.lost_focus() {
                        finished_controls.push(KeyframeDialogEditControl::Expression);
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
                let close = ui.button("Close");
                crate::qa::register_component(
                    "keyframe_dialog.close",
                    "keyframe_dialog_button",
                    close.rect,
                );
                if close.clicked() {
                    should_close = true;
                }
            });

            if let Some(control) = changed_control {
                apply_keyframe_dialog_change(
                    state,
                    control,
                    frame_baseline,
                    history_manager,
                    project_service,
                    project,
                );
            }

            if state
                .transaction
                .active_control
                .is_some_and(|active| finished_controls.contains(&active))
            {
                flush_keyframe_dialog_transaction(state, history_manager, project_service);
            }
        });

    if should_close || !open {
        flush_keyframe_dialog_transaction(
            &mut editor_context.keyframe_dialog,
            history_manager,
            project_service,
        );
    }
    editor_context.keyframe_dialog.is_open = open && !should_close;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::generator_node;
    use library::animation::EasingFunction;
    use library::cache::CacheManager;
    use library::editor::project_service::GeneratorNodeRequest;
    use library::model::frame::color::Color;
    use library::model::property::{Keyframe, Property, Vec2};
    use library::model::Clip;
    use library::plugin::PluginManager;

    #[test]
    fn dialog_converts_global_time_once_and_preserves_the_other_vector_component() {
        let keyframe = Keyframe::new(
            2.0,
            PropertyValue::Vec2(Vec2 {
                x: OrderedFloat(10.0),
                y: OrderedFloat(20.0),
            }),
            EasingFunction::Linear,
        );
        let keyframe_id = keyframe.id;
        let mut node = generator_node(
            "dialog",
            GeneratorNodeRequest::Solid {
                color: Color::default(),
            },
        );
        let node_id = node.id;
        node.set_property("position".to_string(), Property::keyframe(vec![keyframe]))
            .expect("solid factory initializes position");
        let mut clip = Clip::new("mapped", 4.0, 8.0);
        clip.trim_in = OrderedFloat(1.5);
        clip.time_stretch = OrderedFloat(0.5);
        clip.node_ids = vec![node_id];
        clip.output_node_id = Some(node_id);
        let mut project = Project::new("dialog mapping");
        project.add_node(node);
        project.add_clip(clip);

        let state = KeyframeDialogState {
            is_open: true,
            track_id: None,
            entity_id: Some(node_id),
            property_name: "node:position.x".to_string(),
            owner: Some(library::PropertyOwner::Node(node_id)),
            property_key: "position".to_string(),
            keyframe_id: Some(keyframe_id),
            component: KeyframeValueComponent::X,
            time: 6.25,
            value: 99.0,
            easing: EasingFunction::EaseInQuad,
            transaction: Default::default(),
        };

        let prepared = prepare_keyframe_dialog_update(&project, &state).unwrap();
        assert_eq!(prepared.update.time, Some(2.625));
        assert_eq!(
            prepared.update.value,
            Some(PropertyValue::Vec2(Vec2 {
                x: OrderedFloat(99.0),
                y: OrderedFloat(20.0),
            }))
        );
        assert_eq!(prepared.update.easing, Some(EasingFunction::EaseInQuad));
    }

    #[test]
    fn dialog_transactions_flush_on_control_switch_and_close_with_undo_redo() {
        let keyframe = Keyframe::new(
            1.0,
            PropertyValue::Number(OrderedFloat(10.0)),
            EasingFunction::Linear,
        );
        let keyframe_id = keyframe.id;
        let mut node = generator_node(
            "dialog history",
            GeneratorNodeRequest::Solid {
                color: Color::default(),
            },
        );
        let node_id = node.id;
        node.set_property("opacity".to_string(), Property::keyframe(vec![keyframe]))
            .expect("solid factory initializes opacity");
        let mut initial = Project::new("dialog history");
        initial.add_node(node);
        let project = Arc::new(RwLock::new(initial.clone()));
        let service = EditorService::new(
            Arc::clone(&project),
            Arc::new(PluginManager::default()),
            Arc::new(CacheManager::new()),
        )
        .unwrap();
        let mut history = HistoryManager::new();
        history.push_project_state(initial.clone());
        let mut state = KeyframeDialogState {
            is_open: true,
            entity_id: Some(node_id),
            property_name: "node:opacity".to_string(),
            owner: Some(library::PropertyOwner::Node(node_id)),
            property_key: "opacity".to_string(),
            keyframe_id: Some(keyframe_id),
            time: 1.0,
            value: 10.0,
            ..Default::default()
        };
        state.begin_transaction();

        let baseline = state.values();
        state.value = 20.0;
        assert!(apply_keyframe_dialog_change(
            &mut state,
            KeyframeDialogEditControl::Value,
            baseline,
            &mut history,
            &service,
            &project,
        ));
        assert_eq!(
            history.undo_depth(),
            1,
            "live edits remain one pending gesture"
        );

        let baseline = state.values();
        state.value = 30.0;
        assert!(apply_keyframe_dialog_change(
            &mut state,
            KeyframeDialogEditControl::Value,
            baseline,
            &mut history,
            &service,
            &project,
        ));
        assert_eq!(
            history.undo_depth(),
            1,
            "multi-frame drag is not pushed per frame"
        );

        let baseline = state.values();
        state.time = 2.0;
        assert!(apply_keyframe_dialog_change(
            &mut state,
            KeyframeDialogEditControl::Time,
            baseline,
            &mut history,
            &service,
            &project,
        ));
        assert_eq!(
            history.undo_depth(),
            2,
            "control switch flushes the prior gesture"
        );
        assert!(flush_keyframe_dialog_transaction(
            &mut state,
            &mut history,
            &service,
        ));
        assert_eq!(
            history.undo_depth(),
            3,
            "Close/Escape/X flushes the active gesture"
        );

        let edited = project.read().unwrap().clone();
        let after_value = history.undo(&edited).expect("time edit should undo");
        let keyframe = after_value
            .get_node(node_id)
            .unwrap()
            .properties()
            .get("opacity")
            .unwrap()
            .keyframe_by_id(keyframe_id)
            .unwrap();
        assert_eq!(keyframe.time, OrderedFloat(1.0));
        assert_eq!(keyframe.value, PropertyValue::Number(OrderedFloat(30.0)));
        assert_eq!(history.redo(&after_value), Some(edited));
    }
}
