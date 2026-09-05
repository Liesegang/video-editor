//! Shared typed property control used by Inspector surfaces.
//!
//! Numeric metadata, vector layout, and color conversion each stay owned by
//! their existing shared widgets. Domain panels only decide when to commit an
//! edited draft to their model.

use egui::{Id, Response, Ui};
use library::model::authoring::ProjectPalette;
use library::model::frame::color::Color;
use library::model::property::{
    PropertyDefinition, PropertyUiType, PropertyValue, Vec2, Vec3, Vec4,
};
use ordered_float::OrderedFloat;

use super::color_value_picker::color_value_picker;
use super::paint_value_editor::{gradient_value_editor, pattern_value_editor};
use super::property_drag_value::{
    numeric_edit_finished, FloatDragValueConfig, IntegerDragValueConfig,
};
use super::vector_drag_value::{vector_drag_values, VectorAxisResponse};

pub(crate) struct PropertyValueEdit {
    pub response: Response,
    pub changed: bool,
    pub finished: bool,
}

pub(crate) struct PropertyValueEditorSpec<'a> {
    pub definition: Option<&'a PropertyDefinition>,
    pub fallback_suffix: &'a str,
    pub fallback_speed: f64,
    pub palette: &'a ProjectPalette,
}

#[derive(Clone)]
struct LegacyColorDraft {
    source: Color,
    authored: library::model::property::ColorValue,
}

