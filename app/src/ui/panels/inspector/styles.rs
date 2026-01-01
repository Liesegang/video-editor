use super::action_handler::{ActionContext, PropertyTarget};
use super::properties::{render_inspector_properties_grid, PropertyAction, PropertyRenderContext};
use crate::action::HistoryManager;
use crate::state::context::EditorContext;
use crate::ui::widgets::reorderable_list::ReorderableList;
use egui::collapsing_header::CollapsingState;
use egui::Ui;
use library::model::frame::color::Color;
use library::model::project::property::{Property, PropertyMap, PropertyValue};
use library::model::project::property::{PropertyDefinition, PropertyUiType};
use library::model::project::style::StyleInstance;
use library::EditorService as ProjectService;
use ordered_float::OrderedFloat;
use uuid::Uuid;

pub fn render_styles_section(
    ui: &mut Ui,
    project_service: &mut ProjectService,
    history_manager: &mut HistoryManager,
    editor_context: &mut EditorContext,
    selected_entity_id: Uuid,
    current_time: f64,
    fps: f64,
    styles: &Vec<StyleInstance>,
    needs_refresh: &mut bool,
) {
    ui.add_space(10.0);
    ui.heading("Styles");
    ui.separator();

    // Add buttons
    // Add buttons
    ui.horizontal(|ui| {
        use super::properties::render_add_button;
        render_add_button(ui, |ui| {
            if ui.button("Fill").clicked() {
                let mut new_style = StyleInstance::new("fill", PropertyMap::new());
                // Defualts
                new_style.properties.set(
                    "color".to_string(),
                    Property::constant(PropertyValue::Color(Color {
                        r: 255,
                        g: 255,
                        b: 255,
                        a: 255,
                    })),
                );
                new_style.properties.set(
                    "offset".to_string(),
                    Property::constant(PropertyValue::Number(OrderedFloat(0.0))),
                );

                let mut new_styles = styles.clone();
                new_styles.push(new_style);

                project_service
                    .update_track_clip_styles(selected_entity_id, new_styles)
                    .ok();
                *needs_refresh = true;
                ui.close();
            }
            if ui.button("Stroke").clicked() {
                let mut new_style = StyleInstance::new("stroke", PropertyMap::new());
                // Defaults
                new_style.properties.set(
                    "color".to_string(),
                    Property::constant(PropertyValue::Color(Color {
                        r: 0,
                        g: 0,
                        b: 0,
                        a: 255,
                    })),
                );
                new_style.properties.set(
                    "width".to_string(),
                    Property::constant(PropertyValue::Number(OrderedFloat(1.0))),
                );
                new_style.properties.set(
                    "offset".to_string(),
                    Property::constant(PropertyValue::Number(OrderedFloat(0.0))),
                );
                new_style.properties.set(
                    "miter".to_string(),
                    Property::constant(PropertyValue::Number(OrderedFloat(4.0))),
                );

                let mut new_styles = styles.clone();
                new_styles.push(new_style);

                project_service
                    .update_track_clip_styles(selected_entity_id, new_styles)
                    .ok();
                *needs_refresh = true;
                ui.close();
            }
        });
    });

    let mut local_styles = styles.clone();
    let old_styles = local_styles.clone();
    let list_id = ui.make_persistent_id(format!("styles_list_{}", selected_entity_id));
    let mut needs_delete = None;

    ReorderableList::new(list_id, &mut local_styles).show(ui, |ui, visual_index, style, handle| {
        // Because ReorderableList shuffles, we need the ACTUAL index in the original property list for updates?
        // Wait, ReorderableList operates on `local_styles`.
        // The `visual_index` matches `local_styles` index.
        // But we need to call `update_style_property` with the index.
        // As long as we persist the reordering to backend BEFORE or AFTER?
        // If we update property on index 0 but user dragged it to 1, confusion.
        // Ideally we reorder first if changed.
        // But ReorderableList handles drag.

        // For updates, we use the `visual_index` assuming `local_styles` reflects current state.
        // If dragging happened, `local_styles` is already permuted.
        // But the BACKEND is not.
        // If we call `update_style_property(index)` on backend, it targets OLD index.

        // Strategy: Sync order to backend immediately if different.
        // But ReorderableList output happens after show?
        // Actually, we should check `if local_styles != old_styles` at end of function and sync.

        // But inside the loop, we are rendering rows.
        // If we edit a property, we want to call update on backend.
        // The `style` in closure is reference to item in `local_styles`.
        // We need to know its index in BACKEND to update it properly.
        // Or we check `styles` (original) to find matching ID.

        let backend_index = styles
            .iter()
            .position(|s| s.id == style.id)
            .unwrap_or(visual_index);

        let id = ui.make_persistent_id(format!("style_{}", style.id));
        let state = CollapsingState::load_with_default_open(ui.ctx(), id, false);

        let mut remove_clicked = false;
        let header_res = state.show_header(ui, |ui| {
            ui.horizontal(|ui| {
                handle.ui(ui, |ui| {
                    ui.label("::");
                });
                ui.label(egui::RichText::new(style.style_type.clone().to_uppercase()).strong());
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button("X").clicked() {
                        remove_clicked = true;
                    }
                });
            });
        });

        if remove_clicked {
            needs_delete = Some(visual_index);
        }

        header_res.body(|ui| {
            let defs = get_style_definitions(&style.style_type);

            let context = PropertyRenderContext {
                available_fonts: &editor_context.available_fonts,
                in_grid: true,
                current_time,
            };

            let pending_actions = render_inspector_properties_grid(
                ui,
                format!("style_grid_{}", style.id),
                &style.properties,
                &defs,
                project_service,
                &context,
                fps,
            );
            // Process actions outside Grid closure
            let style_props = style.properties.clone();
            let mut ctx = ActionContext::new(
                project_service,
                history_manager,
                selected_entity_id,
                current_time,
            );
            for action in pending_actions {
                match action {
                    PropertyAction::Update(name, val) => {
                        ctx.handle_update(PropertyTarget::Style(backend_index), &name, val, |n| {
                            style_props.get(n).cloned()
                        });
                        *needs_refresh = true;
                    }
                    PropertyAction::Commit => {
                        ctx.handle_commit();
                    }
                    PropertyAction::ToggleKeyframe(name, val) => {
                        ctx.handle_toggle_keyframe(
                            PropertyTarget::Style(backend_index),
                            &name,
                            val,
                            |n| style_props.get(n).cloned(),
                        );
                        *needs_refresh = true;
                    }
                    PropertyAction::SetAttribute(name, key, val) => {
                        ctx.handle_set_attribute(
                            PropertyTarget::Style(backend_index),
                            &name,
                            &key,
                            val,
                        );
                        *needs_refresh = true;
                    }
                }
            }
        });
    });

    if let Some(idx) = needs_delete {
        local_styles.remove(idx);
    }

    // Sync ordering if changed
    // Use IDs to compare
    let ids: Vec<Uuid> = local_styles.iter().map(|s| s.id).collect();
    let old_ids: Vec<Uuid> = old_styles.iter().map(|s| s.id).collect();

    if ids != old_ids {
        project_service
            .update_track_clip_styles(selected_entity_id, local_styles)
            .ok();
        *needs_refresh = true;
    }
}

