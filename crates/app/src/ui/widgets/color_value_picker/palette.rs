use egui::{
    Align, Color32, Id, Layout, Popup, PopupCloseBehavior, PopupKind, Rect, Response, Sense, Stroke,
};
use egui_phosphor::regular as icons;
use library::model::authoring::{Paint, PaintDefinition, PaintDefinitionId, ProjectPalette};
use library::model::property::ColorValue;

use super::PaletteUiIntent;

const SWATCH_SIZE: egui::Vec2 = egui::vec2(72.0, 58.0);
const COLOR_HEIGHT: f32 = 36.0;

#[derive(Clone, Debug)]
#[allow(
    dead_code,
    reason = "geometry is exposed to native-input and widget QA without affecting Palette behavior"
)]
pub(crate) struct PaletteGeometry {
    pub add_current: Rect,
    pub swatches: Vec<(PaintDefinitionId, Rect)>,
    pub context: Option<PaletteContextGeometry>,
}

#[derive(Clone, Copy, Debug)]
#[allow(
    dead_code,
    reason = "geometry is exposed to native-input and widget QA without affecting Palette behavior"
)]
pub(crate) struct PaletteContextGeometry {
    pub popup: Rect,
    pub rename_name: Rect,
    pub rename: Rect,
    pub delete: Rect,
}

#[derive(Clone, Copy, Debug)]
struct PaletteDragPayload {
    definition_id: PaintDefinitionId,
}

#[derive(Clone, Copy, Debug)]
struct PaletteContextState {
    definition_id: PaintDefinitionId,
    position: egui::Pos2,
}

pub(super) struct PaletteRenderResult {
    pub value: Option<ColorValue>,
    pub intent: Option<PaletteUiIntent>,
    pub context_owns_click: bool,
    pub geometry: PaletteGeometry,
}

