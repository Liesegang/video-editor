use egui::{Color32, Id, Ui};
use library::model::path::{FillRule, PathValue};
use library::model::property::{ColorSpaceRef, ColorValue, PropertyValue};

use crate::ui::widgets::color_value_picker::{ColorPickerEdit, color_value_picker};

#[derive(Clone)]
struct ColorDraft {
    source: ColorValue,
    color_space: String,
    rgba: [f64; 4],
}

impl ColorDraft {
    fn from_value(value: &ColorValue) -> Self {
        Self {
            source: value.clone(),
            color_space: value.color_space().as_str().to_string(),
            rgba: value.rgba(),
        }
    }
}

pub(super) struct StructuredEdit {
    pub response: egui::Response,
    pub value: Option<PropertyValue>,
    pub finished: bool,
}

pub(super) fn canonical_color_for_inspector(
    authored: Option<&PropertyValue>,
    default: &PropertyValue,
) -> Option<ColorValue> {
    let adapt = |value: &PropertyValue| match value {
        PropertyValue::ColorValue(color) => Some(color.clone()),
        // Explicit pre-v1 read adapter. The structured editor emits a
        // ColorValue on the first edit; no Project-wide migration is needed.
        PropertyValue::Color(color) => Some(ColorValue::from_straight_srgba8(color)),
        _ => None,
    };
    match authored {
        Some(value) => adapt(value),
        None => adapt(default),
    }
}

pub(super) fn canonical_path_for_inspector(
    authored: Option<&PropertyValue>,
    default: &PropertyValue,
) -> Option<PathValue> {
    let adapt = |value: &PropertyValue| match value {
        PropertyValue::Path(path) => Some(path.clone()),
        // Explicit pre-v1 read adapter. Applying an edit upgrades only this
        // property to canonical PathValue; the Project is never migrated.
        PropertyValue::String(svg) => library::model::path::parse_legacy_svg_path_data(svg).ok(),
        _ => None,
    };
    match authored {
        Some(value) => adapt(value),
        None => adapt(default),
    }
}

/// Edits the authoritative float components directly and offers an explicit
/// display-view picker when the shared color-management service supports the
/// authored space. The picker never replaces these lossless controls.
pub(super) fn color_value(
    ui: &mut Ui,
    id: Id,
    value: &ColorValue,
    qa_component_prefix: &str,
) -> StructuredEdit {
    let mut draft = ui
        .data(|data| data.get_temp::<ColorDraft>(id))
        .filter(|draft| draft.source == *value)
        .unwrap_or_else(|| ColorDraft::from_value(value));
    let mut changed = false;
    let mut finished = false;
    let mut candidate = None;
    let group = ui.vertical(|ui| {
        let picker = color_value_picker(ui, id.with("display_picker"), value);
        let display_picker_available = picker.supported;
        register_color_picker(qa_component_prefix, value, &picker);
        if let Some(picked) = picker.value {
            draft = ColorDraft::from_value(&picked);
            candidate = Some(PropertyValue::ColorValue(picked));
            changed = true;
        }
        finished |= picker.finished;
        ui.horizontal(|ui| {
            ui.small("space");
            let response = ui.add_sized(
                [112.0, 18.0],
                egui::TextEdit::singleline(&mut draft.color_space),
            );
            register_color_component(
                format!("{qa_component_prefix}:color_space"),
                "color_space",
                &response,
                serde_json::json!(draft.color_space),
                display_picker_available,
            );
            changed |= response.changed();
            finished |= response.lost_focus();
        });
        ui.horizontal(|ui| {
            for (label, component) in ["R", "G", "B", "A"].into_iter().zip(&mut draft.rgba) {
                ui.small(label);
                let response =
                    ui.add_sized([62.0, 18.0], egui::DragValue::new(component).speed(0.01));
                register_color_component(
                    format!("{qa_component_prefix}:{}", label.to_ascii_lowercase()),
                    label,
                    &response,
                    serde_json::json!(*component),
                    display_picker_available,
                );
                changed |= response.changed();
                finished |= response.drag_stopped() || response.lost_focus();
            }
        });
        match ColorSpaceRef::new(draft.color_space.clone())
            .and_then(|space| ColorValue::new(space, draft.rgba))
        {
            Ok(value) => {
                if changed {
                    draft.source = value.clone();
                    candidate = Some(PropertyValue::ColorValue(value));
                }
                ui.small("straight alpha · float RGB (HDR/negative allowed)");
            }
            Err(error) => {
                ui.colored_label(Color32::LIGHT_RED, error.to_string());
            }
        }
    });
    ui.data_mut(|data| data.insert_temp(id, draft));
    StructuredEdit {
        response: group.response,
        value: candidate,
        finished,
    }
}