fn get_style_definitions(style_type: &str) -> Vec<PropertyDefinition> {
    match style_type {
        "fill" => vec![
            PropertyDefinition {
                name: "color".to_string(),
                label: "Color".to_string(),
                ui_type: PropertyUiType::Color,
                default_value: PropertyValue::Color(Color {
                    r: 255,
                    g: 255,
                    b: 255,
                    a: 255,
                }),
                category: "Style".to_string(),
            },
            PropertyDefinition {
                name: "offset".to_string(),
                label: "Offset".to_string(),
                ui_type: PropertyUiType::Float {
                    min: -100.0,
                    max: 100.0,
                    step: 0.1,
                    suffix: "px".to_string(),
                },
                default_value: PropertyValue::Number(OrderedFloat(0.0)),
                category: "Style".to_string(),
            },
        ],
        "stroke" => vec![
            PropertyDefinition {
                name: "color".to_string(),
                label: "Color".to_string(),
                ui_type: PropertyUiType::Color,
                default_value: PropertyValue::Color(Color {
                    r: 0,
                    g: 0,
                    b: 0,
                    a: 255,
                }),
                category: "Style".to_string(),
            },
            PropertyDefinition {
                name: "width".to_string(),
                label: "Width".to_string(),
                ui_type: PropertyUiType::Float {
                    min: 0.0,
                    max: 100.0,
                    step: 0.1,
                    suffix: "px".to_string(),
                },
                default_value: PropertyValue::Number(OrderedFloat(1.0)),
                category: "Style".to_string(),
            },
            PropertyDefinition {
                name: "offset".to_string(),
                label: "Offset".to_string(),
                ui_type: PropertyUiType::Float {
                    min: -100.0,
                    max: 100.0,
                    step: 0.1,
                    suffix: "px".to_string(),
                },
                default_value: PropertyValue::Number(OrderedFloat(0.0)),
                category: "Style".to_string(),
            },
            PropertyDefinition {
                name: "miter".to_string(),
                label: "Miter Limit".to_string(),
                ui_type: PropertyUiType::Float {
                    min: 0.0,
                    max: 100.0,
                    step: 0.1,
                    suffix: "".to_string(),
                },
                default_value: PropertyValue::Number(OrderedFloat(4.0)),
                category: "Style".to_string(),
            },
            PropertyDefinition {
                name: "cap".to_string(),
                label: "Line Cap".to_string(),
                ui_type: PropertyUiType::Dropdown {
                    options: vec![
                        "Butt".to_string(),
                        "Round".to_string(),
                        "Square".to_string(),
                    ],
                },
                default_value: PropertyValue::String("Butt".to_string()),
                category: "Style".to_string(),
            },
            PropertyDefinition {
                name: "join".to_string(),
                label: "Line Join".to_string(),
                ui_type: PropertyUiType::Dropdown {
                    options: vec![
                        "Miter".to_string(),
                        "Round".to_string(),
                        "Bevel".to_string(),
                    ],
                },
                default_value: PropertyValue::String("Miter".to_string()),
                category: "Style".to_string(),
            },
            PropertyDefinition {
                name: "dash_array".to_string(),
                label: "Dash Array".to_string(),
                ui_type: PropertyUiType::Text,
                default_value: PropertyValue::String("".to_string()),
                category: "Style".to_string(),
            },
            PropertyDefinition {
                name: "dash_offset".to_string(),
                label: "Dash Offset".to_string(),
                ui_type: PropertyUiType::Float {
                    min: -100.0,
                    max: 100.0,
                    step: 1.0,
                    suffix: "px".to_string(),
                },
                default_value: PropertyValue::Number(OrderedFloat(0.0)),
                category: "Style".to_string(),
            },
        ],
        _ => vec![],
    }
}