pub(super) fn show_palette(
    ui: &mut egui::Ui,
    id: Id,
    current: &ColorValue,
    palette: &ProjectPalette,
) -> PaletteRenderResult {
    let add = ui.button(format!("{} Add Current", icons::PLUS));
    crate::qa::register_component_with_metadata(
        "color_picker.palette.add_current",
        "palette_add_current",
        add.rect,
        true,
        Some(serde_json::json!({
            "action": "add_solid",
            "suggested_name": suggested_name(palette),
        })),
    );

    let mut intent = add.clicked().then(|| PaletteUiIntent::AddSolid {
        suggested_name: suggested_name(palette),
        color: current.clone(),
    });
    let mut selected = None;
    let context_id = id.with("palette_context");
    let mut context_state = ui.data(|data| data.get_temp::<PaletteContextState>(context_id));
    let mut swatches = Vec::new();
    let mut context_geometry = None;
    let mut context_owns_click = false;
    let definitions = palette
        .ungrouped_definitions()
        .enumerate()
        .filter(|(_, definition)| matches!(definition.paint, Paint::Solid(_)))
        .collect::<Vec<_>>();

    ui.add_space(4.0);
    if definitions.is_empty() {
        ui.weak("No Project colors yet");
    } else {
        let scroll = egui::ScrollArea::vertical()
            .id_salt(id.with("palette_scroll"))
            .max_height(320.0)
            .auto_shrink([false, true])
            .show(ui, |ui| {
                ui.horizontal_wrapped(|ui| {
                    ui.spacing_mut().item_spacing = egui::vec2(6.0, 6.0);
                    for (visible_index, (palette_index, definition)) in
                        definitions.iter().enumerate()
                    {
                        let response = swatch(ui, definition);
                        swatches.push((definition.id, response.rect));
                        response.dnd_set_drag_payload(PaletteDragPayload {
                            definition_id: definition.id,
                        });

                        let dragging = egui::DragAndDrop::payload::<PaletteDragPayload>(ui.ctx())
                            .is_some_and(|payload| payload.definition_id == definition.id);
                        register_swatch_qa(ui, definition, &response, dragging, visible_index);

                        if response.clicked() {
                            selected = palette.solid_color(definition.id);
                            clear_context_state(ui.ctx(), id, &mut context_state);
                        }
                        if response.secondary_clicked() {
                            clear_context_state(ui.ctx(), id, &mut context_state);
                            context_state = Some(PaletteContextState {
                                definition_id: definition.id,
                                position: ui
                                    .ctx()
                                    .pointer_hover_pos()
                                    .unwrap_or(response.rect.center()),
                            });
                        }
                        reorder_target(
                            ui,
                            palette,
                            definition,
                            *palette_index,
                            &response,
                            &mut intent,
                        );
                    }
                });
            });
        crate::qa::register_component_with_metadata(
            "color_picker.palette.scroll_area",
            "palette_scroll_area",
            scroll.inner_rect,
            true,
            Some(serde_json::json!({
                "content_height": scroll.content_size.y,
                "viewport_height": scroll.inner_rect.height(),
                "offset_y": scroll.state.offset.y,
                "scrollable": scroll.content_size.y > scroll.inner_rect.height(),
            })),
        );
    }

    if let Some(state) = context_state {
        if let Some(definition) = palette.definitions.get(&state.definition_id) {
            let mut open = true;
            let popup = Popup::new(
                id.with("palette_context_popup"),
                ui.ctx().clone(),
                state.position,
                ui.layer_id(),
            )
            .open_bool(&mut open)
            .kind(PopupKind::Menu)
            .close_behavior(PopupCloseBehavior::CloseOnClickOutside)
            .layout(Layout::top_down_justified(Align::Min))
            .width(220.0)
            .show(|ui| context_actions(id, definition, ui, &mut intent));
            let action_close = popup.as_ref().is_some_and(|response| response.inner.close);
            context_owns_click = popup.as_ref().is_some_and(|response| {
                ui.ctx().input(|input| input.pointer.any_click())
                    && !response.response.clicked_elsewhere()
            });
            context_geometry = popup.map(|response| PaletteContextGeometry {
                popup: response.response.rect,
                rename_name: response.inner.rename_name,
                rename: response.inner.rename,
                delete: response.inner.delete,
            });
            if action_close || !open {
                clear_context_state(ui.ctx(), id, &mut context_state);
            }
        } else {
            clear_context_state(ui.ctx(), id, &mut context_state);
        }
    }
    ui.data_mut(|data| match context_state {
        Some(state) => data.insert_temp(context_id, state),
        None => data.remove::<PaletteContextState>(context_id),
    });

    PaletteRenderResult {
        value: selected,
        intent,
        context_owns_click,
        geometry: PaletteGeometry {
            add_current: add.rect,
            swatches,
            context: context_geometry,
        },
    }
}

pub(super) fn close_context(context: &egui::Context, picker_id: Id) {
    let context_id = picker_id.with("palette_context");
    let mut state = context.data(|data| data.get_temp::<PaletteContextState>(context_id));
    clear_context_state(context, picker_id, &mut state);
}

fn clear_context_state(
    context: &egui::Context,
    picker_id: Id,
    state: &mut Option<PaletteContextState>,
) {
    let previous = state.take();
    context.data_mut(|data| {
        data.remove::<PaletteContextState>(picker_id.with("palette_context"));
        if let Some(previous) = previous {
            data.remove::<String>(picker_id.with(("palette_rename", previous.definition_id)));
        }
    });
}

fn suggested_name(palette: &ProjectPalette) -> String {
    (1..)
        .map(|index| format!("Color {index}"))
        .find(|candidate| {
            palette
                .definitions
                .values()
                .all(|definition| definition.name != candidate.as_str())
        })
        .unwrap_or_else(|| format!("Color {}", PaintDefinitionId::new()))
}

