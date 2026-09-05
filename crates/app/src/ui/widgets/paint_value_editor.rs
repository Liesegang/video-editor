//! Typed Gradient and procedural Pattern controls shared by Inspector and Node properties.

use egui::{Id, Popup, PopupCloseBehavior, Response, Ui};
use egui_phosphor::regular as icons;
use library::model::authoring::{Paint, ProjectPalette};
use library::model::property::{
    GradientGeometry, GradientSpread, GradientStop, GradientValue, PatternKind, PatternValue, Vec2,
};
use ordered_float::OrderedFloat;

use super::color_value_picker::color_value_picker;
use super::property_drag_value::numeric_edit_finished;
use super::property_drag_value::FloatDragValueConfig;
use super::vector_drag_value::vector_drag_values;

pub(crate) struct PaintValueEdit {
    pub response: Response,
    pub changed: bool,
    pub finished: bool,
}

struct PaintVectorEdit {
    response: Response,
    finished: bool,
}

pub(crate) fn gradient_value_editor(
    ui: &mut Ui,
    id: Id,
    qa_id: &str,
    value: &mut GradientValue,
    palette: &ProjectPalette,
) -> PaintValueEdit {
    let response = ui.button(format!("Gradient  {} stops", value.stops().len()));
    let mut candidate = value.clone();
    let mut changed = false;
    let mut finished = false;
    Popup::menu(&response)
        .id(id.with("gradient_popup"))
        .width(330.0)
        .close_behavior(PopupCloseBehavior::IgnoreClicks)
        .show(|ui| {
            ui.set_min_width(330.0);
            ui.strong("Gradient");
            let add_current = ui.button(format!("{} Add Current", icons::PLUS));
            crate::qa::register_component_with_metadata(
                format!("{qa_id}.paint.add_current"),
                "paint_palette_add_current",
                add_current.rect,
                true,
                Some(serde_json::json!({ "paint_kind": "gradient" })),
            );
            if add_current.clicked() {
                super::palette_intent::queue(
                    ui.ctx(),
                    super::color_value_picker::PaletteUiIntent::AddPaint {
                        suggested_name: suggested_paint_name(palette, "Gradient"),
                        paint: Paint::Gradient(candidate.clone()),
                    },
                );
            }
            palette_gradient_menu(ui, palette, &mut candidate, &mut changed, &mut finished);
            ui.separator();

            let mut spread = candidate.spread();
            egui::ComboBox::from_id_salt(id.with("spread"))
                .selected_text(match spread {
                    GradientSpread::Pad => "Pad",
                    GradientSpread::Repeat => "Repeat",
                    GradientSpread::Reflect => "Reflect",
                })
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut spread, GradientSpread::Pad, "Pad");
                    ui.selectable_value(&mut spread, GradientSpread::Repeat, "Repeat");
                    ui.selectable_value(&mut spread, GradientSpread::Reflect, "Reflect");
                });

            let mut geometry = candidate.geometry();
            let mut geometry_changed = false;
            ui.horizontal(|ui| {
                ui.label("Type");
                let radial = matches!(geometry, GradientGeometry::Radial { .. });
                let mut selected = if radial { 1 } else { 0 };
                egui::ComboBox::from_id_salt(id.with("geometry_type"))
                    .selected_text(if radial { "Radial" } else { "Linear" })
                    .show_ui(ui, |ui| {
                        ui.selectable_value(&mut selected, 0, "Linear");
                        ui.selectable_value(&mut selected, 1, "Radial");
                    });
                if selected != usize::from(radial) {
                    geometry = if selected == 0 {
                        GradientGeometry::Linear {
                            start: point(0.0, 0.5),
                            end: point(1.0, 0.5),
                        }
                    } else {
                        GradientGeometry::Radial {
                            center: point(0.5, 0.5),
                            radius: OrderedFloat(0.5),
                        }
                    };
                    geometry_changed = true;
                }
            });
            geometry_changed |= geometry_controls(ui, id, &mut geometry, &mut finished);

            let mut stops = candidate.stops().to_vec();
            let mut remove = None;
            let can_remove_stop = stops.len() > 2;
            for (index, stop) in stops.iter_mut().enumerate() {
                ui.push_id(id.with(("stop", index)), |ui| {
                    ui.horizontal(|ui| {
                        let mut offset = stop.offset();
                        let offset_response =
                            ui.add(float_widget(&mut offset, 0.01, Some(0.0), Some(1.0), ""));
                        crate::qa::register_component_with_metadata(
                            format!("{qa_id}.gradient.stop.{index}.offset"),
                            "gradient_stop_offset",
                            offset_response.rect,
                            true,
                            Some(serde_json::json!({ "offset": offset })),
                        );
                        if offset_response.changed() {
                            if let Ok(updated) = GradientStop::new(offset, stop.color().clone()) {
                                *stop = updated;
                                changed = true;
                            }
                        }
                        finished |= numeric_edit_finished(&offset_response);
                        let picker = color_value_picker(
                            ui,
                            id.with(("stop_color", index)),
                            stop.color(),
                            Some(palette),
                        );
                        if let Some(color) = picker.value {
                            if let Ok(updated) = GradientStop::new(stop.offset(), color) {
                                *stop = updated;
                                changed = true;
                            }
                        }
                        finished |= picker.finished;
                        if ui.small_button(icons::TRASH).clicked() && can_remove_stop {
                            remove = Some(index);
                            changed = true;
                            finished = true;
                        }
                    });
                });
            }
            if let Some(index) = remove {
                stops.remove(index);
            }
            if ui.button(format!("{} Add stop", icons::PLUS)).clicked() {
                let index = stops.len() / 2;
                let color = stops[index].color().clone();
                if let Ok(stop) = GradientStop::new(0.5, color) {
                    stops.push(stop);
                    changed = true;
                    finished = true;
                }
            }
            stops.sort_by_key(|stop| OrderedFloat(stop.offset()));
            if spread != candidate.spread() || geometry_changed {
                changed = true;
                finished = true;
            }
            if changed {
                if let Ok(updated) = GradientValue::new(geometry, spread, stops) {
                    candidate = updated;
                }
            }
        });
    if changed {
        *value = candidate;
    }
    PaintValueEdit {
        response,
        changed,
        finished,
    }
}

