//! Lossless Project-color editing through the shared display transform.
//!
//! This widget owns presentation and gestures only. Color-space conversion is
//! delegated to `library::color_management`, which is also the boundary used
//! by render/runtime consumers. Merely painting or opening this widget never
//! writes the f32 display draft back to the authoritative f64 [`ColorValue`].

use egui::ecolor::{hsv_from_rgb, rgb_from_hsv, HsvaGamma};
use egui::{Color32, Id, Mesh, Popup, PopupCloseBehavior, Rect, Response, Sense, StrokeKind, Ui};
use library::model::authoring::{Paint, PaintDefinitionId, ProjectPalette};
use library::model::property::ColorValue;

mod palette;

use palette::PaletteGeometry;

const BUTTON_SIZE: egui::Vec2 = egui::vec2(96.0, 20.0);
const POPUP_WIDTH: f32 = 360.0;
const SATURATION_VALUE_SIZE: egui::Vec2 = egui::vec2(340.0, 250.0);
const SLIDER_SIZE: egui::Vec2 = egui::vec2(340.0, 24.0);
const GRADIENT_STEPS: u32 = 24;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum PickerTab {
    #[default]
    Picker,
    Palette,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum PaletteUiIntent {
    AddSolid {
        suggested_name: String,
        color: ColorValue,
    },
    AddPaint {
        suggested_name: String,
        paint: Paint,
    },
    Rename {
        id: PaintDefinitionId,
        name: String,
    },
    Reorder {
        id: PaintDefinitionId,
        new_index: usize,
    },
    Delete {
        id: PaintDefinitionId,
    },
}

#[derive(Clone)]
struct PickerDraft {
    source: ColorValue,
    hsva: HsvaGamma,
    display_clipped: bool,
    dirty: bool,
}

impl PickerDraft {
    fn from_source(source: &ColorValue, display_rgba: [f64; 4]) -> Self {
        let display_clipped = display_rgba
            .iter()
            .any(|value| !(0.0..=1.0).contains(value));
        let display = display_rgba.map(|value| value.clamp(0.0, 1.0) as f32);
        let (h, s, v) = hsv_from_rgb([display[0], display[1], display[2]]);
        Self {
            source: source.clone(),
            hsva: HsvaGamma {
                h,
                s,
                v,
                a: display[3],
            },
            display_clipped,
            dirty: false,
        }
    }

    fn display_rgba(&self) -> [f64; 4] {
        let [r, g, b] = rgb_from_hsv((self.hsva.h, self.hsva.s, self.hsva.v));
        [
            f64::from(r),
            f64::from(g),
            f64::from(b),
            f64::from(self.hsva.a),
        ]
    }
}

#[derive(Clone, Copy, Debug)]
#[allow(
    dead_code,
    reason = "geometry is exposed to the native-input QA tests without affecting picker behavior"
)]
pub(crate) struct ColorPickerGeometry {
    pub popup: Rect,
    pub authored_space: Rect,
    pub saturation_value: Rect,
    pub hue: Rect,
    pub alpha: Rect,
}

#[derive(Debug)]
#[allow(
    dead_code,
    reason = "diagnostics and gesture completion are consumed by the native-input QA tests"
)]
pub(crate) struct ColorPickerEdit {
    pub response: Response,
    pub value: Option<ColorValue>,
    pub palette_intent: Option<PaletteUiIntent>,
    pub finished: bool,
    pub supported: bool,
    pub display_clipped: bool,
    pub error: Option<String>,
    pub geometry: Option<ColorPickerGeometry>,
    pub palette_tab_rect: Option<Rect>,
    pub palette_geometry: Option<PaletteGeometry>,
}

