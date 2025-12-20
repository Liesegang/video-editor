use crate::model::node_graph::{MyNodeTemplate, MyValueType};
use eframe::egui::{self, Color32};
use egui_snarl::{
    ui::{PinInfo, SnarlStyle, SnarlViewer},
    InPin, OutPin, Snarl,
};
use log;

// ========= 2. Define the Viewer =========

pub struct MySnarlViewer;

impl SnarlViewer<MyNodeTemplate> for MySnarlViewer {
    fn title(&mut self, node: &MyNodeTemplate) -> String {
        match node {
            MyNodeTemplate::MakeScalar => "New Scalar".to_owned(),
            MyNodeTemplate::AddScalar => "Add Scalar".to_owned(),
            MyNodeTemplate::SubtractScalar => "Subtract Scalar".to_owned(),
            MyNodeTemplate::MultiplyScalar => "Multiply Scalar".to_owned(),
            MyNodeTemplate::MakeVector => "New Vector".to_owned(),
            MyNodeTemplate::AddVector => "Add Vector".to_owned(),
            MyNodeTemplate::Print => "Print Output".to_owned(),
        }
    }

    fn inputs(&mut self, node: &MyNodeTemplate) -> usize {
        match node {
            MyNodeTemplate::MakeScalar => 0,
            MyNodeTemplate::AddScalar => 2,
            MyNodeTemplate::SubtractScalar => 2,
            MyNodeTemplate::MultiplyScalar => 2,
            MyNodeTemplate::MakeVector => 2, // X, Y
            MyNodeTemplate::AddVector => 2,  // v1, v2
            MyNodeTemplate::Print => 1,
        }
    }

    fn outputs(&mut self, node: &MyNodeTemplate) -> usize {
        match node {
            MyNodeTemplate::MakeScalar => 1,
            MyNodeTemplate::AddScalar => 1,
            MyNodeTemplate::SubtractScalar => 1,
            MyNodeTemplate::MultiplyScalar => 1,
            MyNodeTemplate::MakeVector => 1,
            MyNodeTemplate::AddVector => 1,
            MyNodeTemplate::Print => 0,
        }
    }

    fn show_input(
        &mut self,
        pin: &InPin,
        ui: &mut egui::Ui,
        snarl: &mut Snarl<MyNodeTemplate>,
    ) -> PinInfo {
        let node = snarl
            .get_node(pin.id.node)
            .map(|n| *n)
            .unwrap_or(MyNodeTemplate::MakeScalar);

        let label = match node {
            MyNodeTemplate::AddScalar
            | MyNodeTemplate::SubtractScalar
            | MyNodeTemplate::MultiplyScalar => {
                if pin.id.input == 0 {
                    "A"
                } else {
                    "B"
                }
            }
            MyNodeTemplate::MakeVector => {
                if pin.id.input == 0 {
                    "X"
                } else {
                    "Y"
                }
            }
            MyNodeTemplate::AddVector => {
                if pin.id.input == 0 {
                    "V1"
                } else {
                    "V2"
                }
            }
            MyNodeTemplate::Print => "Val",
            _ => "In",
        };

        ui.label(label);

        let scalar_color = Color32::from_rgb(38, 109, 211);
        let vector_color = Color32::from_rgb(238, 207, 109);

        let color = match node {
            MyNodeTemplate::MakeScalar => scalar_color,
            MyNodeTemplate::AddScalar
            | MyNodeTemplate::SubtractScalar
            | MyNodeTemplate::MultiplyScalar => scalar_color,
            MyNodeTemplate::MakeVector => scalar_color,
            MyNodeTemplate::AddVector => vector_color,
            MyNodeTemplate::Print => scalar_color,
        };

        PinInfo::circle().with_fill(color)
    }