pub(crate) fn pattern_value_editor(
    ui: &mut Ui,
    id: Id,
    qa_id: &str,
    value: &mut PatternValue,
    palette: &ProjectPalette,
) -> PaintValueEdit {
    let response = ui.button(format!("{:?} Pattern", value.kind()));
    let mut candidate = value.clone();
    let mut changed = false;
    let mut finished = false;
    Popup::menu(&response)
        .id(id.with("pattern_popup"))
        .width(330.0)
        .close_behavior(PopupCloseBehavior::IgnoreClicks)
        .show(|ui| {
            ui.set_min_width(330.0);
            ui.strong("Pattern");
            let add_current = ui.button(format!("{} Add Current", icons::PLUS));
            crate::qa::register_component_with_metadata(
                format!("{qa_id}.paint.add_current"),
                "paint_palette_add_current",
                add_current.rect,
                true,
                Some(serde_json::json!({ "paint_kind": "pattern" })),
            );
            if add_current.clicked() {
                super::palette_intent::queue(
                    ui.ctx(),
                    super::color_value_picker::PaletteUiIntent::AddPaint {
                        suggested_name: suggested_paint_name(palette, "Pattern"),
                        paint: Paint::Pattern(candidate.clone()),
                    },
                );
            }
            palette_pattern_menu(ui, palette, &mut candidate, &mut changed, &mut finished);
            ui.separator();
            let mut kind = candidate.kind();
            egui::ComboBox::from_id_salt(id.with("kind"))
                .selected_text(format!("{kind:?}"))
                .show_ui(ui, |ui| {
                    for option in [
                        PatternKind::Checker,
                        PatternKind::Stripes,
                        PatternKind::Dots,
                        PatternKind::Grid,
                    ] {
                        ui.selectable_value(&mut kind, option, format!("{option:?}"));
                    }
                });
            let mut foreground = candidate.foreground().clone();
            let mut background = candidate.background().clone();
            let foreground_edit =
                color_value_picker(ui, id.with("foreground"), &foreground, Some(palette));
            if let Some(color) = foreground_edit.value {
                foreground = color;
                changed = true;
            }
            let background_edit =
                color_value_picker(ui, id.with("background"), &background, Some(palette));
            if let Some(color) = background_edit.value {
                background = color;
                changed = true;
            }
            finished |= foreground_edit.finished || background_edit.finished;

            let mut scale = candidate.scale();
            let mut phase = candidate.phase();
            let mut angle = candidate.angle();
            let mut duty = candidate.duty();
            let controls = [
                vector_controls(ui, "Scale", &mut scale, 1.0, Some(0.001)),
                vector_controls(ui, "Phase", &mut phase, 0.1, None),
            ];
            let angle_response = ui.add(float_widget(&mut angle, 1.0, None, None, "°"));
            let duty_response = ui.add(float_widget(&mut duty, 0.01, Some(0.0), Some(1.0), ""));
            crate::qa::register_component_with_metadata(
                format!("{qa_id}.pattern.angle"),
                "pattern_angle",
                angle_response.rect,
                true,
                Some(serde_json::json!({ "angle": angle })),
            );
            crate::qa::register_component_with_metadata(
                format!("{qa_id}.pattern.duty"),
                "pattern_duty",
                duty_response.rect,
                true,
                Some(serde_json::json!({ "duty": duty })),
            );
            let geometry_changed = controls.iter().any(|edit| edit.response.changed())
                || angle_response.changed()
                || duty_response.changed();
            finished |= controls.iter().any(|edit| edit.finished)
                || numeric_edit_finished(&angle_response)
                || numeric_edit_finished(&duty_response);
            if kind != candidate.kind() {
                changed = true;
                finished = true;
            }
            changed |= geometry_changed;
            if changed {
                if let Ok(updated) =
                    PatternValue::new(kind, foreground, background, scale, phase, angle, duty)
                {
                    candidate = updated;
                }
            }
        });
    if changed {
        *value = candidate;
    }
    PaintValueEdit {
        response,
        changed,
        finished,
    }
}