fn swatch(ui: &mut egui::Ui, definition: &PaintDefinition) -> Response {
    let (rect, response) = ui.allocate_exact_size(SWATCH_SIZE, Sense::click_and_drag());
    let color_rect =
        Rect::from_min_max(rect.min, egui::pos2(rect.max.x, rect.min.y + COLOR_HEIGHT));
    let color = display_color(definition).unwrap_or(ui.visuals().error_fg_color);
    ui.painter().rect(
        color_rect,
        3.0,
        color,
        ui.style().interact(&response).bg_stroke,
        egui::StrokeKind::Inside,
    );
    ui.painter().text(
        egui::pos2(rect.center().x, color_rect.bottom() + 9.0),
        egui::Align2::CENTER_CENTER,
        &definition.name,
        egui::TextStyle::Small.resolve(ui.style()),
        ui.style().interact(&response).text_color(),
    );
    response
        .on_hover_text(format!(
            "{}\nDrag to reorder; right-click to manage",
            definition.name
        ))
        .on_hover_cursor(egui::CursorIcon::Grab)
}

fn display_color(definition: &PaintDefinition) -> Option<Color32> {
    let Paint::Solid(color) = &definition.paint else {
        return None;
    };
    let [red, green, blue, alpha] = library::color_management::to_display_srgb(color).ok()?;
    Some(Color32::from_rgba_unmultiplied(
        channel(red),
        channel(green),
        channel(blue),
        channel(alpha),
    ))
}

fn channel(value: f64) -> u8 {
    (value.clamp(0.0, 1.0) * 255.0).round() as u8
}

struct ContextActionsResult {
    close: bool,
    rename_name: Rect,
    rename: Rect,
    delete: Rect,
}

fn context_actions(
    picker_id: Id,
    definition: &PaintDefinition,
    ui: &mut egui::Ui,
    intent: &mut Option<PaletteUiIntent>,
) -> ContextActionsResult {
    let mut close = false;
    ui.label(format!("{} {}", icons::PENCIL_SIMPLE, definition.name));
    let draft_id = picker_id.with(("palette_rename", definition.id));
    let mut name = ui
        .data(|data| data.get_temp::<String>(draft_id))
        .unwrap_or_else(|| definition.name.clone());
    let name_edit = ui.text_edit_singleline(&mut name);
    crate::qa::register_component_with_metadata(
        format!("color_picker.palette.rename_name:{}", definition.id),
        "palette_rename_name",
        name_edit.rect,
        true,
        Some(serde_json::json!({ "paint_definition_id": definition.id })),
    );
    ui.data_mut(|data| data.insert_temp(draft_id, name.clone()));

    let rename = ui.add_enabled(
        !name.trim().is_empty() && name.trim() != definition.name,
        egui::Button::new(format!("{} Rename", icons::CHECK)),
    );
    crate::qa::register_component_with_metadata(
        format!("color_picker.palette.rename:{}", definition.id),
        "palette_rename",
        rename.rect,
        rename.enabled(),
        Some(serde_json::json!({ "paint_definition_id": definition.id })),
    );
    if rename.clicked() {
        *intent = Some(PaletteUiIntent::Rename {
            id: definition.id,
            name: name.trim().to_string(),
        });
        ui.data_mut(|data| data.remove::<String>(draft_id));
        close = true;
    }

    let delete = ui.button(format!("{} Delete", icons::TRASH));
    crate::qa::register_component_with_metadata(
        format!("color_picker.palette.delete:{}", definition.id),
        "palette_delete",
        delete.rect,
        true,
        Some(serde_json::json!({ "paint_definition_id": definition.id })),
    );
    if delete.clicked() {
        *intent = Some(PaletteUiIntent::Delete { id: definition.id });
        close = true;
    }
    ContextActionsResult {
        close,
        rename_name: name_edit.rect,
        rename: rename.rect,
        delete: delete.rect,
    }
}