/// Shows a compact swatch backed by a deliberately large display-space popup.
///
/// The shared color-management adapter is the only conversion authority. Its
/// display result is copied into a temporary f32 HSV draft for the gesture.
/// That draft becomes a Project value only after a real picker edit.
pub(crate) fn color_value_picker(
    ui: &mut Ui,
    id: Id,
    value: &ColorValue,
    palette: Option<&ProjectPalette>,
) -> ColorPickerEdit {
    let display = library::color_management::to_display_srgb(value);
    let mut conversion_error = display.as_ref().err().map(ToString::to_string);
    let mut draft = display.as_ref().ok().map(|display| {
        ui.data(|data| data.get_temp::<PickerDraft>(id))
            .filter(|draft| draft.source == *value)
            .unwrap_or_else(|| PickerDraft::from_source(value, *display))
    });
    let supported = draft.is_some();
    let display_clipped = draft.as_ref().is_some_and(|draft| draft.display_clipped);
    let popup_id = id.with("popup");

    let mut response = ui.add_enabled(supported, egui::Button::new("").min_size(BUTTON_SIZE));
    if let Some(draft) = draft.as_ref() {
        egui::color_picker::show_color_at(ui.painter(), Color32::from(draft.hsva), response.rect);
        ui.painter().rect_stroke(
            response.rect,
            2.0,
            ui.style().interact(&response).bg_stroke,
            StrokeKind::Inside,
        );
        let clipping_note = if draft.display_clipped {
            " The display swatch is clipped to the view gamut; opening it does not alter the authored HDR/negative channels."
        } else {
            ""
        };
        if !Popup::is_id_open(ui.ctx(), popup_id) {
            response = response.on_hover_text(format!(
                "Edit through the sRGB display view; store the result back in {}.{}",
                value.color_space(),
                clipping_note
            ));
        }
    } else {
        response = response.on_disabled_hover_text(format!(
            "No display transform is available for '{}'. Edit the lossless f64 channels directly.{}",
            value.color_space(),
            conversion_error
                .as_deref()
                .map(|error| format!(" ({error})"))
                .unwrap_or_default(),
        ));
    }

    let was_open = Popup::is_id_open(ui.ctx(), popup_id);
    let mut edited = false;
    let mut sub_geometry = None;
    let mut space_candidate = None;
    let tab_id = id.with("tab");
    let mut selected_tab = ui
        .data(|data| data.get_temp::<PickerTab>(tab_id))
        .unwrap_or_default();
    if palette.is_none() {
        selected_tab = PickerTab::Picker;
    }
    let mut palette_value = None;
    let mut palette_intent = None;
    let mut palette_geometry = None;
    let mut palette_context_owns_click = false;
    let mut palette_tab_rect = None;
    let popup = draft.as_mut().and_then(|draft| {
        Popup::menu(&response)
            .id(popup_id)
            .width(POPUP_WIDTH)
            .close_behavior(PopupCloseBehavior::IgnoreClicks)
            .show(|ui| {
                ui.set_min_width(POPUP_WIDTH);
                ui.horizontal(|ui| {
                    let picker_tab = ui.add(
                        egui::Button::new("Picker").selected(selected_tab == PickerTab::Picker),
                    );
                    if picker_tab.clicked() {
                        selected_tab = PickerTab::Picker;
                    }
                    crate::qa::register_component_with_metadata(
                        "color_picker.tab.picker",
                        "color_picker_tab",
                        picker_tab.rect,
                        true,
                        Some(serde_json::json!({ "tab": "picker", "selected": selected_tab == PickerTab::Picker })),
                    );
                    if palette.is_some() {
                        let palette_tab = ui.add(
                            egui::Button::new("Palette")
                                .selected(selected_tab == PickerTab::Palette),
                        );
                        if palette_tab.clicked() {
                            selected_tab = PickerTab::Palette;
                        }
                        crate::qa::register_component_with_metadata(
                            "color_picker.tab.palette",
                            "color_picker_tab",
                            palette_tab.rect,
                            true,
                            Some(serde_json::json!({ "tab": "palette", "selected": selected_tab == PickerTab::Palette })),
                        );
                        palette_tab_rect = Some(palette_tab.rect);
                    }
                });
                ui.separator();
                if selected_tab == PickerTab::Palette {
                    if let Some(palette) = palette {
                        let result = palette::show_palette(ui, id, value, palette);
                        palette_value = result.value;
                        palette_intent = result.intent;
                        palette_context_owns_click = result.context_owns_click;
                        palette_geometry = Some(result.geometry);
                        return;
                    }
                }
                ui.label(egui::RichText::new("Display color").strong());
                ui.label(
                    egui::RichText::new(format!(
                        "sRGB view → authored {} · straight alpha",
                        value.color_space()
                    ))
                    .small()
                    .weak(),
                );
                if draft.display_clipped {
                    ui.colored_label(
                        ui.visuals().warn_fg_color,
                        "The authored value is outside the display gamut. It stays exact until you edit the picker.",
                    );
                }

                let mut authored_space_rect = Rect::NOTHING;
                match library::color_management::available_color_spaces() {
                    Ok(spaces) => {
                        let mut selected_space = value.color_space().as_str().to_string();
                        let before = selected_space.clone();
                        let selected_label = spaces
                            .iter()
                            .find(|space| space.id == selected_space)
                            .map_or(selected_space.as_str(), |space| space.label.as_str())
                            .to_owned();
                        authored_space_rect = ui
                            .push_id(id.with("authored_space"), |ui| {
                                ui.horizontal(|ui| {
                                    ui.label("Authored space");
                                    ui.menu_button(selected_label, |ui| {
                                        for space in &spaces {
                                            ui.selectable_value(
                                                &mut selected_space,
                                                space.id.clone(),
                                                &space.label,
                                            );
                                        }
                                    })
                                    .response
                                })
                                .inner
                            })
                            .inner
                            .rect;
                        if selected_space != before {
                            match library::model::property::ColorSpaceRef::new(selected_space)
                                .map_err(library::color_management::ColorTransformError::from)
                                .and_then(|target| {
                                    library::color_management::transform_color(value, &target)
                                })
                                .and_then(|color| {
                                    library::color_management::to_display_srgb(&color)
                                        .map(|display| (color, display))
                                }) {
                                Ok((color, display)) => {
                                    *draft = PickerDraft::from_source(&color, display);
                                    draft.dirty = true;
                                    space_candidate = Some(color);
                                }
                                Err(error) => conversion_error = Some(error.to_string()),
                            }
                        }
                    }
                    Err(error) => {
                        ui.colored_label(
                            ui.visuals().error_fg_color,
                            format!("Color-space catalog unavailable: {error}"),
                        );
                    }
                }

                let numeric_changed = display_numeric_controls(ui, &mut draft.hsva);
                let saturation_value = saturation_value_control(ui, &mut draft.hsva);
                let hue = hue_control(ui, &mut draft.hsva);
                let alpha = alpha_control(ui, &mut draft.hsva);
                for (id, component_type, response) in [
                    (
                        "color_picker.saturation_value",
                        "color_picker_saturation_value",
                        &saturation_value,
                    ),
                    ("color_picker.hue", "color_picker_hue", &hue),
                    ("color_picker.alpha", "color_picker_alpha", &alpha),
                ] {
                    crate::qa::register_component_with_metadata(
                        id,
                        component_type,
                        response.rect,
                        response.enabled(),
                        Some(serde_json::json!({
                            "display_hue": draft.hsva.h,
                            "display_saturation": draft.hsva.s,
                            "display_value": draft.hsva.v,
                            "straight_alpha": draft.hsva.a,
                            "authored_color_space": value.color_space(),
                        })),
                    );
                }
                edited |= numeric_changed
                    || saturation_value.changed()
                    || hue.changed()
                    || alpha.changed();
                sub_geometry = Some((
                    authored_space_rect,
                    saturation_value.rect,
                    hue.rect,
                    alpha.rect,
                ));
            })
    });
    if popup
        .as_ref()
        .is_some_and(|popup| popup.response.clicked_elsewhere())
        && !response.clicked()
        && !palette_context_owns_click
    {
        Popup::close_id(ui.ctx(), popup_id);
    }
    ui.data_mut(|data| data.insert_temp(tab_id, selected_tab));
    let is_open = Popup::is_id_open(ui.ctx(), popup_id);
    let closed = was_open && !is_open;
    if !is_open || selected_tab != PickerTab::Palette {
        palette::close_context(ui.ctx(), id);
    }

    let palette_applied = palette_value.is_some();
    let value = if palette_applied {
        palette_value
    } else if edited {
        draft
            .as_mut()
            .and_then(|draft| match value_from_display_draft(draft) {
                Ok(value) => {
                    draft.source = value.clone();
                    draft.display_clipped = false;
                    draft.dirty = true;
                    Some(value)
                }
                Err(error) => {
                    conversion_error = Some(error.to_string());
                    None
                }
            })
    } else {
        space_candidate
    };
    if value.is_some() {
        response.mark_changed();
    }
    let finished = palette_applied || closed && draft.as_ref().is_some_and(|draft| draft.dirty);
    if finished {
        if let Some(draft) = draft.as_mut() {
            draft.dirty = false;
        }
    }
    if let Some(draft) = draft {
        ui.data_mut(|data| data.insert_temp(id, draft));
    }

    let geometry = popup.and_then(|popup| {
        sub_geometry.map(
            |(authored_space, saturation_value, hue, alpha)| ColorPickerGeometry {
                popup: popup.response.rect,
                authored_space,
                saturation_value,
                hue,
                alpha,
            },
        )
    });
    if let Some(error) = conversion_error.as_deref() {
        ui.colored_label(
            ui.visuals().error_fg_color,
            egui::RichText::new("Color transform unavailable").small(),
        )
        .on_hover_text(error);
    }
    ColorPickerEdit {
        response,
        value,
        palette_intent,
        finished,
        supported,
        display_clipped,
        error: conversion_error,
        geometry,
        palette_tab_rect,
        palette_geometry,
    }
}