fn register_color_component(
    component_id: String,
    component: &str,
    response: &egui::Response,
    value: serde_json::Value,
    display_picker_available: bool,
) {
    crate::qa::register_component_with_metadata(
        component_id,
        "inspector_canonical_color_component",
        response.rect,
        response.enabled(),
        Some(serde_json::json!({
            "component": component,
            "value": value,
            "storage": "canonical_color_value",
            "numeric": "f64",
            "alpha": "straight",
            "legacy_srgba8_picker": false,
            "display_picker_available": display_picker_available,
        })),
    );
}

pub(in crate::ui::panels::inspector) fn register_color_picker(
    qa_component_prefix: &str,
    value: &ColorValue,
    picker: &ColorPickerEdit,
) {
    let picker_id = format!("{qa_component_prefix}:picker");
    crate::qa::register_component_with_metadata(
        picker_id.clone(),
        "canonical_color_display_picker",
        picker.response.rect,
        picker.response.enabled(),
        Some(color_picker_metadata(value, picker)),
    );
    let Some(geometry) = picker.geometry else {
        return;
    };
    let dimensions = serde_json::json!({
        "popup_width": geometry.popup.width(),
        "popup_height": geometry.popup.height(),
        "saturation_value_width": geometry.saturation_value.width(),
        "saturation_value_height": geometry.saturation_value.height(),
    });
    for (suffix, component_type, rect, control) in [
        (
            "picker_popup",
            "canonical_color_picker_popup",
            geometry.popup,
            "popup",
        ),
        (
            "picker_saturation_value",
            "canonical_color_picker_saturation_value",
            geometry.saturation_value,
            "saturation_value",
        ),
        (
            "picker_authored_space",
            "canonical_color_picker_authored_space",
            geometry.authored_space,
            "authored_space",
        ),
        (
            "picker_hue",
            "canonical_color_picker_hue",
            geometry.hue,
            "hue",
        ),
        (
            "picker_alpha",
            "canonical_color_picker_alpha",
            geometry.alpha,
            "alpha",
        ),
    ] {
        crate::qa::register_component_with_metadata(
            format!("{qa_component_prefix}:{suffix}"),
            component_type,
            rect,
            true,
            Some(serde_json::json!({
                "control": control,
                "authored_space": value.color_space().as_str(),
                "display_space": "srgb",
                "transform_authority": "ruvie-color-management",
                "large_picker": true,
                "dimensions": dimensions,
                "trigger": picker_id,
            })),
        );
    }
}

fn color_picker_metadata(value: &ColorValue, picker: &ColorPickerEdit) -> serde_json::Value {
    serde_json::json!({
        "storage": "canonical_color_value",
        "numeric": "f64",
        "alpha": "straight",
        "authored_space": value.color_space().as_str(),
        "display_space": "srgb",
        "transform_authority": "ruvie-color-management",
        "supported": picker.supported,
        "display_clipped": picker.display_clipped,
        "no_op_preserves_authored_value": true,
        "error": picker.error,
    })
}

#[derive(Clone)]
struct PathDraft {
    source: PathValue,
    json: String,
    error: Option<String>,
}

