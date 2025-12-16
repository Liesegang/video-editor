use egui::Ui;
use egui_phosphor::fill::DIAMOND as ICON_DIAMOND_FILLED;
use egui_phosphor::regular::DIAMOND as ICON_DIAMOND;
use egui_phosphor::regular::TIMER as ICON_TIMER;
use library::model::frame::color::Color;
use library::model::project::property::{Property, PropertyValue, Vec2, Vec3, Vec4};
use library::plugin::{PropertyDefinition, PropertyUiType};
use ordered_float::OrderedFloat;

pub struct PropertyRenderContext<'a> {
    pub available_fonts: &'a [String],
    pub in_grid: bool,
    pub current_time: f64,
}

pub enum PropertyAction {
    Update(String, PropertyValue),
    Commit(String),
    ToggleKeyframe(String, PropertyValue),
}

// Helper function to render generic property rows
// Returns a list of actions to transform the state
pub fn render_property_rows<G, GP>(
    ui: &mut Ui,
    properties: &[PropertyDefinition],
    get_value: G,
    get_property: GP,
    context: &PropertyRenderContext,
) -> Vec<PropertyAction>
where
    G: Fn(&str) -> Option<PropertyValue>,
    GP: Fn(&str) -> Option<Property>,
{
    let mut actions = Vec::new();

    for prop_def in properties {
        // 1. Render Label Column (with Keyframe Icon)
        ui.horizontal(|ui| {
            let prop_meta = get_property(&prop_def.name);
            if let Some(prop) = prop_meta {
                let is_keyframed = prop.evaluator == "keyframe";
                let is_on_key = if is_keyframed {
                    prop.keyframes()
                        .iter()
                        .any(|k| (k.time.into_inner() - context.current_time).abs() < 0.001)
                } else {
                    false
                };

                let (icon, color) = if is_keyframed {
                    if is_on_key {
                        (
                            ICON_DIAMOND_FILLED,
                            ui.visuals().widgets.active.text_color(),
                        )
                    } else {
                        (ICON_DIAMOND, ui.visuals().text_color())
                    }
                } else {
                    (ICON_TIMER, ui.visuals().text_color().gamma_multiply(0.5))
                };

                let btn =
                    ui.add(egui::Button::new(egui::RichText::new(icon).color(color)).frame(false));
                if btn.clicked() {
                    if let Some(val) = get_value(&prop_def.name) {
                        actions.push(PropertyAction::ToggleKeyframe(prop_def.name.clone(), val));
                    }
                }
                if is_keyframed {
                    btn.on_hover_text("Toggle keyframe at current time");
                } else {
                    btn.on_hover_text("Enable keyframing");
                }
            }
            ui.label(&prop_def.label);
        });

        // 2. Render Input Column
        match &prop_def.ui_type {
            PropertyUiType::Float { step, suffix, .. } => {
                let current_val = get_value(&prop_def.name)
                    .and_then(|v| {
                        v.get_as::<f64>()
                            .or_else(|| v.get_as::<f32>().map(|f| f as f64))
                    })
                    .unwrap_or(prop_def.default_value.get_as::<f64>().unwrap_or(0.0));

                let mut val_mut = current_val;
                let response = ui.add(
                    egui::DragValue::new(&mut val_mut)
                        .speed(*step)
                        .suffix(suffix),
                );
                if response.changed() {
                    actions.push(PropertyAction::Update(
                        prop_def.name.clone(),
                        PropertyValue::Number(OrderedFloat(val_mut)),
                    ));
                }
                if response.middle_clicked() {
                    actions.push(PropertyAction::Update(
                        prop_def.name.clone(),
                        prop_def.default_value.clone(),
                    ));
                    actions.push(PropertyAction::Commit(prop_def.name.clone()));
                }
                if response.drag_stopped() || response.lost_focus() {
                    actions.push(PropertyAction::Commit(prop_def.name.clone()));
                }
                if context.in_grid {
                    ui.end_row();
                }
            }
            PropertyUiType::Integer { suffix, .. } => {
                let current_val = get_value(&prop_def.name)
                    .and_then(|v| v.get_as::<i64>())
                    .unwrap_or(prop_def.default_value.get_as::<i64>().unwrap_or(0));

                let mut val_mut = current_val;
                let response = ui.add(egui::DragValue::new(&mut val_mut).speed(1.0).suffix(suffix));
                if response.changed() {
                    actions.push(PropertyAction::Update(
                        prop_def.name.clone(),
                        PropertyValue::Integer(val_mut),
                    ));
                }
                if response.middle_clicked() {
                    actions.push(PropertyAction::Update(
                        prop_def.name.clone(),
                        prop_def.default_value.clone(),
                    ));
                    actions.push(PropertyAction::Commit(prop_def.name.clone()));
                }
                if response.drag_stopped() || response.lost_focus() {
                    actions.push(PropertyAction::Commit(prop_def.name.clone()));
                }
                if context.in_grid {
                    ui.end_row();
                }
            }
            PropertyUiType::Color => {
                let current_val = get_value(&prop_def.name)
                    .and_then(|v| v.get_as::<Color>())
                    .unwrap_or(prop_def.default_value.get_as::<Color>().unwrap_or_default());

                let mut color32 = egui::Color32::from_rgba_premultiplied(
                    current_val.r,
                    current_val.g,
                    current_val.b,
                    current_val.a,
                );
                let response = ui.color_edit_button_srgba(&mut color32);
                if response.changed() {
                    let new_color = Color {
                        r: color32.r(),
                        g: color32.g(),
                        b: color32.b(),
                        a: color32.a(),
                    };
                    actions.push(PropertyAction::Update(
                        prop_def.name.clone(),
                        PropertyValue::Color(new_color),
                    ));
                }
                if response.middle_clicked() {
                    actions.push(PropertyAction::Update(
                        prop_def.name.clone(),
                        prop_def.default_value.clone(),
                    ));
                    actions.push(PropertyAction::Commit(prop_def.name.clone()));
                }
                if response.drag_stopped() || response.lost_focus() {
                    actions.push(PropertyAction::Commit(prop_def.name.clone()));
                }
                if context.in_grid {
                    ui.end_row();
                }
            }
            PropertyUiType::Bool => {
                let current_val = get_value(&prop_def.name)
                    .and_then(|v| v.get_as::<bool>())
                    .unwrap_or(prop_def.default_value.get_as().unwrap_or(false));
                let mut val_mut = current_val;
                let response = ui.checkbox(&mut val_mut, "");
                if response.changed() {
                    actions.push(PropertyAction::Update(
                        prop_def.name.clone(),
                        PropertyValue::Boolean(val_mut),
                    ));
                    actions.push(PropertyAction::Commit(prop_def.name.clone()));
                }
                if response.middle_clicked() {
                    actions.push(PropertyAction::Update(
                        prop_def.name.clone(),
                        prop_def.default_value.clone(),
                    ));
                    actions.push(PropertyAction::Commit(prop_def.name.clone()));
                }
                if context.in_grid {
                    ui.end_row();
                }
            }
            PropertyUiType::Dropdown { options } => {
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
                    actions.push(PropertyAction::Update(
                        prop_def.name.clone(),
                        PropertyValue::String(selected),
                    ));
                    actions.push(PropertyAction::Commit(prop_def.name.clone()));
                }
                if response.response.middle_clicked() {
                    actions.push(PropertyAction::Update(
                        prop_def.name.clone(),
                        prop_def.default_value.clone(),
                    ));
                    actions.push(PropertyAction::Commit(prop_def.name.clone()));
                }
                if context.in_grid {
                    ui.end_row();
                }
            }
            PropertyUiType::Font => {
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
                    actions.push(PropertyAction::Update(
                        prop_def.name.clone(),
                        PropertyValue::String(selected),
                    ));
                    actions.push(PropertyAction::Commit(prop_def.name.clone()));
                }
                if context.in_grid {
                    ui.end_row();
                }
            }
            PropertyUiType::Text | PropertyUiType::MultilineText => {
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
                    actions.push(PropertyAction::Update(
                        prop_def.name.clone(),
                        PropertyValue::String(text),
                    ));
                }
                if response.middle_clicked() {
                    actions.push(PropertyAction::Update(
                        prop_def.name.clone(),
                        prop_def.default_value.clone(),
                    ));
                    actions.push(PropertyAction::Commit(prop_def.name.clone()));
                }
                if response.lost_focus() {
                    actions.push(PropertyAction::Commit(prop_def.name.clone()));
                }
                if context.in_grid {
                    ui.end_row();
                }
            }
            PropertyUiType::Vec2 { suffix } => {
                let current_val = get_value(&prop_def.name)
                    .and_then(|v| v.get_as::<Vec2>())
                    .unwrap_or_else(|| {
                        prop_def.default_value.get_as::<Vec2>().unwrap_or(Vec2 {
                            x: OrderedFloat(0.0),
                            y: OrderedFloat(0.0),
                        })
                    });

                let mut x = current_val.x.into_inner() as f32;
                let mut y = current_val.y.into_inner() as f32;

                let mut changed_here = false;
                let mut committed_here = false;

                ui.horizontal(|ui| {
                    ui.label("X");
                    let rx = ui.add(egui::DragValue::new(&mut x).speed(0.1).suffix(suffix));
                    if rx.changed() {
                        changed_here = true;
                    }
                    if rx.middle_clicked() {
                        actions.push(PropertyAction::Update(
                            prop_def.name.clone(),
                            prop_def.default_value.clone(),
                        ));
                        actions.push(PropertyAction::Commit(prop_def.name.clone()));
                        changed_here = false; // Override handled above
                    }
                    if rx.drag_stopped() || rx.lost_focus() {
                        committed_here = true;
                    }

                    ui.label("Y");
                    let ry = ui.add(egui::DragValue::new(&mut y).speed(0.1).suffix(suffix));
                    if ry.changed() {
                        changed_here = true;
                    }
                    if ry.middle_clicked() {
                        actions.push(PropertyAction::Update(
                            prop_def.name.clone(),
                            prop_def.default_value.clone(),
                        ));
                        actions.push(PropertyAction::Commit(prop_def.name.clone()));
                        changed_here = false;
                    }
                    if ry.drag_stopped() || ry.lost_focus() {
                        committed_here = true;
                    }
                });

                if changed_here {
                    let new_val = Vec2 {
                        x: OrderedFloat(x as f64),
                        y: OrderedFloat(y as f64),
                    };
                    actions.push(PropertyAction::Update(
                        prop_def.name.clone(),
                        PropertyValue::Vec2(new_val),
                    ));
                }
                if committed_here {
                    actions.push(PropertyAction::Commit(prop_def.name.clone()));
                }
                if context.in_grid {
                    ui.end_row();
                }
            }
            PropertyUiType::Vec3 { suffix } => {
                let current_val = get_value(&prop_def.name)
                    .and_then(|v| v.get_as::<Vec3>())
                    .unwrap_or_else(|| {
                        prop_def.default_value.get_as::<Vec3>().unwrap_or(Vec3 {
                            x: OrderedFloat(0.0),
                            y: OrderedFloat(0.0),
                            z: OrderedFloat(0.0),
                        })
                    });

                let mut x = current_val.x.into_inner() as f32;
                let mut y = current_val.y.into_inner() as f32;
                let mut z = current_val.z.into_inner() as f32;

                let mut changed_here = false;
                let mut committed_here = false;

                ui.horizontal(|ui| {
                    ui.label("X");
                    let rx = ui.add(egui::DragValue::new(&mut x).speed(0.1).suffix(suffix));
                    if rx.changed() {
                        changed_here = true;
                    }
                    if rx.middle_clicked() {
                        actions.push(PropertyAction::Update(
                            prop_def.name.clone(),
                            prop_def.default_value.clone(),
                        ));
                        actions.push(PropertyAction::Commit(prop_def.name.clone()));
                        changed_here = false;
                    }
                    if rx.drag_stopped() || rx.lost_focus() {
                        committed_here = true;
                    }

                    ui.label("Y");
                    let ry = ui.add(egui::DragValue::new(&mut y).speed(0.1).suffix(suffix));
                    if ry.changed() {
                        changed_here = true;
                    }
                    if ry.middle_clicked() {
                        actions.push(PropertyAction::Update(
                            prop_def.name.clone(),
                            prop_def.default_value.clone(),
                        ));
                        actions.push(PropertyAction::Commit(prop_def.name.clone()));
                        changed_here = false;
                    }
                    if ry.drag_stopped() || ry.lost_focus() {
                        committed_here = true;
                    }

                    ui.label("Z");
                    let rz = ui.add(egui::DragValue::new(&mut z).speed(0.1).suffix(suffix));
                    if rz.changed() {
                        changed_here = true;
                    }
                    if rz.middle_clicked() {
                        actions.push(PropertyAction::Update(
                            prop_def.name.clone(),
                            prop_def.default_value.clone(),
                        ));
                        actions.push(PropertyAction::Commit(prop_def.name.clone()));
                        changed_here = false;
                    }
                    if rz.drag_stopped() || rz.lost_focus() {
                        committed_here = true;
                    }
                });

                if changed_here {
                    let new_val = Vec3 {
                        x: OrderedFloat(x as f64),
                        y: OrderedFloat(y as f64),
                        z: OrderedFloat(z as f64),
                    };
                    actions.push(PropertyAction::Update(
                        prop_def.name.clone(),
                        PropertyValue::Vec3(new_val),
                    ));
                }
                if committed_here {
                    actions.push(PropertyAction::Commit(prop_def.name.clone()));
                }
                if context.in_grid {
                    ui.end_row();
                }
            }
            PropertyUiType::Vec4 { suffix } => {
                let current_val = get_value(&prop_def.name)
                    .and_then(|v| v.get_as::<Vec4>())
                    .unwrap_or_else(|| {
                        prop_def.default_value.get_as::<Vec4>().unwrap_or(Vec4 {
                            x: OrderedFloat(0.0),
                            y: OrderedFloat(0.0),
                            z: OrderedFloat(0.0),
                            w: OrderedFloat(0.0),
                        })
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
                    if rx.changed() {
                        changed_here = true;
                    }
                    if rx.middle_clicked() {
                        actions.push(PropertyAction::Update(
                            prop_def.name.clone(),
                            prop_def.default_value.clone(),
                        ));
                        actions.push(PropertyAction::Commit(prop_def.name.clone()));
                        changed_here = false;
                    }
                    if rx.drag_stopped() || rx.lost_focus() {
                        committed_here = true;
                    }
                    ui.label("Y");
                    let ry = ui.add(egui::DragValue::new(&mut y).speed(0.1).suffix(suffix));
                    if ry.changed() {
                        changed_here = true;
                    }
                    if ry.middle_clicked() {
                        actions.push(PropertyAction::Update(
                            prop_def.name.clone(),
                            prop_def.default_value.clone(),
                        ));
                        actions.push(PropertyAction::Commit(prop_def.name.clone()));
                        changed_here = false;
                    }
                    if ry.drag_stopped() || ry.lost_focus() {
                        committed_here = true;
                    }
                    ui.label("Z");
                    let rz = ui.add(egui::DragValue::new(&mut z).speed(0.1).suffix(suffix));
                    if rz.changed() {
                        changed_here = true;
                    }
                    if rz.middle_clicked() {
                        actions.push(PropertyAction::Update(
                            prop_def.name.clone(),
                            prop_def.default_value.clone(),
                        ));
                        actions.push(PropertyAction::Commit(prop_def.name.clone()));
                        changed_here = false;
                    }
                    if rz.drag_stopped() || rz.lost_focus() {
                        committed_here = true;
                    }
                    ui.label("W");
                    let rw = ui.add(egui::DragValue::new(&mut w).speed(0.1).suffix(suffix));
                    if rw.changed() {
                        changed_here = true;
                    }
                    if rw.middle_clicked() {
                        actions.push(PropertyAction::Update(
                            prop_def.name.clone(),
                            prop_def.default_value.clone(),
                        ));
                        actions.push(PropertyAction::Commit(prop_def.name.clone()));
                        changed_here = false;
                    }
                    if rw.drag_stopped() || rw.lost_focus() {
                        committed_here = true;
                    }
                });

                if changed_here {
                    let new_val = Vec4 {
                        x: OrderedFloat(x as f64),
                        y: OrderedFloat(y as f64),
                        z: OrderedFloat(z as f64),
                        w: OrderedFloat(w as f64),
                    };
                    actions.push(PropertyAction::Update(
                        prop_def.name.clone(),
                        PropertyValue::Vec4(new_val),
                    ));
                }
                if committed_here {
                    actions.push(PropertyAction::Commit(prop_def.name.clone()));
                }
                if context.in_grid {
                    ui.end_row();
                }
            }
            PropertyUiType::Styles => {
                ui.label("Styles UI handled separately");
                ui.end_row();
            }
        }
    }
    actions
}

#[allow(clippy::too_many_arguments)]
pub fn handle_drag_value_property_legacy(
    ui: &mut Ui,
    value: &mut f32,
    speed: f32,
    suffix: &str,
) -> egui::Response {
    ui.add(egui::DragValue::new(value).speed(speed).suffix(suffix))
}
