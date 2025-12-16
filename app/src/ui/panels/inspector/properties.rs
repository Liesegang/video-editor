use egui::Ui;
use library::model::project::property::{PropertyValue, Vec2, Vec3, Vec4};
use library::plugin::{PropertyDefinition, PropertyUiType};
use ordered_float::OrderedFloat;
use library::model::frame::color::Color;
use library::model::frame::draw_type::{CapType, JoinType};

pub struct PropertyRenderContext<'a> {
    pub available_fonts: &'a [String],
    pub in_grid: bool,
}

// Helper function to render generic property rows
// Returns true if any property was changed
pub fn render_property_rows<G, S, C>(
    ui: &mut Ui,
    properties: &[PropertyDefinition],
    get_value: G,
    mut set_value: S,
    mut on_commit: C,
    context: &PropertyRenderContext,
) -> bool
where
    G: Fn(&str) -> Option<PropertyValue>,
    S: FnMut(&str, PropertyValue),
    C: FnMut(&str),
{
    let mut changed = false;
    for prop_def in properties {
        match &prop_def.ui_type {
            PropertyUiType::Float { step, suffix, .. } => {
                ui.label(&prop_def.label);
                let current_val = get_value(&prop_def.name)
                    .and_then(|v| v.get_as::<f64>().or_else(|| v.get_as::<f32>().map(|f| f as f64)))
                    .unwrap_or(prop_def.default_value.get_as::<f64>().unwrap_or(0.0));
                
                let mut val_mut = current_val;
                // Use f32 for drag value as egui uses f32 mostly, but we store f64 optionally
                // Project uses f64.
                let response = ui.add(egui::DragValue::new(&mut val_mut).speed(*step).suffix(suffix));
                if response.changed() {
                    set_value(&prop_def.name, PropertyValue::Number(OrderedFloat(val_mut)));
                    changed = true;
                }
                if response.drag_stopped() || response.lost_focus() {
                    on_commit(&prop_def.name);
                }
                if context.in_grid { ui.end_row(); }
            }
            PropertyUiType::Integer { suffix, .. } => {
                ui.label(&prop_def.label);
                let current_val = get_value(&prop_def.name)
                    .and_then(|v| v.get_as::<i64>())
                    .unwrap_or(prop_def.default_value.get_as::<i64>().unwrap_or(0));
                
                let mut val_mut = current_val;
                let response = ui.add(egui::DragValue::new(&mut val_mut).speed(1.0).suffix(suffix));
                if response.changed() {
                    set_value(&prop_def.name, PropertyValue::Integer(val_mut));
                    changed = true;
                }
                if response.drag_stopped() || response.lost_focus() {
                    on_commit(&prop_def.name);
                }
                if context.in_grid { ui.end_row(); }
            }
            PropertyUiType::Color => {
                ui.label(&prop_def.label);
                let current_val = get_value(&prop_def.name)
                    .and_then(|v| v.get_as::<Color>())
                    .unwrap_or(prop_def.default_value.get_as::<Color>().unwrap_or_default());

                let mut color32 = egui::Color32::from_rgba_premultiplied(
                    current_val.r, current_val.g, current_val.b, current_val.a,
                );
                let response = ui.color_edit_button_srgba(&mut color32);
                if response.changed() {
                    let new_color = Color {
                        r: color32.r(), g: color32.g(), b: color32.b(), a: color32.a(),
                    };
                    set_value(&prop_def.name, PropertyValue::Color(new_color));
                    changed = true;
                }
                if response.drag_stopped() || response.lost_focus() {
                    on_commit(&prop_def.name);
                }
                if context.in_grid { ui.end_row(); }
            }
            PropertyUiType::Bool => {
                let current_val = get_value(&prop_def.name)
                    .and_then(|v| v.get_as::<bool>())
                    .unwrap_or(prop_def.default_value.get_as().unwrap_or(false));
                let mut val_mut = current_val;
                ui.label(&prop_def.label);
                if ui.checkbox(&mut val_mut, "").changed() {
                    set_value(&prop_def.name, PropertyValue::Boolean(val_mut));
                    changed = true;
                    on_commit(&prop_def.name); // Checkbox commit is immediate
                }
                if context.in_grid { ui.end_row(); }
            }
             PropertyUiType::Dropdown { options } => {
                ui.label(&prop_def.label);
                let current_val = get_value(&prop_def.name)
                    .and_then(|v| v.get_as::<String>())
                    .unwrap_or(prop_def.default_value.get_as().unwrap_or_default());
                
                let mut selected = current_val.clone();
                let response = egui::ComboBox::from_id_salt(format!("combo_{}", prop_def.name))
                    .selected_text(&selected)
                    .show_ui(ui, |ui| {
                        for opt in options {
                            ui.selectable_value(&mut selected, opt.clone(), opt.clone());
                        }
                    });
                
                if selected != current_val {
                    set_value(&prop_def.name, PropertyValue::String(selected));
                    changed = true;
                    on_commit(&prop_def.name);
                }
                if context.in_grid { ui.end_row(); }
             }
             PropertyUiType::Font => {
                ui.label(&prop_def.label);
                let current_val = get_value(&prop_def.name)
                    .and_then(|v| v.get_as::<String>())
                    .unwrap_or(prop_def.default_value.get_as().unwrap_or_default());
                
                let mut selected = current_val.clone();
                egui::ComboBox::from_id_salt(format!("combo_font_{}", prop_def.name))
                    .selected_text(&selected)
                    .width(200.0)
                    .show_ui(ui, |ui| {
                        for font in context.available_fonts {
                            ui.selectable_value(&mut selected, font.clone(), font.clone());
                        }
                    });
                
                if selected != current_val {
                    set_value(&prop_def.name, PropertyValue::String(selected));
                    changed = true;
                    on_commit(&prop_def.name);
                }
                if context.in_grid { ui.end_row(); }
             }
             PropertyUiType::Text | PropertyUiType::MultilineText => {
                ui.label(&prop_def.label);
                let current_val = get_value(&prop_def.name)
                    .and_then(|v| v.get_as::<String>())
                    .unwrap_or(prop_def.default_value.get_as().unwrap_or_default());
                let mut text = current_val.clone();
                let response = if matches!(prop_def.ui_type, PropertyUiType::MultilineText) {
                    ui.text_edit_multiline(&mut text)
                } else {
                    ui.text_edit_singleline(&mut text)
                };
                if response.changed() {
                     set_value(&prop_def.name, PropertyValue::String(text));
                     changed = true;
                }
                if response.lost_focus() {
                    on_commit(&prop_def.name);
                }
                if context.in_grid { ui.end_row(); }
             }
             PropertyUiType::Vec2 { suffix } => {
                ui.label(&prop_def.label);
                let current_val = get_value(&prop_def.name)
                    .and_then(|v| v.get_as::<Vec2>())
                    .unwrap_or_else(|| {
                        prop_def.default_value.get_as::<Vec2>()
                            .unwrap_or(Vec2 { x: OrderedFloat(0.0), y: OrderedFloat(0.0) })
                    });
                
                let mut x = current_val.x.into_inner() as f32;
                let mut y = current_val.y.into_inner() as f32;
                
                let mut changed_here = false;
                let mut committed_here = false;

                ui.horizontal(|ui| {
                    ui.label("X");
                    let rx = ui.add(egui::DragValue::new(&mut x).speed(0.1).suffix(suffix));
                    if rx.changed() { changed_here = true; }
                    if rx.drag_stopped() || rx.lost_focus() { committed_here = true; }

                    ui.label("Y");
                    let ry = ui.add(egui::DragValue::new(&mut y).speed(0.1).suffix(suffix));
                    if ry.changed() { changed_here = true; }
                    if ry.drag_stopped() || ry.lost_focus() { committed_here = true; }
                });

                if changed_here {
                    let new_val = Vec2 {
                         x: OrderedFloat(x as f64),
                         y: OrderedFloat(y as f64),
                    };
                    set_value(&prop_def.name, PropertyValue::Vec2(new_val));
                    changed = true;
                }
                if committed_here {
                     on_commit(&prop_def.name);
                }
                if context.in_grid { ui.end_row(); }
             }
             PropertyUiType::Vec3 { suffix } => {
                ui.label(&prop_def.label);
                let current_val = get_value(&prop_def.name)
                    .and_then(|v| v.get_as::<Vec3>())
                    .unwrap_or_else(|| {
                        prop_def.default_value.get_as::<Vec3>()
                            .unwrap_or(Vec3 { x: OrderedFloat(0.0), y: OrderedFloat(0.0), z: OrderedFloat(0.0) })
                    });
                
                let mut x = current_val.x.into_inner() as f32;
                let mut y = current_val.y.into_inner() as f32;
                let mut z = current_val.z.into_inner() as f32;
                
                let mut changed_here = false;
                let mut committed_here = false;

                ui.horizontal(|ui| {
                    ui.label("X");
                    let rx = ui.add(egui::DragValue::new(&mut x).speed(0.1).suffix(suffix));
                    if rx.changed() { changed_here = true; }
                    if rx.drag_stopped() || rx.lost_focus() { committed_here = true; }
                    ui.label("Y");
                    let ry = ui.add(egui::DragValue::new(&mut y).speed(0.1).suffix(suffix));
                    if ry.changed() { changed_here = true; }
                    if ry.drag_stopped() || ry.lost_focus() { committed_here = true; }
                    ui.label("Z");
                    let rz = ui.add(egui::DragValue::new(&mut z).speed(0.1).suffix(suffix));
                    if rz.changed() { changed_here = true; }
                    if rz.drag_stopped() || rz.lost_focus() { committed_here = true; }
                });

                if changed_here {
                    let new_val = Vec3 {
                         x: OrderedFloat(x as f64),
                         y: OrderedFloat(y as f64),
                         z: OrderedFloat(z as f64),
                    };
                    set_value(&prop_def.name, PropertyValue::Vec3(new_val));
                    changed = true;
                }
                 if committed_here {
                     on_commit(&prop_def.name);
                }
                if context.in_grid { ui.end_row(); }
             }
             PropertyUiType::Vec4 { suffix } => {
                ui.label(&prop_def.label);
                let current_val = get_value(&prop_def.name)
                    .and_then(|v| v.get_as::<Vec4>())
                    .unwrap_or_else(|| {
                        prop_def.default_value.get_as::<Vec4>()
                            .unwrap_or(Vec4 { x: OrderedFloat(0.0), y: OrderedFloat(0.0), z: OrderedFloat(0.0), w: OrderedFloat(0.0) })
                    });
                
                let mut x = current_val.x.into_inner() as f32;
                let mut y = current_val.y.into_inner() as f32;
                let mut z = current_val.z.into_inner() as f32;
                let mut w = current_val.w.into_inner() as f32;
                
                let mut changed_here = false;
                let mut committed_here = false;

                ui.horizontal(|ui| {
                    ui.label("X");
                    let rx = ui.add(egui::DragValue::new(&mut x).speed(0.1).suffix(suffix));
                    if rx.changed() { changed_here = true; }
                    if rx.drag_stopped() || rx.lost_focus() { committed_here = true; }
                    ui.label("Y");
                    let ry = ui.add(egui::DragValue::new(&mut y).speed(0.1).suffix(suffix));
                    if ry.changed() { changed_here = true; }
                    if ry.drag_stopped() || ry.lost_focus() { committed_here = true; }
                    ui.label("Z");
                    let rz = ui.add(egui::DragValue::new(&mut z).speed(0.1).suffix(suffix));
                    if rz.changed() { changed_here = true; }
                    if rz.drag_stopped() || rz.lost_focus() { committed_here = true; }
                    ui.label("W");
                    let rw = ui.add(egui::DragValue::new(&mut w).speed(0.1).suffix(suffix));
                    if rw.changed() { changed_here = true; }
                    if rw.drag_stopped() || rw.lost_focus() { committed_here = true; }
                });

                if changed_here {
                    let new_val = Vec4 {
                         x: OrderedFloat(x as f64),
                         y: OrderedFloat(y as f64),
                         z: OrderedFloat(z as f64),
                         w: OrderedFloat(w as f64),
                    };
                    set_value(&prop_def.name, PropertyValue::Vec4(new_val));
                    changed = true;
                }
                if committed_here {
                     on_commit(&prop_def.name);
                }
                if context.in_grid { ui.end_row(); }
             }
             PropertyUiType::Styles => {
                 // Styles are complex and typically rendered via a dedicated manager/UI elsewhere
                 // or skipped here. Caller must handle if they pass Styles def.
                 // We will skip here or print unimplemented.
                 // Ideally Styles should utilize render_property_rows recursively or ReorderableList
                 // which is handled in styles.rs.
                 ui.label(&prop_def.label);
                 ui.label("Styles UI handled separately");
                 ui.end_row();
             }
        }
    }
    changed
}

// Kept for partial backward compatibility but usage is discouraged in favor of render_property_rows
// which has on_commit handling.
#[allow(clippy::too_many_arguments)]
pub fn handle_drag_value_property_legacy(
    ui: &mut Ui,
    value: &mut f32,
    speed: f32,
    suffix: &str,
) -> egui::Response {
    ui.add(egui::DragValue::new(value).speed(speed).suffix(suffix))
}
