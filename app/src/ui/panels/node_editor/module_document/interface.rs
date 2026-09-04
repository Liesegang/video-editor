use egui_phosphor::regular as icons;

use super::*;

pub(super) fn port_interface_actions(
    ui: &mut egui::Ui,
    definition: &ModuleDefinition,
    ports: &[PortVisual],
    transform: egui::emath::TSTransform,
    viewport: egui::Rect,
) -> Vec<ModuleEditorAction> {
    let mut actions = Vec::new();
    for port in ports {
        if is_module_output_node(definition, port.id.address.node_id) {
            continue;
        }
        let center = transform * port.center;
        let label_center = match port.id.direction {
            PortDirection::Input => center + egui::vec2(54.0, 0.0),
            PortDirection::Output => center - egui::vec2(54.0, 0.0),
        };
        let rect =
            egui::Rect::from_center_size(label_center, egui::vec2(100.0, 20.0)).intersect(viewport);
        if !rect.is_positive() {
            continue;
        }
        let response = ui.interact(
            rect,
            ui.id().with((
                "module-port-interface-menu",
                port.id.address.node_id,
                port.id.direction,
                port.id.address.port.as_str(),
            )),
            egui::Sense::click(),
        );
        response.context_menu(|ui| {
            show_interface_menu(ui, definition, port, &mut actions);
        });
    }
    actions
}

fn show_interface_menu(
    ui: &mut egui::Ui,
    definition: &ModuleDefinition,
    port: &PortVisual,
    actions: &mut Vec<ModuleEditorAction>,
) {
    ui.strong(&port.label);
    ui.weak(format!("{:?}", port.data_type));
    ui.separator();

    if let Some(parameter) = definition
        .interface
        .parameters
        .iter()
        .find(|entry| entry.target == port.id.address)
    {
        ui.label(format!("Published parameter: {}", parameter.name));
        if ui.button("Unpublish parameter").clicked() {
            actions.push(ModuleEditorAction::EditInterface(
                ModuleInterfaceCommand::UnpublishParameter {
                    parameter_id: parameter.id,
                },
            ));
            ui.close();
        }
        return;
    }
    if let Some(input) = definition
        .interface
        .media_inputs
        .iter()
        .find(|entry| entry.target == port.id.address)
    {
        let role = if input.primary {
            "Primary input"
        } else {
            "Published input"
        };
        ui.label(format!("{role}: {}", input.name));
        if ui.button("Unpublish media input").clicked() {
            actions.push(ModuleEditorAction::EditInterface(
                ModuleInterfaceCommand::UnpublishMediaInput { input_id: input.id },
            ));
            ui.close();
        }
        return;
    }

    match port.id.direction {
        PortDirection::Input
            if matches!(port.data_type, PortDataType::Image | PortDataType::Audio) =>
        {
            let connected = definition
                .graph
                .connections
                .iter()
                .any(|connection| connection.to == port.id.address);
            let primary = primary_media_input_action(definition, port);
            let button = ui.add_enabled(
                primary.is_ok(),
                egui::Button::new(format!("{} Set as primary input", icons::ARROW_RIGHT)),
            );
            let clicked = button.clicked();
            match primary {
                Ok(action) if clicked => {
                    actions.push(ModuleEditorAction::EditInterface(action));
                    ui.close();
                }
                Err(reason) => {
                    button.on_hover_text(reason);
                }
                Ok(_) => {}
            }
            if ui
                .add_enabled(
                    !connected,
                    egui::Button::new(format!("{} Publish as additional input", icons::PLUG)),
                )
                .clicked()
            {
                actions.push(ModuleEditorAction::EditInterface(
                    ModuleInterfaceCommand::PublishMediaInput {
                        name: port.label.clone(),
                        target: port.id.address.clone(),
                        required: false,
                        primary: false,
                    },
                ));
                ui.close();
            }
            if connected {
                ui.weak("Disconnect this port before exposing it.");
            }
        }
        PortDirection::Input => {
            let default_value = module_port_default(definition, &port.id.address);
            if ui
                .add_enabled(
                    default_value.is_some(),
                    egui::Button::new("Publish as parameter"),
                )
                .clicked()
            {
                if let Some(default_value) = default_value.clone() {
                    actions.push(ModuleEditorAction::EditInterface(
                        ModuleInterfaceCommand::PublishParameter {
                            name: port.label.clone(),
                            default_value,
                            target: port.id.address.clone(),
                        },
                    ));
                    ui.close();
                }
            }
            if default_value.is_none() {
                ui.weak("This input has no publishable authored default.");
            }
        }
        PortDirection::Output => {
            ui.weak("Connect this port to a dedicated Output node to render it.");
        }
    }
}

fn primary_media_input_action(
    definition: &ModuleDefinition,
    port: &PortVisual,
) -> Result<ModuleInterfaceCommand, String> {
    if port.id.direction != PortDirection::Input
        || !matches!(port.data_type, PortDataType::Image | PortDataType::Audio)
    {
        return Err("Only media input ports can be the primary input.".to_string());
    }
    if definition
        .graph
        .connections
        .iter()
        .any(|connection| connection.to == port.id.address)
    {
        return Err("Disconnect this port before exposing it.".to_string());
    }
    let already_published = definition
        .interface
        .parameters
        .iter()
        .any(|entry| entry.target == port.id.address)
        || definition
            .interface
            .media_inputs
            .iter()
            .any(|entry| entry.target == port.id.address)
        || definition
            .interface
            .actions
            .iter()
            .any(|entry| entry.target == port.id.address);
    if already_published {
        return Err("This port is already part of the Published Interface.".to_string());
    }
    let Some(primary) = definition
        .interface
        .media_inputs
        .iter()
        .find(|entry| entry.primary)
    else {
        return Ok(ModuleInterfaceCommand::PublishMediaInput {
            name: port.label.clone(),
            target: port.id.address.clone(),
            required: true,
            primary: true,
        });
    };
    if primary.data_type != port.data_type {
        return Err(format!(
            "The primary input is {:?}; this port is {:?}.",
            primary.data_type, port.data_type
        ));
    }
    Ok(ModuleInterfaceCommand::RetargetPrimaryMediaInput {
        input_id: primary.id,
        target: port.id.address.clone(),
    })
}

fn module_port_default(
    definition: &ModuleDefinition,
    address: &ModulePortAddress,
) -> Option<PropertyValue> {
    let node = definition.graph.nodes.get(&address.node_id)?;
    let key = library::plugin::property_name_from_port(&address.port).unwrap_or(&address.port);
    node.properties().get(key)?.value().cloned()
}
