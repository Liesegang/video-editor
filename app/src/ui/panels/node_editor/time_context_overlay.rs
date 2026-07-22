//! Hold-to-reveal policy for inherited Time wires.
//!
//! The runtime relationship remains derived from `EvaluationScope`. This
//! module only decides whether the Node Editor should reveal that relationship
//! for the current frame and publishes one read-only QA surface.

use eframe::egui;

pub(in crate::ui::panels::node_editor) const IMPLICIT_TIME_OVERLAY_KEY: egui::Key = egui::Key::T;
pub(in crate::ui::panels::node_editor) const IMPLICIT_TIME_OVERLAY_KEY_LABEL: &str = "T";

pub(in crate::ui::panels::node_editor) fn implicit_time_overlay_requested(
    context: &egui::Context,
    canvas_clip: egui::Rect,
) -> bool {
    if context.wants_keyboard_input() {
        return false;
    }

    context.input(|input| {
        input
            .pointer
            .hover_pos()
            .is_some_and(|position| canvas_clip.contains(position))
            && input.key_down(IMPLICIT_TIME_OVERLAY_KEY)
            && input.modifiers.is_none()
    })
}

pub(in crate::ui::panels::node_editor) fn register_implicit_time_overlay(
    canvas_clip: egui::Rect,
    wire_count: usize,
) {
    crate::qa::register_component_with_metadata(
        "node_editor.time_context_overlay",
        "node_time_context_overlay",
        canvas_clip,
        false,
        Some(serde_json::json!({
            "active": true,
            "gesture": "hold",
            "trigger": "hold_key",
            "held": true,
            "key": IMPLICIT_TIME_OVERLAY_KEY_LABEL,
            "wire_count": wire_count,
            "editable": false,
            "hit_testable": false,
            "runtime_authority": "evaluation_scope",
            "presentation_persisted": false,
        })),
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key_event(pressed: bool, modifiers: egui::Modifiers) -> egui::Event {
        egui::Event::Key {
            key: IMPLICIT_TIME_OVERLAY_KEY,
            physical_key: Some(IMPLICIT_TIME_OVERLAY_KEY),
            pressed,
            repeat: false,
            modifiers,
        }
    }

    #[test]
    fn plain_t_reveals_only_while_held() {
        let context = egui::Context::default();
        let canvas = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(100.0, 100.0));
        let mut revealed = false;

        drop(context.run(
            egui::RawInput {
                events: vec![
                    egui::Event::PointerMoved(egui::pos2(50.0, 50.0)),
                    key_event(true, egui::Modifiers::NONE),
                ],
                ..Default::default()
            },
            |context| revealed = implicit_time_overlay_requested(context, canvas),
        ));
        assert!(revealed);

        drop(context.run(egui::RawInput::default(), |context| {
            revealed = implicit_time_overlay_requested(context, canvas);
        }));
        assert!(
            revealed,
            "the overlay remains visible between press and release"
        );

        drop(context.run(
            egui::RawInput {
                events: vec![key_event(false, egui::Modifiers::NONE)],
                ..Default::default()
            },
            |context| revealed = implicit_time_overlay_requested(context, canvas),
        ));
        assert!(!revealed, "release hides the overlay in the same frame");
    }

    #[test]
    fn text_focus_and_modified_t_do_not_reveal() {
        let context = egui::Context::default();
        let canvas = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(100.0, 100.0));
        let mut text = String::new();

        drop(context.run(egui::RawInput::default(), |context| {
            egui::CentralPanel::default().show(context, |ui| {
                ui.text_edit_singleline(&mut text).request_focus();
            });
        }));

        let mut revealed = true;
        drop(context.run(
            egui::RawInput {
                events: vec![
                    egui::Event::PointerMoved(egui::pos2(50.0, 50.0)),
                    key_event(true, egui::Modifiers::NONE),
                ],
                ..Default::default()
            },
            |context| {
                egui::CentralPanel::default().show(context, |ui| {
                    ui.text_edit_singleline(&mut text);
                    revealed = implicit_time_overlay_requested(ui.ctx(), canvas);
                });
            },
        ));
        assert!(!revealed, "typing T must not activate the graph overlay");

        let modified_context = egui::Context::default();
        drop(modified_context.run(
            egui::RawInput {
                modifiers: egui::Modifiers::SHIFT,
                events: vec![
                    egui::Event::PointerMoved(egui::pos2(50.0, 50.0)),
                    key_event(true, egui::Modifiers::SHIFT),
                ],
                ..Default::default()
            },
            |context| revealed = implicit_time_overlay_requested(context, canvas),
        ));
        assert!(
            !revealed,
            "modified T remains available for other shortcuts"
        );
    }

    #[test]
    fn plain_t_outside_node_canvas_does_not_reveal() {
        let context = egui::Context::default();
        let canvas = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(100.0, 100.0));
        let mut revealed = true;

        drop(context.run(
            egui::RawInput {
                events: vec![
                    egui::Event::PointerMoved(egui::pos2(150.0, 50.0)),
                    key_event(true, egui::Modifiers::NONE),
                ],
                ..Default::default()
            },
            |context| revealed = implicit_time_overlay_requested(context, canvas),
        ));

        assert!(!revealed, "T belongs to the panel under the pointer");
    }
}
