use super::properties::{render_property_rows, PropertyRenderContext};
use crate::action::HistoryManager;
use crate::ui::widgets::reorderable_list::ReorderableList;
use egui::collapsing_header::CollapsingState;
use egui::Ui;
use library::model::frame::color::Color;
use library::model::frame::draw_type::{CapType, DrawStyle, JoinType};
use library::model::frame::entity::StyleConfig;
use library::model::project::property::PropertyValue;
use library::plugin::{EvaluationContext, PropertyDefinition, PropertyUiType};
use library::service::project_service::ProjectService;
use ordered_float::OrderedFloat;
use std::hash::{Hash, Hasher};
use std::ops::{Deref, DerefMut};
use uuid::Uuid;

#[derive(Clone, Debug)]
struct EditableStyle(StyleConfig);

impl Hash for EditableStyle {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.0.id.hash(state);
    }
}

impl PartialEq for EditableStyle {
    fn eq(&self, other: &Self) -> bool {
        self.0.id == other.0.id
    }
}

impl Eq for EditableStyle {}

impl Deref for EditableStyle {
    type Target = StyleConfig;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl DerefMut for EditableStyle {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

// Helper function to render styles list
#[allow(clippy::too_many_arguments)]
pub fn render_styles_property(
    ui: &mut Ui,
    project_service: &mut ProjectService,
    comp_id: Uuid,
    track_id: Uuid,
    selected_entity_id: Uuid,
    property_name: &str,
    def: &library::plugin::PropertyDefinition,
    current_time: f64,
    properties: &library::model::project::property::PropertyMap,
    history_manager: &mut HistoryManager,
    needs_refresh: &mut bool,
) {
    ui.label(&def.label);

    // Evaluate property
    let evaluator_registry = project_service
        .get_plugin_manager()
        .get_property_evaluators();
    let ctx = EvaluationContext {
        property_map: properties,
    };
    let property = properties.get(property_name); // Option<&Property>

    let mut needs_upgrade = false;

    let styles: Vec<StyleConfig> = if let Some(prop) = property {
        let val = evaluator_registry.evaluate(prop, current_time, &ctx);
        if let PropertyValue::Array(arr) = val {
            let mut extracted = Vec::new();
            for v in arr {
                let json: serde_json::Value = (&v).into();
                if let Ok(config) = serde_json::from_value::<StyleConfig>(json.clone()) {
                    extracted.push(config);
                } else if let Ok(style) = serde_json::from_value::<DrawStyle>(json) {
                    // Legacy DrawStyle found, upgrade it
                    extracted.push(StyleConfig {
                        id: Uuid::new_v4(),
                        style,
                    });
                    needs_upgrade = true;
                }
            }
            extracted
        } else {
            vec![]
        }
    } else {
        match def.default_value.clone() {
            PropertyValue::Array(arr) => {
                let mut extracted = Vec::new();
                for v in arr {
                    let json: serde_json::Value = (&v).into();
                    if let Ok(config) = serde_json::from_value::<StyleConfig>(json.clone()) {
                        extracted.push(config);
                    } else if let Ok(style) = serde_json::from_value::<DrawStyle>(json) {
                        // Legacy DrawStyle found in default value, upgrade it
                        extracted.push(StyleConfig {
                            id: Uuid::new_v4(),
                            style,
                        });
                        needs_upgrade = true;
                    }
                }
                extracted
            }
            _ => vec![],
        }
    };

    let old_styles = styles.clone();
    let mut items: Vec<EditableStyle> = styles.into_iter().map(EditableStyle).collect();
    let mut committed = false;
    let list_id = ui.make_persistent_id(format!("styles_{}", property_name));

    // Add Buttons
    ui.horizontal(|ui| {
        if ui.button("+ Fill").clicked() {
            items.push(EditableStyle(StyleConfig {
                id: Uuid::new_v4(),
                style: DrawStyle::Fill {
                    color: Color {
                        r: 255,
                        g: 255,
                        b: 255,
                        a: 255,
                    },
                    offset: 0.0,
                },
            }));
            committed = true;
        }
        if ui.button("+ Stroke").clicked() {
            items.push(EditableStyle(StyleConfig {
                id: Uuid::new_v4(),
                style: DrawStyle::Stroke {
                    color: Color {
                        r: 0,
                        g: 0,
                        b: 0,
                        a: 255,
                    },
                    width: 1.0,
                    offset: 0.0,
                    cap: Default::default(),
                    join: Default::default(),
                    miter: 4.0,
                    dash_array: Vec::new(),
                    dash_offset: 0.0,
                },
            }));
            committed = true;
        }
    });

    let mut needs_delete = None;
    ReorderableList::new(list_id, &mut items).show(ui, |ui, index, item, handle| {
        let id = ui.make_persistent_id(format!("style_{}", item.id));
        let state = CollapsingState::load_with_default_open(ui.ctx(), id, true);

        let item_read = item.clone();
        let (label, defs) = match &item.style {
            DrawStyle::Fill { .. } => (
                "Fill",
                vec![
                    PropertyDefinition {
                        name: "color".to_string(),
                        label: "Color".to_string(),
                        ui_type: PropertyUiType::Color,
                        default_value: PropertyValue::Color(Default::default()),
                        category: "Style".to_string(),
                    },
                    PropertyDefinition {
                        name: "offset".to_string(),
                        label: "Offset".to_string(),
                        ui_type: PropertyUiType::Float {
                            min: -1000.0,
                            max: 1000.0,
                            step: 0.1,
                            suffix: "px".to_string(),
                        },
                        default_value: PropertyValue::Number(OrderedFloat(0.0)),
                        category: "Style".to_string(),
                    },
                ],
            ),
            DrawStyle::Stroke { join, .. } => {
                let mut d = vec![
                    PropertyDefinition {
                        name: "color".to_string(),
                        label: "Color".to_string(),
                        ui_type: PropertyUiType::Color,
                        default_value: PropertyValue::Color(Default::default()),
                        category: "Style".to_string(),
                    },
                    PropertyDefinition {
                        name: "width".to_string(),
                        label: "Width".to_string(),
                        ui_type: PropertyUiType::Float {
                            min: 0.0,
                            max: 1000.0,
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
                            min: -1000.0,
                            max: 1000.0,
                            step: 0.1,
                            suffix: "px".to_string(),
                        },
                        default_value: PropertyValue::Number(OrderedFloat(0.0)),
                        category: "Style".to_string(),
                    },
                    PropertyDefinition {
                        name: "cap".to_string(),
                        label: "Cap".to_string(),
                        ui_type: PropertyUiType::Dropdown {
                            options: vec![
                                "Round".to_string(),
                                "Square".to_string(),
                                "Butt".to_string(),
                            ],
                        },
                        default_value: PropertyValue::String("Round".to_string()),
                        category: "Style".to_string(),
                    },
                    PropertyDefinition {
                        name: "join".to_string(),
                        label: "Join".to_string(),
                        ui_type: PropertyUiType::Dropdown {
                            options: vec![
                                "Round".to_string(),
                                "Bevel".to_string(),
                                "Miter".to_string(),
                            ],
                        },
                        default_value: PropertyValue::String("Round".to_string()),
                        category: "Style".to_string(),
                    },
                ];
                if *join == JoinType::Miter {
                    d.push(PropertyDefinition {
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
                    });
                }
                d.push(PropertyDefinition {
                    name: "dash_array".to_string(),
                    label: "Dash Array".to_string(),
                    ui_type: PropertyUiType::Text, // Space separated
                    default_value: PropertyValue::String("".to_string()),
                    category: "Style".to_string(),
                });
                d.push(PropertyDefinition {
                    name: "dash_offset".to_string(),
                    label: "Dash Offset".to_string(),
                    ui_type: PropertyUiType::Float {
                        min: 0.0,
                        max: 1000.0,
                        step: 1.0,
                        suffix: "px".to_string(),
                    },
                    default_value: PropertyValue::Number(OrderedFloat(0.0)),
                    category: "Style".to_string(),
                });
                ("Stroke", d)
            }
        };

        let mut remove_clicked = false;
        let header_res = state.show_header(ui, |ui| {
            ui.horizontal(|ui| {
                handle.ui(ui, |ui| {
                    ui.label("::");
                });
                ui.label(label);
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button("X").clicked() {
                        remove_clicked = true;
                    }
                });
            });
        });

        if remove_clicked {
            needs_delete = Some(index);
        }

        header_res.body(|ui| {
            egui::Grid::new(format!("style_props_{}", item_read.id))
                .striped(true)
                .show(ui, |ui| {
                    let actions = render_property_rows(
                        ui,
                        &defs,
                        |name| match &item_read.style {
                            DrawStyle::Fill { color, offset } => match name {
                                "color" => Some(PropertyValue::Color(color.clone())),
                                "offset" => Some(PropertyValue::Number(OrderedFloat(*offset))),
                                _ => None,
                            },
                            DrawStyle::Stroke {
                                color,
                                width,
                                offset,
                                cap,
                                join,
                                miter,
                                dash_array,
                                dash_offset,
                            } => match name {
                                "color" => Some(PropertyValue::Color(color.clone())),
                                "width" => Some(PropertyValue::Number(OrderedFloat(*width))),
                                "offset" => Some(PropertyValue::Number(OrderedFloat(*offset))),
                                "cap" => Some(PropertyValue::String(format!("{:?}", cap))),
                                "join" => Some(PropertyValue::String(format!("{:?}", join))),
                                "miter" => Some(PropertyValue::Number(OrderedFloat(*miter))),
                                "dash_offset" => {
                                    Some(PropertyValue::Number(OrderedFloat(*dash_offset)))
                                }
                                "dash_array" => Some(PropertyValue::String(
                                    dash_array
                                        .iter()
                                        .map(|v| v.to_string())
                                        .collect::<Vec<_>>()
                                        .join(" "),
                                )),
                                _ => None,
                            },
                        },
                        |_| None,
                        &PropertyRenderContext {
                            available_fonts: &[],
                            in_grid: true,
                            current_time,
                        },
                    );

                    for action in actions {
                        match action {
                            crate::ui::panels::inspector::properties::PropertyAction::Update(
                                name,
                                val,
                            ) => match &mut item.style {
                                DrawStyle::Fill { color, offset } => match name.as_str() {
                                    "color" => {
                                        if let PropertyValue::Color(c) = val {
                                            *color = c;
                                        }
                                    }
                                    "offset" => {
                                        if let PropertyValue::Number(n) = val {
                                            *offset = n.0;
                                        }
                                    }
                                    _ => {}
                                },
                                DrawStyle::Stroke {
                                    color,
                                    width,
                                    offset,
                                    cap,
                                    join,
                                    miter,
                                    dash_array,
                                    dash_offset,
                                } => match name.as_str() {
                                    "color" => {
                                        if let PropertyValue::Color(c) = val {
                                            *color = c;
                                        }
                                    }
                                    "width" => {
                                        if let PropertyValue::Number(n) = val {
                                            *width = n.0;
                                        }
                                    }
                                    "offset" => {
                                        if let PropertyValue::Number(n) = val {
                                            *offset = n.0;
                                        }
                                    }
                                    "cap" => {
                                        if let PropertyValue::String(s) = val {
                                            *cap = match s.as_str() {
                                                "Square" => CapType::Square,
                                                "Butt" => CapType::Butt,
                                                _ => CapType::Round,
                                            };
                                        }
                                    }
                                    "join" => {
                                        if let PropertyValue::String(s) = val {
                                            *join = match s.as_str() {
                                                "Bevel" => JoinType::Bevel,
                                                "Miter" => JoinType::Miter,
                                                _ => JoinType::Round,
                                            };
                                        }
                                    }
                                    "miter" => {
                                        if let PropertyValue::Number(n) = val {
                                            *miter = n.0;
                                        }
                                    }
                                    "dash_offset" => {
                                        if let PropertyValue::Number(n) = val {
                                            *dash_offset = n.0;
                                        }
                                    }
                                    "dash_array" => {
                                        if let PropertyValue::String(s) = val {
                                            *dash_array = s
                                                .split_whitespace()
                                                .filter_map(|x| x.parse().ok())
                                                .collect();
                                        }
                                    }
                                    _ => {}
                                },
                            },
                            crate::ui::panels::inspector::properties::PropertyAction::Commit => {
                                committed = true;
                            }
                            _ => {}
                        }
                    }
                });
        });
    });

    if let Some(idx) = needs_delete {
        items.remove(idx);
        committed = true;
    }

    let styles: Vec<StyleConfig> = items.into_iter().map(|w| w.0).collect();

    // Persist if changes made committed OR if we performed an upgrade (new UUIDs generated)
    if committed || styles != old_styles || needs_upgrade {
        let json_val = serde_json::to_value(&styles).unwrap();
        // Conversion back to PropertyValue
        let prop_val: PropertyValue = json_val.into();

        project_service
            .update_property_or_keyframe(
                comp_id,
                track_id,
                selected_entity_id,
                property_name,
                current_time,
                prop_val,
                None,
            )
            .ok();
        *needs_refresh = true;
    }

    let ids: Vec<Uuid> = styles.iter().map(|s| s.id).collect();
    let old_ids: Vec<Uuid> = old_styles.iter().map(|s| s.id).collect();
    let reordered = ids != old_ids;

    if committed || reordered || needs_upgrade {
        let current_state = project_service.get_project().read().unwrap().clone();
        history_manager.push_project_state(current_state);
    }
}