/// Render a compact editor for one typed property value.
///
/// `fallback_speed` and `fallback_suffix` apply only when no property
/// definition is available. A definition remains authoritative whenever one
/// exists.
pub(crate) fn property_value_editor(
    ui: &mut Ui,
    id: Id,
    qa_id: &str,
    value: &mut PropertyValue,
    spec: PropertyValueEditorSpec<'_>,
) -> PropertyValueEdit {
    let PropertyValueEditorSpec {
        definition,
        fallback_suffix,
        fallback_speed,
        palette,
    } = spec;
    let default = definition.map(PropertyDefinition::default_value);
    let mut edit = match value {
        PropertyValue::Number(number) => {
            let config = float_config(definition, fallback_suffix, fallback_speed);
            let mut raw = number.into_inner();
            let response = ui.add(config.widget(&mut raw));
            let changed = response.changed();
            if changed {
                *number = OrderedFloat(raw);
            }
            PropertyValueEdit {
                finished: numeric_edit_finished(&response),
                response,
                changed,
            }
        }
        PropertyValue::Integer(integer) => {
            let config = definition
                .and_then(|definition| IntegerDragValueConfig::from_ui_type(definition.ui_type()))
                .unwrap_or(IntegerDragValueConfig {
                    suffix: fallback_suffix.to_string(),
                    hard_min: None,
                    hard_max: None,
                });
            let response = ui.add(config.widget(integer));
            PropertyValueEdit {
                changed: response.changed(),
                finished: numeric_edit_finished(&response),
                response,
            }
        }
        PropertyValue::Boolean(boolean) => {
            let response = ui.checkbox(boolean, "");
            PropertyValueEdit {
                changed: response.changed(),
                finished: response.changed(),
                response,
            }
        }
        PropertyValue::String(text) => string_editor(ui, id, qa_id, text, definition),
        PropertyValue::Vec2(vector) => vector2_editor(
            ui,
            qa_id,
            vector,
            definition,
            fallback_suffix,
            fallback_speed,
        ),
        PropertyValue::Vec3(vector) => vector3_editor(
            ui,
            qa_id,
            vector,
            definition,
            fallback_suffix,
            fallback_speed,
        ),
        PropertyValue::Vec4(vector) => vector4_editor(
            ui,
            qa_id,
            vector,
            definition,
            fallback_suffix,
            fallback_speed,
        ),
        PropertyValue::ColorValue(color) => {
            let picker = color_value_picker(ui, id.with("color"), color, Some(palette));
            if let Some(intent) = picker.palette_intent {
                super::palette_intent::queue(ui.ctx(), intent);
            }
            let changed = picker.value.is_some();
            if let Some(candidate) = picker.value {
                *color = candidate;
            }
            PropertyValueEdit {
                response: picker.response,
                changed,
                finished: picker.finished,
            }
        }
        PropertyValue::Color(color) => {
            let draft_id = id.with("legacy_color_draft");
            let mut draft = ui
                .data(|data| data.get_temp::<LegacyColorDraft>(draft_id))
                .filter(|draft| &draft.source == color)
                .unwrap_or_else(|| LegacyColorDraft {
                    source: color.clone(),
                    authored: library::model::property::ColorValue::from_straight_srgba8(color),
                });
            // Legacy Color is an encoded sRGBA8 raster/plugin boundary. Do
            // not offer managed Palette copy here: an HDR or wide-gamut
            // PaintDefinition cannot be represented without data loss.
            let picker = color_value_picker(ui, id.with("color"), &draft.authored, None);
            let changed = picker.value.is_some();
            if let Some(candidate) = picker.value {
                match library::color_management::to_renderer_srgba8(&candidate) {
                    Ok(render_color) => {
                        *color = render_color.clone();
                        draft.source = render_color;
                        draft.authored = candidate;
                    }
                    Err(error) => {
                        ui.colored_label(ui.visuals().error_fg_color, error.to_string());
                    }
                }
            }
            ui.data_mut(|data| data.insert_temp(draft_id, draft));
            PropertyValueEdit {
                response: picker.response,
                changed,
                finished: picker.finished,
            }
        }
        PropertyValue::Gradient(gradient) => {
            let edited = gradient_value_editor(ui, id.with("gradient"), qa_id, gradient, palette);
            PropertyValueEdit {
                response: edited.response,
                changed: edited.changed,
                finished: edited.finished,
            }
        }
        PropertyValue::Pattern(pattern) => {
            let edited = pattern_value_editor(ui, id.with("pattern"), qa_id, pattern, palette);
            PropertyValueEdit {
                response: edited.response,
                changed: edited.changed,
                finished: edited.finished,
            }
        }
        PropertyValue::Path(_)
        | PropertyValue::Array(_)
        | PropertyValue::Map(_)
        | PropertyValue::OpaqueJson(_) => {
            let response = ui.weak("Edit in Node Editor");
            PropertyValueEdit {
                response,
                changed: false,
                finished: false,
            }
        }
    };

    if edit.response.middle_clicked() {
        if let Some(default) = default {
            if *value != *default {
                *value = default.clone();
                edit.changed = true;
                edit.finished = true;
            }
        }
    }

    crate::qa::register_component_with_metadata(
        qa_id,
        "inspector_property_control",
        edit.response.rect,
        edit.response.enabled(),
        Some(serde_json::json!({
            "value": &*value,
            "has_definition": definition.is_some(),
            "editor_kind": definition.map(|definition| property_ui_kind(definition.ui_type())),
            "changed": edit.changed,
        })),
    );
    edit
}

fn property_ui_kind(ui_type: &PropertyUiType) -> &'static str {
    match ui_type {
        PropertyUiType::Float { .. } => "float",
        PropertyUiType::Integer { .. } => "integer",
        PropertyUiType::ColorValue => "managed_color",
        PropertyUiType::Gradient => "gradient",
        PropertyUiType::Pattern => "pattern",
        PropertyUiType::Path => "path",
        PropertyUiType::Color => "encoded_color",
        PropertyUiType::Text => "text",
        PropertyUiType::MultilineText => "multiline_text",
        PropertyUiType::Bool => "boolean",
        PropertyUiType::Vec2 { .. } => "vec2",
        PropertyUiType::Vec3 { .. } => "vec3",
        PropertyUiType::Vec4 { .. } => "vec4",
        PropertyUiType::Dropdown { .. } => "dropdown",
        PropertyUiType::Font => "font",
    }
}

