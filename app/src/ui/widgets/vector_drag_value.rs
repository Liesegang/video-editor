use egui::Ui;

use super::property_drag_value::FloatDragValueConfig;

const CONTROL_WIDTH: f32 = 184.0;
const COMPONENT_GAP: f32 = 2.0;
// Two glyphs (for example `px`) stay visible; longer units are truncated and
// available in the hover text so Vec4 keeps useful keyboard-editing width.
const SUFFIX_WIDTH: f32 = 14.0;

pub(crate) struct VectorAxisResponse {
    pub(crate) axis: &'static str,
    pub(crate) response: egui::Response,
    pub(crate) value: f64,
}

pub(crate) struct VectorDragValueResponse {
    pub(crate) response: egui::Response,
    pub(crate) axes: Vec<VectorAxisResponse>,
    pub(crate) changed: bool,
    pub(crate) reset: bool,
    pub(crate) finished: bool,
}

/// Render X/Y/Z/W in one fixed-width row without wrapping.
///
/// The total width is shared by every vector arity, so Vec4 remains compact
/// instead of widening its panel four times more than a scalar control.
pub(crate) fn vector_drag_values(
    ui: &mut Ui,
    config: &FloatDragValueConfig,
    components: &mut [(&'static str, &mut f64)],
    height: f32,
) -> VectorDragValueResponse {
    let mut changed = false;
    let mut reset = false;
    let mut finished = false;
    let mut axes = Vec::with_capacity(components.len());
    let has_suffix = !config.suffix.is_empty();
    let item_count = components.len() + usize::from(has_suffix);
    let components_width = CONTROL_WIDTH - if has_suffix { SUFFIX_WIDTH } else { 0.0 };
    let component_width = (components_width - COMPONENT_GAP * item_count.saturating_sub(1) as f32)
        / components.len().max(1) as f32;

    let group = ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = COMPONENT_GAP;
        for (axis, value) in components {
            let response = ui.add_sized(
                [component_width, height],
                config
                    .widget_without_suffix(value)
                    .prefix(format!("{axis} ")),
            );
            changed |= response.changed();
            reset |= response.middle_clicked();
            finished |= response.drag_stopped()
                || response.lost_focus()
                || (response.has_focus() && ui.input(|input| input.key_pressed(egui::Key::Enter)));
            axes.push(VectorAxisResponse {
                axis,
                response,
                value: **value,
            });
        }
        if has_suffix {
            ui.add_sized(
                [SUFFIX_WIDTH, height],
                egui::Label::new(config.suffix.trim())
                    .selectable(false)
                    .truncate(),
            )
            .on_hover_text(format!("Unit: {}", config.suffix.trim()));
        }
    });

    VectorDragValueResponse {
        response: group.response,
        axes,
        changed,
        reset,
        finished,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vec4_uses_one_fixed_width_ordered_row() {
        let context = egui::Context::default();
        let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(500.0, 120.0));
        let config = FloatDragValueConfig {
            speed: 0.25,
            suffix: " px".to_string(),
            hard_min: None,
            hard_max: None,
        };
        let (mut x, mut y, mut z, mut w) = (1.0, 2.0, 3.0, 4.0);
        let mut rects = Vec::new();
        let mut group_rect = egui::Rect::NOTHING;
        drop(context.run(
            egui::RawInput {
                screen_rect: Some(screen),
                ..Default::default()
            },
            |context| {
                egui::CentralPanel::default().show(context, |ui| {
                    let rendered = vector_drag_values(
                        ui,
                        &config,
                        &mut [("X", &mut x), ("Y", &mut y), ("Z", &mut z), ("W", &mut w)],
                        20.0,
                    );
                    assert_eq!(
                        rendered
                            .axes
                            .iter()
                            .map(|axis| axis.axis)
                            .collect::<Vec<_>>(),
                        ["X", "Y", "Z", "W"]
                    );
                    group_rect = rendered.response.rect;
                    rects = rendered
                        .axes
                        .into_iter()
                        .map(|axis| axis.response.rect)
                        .collect();
                });
            },
        ));

        assert_eq!(rects.len(), 4);
        assert!(rects
            .windows(2)
            .all(|pair| pair[0].right() < pair[1].left()));
        assert!(rects
            .windows(2)
            .all(|pair| (pair[0].center().y - pair[1].center().y).abs() < 0.01));
        let total_width = group_rect.width();
        assert!(
            (CONTROL_WIDTH - 0.1..=CONTROL_WIDTH + 0.1).contains(&total_width),
            "expected {CONTROL_WIDTH}px total control width, got {total_width}px: {rects:?}"
        );
    }
}
