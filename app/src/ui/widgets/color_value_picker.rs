//! Lossless Project-color editing through the shared display transform.
//!
//! This widget owns presentation and gestures only. Color-space conversion is
//! delegated to `library::color_management`, which is also the boundary used
//! by render/runtime consumers. Merely painting or opening this widget never
//! writes the f32 display draft back to the authoritative f64 [`ColorValue`].

use egui::ecolor::{hsv_from_rgb, rgb_from_hsv, HsvaGamma};
use egui::{Color32, Id, Mesh, Popup, PopupCloseBehavior, Rect, Response, Sense, StrokeKind, Ui};
use library::model::property::ColorValue;

const BUTTON_SIZE: egui::Vec2 = egui::vec2(96.0, 20.0);
const POPUP_WIDTH: f32 = 360.0;
const SATURATION_VALUE_SIZE: egui::Vec2 = egui::vec2(340.0, 250.0);
const SLIDER_SIZE: egui::Vec2 = egui::vec2(340.0, 24.0);
const GRADIENT_STEPS: u32 = 24;

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
pub(crate) struct ColorPickerGeometry {
    pub popup: Rect,
    pub saturation_value: Rect,
    pub hue: Rect,
    pub alpha: Rect,
}

#[derive(Debug)]
pub(crate) struct ColorPickerEdit {
    pub response: Response,
    pub value: Option<ColorValue>,
    pub finished: bool,
    pub supported: bool,
    pub display_clipped: bool,
    pub error: Option<String>,
    pub geometry: Option<ColorPickerGeometry>,
}