fn string_editor(
    ui: &mut Ui,
    id: Id,
    qa_id: &str,
    text: &mut String,
    definition: Option<&PropertyDefinition>,
) -> PropertyValueEdit {
    match definition.map(PropertyDefinition::ui_type) {
        Some(PropertyUiType::Dropdown { options }) => {
            return combo_string_editor(ui, id.with("dropdown"), qa_id, text, options);
        }
        Some(PropertyUiType::Font) => {
            static AVAILABLE_FONTS: std::sync::OnceLock<Vec<String>> = std::sync::OnceLock::new();
            let fonts = AVAILABLE_FONTS
                .get_or_init(library::core::rendering::skia_utils::get_available_fonts);
            return combo_string_editor(ui, id.with("font"), qa_id, text, fonts);
        }
        Some(PropertyUiType::MultilineText) => {
            let response = ui.add(
                egui::TextEdit::multiline(text)
                    .desired_rows(3)
                    .desired_width(184.0),
            );
            return PropertyValueEdit {
                changed: response.changed(),
                finished: response.lost_focus()
                    || (response.has_focus()
                        && ui.input(|input| {
                            input.modifiers.command && input.key_pressed(egui::Key::Enter)
                        })),
                response,
            };
        }
        _ => {}
    }

    let response = ui.add(egui::TextEdit::singleline(text).desired_width(184.0));
    PropertyValueEdit {
        changed: response.changed(),
        finished: response.lost_focus()
            || (response.has_focus() && ui.input(|input| input.key_pressed(egui::Key::Enter))),
        response,
    }
}

fn combo_string_editor(
    ui: &mut Ui,
    id: Id,
    qa_id: &str,
    text: &mut String,
    options: &[String],
) -> PropertyValueEdit {
    let previous = text.clone();
    let combo = egui::ComboBox::from_id_salt(id)
        .selected_text(text.as_str())
        .width(184.0)
        .show_ui(ui, |ui| {
            for option in options {
                let response = ui.selectable_value(text, option.clone(), option);
                crate::qa::register_component_with_metadata(
                    format!("{qa_id}.option:{option}"),
                    "inspector_property_option",
                    response.rect,
                    response.enabled(),
                    Some(serde_json::json!({"value": option, "selected": text == option})),
                );
            }
        });
    let changed = *text != previous;
    PropertyValueEdit {
        response: combo.response,
        changed,
        finished: changed,
    }
}

fn vector2_editor(
    ui: &mut Ui,
    qa_id: &str,
    vector: &mut Vec2,
    definition: Option<&PropertyDefinition>,
    suffix: &str,
    speed: f64,
) -> PropertyValueEdit {
    let config = float_config(definition, suffix, speed);
    let (mut x, mut y) = (vector.x.into_inner(), vector.y.into_inner());
    let group = vector_drag_values(
        ui,
        &config,
        &mut [("X", &mut x), ("Y", &mut y)],
        ui.spacing().interact_size.y,
    );
    register_vector_axes(qa_id, &group.axes);
    let reset = group.reset;
    let changed = group.changed || reset;
    if reset {
        if let Some(PropertyValue::Vec2(default)) =
            definition.map(PropertyDefinition::default_value)
        {
            *vector = *default;
        }
    } else if group.changed {
        vector.x = OrderedFloat(x);
        vector.y = OrderedFloat(y);
    }
    PropertyValueEdit {
        response: group.response,
        changed,
        finished: group.finished || reset,
    }
}