fn value_from_display_draft(
    draft: &PickerDraft,
) -> Result<ColorValue, library::color_management::ColorTransformError> {
    library::color_management::from_display_srgb(draft.display_rgba(), draft.source.color_space())
}

fn display_numeric_controls(ui: &mut Ui, hsva: &mut HsvaGamma) -> bool {
    let [mut red, mut green, mut blue] = rgb_from_hsv((hsva.h, hsva.s, hsva.v));
    let mut changed = false;
    ui.horizontal(|ui| {
        for (label, channel) in ["R", "G", "B"]
            .into_iter()
            .zip([&mut red, &mut green, &mut blue])
        {
            changed |= ui
                .add(
                    egui::DragValue::new(channel)
                        .prefix(format!("{label} "))
                        .range(0.0..=1.0)
                        .speed(0.005)
                        .max_decimals(4),
                )
                .changed();
        }
        changed |= ui
            .add(
                egui::DragValue::new(&mut hsva.a)
                    .prefix("A ")
                    .range(0.0..=1.0)
                    .speed(0.005)
                    .max_decimals(4),
            )
            .changed();
    });
    if changed {
        let (h, s, v) = hsv_from_rgb([red, green, blue]);
        hsva.h = h;
        hsva.s = s;
        hsva.v = v;
    }
    changed
}