impl PathDraft {
    fn from_value(value: &PathValue) -> Self {
        Self {
            source: value.clone(),
            json: serde_json::to_string_pretty(value).unwrap_or_else(|_| "{}".to_string()),
            error: None,
        }
    }
}

/// Keeps canonical JSON as an explicit import/edit boundary. SVG remains an
/// interchange adapter and is never stored as this Node's authoritative value.
pub(super) fn path_value(
    ui: &mut Ui,
    id: Id,
    value: &PathValue,
    qa_component_prefix: &str,
) -> StructuredEdit {
    let mut draft = ui
        .data(|data| data.get_temp::<PathDraft>(id))
        .filter(|draft| draft.source == *value)
        .unwrap_or_else(|| PathDraft::from_value(value));
    let mut candidate = None;
    let group = ui.vertical(|ui| {
        let contours = value.contours().len();
        let segments = value
            .contours()
            .iter()
            .map(|contour| contour.segments().len())
            .sum::<usize>();
        let fill = match value.fill_rule() {
            FillRule::NonZero => "non-zero",
            FillRule::EvenOdd => "even-odd",
        };
        ui.small(format!(
            "{contours} contours · {segments} segments · {fill}"
        ));
        let editor = egui::CollapsingHeader::new("Edit canonical JSON")
            .id_salt(id.with("canonical_json"))
            .show(ui, |ui| {
                let json_editor = egui::ScrollArea::vertical()
                    .id_salt(id.with("canonical_json_scroll"))
                    .max_height(120.0)
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        ui.add(
                            egui::TextEdit::multiline(&mut draft.json)
                                .code_editor()
                                .desired_rows(6)
                                .desired_width(360.0),
                        )
                    });
                let json_response = json_editor.inner;
                record_structured_test_rect(
                    format!("{qa_component_prefix}:json"),
                    json_editor.inner_rect,
                );
                crate::qa::register_component_with_metadata(
                    format!("{qa_component_prefix}:json"),
                    "inspector_canonical_path_json",
                    json_editor.inner_rect,
                    json_response.enabled(),
                    Some(serde_json::json!({
                        "storage": "canonical_path_value",
                        "format": "canonical_json",
                        "svg_authoritative": false,
                        "validation_error": draft.error,
                    })),
                );
                ui.horizontal(|ui| {
                    let apply = ui.button("Apply");
                    record_structured_test_rect(format!("{qa_component_prefix}:apply"), apply.rect);
                    crate::qa::register_component_with_metadata(
                        format!("{qa_component_prefix}:apply"),
                        "inspector_canonical_path_apply",
                        apply.rect,
                        apply.enabled(),
                        Some(serde_json::json!({
                            "storage": "canonical_path_value",
                            "format": "canonical_json",
                            "svg_authoritative": false,
                        })),
                    );
                    if apply.clicked() {
                        match serde_json::from_str::<PathValue>(&draft.json) {
                            Ok(path) => {
                                draft.source = path.clone();
                                draft.json = serde_json::to_string_pretty(&path)
                                    .unwrap_or_else(|_| draft.json.clone());
                                draft.error = None;
                                candidate = Some(PropertyValue::Path(path));
                            }
                            Err(error) => draft.error = Some(error.to_string()),
                        }
                    }
                    let reset = ui.button("Reset draft");
                    crate::qa::register_component_with_metadata(
                        format!("{qa_component_prefix}:reset"),
                        "inspector_canonical_path_reset",
                        reset.rect,
                        reset.enabled(),
                        None,
                    );
                    if reset.clicked() {
                        draft = PathDraft::from_value(value);
                    }
                    ui.small("Validated canonical PathValue; not SVG path data");
                });
                if let Some(error) = &draft.error {
                    ui.colored_label(Color32::LIGHT_RED, error);
                }
            });
        crate::qa::register_component_with_metadata(
            format!("{qa_component_prefix}:toggle"),
            "inspector_canonical_path_toggle",
            editor.header_response.rect,
            editor.header_response.enabled(),
            Some(serde_json::json!({
                "storage": "canonical_path_value",
                "format": "canonical_json",
                "svg_authoritative": false,
                "open": editor.body_returned.is_some(),
            })),
        );
    });
    ui.data_mut(|data| data.insert_temp(id, draft));
    StructuredEdit {
        response: group.response,
        finished: candidate.is_some(),
        value: candidate,
    }
}