fn vector3_editor(
    ui: &mut Ui,
    qa_id: &str,
    vector: &mut Vec3,
    definition: Option<&PropertyDefinition>,
    suffix: &str,
    speed: f64,
) -> PropertyValueEdit {
    let config = float_config(definition, suffix, speed);
    let (mut x, mut y, mut z) = (
        vector.x.into_inner(),
        vector.y.into_inner(),
        vector.z.into_inner(),
    );
    let group = vector_drag_values(
        ui,
        &config,
        &mut [("X", &mut x), ("Y", &mut y), ("Z", &mut z)],
        ui.spacing().interact_size.y,
    );
    register_vector_axes(qa_id, &group.axes);
    let reset = group.reset;
    let changed = group.changed || reset;
    if reset {
        if let Some(PropertyValue::Vec3(default)) =
            definition.map(PropertyDefinition::default_value)
        {
            *vector = *default;
        }
    } else if group.changed {
        vector.x = OrderedFloat(x);
        vector.y = OrderedFloat(y);
        vector.z = OrderedFloat(z);
    }
    PropertyValueEdit {
        response: group.response,
        changed,
        finished: group.finished || reset,
    }
}

fn vector4_editor(
    ui: &mut Ui,
    qa_id: &str,
    vector: &mut Vec4,
    definition: Option<&PropertyDefinition>,
    suffix: &str,
    speed: f64,
) -> PropertyValueEdit {
    let config = float_config(definition, suffix, speed);
    let (mut x, mut y, mut z, mut w) = (
        vector.x.into_inner(),
        vector.y.into_inner(),
        vector.z.into_inner(),
        vector.w.into_inner(),
    );
    let group = vector_drag_values(
        ui,
        &config,
        &mut [("X", &mut x), ("Y", &mut y), ("Z", &mut z), ("W", &mut w)],
        ui.spacing().interact_size.y,
    );
    register_vector_axes(qa_id, &group.axes);
    let reset = group.reset;
    let changed = group.changed || reset;
    if reset {
        if let Some(PropertyValue::Vec4(default)) =
            definition.map(PropertyDefinition::default_value)
        {
            *vector = *default;
        }
    } else if group.changed {
        vector.x = OrderedFloat(x);
        vector.y = OrderedFloat(y);
        vector.z = OrderedFloat(z);
        vector.w = OrderedFloat(w);
    }
    PropertyValueEdit {
        response: group.response,
        changed,
        finished: group.finished || reset,
    }
}

fn float_config(
    definition: Option<&PropertyDefinition>,
    suffix: &str,
    speed: f64,
) -> FloatDragValueConfig {
    definition
        .and_then(FloatDragValueConfig::from_definition)
        .unwrap_or(FloatDragValueConfig {
            speed,
            suffix: suffix.to_string(),
            hard_min: None,
            hard_max: None,
        })
}

fn register_vector_axes(qa_id: &str, axes: &[VectorAxisResponse]) {
    for axis in axes {
        crate::qa::register_component_with_metadata(
            format!("{qa_id}:{}", axis.axis.to_ascii_lowercase()),
            "inspector_vector_component_control",
            axis.response.rect,
            axis.response.enabled(),
            Some(serde_json::json!({
                "axis": axis.axis,
                "value": axis.value,
            })),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vector_editor_keeps_components_in_one_shared_control() {
        let context = egui::Context::default();
        let mut value = PropertyValue::Vec2(Vec2 {
            x: OrderedFloat(10.0),
            y: OrderedFloat(20.0),
        });
        let mut rect = egui::Rect::NOTHING;
        let palette = ProjectPalette::default();
        drop(context.run(egui::RawInput::default(), |context| {
            egui::CentralPanel::default().show(context, |ui| {
                rect = property_value_editor(
                    ui,
                    egui::Id::new("position"),
                    "inspector.test.position",
                    &mut value,
                    PropertyValueEditorSpec {
                        definition: None,
                        fallback_suffix: " px",
                        fallback_speed: 1.0,
                        palette: &palette,
                    },
                )
                .response
                .rect;
            });
        }));
        assert!(rect.width() >= 180.0);
        assert_eq!(
            value,
            PropertyValue::Vec2(Vec2 {
                x: OrderedFloat(10.0),
                y: OrderedFloat(20.0),
            })
        );
    }
}