fn saturation_value_control(ui: &mut Ui, hsva: &mut HsvaGamma) -> Response {
    let (rect, mut response) =
        ui.allocate_exact_size(SATURATION_VALUE_SIZE, Sense::click_and_drag());
    if let Some(pointer) = response.interact_pointer_pos() {
        let saturation = egui::remap_clamp(pointer.x, rect.left()..=rect.right(), 0.0..=1.0);
        let value = egui::remap_clamp(pointer.y, rect.bottom()..=rect.top(), 0.0..=1.0);
        if saturation != hsva.s || value != hsva.v {
            hsva.s = saturation;
            hsva.v = value;
            response.mark_changed();
        }
    }
    if ui.is_rect_visible(rect) {
        let mut mesh = Mesh::default();
        for x in 0..=GRADIENT_STEPS {
            for y in 0..=GRADIENT_STEPS {
                let saturation = x as f32 / GRADIENT_STEPS as f32;
                let value = 1.0 - y as f32 / GRADIENT_STEPS as f32;
                let position = egui::pos2(
                    egui::lerp(rect.left()..=rect.right(), saturation),
                    egui::lerp(rect.top()..=rect.bottom(), 1.0 - value),
                );
                mesh.colored_vertex(
                    position,
                    HsvaGamma {
                        h: hsva.h,
                        s: saturation,
                        v: value,
                        a: 1.0,
                    }
                    .into(),
                );
                if x < GRADIENT_STEPS && y < GRADIENT_STEPS {
                    let row = GRADIENT_STEPS + 1;
                    let top_left = y * row + x;
                    mesh.add_triangle(top_left, top_left + 1, top_left + row);
                    mesh.add_triangle(top_left + 1, top_left + row, top_left + row + 1);
                }
            }
        }
        ui.painter().add(egui::Shape::mesh(mesh));
        ui.painter().rect_stroke(
            rect,
            0.0,
            ui.style().interact(&response).bg_stroke,
            StrokeKind::Inside,
        );
        let marker = egui::pos2(
            egui::lerp(rect.left()..=rect.right(), hsva.s),
            egui::lerp(rect.bottom()..=rect.top(), hsva.v),
        );
        ui.painter()
            .circle_stroke(marker, 7.0, (2.0, Color32::WHITE));
        ui.painter()
            .circle_stroke(marker, 9.0, (1.0, Color32::BLACK));
    }
    response
}