    fn show_output(
        &mut self,
        pin: &OutPin,
        ui: &mut egui::Ui,
        snarl: &mut Snarl<MyNodeTemplate>,
    ) -> PinInfo {
        let node = snarl
            .get_node(pin.id.node)
            .map(|n| *n)
            .unwrap_or(MyNodeTemplate::MakeScalar);

        let label = match node {
            MyNodeTemplate::MakeVector | MyNodeTemplate::AddVector => "Vec",
            _ => "Out",
        };

        ui.label(label);

        let scalar_color = Color32::from_rgb(38, 109, 211);
        let vector_color = Color32::from_rgb(238, 207, 109);

        let color = match node {
            MyNodeTemplate::MakeScalar => scalar_color,
            MyNodeTemplate::AddScalar
            | MyNodeTemplate::SubtractScalar
            | MyNodeTemplate::MultiplyScalar => scalar_color,
            MyNodeTemplate::MakeVector => vector_color,
            MyNodeTemplate::AddVector => vector_color,
            MyNodeTemplate::Print => scalar_color,
        };

        PinInfo::circle().with_fill(color)
    }
}

// ========= 3. The Panel Logic =========

// Add at the top with other imports if not present, or check usages.
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, serde::Deserialize, serde::Serialize)]
struct ContextMenuData {
    x: f32,
    y: f32,
}

pub fn node_editor_panel(ui: &mut egui::Ui, snarl: &mut Snarl<MyNodeTemplate>) {
    let mut viewer = MySnarlViewer;
    let style = SnarlStyle::default();

    let id = egui::Id::new("my_snarl_editor");
    snarl.show(&mut viewer, &style, id, ui);

    let popup_id = ui.make_persistent_id("node_graph_context_menu");

    // 1. Open Logic
    if ui.input(|i| i.pointer.secondary_clicked()) {
        if let Some(pos) = ui.input(|i| i.pointer.hover_pos()) {
            log::info!("Opening context menu at {:?}", pos);
            let data = ContextMenuData { x: pos.x, y: pos.y };
            ui.memory_mut(|m| m.data.insert_persisted(popup_id, data));
        }
    }

    // 2. Render Logic
    let menu_data: Option<ContextMenuData> = ui.memory_mut(|m| m.data.get_persisted(popup_id));

    if let Some(data) = menu_data {
        let pos = egui::Pos2::new(data.x, data.y);
        let mut close_menu = false;

        let response = egui::Area::new(popup_id)
            .order(egui::Order::Foreground)
            .fixed_pos(pos)
            .constrain(true) // Keep on screen
            .show(ui.ctx(), |ui| {
                egui::Frame::menu(ui.style()).show(ui, |ui| {
                    if add_node_menu(ui, snarl) {
                        close_menu = true;
                    }
                    ui.separator();
                    // Optional: remove dedicated close button if click-outside works,
                    // but keeping it doesn't hurt.
                    if ui.button("Close Menu").clicked() {
                        close_menu = true;
                    }
                });
            });

        // 3. Close Logic: Click outside
        if ui.input(|i| i.pointer.any_pressed()) {
            if !response.response.hovered() {
                close_menu = true;
            }
        }

        if close_menu {
            ui.memory_mut(|m| m.data.remove::<ContextMenuData>(popup_id));
        }
    }
}

// Returns true if an action was taken (should close menu)
fn add_node_menu(ui: &mut egui::Ui, snarl: &mut Snarl<MyNodeTemplate>) -> bool {
    ui.label("Add Node");
    ui.separator();
    let mut action_taken = false;
    let graph_pos = egui::Pos2::ZERO; // Default to origin until we can access pan/scale

    if ui.button("Scalar").clicked() {
        snarl.insert_node(graph_pos, MyNodeTemplate::MakeScalar);
        action_taken = true;
    }
    if ui.button("Add Scalar").clicked() {
        snarl.insert_node(graph_pos, MyNodeTemplate::AddScalar);
        action_taken = true;
    }
    if ui.button("Subtract Scalar").clicked() {
        snarl.insert_node(graph_pos, MyNodeTemplate::SubtractScalar);
        action_taken = true;
    }
    if ui.button("Multiply Scalar").clicked() {
        snarl.insert_node(graph_pos, MyNodeTemplate::MultiplyScalar);
        action_taken = true;
    }
    if ui.button("Make Vector").clicked() {
        snarl.insert_node(graph_pos, MyNodeTemplate::MakeVector);
        action_taken = true;
    }
    if ui.button("Add Vector").clicked() {
        snarl.insert_node(graph_pos, MyNodeTemplate::AddVector);
        action_taken = true;
    }
    if ui.button("Print").clicked() {
        snarl.insert_node(graph_pos, MyNodeTemplate::Print);
        action_taken = true;
    }
    action_taken
}