/// Shows a compact swatch backed by a deliberately large display-space popup.
///
/// The shared color-management adapter is the only conversion authority. Its
/// display result is copied into a temporary f32 HSV draft for the gesture.
/// That draft becomes a Project value only after a real picker edit.
pub(crate) fn color_value_picker(ui: &mut Ui, id: Id, value: &ColorValue) -> ColorPickerEdit {
    let display = library::color_management::to_display_srgb(value);
    let mut conversion_error = display.as_ref().err().map(ToString::to_string);
    let mut draft = display.as_ref().ok().map(|display| {
        ui.data(|data| data.get_temp::<PickerDraft>(id))
            .filter(|draft| draft.source == *value)
            .unwrap_or_else(|| PickerDraft::from_source(value, *display))
    });
    let supported = draft.is_some();
    let display_clipped = draft.as_ref().is_some_and(|draft| draft.display_clipped);

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
        response = response.on_hover_text(format!(
            "Edit through the sRGB display view; store the result back in {}.{}",
            value.color_space(),
            clipping_note
        ));
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

    let popup_id = id.with("popup");
    let was_open = Popup::is_id_open(ui.ctx(), popup_id);
    let mut edited = false;
    let mut sub_geometry = None;
    let mut space_candidate = None;
    let popup = draft.as_mut().and_then(|draft| {
        Popup::menu(&response)
            .id(popup_id)
            .width(POPUP_WIDTH)
            .close_behavior(PopupCloseBehavior::CloseOnClickOutside)
            .show(|ui| {
                ui.set_min_width(POPUP_WIDTH);
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

                match library::color_management::available_color_spaces() {
                    Ok(spaces) => {
                        let mut selected_space = value.color_space().as_str().to_string();
                        let before = selected_space.clone();
                        egui::ComboBox::from_id_salt(id.with("authored_space"))
                            .selected_text(
                                spaces
                                    .iter()
                                    .find(|space| space.id == selected_space)
                                    .map_or(selected_space.as_str(), |space| space.label.as_str()),
                            )
                            .show_ui(ui, |ui| {
                                for space in &spaces {
                                    ui.selectable_value(
                                        &mut selected_space,
                                        space.id.clone(),
                                        &space.label,
                                    );
                                }
                            });
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
                edited |= numeric_changed
                    || saturation_value.changed()
                    || hue.changed()
                    || alpha.changed();
                sub_geometry = Some((saturation_value.rect, hue.rect, alpha.rect));
            })
    });
    let is_open = Popup::is_id_open(ui.ctx(), popup_id);
    let closed = was_open && !is_open;

    let value = if edited {
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
    let finished = closed && draft.as_ref().is_some_and(|draft| draft.dirty);
    if finished {
        if let Some(draft) = draft.as_mut() {
            draft.dirty = false;
        }
    }
    if let Some(draft) = draft {
        ui.data_mut(|data| data.insert_temp(id, draft));
    }

    let geometry = popup.and_then(|popup| {
        sub_geometry.map(|(saturation_value, hue, alpha)| ColorPickerGeometry {
            popup: popup.response.rect,
            saturation_value,
            hue,
            alpha,
        })
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
        finished,
        supported,
        display_clipped,
        error: conversion_error,
        geometry,
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
mod tests {
    use super::*;
    use library::model::property::ColorSpaceRef;
    use std::io;

    #[derive(Default)]
    struct Snapshot {
        button: Option<Rect>,
        geometry: Option<ColorPickerGeometry>,
        values: Vec<ColorValue>,
        finished: bool,
        supported: bool,
    }

    fn pointer_button(position: egui::Pos2, pressed: bool) -> egui::Event {
        egui::Event::PointerButton {
            pos: position,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        }
    }

    fn render(
        context: &egui::Context,
        source: &ColorValue,
        events: Vec<egui::Event>,
        frame: usize,
        snapshot: &mut Snapshot,
    ) {
        let screen = Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1000.0, 800.0));
        drop(context.run(
            egui::RawInput {
                screen_rect: Some(screen),
                time: Some(frame as f64 / 60.0),
                events,
                ..Default::default()
            },
            |context| {
                egui::CentralPanel::default().show(context, |ui| {
                    let edit = color_value_picker(ui, Id::new("color-picker-test"), source);
                    snapshot.button = Some(edit.response.rect);
                    snapshot.geometry = edit.geometry;
                    snapshot.values.extend(edit.value);
                    snapshot.finished |= edit.finished;
                    snapshot.supported = edit.supported;
                });
            },
        ));
    }

    fn open_picker(
        context: &egui::Context,
        source: &ColorValue,
        snapshot: &mut Snapshot,
        frame: &mut usize,
    ) -> Result<ColorPickerGeometry, io::Error> {
        render(context, source, Vec::new(), *frame, snapshot);
        *frame += 1;
        let button = snapshot
            .button
            .ok_or_else(|| io::Error::other("picker button missing"))?;
        let position = button.center();
        render(
            context,
            source,
            vec![
                egui::Event::PointerMoved(position),
                pointer_button(position, true),
            ],
            *frame,
            snapshot,
        );
        *frame += 1;
        render(
            context,
            source,
            vec![pointer_button(position, false)],
            *frame,
            snapshot,
        );
        *frame += 1;
        render(context, source, Vec::new(), *frame, snapshot);
        *frame += 1;
        snapshot
            .geometry
            .ok_or_else(|| io::Error::other("large picker popup did not open"))
    }

    #[test]
    fn opening_and_closing_hdr_picker_never_rewrites_the_f64_source(
    ) -> Result<(), Box<dyn std::error::Error>> {
        let context = egui::Context::default();
        context.memory_mut(|memory| memory.set_everything_is_visible(true));
        let source = ColorValue::new(ColorSpaceRef::srgb(), [-0.125, 4.25, 0.333333333333, 0.5])?;
        let original = source.clone();
        let mut snapshot = Snapshot::default();
        let mut frame = 0;
        let geometry = open_picker(&context, &source, &mut snapshot, &mut frame)?;
        assert!(geometry.saturation_value.width() >= 340.0);
        assert!(geometry.saturation_value.height() >= 250.0);
        assert!(snapshot.values.is_empty());

        let outside = egui::pos2(900.0, 740.0);
        render(
            &context,
            &source,
            vec![
                egui::Event::PointerMoved(outside),
                pointer_button(outside, true),
            ],
            frame,
            &mut snapshot,
        );
        frame += 1;
        render(
            &context,
            &source,
            vec![pointer_button(outside, false)],
            frame,
            &mut snapshot,
        );
        assert_eq!(source, original);
        assert!(snapshot.values.is_empty());
        assert!(!snapshot.finished, "a no-op popup must not create history");
        Ok(())
    }

    #[test]
    fn real_palette_gesture_roundtrips_through_shared_linear_srgb_transform(
    ) -> Result<(), Box<dyn std::error::Error>> {
        let context = egui::Context::default();
        context.memory_mut(|memory| memory.set_everything_is_visible(true));
        let source = ColorValue::new(
            ColorSpaceRef::linear_srgb(),
            [0.21404114048223255, 0.033104766570885055, 0.0, 0.75],
        )?;
        assert_eq!(
            library::color_management::to_display_srgb(&source)?,
            [0.5, 0.2, 0.0, 0.75]
        );
        let mut snapshot = Snapshot::default();
        let mut frame = 0;
        let geometry = open_picker(&context, &source, &mut snapshot, &mut frame)?;
        let position = egui::pos2(
            geometry.saturation_value.left() + geometry.saturation_value.width() * 0.82,
            geometry.saturation_value.top() + geometry.saturation_value.height() * 0.18,
        );
        render(
            &context,
            &source,
            vec![
                egui::Event::PointerMoved(position),
                pointer_button(position, true),
            ],
            frame,
            &mut snapshot,
        );
        frame += 1;
        render(
            &context,
            &source,
            vec![pointer_button(position, false)],
            frame,
            &mut snapshot,
        );
        let edited = snapshot
            .values
            .last()
            .ok_or_else(|| io::Error::other("palette gesture emitted no value"))?;
        assert_eq!(edited.color_space(), &ColorSpaceRef::linear_srgb());
        assert_ne!(edited, &source);
        let display = library::color_management::to_display_srgb(edited)?;
        assert!(display[..3]
            .iter()
            .all(|component| (0.0..=1.0).contains(component)));
        assert!((display[3] - 0.75).abs() < f64::EPSILON);
        Ok(())
    }

    #[test]
    fn unsupported_tagged_space_is_explicitly_numeric_only(
    ) -> Result<(), Box<dyn std::error::Error>> {
        let context = egui::Context::default();
        let source = ColorValue::new(ColorSpaceRef::new("acescg")?, [0.5, 0.25, 2.0, 1.0])?;
        let mut snapshot = Snapshot::default();
        render(&context, &source, Vec::new(), 0, &mut snapshot);
        assert!(!snapshot.supported);
        assert!(snapshot.geometry.is_none());
        assert!(snapshot.values.is_empty());
        Ok(())
    }

    #[test]
    fn display_edit_in_a_space_switch_frame_targets_the_new_authored_space(
    ) -> Result<(), Box<dyn std::error::Error>> {
        let encoded = ColorValue::new(ColorSpaceRef::srgb(), [0.5, 0.2, 0.0, 0.75])?;
        let linear =
            library::color_management::transform_color(&encoded, &ColorSpaceRef::linear_srgb())?;
        let display = library::color_management::to_display_srgb(&linear)?;
        let mut draft = PickerDraft::from_source(&linear, display);
        draft.hsva.s = 0.4;
        draft.hsva.v = 0.8;
        let edited = value_from_display_draft(&draft)?;
        assert_eq!(edited.color_space(), &ColorSpaceRef::linear_srgb());
        assert_eq!(
            library::color_management::to_display_srgb(&edited)?[3],
            0.75
        );
        Ok(())
    }
}
