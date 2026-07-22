use egui::{Color32, Id, Ui};
use library::model::path::{FillRule, PathValue};
use library::model::property::{ColorSpaceRef, ColorValue, PropertyValue};

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

/// Edits the authoritative float components directly. A display color picker
/// is intentionally absent: it would clip HDR/negative RGB and erase the
/// color-space tag before the user explicitly requested a conversion.
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
        })),
    );
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
    fn long_canonical_path_keeps_apply_clickable_and_emits_exact_value(
    ) -> Result<(), Box<dyn std::error::Error>> {
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