fn hue_control(ui: &mut Ui, hsva: &mut HsvaGamma) -> Response {
    gradient_slider(ui, &mut hsva.h, |hue| {
        HsvaGamma {
            h: hue,
            s: 1.0,
            v: 1.0,
            a: 1.0,
        }
        .into()
    })
    .on_hover_text("Display hue")
}

fn alpha_control(ui: &mut Ui, hsva: &mut HsvaGamma) -> Response {
    let opaque = HsvaGamma { a: 1.0, ..*hsva };
    gradient_slider(ui, &mut hsva.a, |alpha| {
        HsvaGamma { a: alpha, ..opaque }.into()
    })
    .on_hover_text("Straight alpha")
}

fn gradient_slider(ui: &mut Ui, value: &mut f32, color_at: impl Fn(f32) -> Color32) -> Response {
    let (rect, mut response) = ui.allocate_exact_size(SLIDER_SIZE, Sense::click_and_drag());
    if let Some(pointer) = response.interact_pointer_pos() {
        let candidate = egui::remap_clamp(pointer.x, rect.left()..=rect.right(), 0.0..=1.0);
        if candidate != *value {
            *value = candidate;
            response.mark_changed();
        }
    }
    if ui.is_rect_visible(rect) {
        let mut mesh = Mesh::default();
        for index in 0..=GRADIENT_STEPS {
            let amount = index as f32 / GRADIENT_STEPS as f32;
            let x = egui::lerp(rect.left()..=rect.right(), amount);
            mesh.colored_vertex(egui::pos2(x, rect.top()), color_at(amount));
            mesh.colored_vertex(egui::pos2(x, rect.bottom()), color_at(amount));
            if index < GRADIENT_STEPS {
                let vertex = index * 2;
                mesh.add_triangle(vertex, vertex + 1, vertex + 2);
                mesh.add_triangle(vertex + 1, vertex + 2, vertex + 3);
            }
        }
        ui.painter().add(egui::Shape::mesh(mesh));
        ui.painter().rect_stroke(
            rect,
            0.0,
            ui.style().interact(&response).bg_stroke,
            StrokeKind::Inside,
        );
        let x = egui::lerp(rect.left()..=rect.right(), *value);
        ui.painter().line_segment(
            [egui::pos2(x, rect.top()), egui::pos2(x, rect.bottom())],
            (2.0, Color32::WHITE),
        );
    }
    response
}

#[cfg(test)]
#[path = "color_value_picker/tests.rs"]
mod tests;
