use super::action_handler::{ActionContext, PropertyTarget};
use super::properties::{render_inspector_properties_grid, PropertyRenderContext};
use crate::action::HistoryManager;
use crate::state::context::EditorContext;

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

                let current_state = project_service.get_project().read().unwrap().clone();
                history_manager.push_project_state(current_state);

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

                let current_state = project_service.get_project().read().unwrap().clone();
                history_manager.push_project_state(current_state);

                *needs_refresh = true;
                ui.close();
            }
        });
    });

    let mut local_styles = styles.clone();
    let list_id = egui::Id::new(format!("styles_list_{}", selected_entity_id));

    crate::ui::widgets::collection_editor::CollectionEditor::new(
        list_id,
        &mut local_styles,
        |s| egui::Id::new(s.id),
        |ui, visual_index, style, handle, history_manager, project_service, needs_refresh| {
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
                if ctx.handle_actions(pending_actions, PropertyTarget::Style(backend_index), |n| {
                    style_props.get(n).cloned()
                }) {
                    *needs_refresh = true;
                }
            });

            remove_clicked
        },
        |new_styles, project_service| {
            project_service.update_track_clip_styles(selected_entity_id, new_styles)
        },
    )
    .show(ui, history_manager, project_service, needs_refresh);
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
