use super::properties::{render_property_rows, PropertyRenderContext};
use crate::action::HistoryManager;
use crate::state::context::EditorContext;
use crate::ui::widgets::reorderable_list::ReorderableList;
use egui::collapsing_header::CollapsingState;
use egui::Ui;
use library::model::frame::color::Color;
use library::model::project::property::{Property, PropertyMap, PropertyValue};
use library::model::project::style::StyleInstance;
use library::plugin::{PropertyDefinition, PropertyUiType};
use library::EditorService as ProjectService;
use ordered_float::OrderedFloat;
use uuid::Uuid;

pub fn render_styles_section(
    ui: &mut Ui,
    project_service: &mut ProjectService,
    history_manager: &mut HistoryManager,
    editor_context: &mut EditorContext,
    comp_id: Uuid,
    track_id: Uuid,
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
    ui.horizontal(|ui| {
        if ui.button("+ Fill").clicked() {
            let mut new_style = StyleInstance::new(
                "fill",
                PropertyMap::new(),
            );
            // Defualts
            new_style.properties.set("color".to_string(), Property::constant(PropertyValue::Color(Color { r: 255, g: 255, b: 255, a: 255 })));
            new_style.properties.set("offset".to_string(), Property::constant(PropertyValue::Number(OrderedFloat(0.0))));
             
            let mut new_styles = styles.clone();
            new_styles.push(new_style);
            
            project_service.update_track_clip_styles(comp_id, track_id, selected_entity_id, new_styles).ok();
            *needs_refresh = true;
        }
        if ui.button("+ Stroke").clicked() {
             let mut new_style = StyleInstance::new(
                "stroke",
                PropertyMap::new(),
            );
            // Defaults
            new_style.properties.set("color".to_string(), Property::constant(PropertyValue::Color(Color { r: 0, g: 0, b: 0, a: 255 })));
            new_style.properties.set("width".to_string(), Property::constant(PropertyValue::Number(OrderedFloat(1.0))));
            new_style.properties.set("offset".to_string(), Property::constant(PropertyValue::Number(OrderedFloat(0.0))));
            new_style.properties.set("miter".to_string(), Property::constant(PropertyValue::Number(OrderedFloat(4.0))));
            
            let mut new_styles = styles.clone();
            new_styles.push(new_style);
            
            project_service.update_track_clip_styles(comp_id, track_id, selected_entity_id, new_styles).ok();
            *needs_refresh = true;
        }
    });
    
    let mut local_styles = styles.clone();
    let old_styles = local_styles.clone();
    let list_id = ui.make_persistent_id(format!("styles_list_{}", selected_entity_id));
    let mut needs_delete = None;
    
     ReorderableList::new(list_id, &mut local_styles)
        .show(ui, |ui, visual_index, style, handle| {
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
            
            let backend_index = styles.iter().position(|s| s.id == style.id).unwrap_or(visual_index);
            
            let id = ui.make_persistent_id(format!("style_{}", style.id));
            let state = CollapsingState::load_with_default_open(ui.ctx(), id, false);
            
            let mut remove_clicked = false;
            let header_res = state.show_header(ui, |ui| {
                 ui.horizontal(|ui| {
                    handle.ui(ui, |ui| { ui.label("::"); });
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
                 
                 egui::Grid::new(format!("style_grid_{}", style.id))
                    .striped(true)
                    .show(ui, |ui| {
                        let actions = render_property_rows(
                            ui,
                            &defs,
                            |name| style.properties.get(name).and_then(|p| Some(project_service.evaluate_property_value(p, &style.properties, current_time, fps))),
                            |name| style.properties.get(name).cloned(),
                            &PropertyRenderContext { available_fonts: &editor_context.available_fonts, in_grid: true, current_time }
                        );
                        
                        for action in actions {
                            match action {
                                crate::ui::panels::inspector::properties::PropertyAction::Update(name, val) => {
                                     project_service.update_style_property_or_keyframe(
                                        comp_id, track_id, selected_entity_id, backend_index, &name, current_time, val, None
                                     ).ok();
                                     *needs_refresh = true;
                                }
                                crate::ui::panels::inspector::properties::PropertyAction::SetAttribute(name, attr_key, attr_val) => {
                                     project_service.set_style_property_attribute(
                                        comp_id, track_id, selected_entity_id, backend_index, &name, &attr_key, attr_val
                                     ).ok();
                                     *needs_refresh = true;
                                }
                                crate::ui::panels::inspector::properties::PropertyAction::Commit => {
                                      let current_state = project_service.get_project().read().unwrap().clone();
                                      history_manager.push_project_state(current_state);
                                }
                                crate::ui::panels::inspector::properties::PropertyAction::ToggleKeyframe(name, val) => {
                                     // Check if keyframe exists using backend style info logic (or redundant logic here)
                                     // Using helper from KeyframeHandler via ProjectService
                                     let mut remove = false;
                                      if let Some(prop) = style.properties.get(&name) {
                                         if prop.evaluator == "keyframe" {
                                              if let Some(idx) = prop.keyframes().iter().position(|k| (k.time.into_inner() - current_time).abs() < 0.001) {
                                                  // Remove keyframe
                                                   project_service.remove_style_keyframe(
                                                       comp_id, track_id, selected_entity_id, backend_index, &name, idx
                                                   ).ok();
                                                   remove = true;
                                              }
                                         }
                                      }
                                      
                                      if !remove {
                                          project_service.add_style_keyframe(
                                              comp_id, track_id, selected_entity_id, backend_index, &name, current_time, val, None
                                          ).ok();
                                      }
                                      *needs_refresh = true;
                                }
                            }
                        }
                    });
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
        project_service.update_track_clip_styles(comp_id, track_id, selected_entity_id, local_styles).ok();
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
                default_value: PropertyValue::Color(Color { r: 255, g: 255, b: 255, a: 255 }),
                category: "Style".to_string(),
            },
            PropertyDefinition {
                name: "offset".to_string(),
                label: "Offset".to_string(),
                ui_type: PropertyUiType::Float { min: -100.0, max: 100.0, step: 0.1, suffix: "px".to_string() },
                default_value: PropertyValue::Number(OrderedFloat(0.0)),
                category: "Style".to_string(),
            },
        ],
        "stroke" => vec![
             PropertyDefinition {
                name: "color".to_string(),
                label: "Color".to_string(),
                ui_type: PropertyUiType::Color,
                default_value: PropertyValue::Color(Color { r: 0, g: 0, b: 0, a: 255 }),
                category: "Style".to_string(),
            },
             PropertyDefinition {
                name: "width".to_string(),
                label: "Width".to_string(),
                ui_type: PropertyUiType::Float { min: 0.0, max: 100.0, step: 0.1, suffix: "px".to_string() },
                default_value: PropertyValue::Number(OrderedFloat(1.0)),
                category: "Style".to_string(),
            },
            PropertyDefinition {
                name: "offset".to_string(),
                label: "Offset".to_string(),
                ui_type: PropertyUiType::Float { min: -100.0, max: 100.0, step: 0.1, suffix: "px".to_string() },
                default_value: PropertyValue::Number(OrderedFloat(0.0)),
                category: "Style".to_string(),
            },
            PropertyDefinition {
                name: "miter".to_string(),
                label: "Miter Limit".to_string(),
                ui_type: PropertyUiType::Float { min: 0.0, max: 100.0, step: 0.1, suffix: "".to_string() },
                default_value: PropertyValue::Number(OrderedFloat(4.0)),
                category: "Style".to_string(),
            },
            // TODO: Caps, Joins, Dash Array
        ],
        _ => vec![],
    }
}
