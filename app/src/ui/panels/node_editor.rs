use egui_snarl::{
    ui::{PinInfo, SnarlStyle, SnarlViewer},
    InPin, OutPin, Snarl,
};
use eframe::egui::{self, Color32};
use crate::model::node_graph::{MyNodeTemplate, MyValueType};

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
            MyNodeTemplate::AddVector => 2, // v1, v2
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
        let node = snarl.get_node(pin.id.node).map(|n| *n).unwrap_or(MyNodeTemplate::MakeScalar);

        let label = match node {
             MyNodeTemplate::AddScalar | MyNodeTemplate::SubtractScalar | MyNodeTemplate::MultiplyScalar => if pin.id.input == 0 { "A" } else { "B" },
             MyNodeTemplate::MakeVector => if pin.id.input == 0 { "X" } else { "Y" },
             MyNodeTemplate::AddVector => if pin.id.input == 0 { "V1" } else { "V2" },
             MyNodeTemplate::Print => "Val",
             _ => "In",
        };

        ui.label(label);

        let scalar_color = Color32::from_rgb(38, 109, 211);
        let vector_color = Color32::from_rgb(238, 207, 109);

        let color = match node {
            MyNodeTemplate::MakeScalar => scalar_color,
            MyNodeTemplate::AddScalar | MyNodeTemplate::SubtractScalar | MyNodeTemplate::MultiplyScalar => scalar_color,
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
        let node = snarl.get_node(pin.id.node).map(|n| *n).unwrap_or(MyNodeTemplate::MakeScalar);

        let label = match node {
            MyNodeTemplate::MakeVector | MyNodeTemplate::AddVector => "Vec",
            _ => "Out",
        };

        ui.label(label);

        let scalar_color = Color32::from_rgb(38, 109, 211);
        let vector_color = Color32::from_rgb(238, 207, 109);

        let color = match node {
            MyNodeTemplate::MakeScalar => scalar_color,
            MyNodeTemplate::AddScalar | MyNodeTemplate::SubtractScalar | MyNodeTemplate::MultiplyScalar => scalar_color,
            MyNodeTemplate::MakeVector => vector_color,
            MyNodeTemplate::AddVector => vector_color,
            MyNodeTemplate::Print => scalar_color,
        };

        PinInfo::circle().with_fill(color)
    }
}

// ========= 3. The Panel Logic =========

pub fn node_editor_panel(
    ui: &mut egui::Ui,
    snarl: &mut Snarl<MyNodeTemplate>,
) {
    let mut viewer = MySnarlViewer;
    let style = SnarlStyle::default();

    let id = egui::Id::new("my_snarl_editor");
    snarl.show(&mut viewer, &style, id, ui);

    let popup_id = ui.make_persistent_id("node_graph_context_menu");
    if ui.input(|i| i.pointer.secondary_clicked()) && !ui.memory(|m| m.is_popup_open(popup_id)) {
        ui.memory_mut(|m| m.toggle_popup(popup_id));
    }

    if ui.memory(|m| m.is_popup_open(popup_id)) {
        let pos = ui.input(|i| i.pointer.hover_pos()).unwrap_or_default();
        egui::Area::new(popup_id)
            .order(egui::Order::Foreground)
            .fixed_pos(pos)
            .show(ui.ctx(), |ui| {
                egui::Frame::menu(ui.style()).show(ui, |ui| {
                    add_node_menu(ui, snarl, pos);
                });
            });
    }
}

fn add_node_menu(ui: &mut egui::Ui, snarl: &mut Snarl<MyNodeTemplate>, pos: egui::Pos2) {
     ui.label("Add Node");
     ui.separator();
     if ui.button("Scalar").clicked() {
         snarl.insert_node(pos, MyNodeTemplate::MakeScalar);
         ui.close();
     }
     if ui.button("Add Scalar").clicked() {
         snarl.insert_node(pos, MyNodeTemplate::AddScalar);
         ui.close();
     }
     if ui.button("Subtract Scalar").clicked() {
         snarl.insert_node(pos, MyNodeTemplate::SubtractScalar);
         ui.close();
     }
     if ui.button("Multiply Scalar").clicked() {
         snarl.insert_node(pos, MyNodeTemplate::MultiplyScalar);
         ui.close();
     }
     if ui.button("Make Vector").clicked() {
         snarl.insert_node(pos, MyNodeTemplate::MakeVector);
         ui.close();
     }
     if ui.button("Add Vector").clicked() {
         snarl.insert_node(pos, MyNodeTemplate::AddVector);
         ui.close();
     }
     if ui.button("Print").clicked() {
         snarl.insert_node(pos, MyNodeTemplate::Print);
         ui.close();
     }
}