#[cfg(test)]
thread_local! {
    static STRUCTURED_TEST_RECTS: std::cell::RefCell<std::collections::HashMap<String, egui::Rect>> =
        std::cell::RefCell::new(std::collections::HashMap::new());
}

fn record_structured_test_rect(component_id: String, rect: egui::Rect) {
    #[cfg(test)]
    STRUCTURED_TEST_RECTS.with(|rects| {
        rects.borrow_mut().insert(component_id.clone(), rect);
    });
    let _ = (component_id, rect);
}

#[cfg(test)]
mod tests {
    use super::*;
    use library::model::frame::color::Color;
    use library::model::path::{PathContour, PathPoint, PathSegment};
    use std::io;

    fn long_path() -> Result<PathValue, library::model::path::PathValidationError> {
        let segments = (0..24)
            .map(|index| {
                let x = f64::from(index);
                PathSegment::cubic(
                    PathPoint::new(x + 0.25, -x),
                    PathPoint::new(x + 0.5, x * 2.0),
                    PathPoint::new(x + 1.0, x),
                )
            })
            .collect();
        PathValue::new(
            FillRule::EvenOdd,
            vec![PathContour::new(PathPoint::new(-2.5, 4.0), segments, true)],
        )
    }

    #[test]
    fn legacy_color_is_displayed_exactly_in_a_canonical_color_control()
    -> Result<(), Box<dyn std::error::Error>> {
        let legacy = Color {
            r: 17,
            g: 128,
            b: 239,
            a: 64,
        };
        let default = PropertyValue::ColorValue(ColorValue::from_straight_srgba8(&Color::white()));
        let adapted =
            canonical_color_for_inspector(Some(&PropertyValue::Color(legacy.clone())), &default)
                .ok_or("legacy color was not adapted")?;
        assert_eq!(adapted.try_to_straight_srgba8(), Ok(legacy));
        Ok(())
    }