fn geometry_controls(
    ui: &mut Ui,
    id: Id,
    geometry: &mut GradientGeometry,
    finished: &mut bool,
) -> bool {
    ui.push_id(id.with("geometry"), |ui| match geometry {
        GradientGeometry::Linear { start, end } => {
            let responses = [
                vector_controls(ui, "Start", start, 0.01, None),
                vector_controls(ui, "End", end, 0.01, None),
            ];
            *finished |= responses.iter().any(|edit| edit.finished);
            responses.iter().any(|edit| edit.response.changed())
        }
        GradientGeometry::Radial { center, radius } => {
            let center_response = vector_controls(ui, "Center", center, 0.01, None);
            let mut raw_radius = radius.into_inner();
            let radius_response = ui.add(float_widget(
                &mut raw_radius,
                0.01,
                Some(0.001),
                Some(10.0),
                "",
            ));
            if radius_response.changed() {
                *radius = OrderedFloat(raw_radius);
            }
            *finished |= center_response.finished || numeric_edit_finished(&radius_response);
            center_response.response.changed() || radius_response.changed()
        }
    })
    .inner
}

fn vector_controls(
    ui: &mut Ui,
    label: &str,
    value: &mut Vec2,
    speed: f64,
    hard_min: Option<f64>,
) -> PaintVectorEdit {
    ui.vertical(|ui| {
        ui.label(label);
        let mut x = value.x.into_inner();
        let mut y = value.y.into_inner();
        let config = FloatDragValueConfig {
            speed,
            suffix: String::new(),
            hard_min,
            hard_max: None,
        };
        let response = vector_drag_values(
            ui,
            &config,
            &mut [("X", &mut x), ("Y", &mut y)],
            ui.spacing().interact_size.y,
        );
        if response.changed {
            *value = point(x, y);
        }
        PaintVectorEdit {
            response: response.response,
            finished: response.finished,
        }
    })
    .inner
}

fn float_widget<'a>(
    value: &'a mut f64,
    speed: f64,
    hard_min: Option<f64>,
    hard_max: Option<f64>,
    suffix: &str,
) -> egui::DragValue<'a> {
    FloatDragValueConfig {
        speed,
        suffix: suffix.to_string(),
        hard_min,
        hard_max,
    }
    .widget(value)
}

fn palette_gradient_menu(
    ui: &mut Ui,
    palette: &ProjectPalette,
    candidate: &mut GradientValue,
    changed: &mut bool,
    finished: &mut bool,
) {
    ui.menu_button("Project Palette", |ui| {
        for definition in palette.ungrouped_definitions() {
            let Paint::Gradient(gradient) = &definition.paint else {
                continue;
            };
            if ui.button(&definition.name).clicked() {
                *candidate = gradient.clone();
                *changed = true;
                *finished = true;
                ui.close();
            }
        }
    });
}

fn palette_pattern_menu(
    ui: &mut Ui,
    palette: &ProjectPalette,
    candidate: &mut PatternValue,
    changed: &mut bool,
    finished: &mut bool,
) {
    ui.menu_button("Project Palette", |ui| {
        for definition in palette.ungrouped_definitions() {
            let Paint::Pattern(pattern) = &definition.paint else {
                continue;
            };
            if ui.button(&definition.name).clicked() {
                *candidate = pattern.clone();
                *changed = true;
                *finished = true;
                ui.close();
            }
        }
    });
}

fn point(x: f64, y: f64) -> Vec2 {
    Vec2 {
        x: OrderedFloat(x),
        y: OrderedFloat(y),
    }
}

fn suggested_paint_name(palette: &ProjectPalette, base: &str) -> String {
    let mut suffix = 1;
    loop {
        let candidate = format!("{base} {suffix}");
        if palette
            .definitions
            .values()
            .all(|definition| definition.name != candidate)
        {
            return candidate;
        }
        suffix += 1;
    }
}