fn reorder_target(
    ui: &egui::Ui,
    palette: &ProjectPalette,
    definition: &PaintDefinition,
    hovered_index: usize,
    response: &Response,
    intent: &mut Option<PaletteUiIntent>,
) {
    let Some(payload) = response.dnd_hover_payload::<PaletteDragPayload>() else {
        return;
    };
    if payload.definition_id == definition.id {
        return;
    }
    let insert_after = ui
        .ctx()
        .pointer_interact_pos()
        .is_some_and(|pointer| pointer.x >= response.rect.center().x);
    let insertion_index = hovered_index + usize::from(insert_after);
    let Some(new_index) = final_index(palette, payload.definition_id, insertion_index) else {
        return;
    };
    let x = if insert_after {
        response.rect.right() + 2.0
    } else {
        response.rect.left() - 2.0
    };
    ui.painter().vline(
        x,
        response.rect.y_range(),
        Stroke::new(3.0, ui.visuals().selection.stroke.color),
    );
    crate::qa::register_component_with_metadata(
        format!("color_picker.palette.reorder_target:{new_index}"),
        "palette_reorder_target",
        response.rect,
        true,
        Some(serde_json::json!({
            "paint_definition_id": payload.definition_id,
            "new_index": new_index,
            "preview": true,
        })),
    );
    if response
        .dnd_release_payload::<PaletteDragPayload>()
        .is_some()
    {
        *intent = Some(PaletteUiIntent::Reorder {
            id: payload.definition_id,
            new_index,
        });
    }
}

fn final_index(
    palette: &ProjectPalette,
    moved: PaintDefinitionId,
    insertion_index: usize,
) -> Option<usize> {
    let old_index = palette
        .ungrouped_order
        .iter()
        .position(|candidate| *candidate == moved)?;
    let after_removal = if old_index < insertion_index {
        insertion_index.saturating_sub(1)
    } else {
        insertion_index
    };
    Some(after_removal.min(palette.ungrouped_order.len().saturating_sub(1)))
}

fn register_swatch_qa(
    ui: &egui::Ui,
    definition: &PaintDefinition,
    response: &Response,
    dragging: bool,
    index: usize,
) {
    if !ui.is_rect_visible(response.rect) {
        return;
    }
    crate::qa::register_component_with_metadata(
        format!("color_picker.palette.swatch:{}", definition.id),
        "palette_swatch",
        response.rect,
        true,
        Some(serde_json::json!({
            "action": "apply_copy",
            "paint_definition_id": definition.id,
            "name": definition.name,
            "index": index,
            "dragging": dragging,
        })),
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use library::model::authoring::{Paint, PaintDefinition};
    use library::model::property::{ColorSpaceRef, ColorValue};
    use std::collections::HashMap;

    fn palette() -> (ProjectPalette, PaintDefinitionId, PaintDefinitionId) {
        let first = PaintDefinitionId::new();
        let second = PaintDefinitionId::new();
        let color = |value| {
            Paint::Solid(ColorValue::new(ColorSpaceRef::srgb(), [value, 0.0, 0.0, 1.0]).unwrap())
        };
        (
            ProjectPalette {
                definitions: HashMap::from([
                    (
                        first,
                        PaintDefinition {
                            id: first,
                            name: "First".into(),
                            paint: color(0.2),
                            tags: Vec::new(),
                        },
                    ),
                    (
                        second,
                        PaintDefinition {
                            id: second,
                            name: "Second".into(),
                            paint: color(0.8),
                            tags: Vec::new(),
                        },
                    ),
                ]),
                groups: Vec::new(),
                ungrouped_order: vec![first, second],
            },
            first,
            second,
        )
    }

    #[test]
    fn insertion_positions_become_final_indices_after_removing_source() {
        let (palette, first, second) = palette();
        assert_eq!(final_index(&palette, first, 2), Some(1));
        assert_eq!(final_index(&palette, second, 0), Some(0));
        assert_eq!(final_index(&palette, PaintDefinitionId::new(), 0), None);
    }

    #[test]
    fn suggested_names_skip_existing_numeric_gaps_without_colliding() {
        let (mut palette, first, second) = palette();
        palette.definitions.get_mut(&first).unwrap().name = "Color 1".into();
        palette.definitions.get_mut(&second).unwrap().name = "Color 3".into();
        assert_eq!(suggested_name(&palette), "Color 2");
    }
}