    #[test]
    fn canonical_picker_metadata_keeps_stable_transform_and_storage_contract()
    -> Result<(), Box<dyn std::error::Error>> {
        let context = egui::Context::default();
        let color = ColorValue::new(ColorSpaceRef::linear_srgb(), [0.25, 0.5, 0.75, 1.0])?;
        let mut metadata = None;
        drop(context.run(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(600.0, 400.0),
                )),
                ..Default::default()
            },
            |context| {
                egui::CentralPanel::default().show(context, |ui| {
                    let picker =
                        color_value_picker(ui, egui::Id::new("canonical-picker-metadata"), &color);
                    metadata = Some(color_picker_metadata(&color, &picker));
                });
            },
        ));
        assert_eq!(
            metadata,
            Some(serde_json::json!({
                "storage": "canonical_color_value",
                "numeric": "f64",
                "alpha": "straight",
                "authored_space": "linear-srgb",
                "display_space": "srgb",
                "transform_authority": "ruvie-color-management",
                "supported": true,
                "display_clipped": false,
                "no_op_preserves_authored_value": true,
                "error": null,
            }))
        );
        Ok(())
    }

    #[test]
    fn legacy_svg_is_displayed_as_its_actual_canonical_path()
    -> Result<(), Box<dyn std::error::Error>> {
        let legacy = "M 2 3 Q 20 40 38 3 Z";
        let default = PropertyValue::Path(PathValue::empty(FillRule::NonZero));
        let adapted = canonical_path_for_inspector(
            Some(&PropertyValue::String(legacy.to_string())),
            &default,
        )
        .ok_or_else(|| io::Error::other("legacy SVG was not adapted"))?;
        assert_eq!(
            adapted,
            library::model::path::parse_legacy_svg_path_data(legacy)?
        );
        assert!(!adapted.contours().is_empty());
        Ok(())
    }

    #[test]
    fn malformed_canonical_values_are_not_displayed_as_defaults() {
        let color_default =
            PropertyValue::ColorValue(ColorValue::from_straight_srgba8(&Color::white()));
        let path_default = PropertyValue::Path(PathValue::empty(FillRule::NonZero));
        let malformed = PropertyValue::Map(std::collections::HashMap::new());
        assert!(canonical_color_for_inspector(Some(&malformed), &color_default).is_none());
        assert!(canonical_path_for_inspector(Some(&malformed), &path_default).is_none());
    }

    #[test]
    fn long_canonical_path_keeps_apply_clickable_and_emits_exact_value()
    -> Result<(), Box<dyn std::error::Error>> {
        const PREFIX: &str = "test.path:value";
        let context = egui::Context::default();
        context.memory_mut(|memory| memory.set_everything_is_visible(true));
        let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(500.0, 320.0));
        let id = egui::Id::new("canonical_path_test");
        let initial = PathValue::empty(FillRule::NonZero);
        let expected = long_path()?;
        let expected_json = serde_json::to_string_pretty(&expected)?;
        context.data_mut(|data| {
            data.insert_temp(
                id,
                PathDraft {
                    source: initial.clone(),
                    json: expected_json,
                    error: None,
                },
            );
        });
        STRUCTURED_TEST_RECTS.with(|rects| rects.borrow_mut().clear());
        let mut updates = Vec::new();

        let mut render = |events: Vec<egui::Event>, frame: usize| {
            drop(context.run(
                egui::RawInput {
                    screen_rect: Some(screen),
                    time: Some(frame as f64 / 60.0),
                    events,
                    ..Default::default()
                },
                |context| {
                    egui::CentralPanel::default().show(context, |ui| {
                        let collapse_id = ui.make_persistent_id(id.with("canonical_json"));
                        let mut collapse =
                            egui::collapsing_header::CollapsingState::load_with_default_open(
                                context,
                                collapse_id,
                                true,
                            );
                        collapse.set_open(true);
                        collapse.store(context);
                        let edit = path_value(ui, id, &initial, PREFIX);
                        if let Some(value) = edit.value {
                            updates.push(value);
                        }
                    });
                },
            ));
        };

        render(Vec::new(), 0);
        render(Vec::new(), 1);
        let (json_rect, apply_rect) = STRUCTURED_TEST_RECTS.with(|rects| {
            let rects = rects.borrow();
            let json_rect = rects
                .get(&format!("{PREFIX}:json"))
                .copied()
                .ok_or_else(|| io::Error::other("canonical Path JSON rect was not rendered"))?;
            let apply_rect = rects
                .get(&format!("{PREFIX}:apply"))
                .copied()
                .ok_or_else(|| io::Error::other("canonical Path Apply rect was not rendered"))?;
            Ok::<_, io::Error>((json_rect, apply_rect))
        })?;
        assert!(json_rect.height() <= 120.0);
        assert!(screen.contains(apply_rect.center()));

        let point = apply_rect.center();
        render(
            vec![
                egui::Event::PointerMoved(point),
                egui::Event::PointerButton {
                    pos: point,
                    button: egui::PointerButton::Primary,
                    pressed: true,
                    modifiers: egui::Modifiers::NONE,
                },
            ],
            2,
        );
        render(
            vec![egui::Event::PointerButton {
                pos: point,
                button: egui::PointerButton::Primary,
                pressed: false,
                modifiers: egui::Modifiers::NONE,
            }],
            3,
        );
        assert_eq!(updates, vec![PropertyValue::Path(expected)]);
        Ok(())
    }
}
