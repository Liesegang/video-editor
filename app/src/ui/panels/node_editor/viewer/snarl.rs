use super::ProjectNodeViewer;
use crate::state::context_types::NodeEditorEditableWire;
use crate::ui::panels::node_editor::*;
use eframe::egui::{self, Color32};
use egui_phosphor::regular as icons;
use egui_snarl::{
    ui::{BackgroundPattern, NodeLayout, SnarlPin, SnarlStyle, SnarlViewer},
    InPin, OutPin, Snarl,
};
use library::model::project::{PortAddress, PortDataType, PortDirection, PortOwner};
use library::model::property::PropertyValue;
use library::model::{GeneratorContent, NodeContent};
use library::plugin::property_name_from_port;
use std::sync::Arc;

impl SnarlViewer<GraphItem> for ProjectNodeViewer<'_> {
    fn node_layout(
        &mut self,
        _default: NodeLayout,
        _node_id: egui_snarl::NodeId,
        _inputs: &[InPin],
        _outputs: &[OutPin],
        _snarl: &Snarl<GraphItem>,
    ) -> NodeLayout {
        // Coil keeps inputs on the left and outputs on the right. Each side is
        // a top-down list, so pins remain one-per-row without turning the data
        // flow into a top-to-bottom graph. Width is bounded by the label/body
        // helpers below instead of changing pin sides.
        NodeLayout::coil()
            .with_min_pin_row_height(PORT_ROW_HEIGHT)
            .with_equal_pin_rows()
    }

    fn node_frame(
        &mut self,
        default: egui::Frame,
        node_id: egui_snarl::NodeId,
        _inputs: &[InPin],
        _outputs: &[OutPin],
        snarl: &Snarl<GraphItem>,
    ) -> egui::Frame {
        let Some(item) = snarl.get_node(node_id).copied() else {
            return default;
        };
        match item {
            GraphItem::Container(_) | GraphItem::PortAnchor { .. } => egui::Frame::NONE,
            GraphItem::Node(project_node_id) => {
                let palette = node_palette(self.project, project_node_id);
                let inactive = graph_item_inactive(self.project, item, self.current_time);
                let fill = if inactive {
                    palette.body.gamma_multiply(0.42)
                } else {
                    palette.body
                };
                let stroke = if inactive {
                    palette.accent.gamma_multiply(0.48)
                } else {
                    palette.accent
                };
                let stroke_width = if node_editor_details_visible(self.to_global.scaling) {
                    1.25
                } else {
                    screen_stroke_in_graph_units(1.1, self.to_global.scaling)
                };
                egui::Frame::new()
                    .inner_margin(egui::Margin::symmetric(9, 8))
                    .corner_radius(10)
                    .fill(fill)
                    .stroke(egui::Stroke::new(stroke_width, stroke))
            }
        }
    }

    fn header_frame(
        &mut self,
        default: egui::Frame,
        node_id: egui_snarl::NodeId,
        _inputs: &[InPin],
        _outputs: &[OutPin],
        snarl: &Snarl<GraphItem>,
    ) -> egui::Frame {
        let Some(item) = snarl.get_node(node_id).copied() else {
            return default;
        };
        match item {
            GraphItem::Container(_) | GraphItem::PortAnchor { .. } => egui::Frame::NONE,
            GraphItem::Node(project_node_id) => {
                let palette = node_palette(self.project, project_node_id);
                let fill = if graph_item_inactive(self.project, item, self.current_time) {
                    palette.header.gamma_multiply(0.42)
                } else {
                    palette.header
                };
                egui::Frame::new()
                    .inner_margin(egui::Margin::symmetric(9, 7))
                    .corner_radius(egui::CornerRadius {
                        nw: 9,
                        ne: 9,
                        sw: 3,
                        se: 3,
                    })
                    .fill(fill)
            }
        }
    }

    fn show_header(
        &mut self,
        node_id: egui_snarl::NodeId,
        _inputs: &[InPin],
        _outputs: &[OutPin],
        ui: &mut egui::Ui,
        snarl: &mut Snarl<GraphItem>,
    ) {
        let Some(item) = snarl.get_node(node_id).copied() else {
            return;
        };

        match item {
            GraphItem::Node(project_node_id) => {
                let palette = node_palette(self.project, project_node_id);
                let inactive = graph_item_inactive(
                    self.project,
                    GraphItem::Node(project_node_id),
                    self.current_time,
                );
                ui.set_min_width(NODE_HEADER_WIDTH);
                let response = if node_editor_details_visible(self.to_global.scaling) {
                    ui.horizontal(|ui| {
                        let icon = node_icon(self.project, project_node_id);
                        non_selectable_label(
                            ui,
                            egui::RichText::new(icon.glyph)
                                .color(palette.accent)
                                .strong(),
                        )
                        .on_hover_text(icon.label);
                        bounded_strong_non_selectable_label(
                            ui,
                            node_title(self.project, project_node_id),
                            NODE_HEADER_WIDTH - 48.0,
                        );
                        let (status, status_label) = if inactive {
                            (icons::CIRCLE_DASHED, "Node has no output")
                        } else {
                            (icons::CHECK_CIRCLE, "Node is active")
                        };
                        non_selectable_label(ui, egui::RichText::new(status).color(palette.accent))
                            .on_hover_text(status_label);
                    })
                    .response
                } else {
                    ui.allocate_response(
                        egui::vec2(NODE_HEADER_WIDTH, PORT_ROW_HEIGHT),
                        egui::Sense::hover(),
                    )
                };
                let response = graph_item_inactive_reason(
                    self.project,
                    GraphItem::Node(project_node_id),
                    self.current_time,
                )
                .map_or(response.clone(), |reason| {
                    response.on_hover_text(reason.tooltip())
                });
                let unclipped_header_rect = *self.to_global * response.rect;
                let header_rect = clipped_qa_rect(unclipped_header_rect, *self.canvas_clip);
                let coordinate_clicked = ui.input(|input| {
                    input.pointer.primary_clicked()
                        && input
                            .pointer
                            .interact_pos()
                            .is_some_and(|position| header_rect.contains(position))
                });
                let coordinate_double_clicked = ui.input(|input| {
                    input
                        .pointer
                        .button_double_clicked(egui::PointerButton::Primary)
                        && input
                            .pointer
                            .interact_pos()
                            .is_some_and(|position| header_rect.contains(position))
                });
                let component_id = format!("node_editor.node_header:{project_node_id}");
                #[cfg(test)]
                capture_test_rect(&component_id, header_rect);
                crate::qa::register_component_with_metadata(
                    component_id,
                    "node_header",
                    header_rect,
                    response.enabled(),
                    Some(serde_json::json!({
                        "node_id": project_node_id,
                        "hovered": response.hovered(),
                        "unclipped_rect": qa_rect_metadata(unclipped_header_rect),
                        "visible_in_canvas": header_rect.is_positive(),
                    })),
                );
                if coordinate_clicked {
                    *self.pending_selection = Some(PortOwner::Node(project_node_id));
                }
                if coordinate_double_clicked {
                    if let Some(node) = self.project.get_node(project_node_id) {
                        if let NodeContent::Reference(reference) = node.content() {
                            *self.pending_navigation = Some(reference.target_id);
                        }
                    }
                }
            }
            GraphItem::Container(owner) => {
                let collapsed = container_collapsed(self.project, owner).unwrap_or(false);
                let header_width = container_name_and_size(self.project, owner)
                    .map_or(240.0, |(_, size)| (size[0] - 28.0).max(240.0));
                ui.set_min_width(header_width);
                let response = if node_editor_details_visible(self.to_global.scaling) {
                    ui.horizontal(|ui| {
                        let (toggle_icon, toggle_label, toggle_action) = if collapsed {
                            (icons::CARET_RIGHT, "Expand container", "expand")
                        } else {
                            (icons::CARET_DOWN, "Collapse container", "collapse")
                        };
                        let toggle = ui.small_button(toggle_icon).on_hover_text(toggle_label);
                        let unclipped_toggle_rect = *self.to_global * toggle.rect;
                        let toggle_rect = clipped_qa_rect(unclipped_toggle_rect, *self.canvas_clip);
                        let coordinate_clicked = ui.input(|input| {
                            input.pointer.primary_clicked()
                                && input
                                    .pointer
                                    .interact_pos()
                                    .is_some_and(|position| toggle_rect.contains(position))
                        });
                        let toggle_id =
                            format!("node_editor.container_toggle.{}", qa_container_key(owner));
                        #[cfg(test)]
                        capture_test_rect(&toggle_id, toggle_rect);
                        crate::qa::register_component_with_metadata(
                            toggle_id,
                            "node_container_toggle",
                            toggle_rect,
                            toggle.enabled(),
                            Some(serde_json::json!({
                                "owner": qa_container_key(owner),
                                "collapsed": collapsed,
                                "action": toggle_action,
                                "icon": toggle_label,
                                "unclipped_rect": qa_rect_metadata(unclipped_toggle_rect),
                                "visible_in_canvas": toggle_rect.is_positive(),
                            })),
                        );
                        if toggle.clicked() || coordinate_clicked {
                            self.edits
                                .push(QueuedNodeEdit::Atomic(NodeEdit::ToggleContainer { owner }));
                        }
                        let icon = container_icon(owner);
                        non_selectable_label(ui, egui::RichText::new(icon.glyph).strong())
                            .on_hover_text(icon.label);
                        strong_non_selectable_label(ui, container_title(self.project, owner));
                    })
                    .response
                } else {
                    ui.allocate_response(
                        egui::vec2(header_width, PORT_ROW_HEIGHT),
                        egui::Sense::hover(),
                    )
                };
                let response = if container_inactive(self.project, owner, self.current_time) {
                    response
                        .on_hover_text("No output (outside Clip range). The Clip remains editable.")
                } else {
                    response
                };
                let unclipped_header_rect = *self.to_global * response.rect;
                let header_rect = clipped_qa_rect(unclipped_header_rect, *self.canvas_clip);
                let component_id =
                    format!("node_editor.container_header.{}", qa_container_key(owner));
                #[cfg(test)]
                capture_test_rect(&component_id, header_rect);
                crate::qa::register_component_with_metadata(
                    component_id,
                    "node_container_header",
                    header_rect,
                    response.enabled(),
                    Some(serde_json::json!({
                        "owner": qa_container_key(owner),
                        "unclipped_rect": qa_rect_metadata(unclipped_header_rect),
                        "visible_in_canvas": header_rect.is_positive(),
                    })),
                );
                if ui.input(|input| {
                    input.pointer.primary_clicked()
                        && input
                            .pointer
                            .interact_pos()
                            .is_some_and(|position| header_rect.contains(position))
                }) {
                    *self.pending_selection = Some(owner);
                }
            }
            GraphItem::PortAnchor { .. } => {
                ui.allocate_space(egui::Vec2::ZERO);
            }
        }
    }

    fn title(&mut self, item: &GraphItem) -> String {
        graph_item_title(self.project, *item)
    }

    fn inputs(&mut self, item: &GraphItem) -> usize {
        input_definitions(self.project, *item).len()
    }

    fn outputs(&mut self, item: &GraphItem) -> usize {
        output_definitions(self.project, *item).len()
    }

    fn show_input(
        &mut self,
        pin: &InPin,
        ui: &mut egui::Ui,
        snarl: &mut Snarl<GraphItem>,
    ) -> impl SnarlPin + 'static {
        let item = snarl.get_node(pin.id.node).copied();
        let definition = snarl
            .get_node(pin.id.node)
            .and_then(|item| {
                input_definitions(self.project, *item)
                    .get(pin.id.input)
                    .cloned()
            })
            .unwrap_or_else(|| PinDefinition {
                key: "missing".to_string(),
                name: "Input".to_string(),
                data_type: PortDataType::Any,
            });
        let connected = !pin.remotes.is_empty();
        if let Some(GraphItem::Node(node_id)) = item {
            if node_editor_details_visible(self.to_global.scaling) {
                let property_key = property_name_from_port(&definition.key)
                    .unwrap_or(&definition.key)
                    .to_string();
                let property_definition = self.project.get_node(node_id).and_then(|node| {
                    node_property_definition(self.plugin_manager, node, &property_key)
                });
                self.show_node_input_row(
                    ui,
                    node_id,
                    &definition,
                    &property_key,
                    property_definition.as_ref(),
                    connected,
                );
            } else {
                ui.allocate_space(egui::vec2(PORT_LABEL_WIDTH + 80.0, PORT_ROW_HEIGHT));
            }
        } else {
            ui.allocate_space(egui::vec2(0.0, PORT_ROW_HEIGHT));
        }
        let address = item
            .and_then(graph_item_owner)
            .map(|owner| PortAddress::new(owner, definition.key.clone()));
        QaPin {
            info: pin_info(definition.data_type, connected),
            component_id: qa_port_id(self.project, item, "input", &definition.key),
            to_global: *self.to_global,
            graph_center: embedded_pin_center(
                self.containers,
                item,
                PortDirection::Input,
                pin.id.input,
            ),
            address,
            direction: PortDirection::Input,
            connected,
            canvas_clip: *self.canvas_clip,
            rendered_ports: Arc::clone(&self.rendered_ports),
        }
    }

    fn show_output(
        &mut self,
        pin: &OutPin,
        ui: &mut egui::Ui,
        snarl: &mut Snarl<GraphItem>,
    ) -> impl SnarlPin + 'static {
        let item = snarl.get_node(pin.id.node).copied();
        let definition = snarl
            .get_node(pin.id.node)
            .and_then(|item| {
                output_definitions(self.project, *item)
                    .get(pin.id.output)
                    .cloned()
            })
            .unwrap_or_else(|| PinDefinition {
                key: "missing".to_string(),
                name: "Output".to_string(),
                data_type: PortDataType::Any,
            });
        if matches!(item, Some(GraphItem::Node(_))) {
            if node_editor_details_visible(self.to_global.scaling) {
                bounded_non_selectable_label(
                    ui,
                    definition.name.clone(),
                    port_label_width(item),
                    egui::Align::RIGHT,
                );
            } else {
                ui.allocate_space(egui::vec2(PORT_LABEL_WIDTH, PORT_ROW_HEIGHT));
            }
        } else {
            ui.allocate_space(egui::vec2(0.0, PORT_ROW_HEIGHT));
        }
        let address = item
            .and_then(graph_item_owner)
            .map(|owner| PortAddress::new(owner, definition.key.clone()));
        let connected = !pin.remotes.is_empty();
        QaPin {
            info: pin_info(definition.data_type, connected),
            component_id: qa_port_id(self.project, item, "output", &definition.key),
            to_global: *self.to_global,
            graph_center: embedded_pin_center(
                self.containers,
                item,
                PortDirection::Output,
                pin.id.output,
            ),
            address,
            direction: PortDirection::Output,
            connected,
            canvas_clip: *self.canvas_clip,
            rendered_ports: Arc::clone(&self.rendered_ports),
        }
    }

    fn has_body(&mut self, item: &GraphItem) -> bool {
        matches!(
            item,
            GraphItem::Node(node_id)
                if self
                    .project
                    .get_node(*node_id)
                    .is_some_and(|node| matches!(node.content(), NodeContent::Merge))
        )
    }

    fn show_body(
        &mut self,
        node_id: egui_snarl::NodeId,
        _inputs: &[InPin],
        _outputs: &[OutPin],
        ui: &mut egui::Ui,
        snarl: &mut Snarl<GraphItem>,
    ) {
        let Some(item) = snarl.get_node(node_id).copied() else {
            return;
        };
        if let GraphItem::Container(owner) = item {
            ui.vertical(|ui| {
                ui.set_width(258.0);
                self.show_container_body(owner, ui);
            });
            return;
        }
        let GraphItem::Node(project_node_id) = item else {
            return;
        };
        if self
            .project
            .get_node(project_node_id)
            .is_some_and(|node| matches!(node.content(), NodeContent::Merge))
        {
            ui.vertical(|ui| {
                ui.set_width(MERGE_BODY_WIDTH);
                self.show_merge_layers(project_node_id, ui);
            });
            return;
        }
        ui.vertical(|ui| {
            ui.set_width(NODE_BODY_WIDTH);
            let Some(node) = self.project.get_node(project_node_id) else {
                return;
            };

            let mut name = node.name.clone();
            ui.horizontal(|ui| {
                property_label(ui, "Name");
                let response = ui.add_sized(
                    [INLINE_CONTROL_WIDTH, PORT_ROW_HEIGHT],
                    egui::TextEdit::singleline(&mut name),
                );
                let finished = continuous_response_finished(ui, &response);
                let edit = response.changed().then_some(NodeEdit::Rename {
                    node_id: project_node_id,
                    name,
                });
                self.queue_continuous_edit(
                    PortOwner::Node(project_node_id),
                    "$name",
                    edit,
                    finished,
                );
            });

            match node.content() {
                NodeContent::Generator(GeneratorContent::Text) => {
                    self.edit_string_property(ui, project_node_id, node, "text", "Text", "");
                    self.edit_string_property(
                        ui,
                        project_node_id,
                        node,
                        "font_family",
                        "Font",
                        library::editor::project_service::DEFAULT_TEXT_FONT,
                    );
                }
                NodeContent::Generator(GeneratorContent::Shape) => {
                    self.edit_string_property(
                        ui,
                        project_node_id,
                        node,
                        "path",
                        "Path",
                        library::editor::project_service::DEFAULT_SHAPE_PATH,
                    );
                }
                NodeContent::Generator(GeneratorContent::SkSL) => {
                    self.edit_string_property(
                        ui,
                        project_node_id,
                        node,
                        "shader",
                        "Shader",
                        library::editor::project_service::DEFAULT_SKSL_SHADER,
                    );
                }
                NodeContent::Generator(GeneratorContent::Solid) => {
                    let property_time =
                        node_property_time(self.project, project_node_id, self.current_time);
                    let evaluated = node.properties().get("color").map(|property| {
                        evaluate_node_property(
                            self.project,
                            self.plugin_manager,
                            project_node_id,
                            property,
                            property_time,
                        )
                    });
                    let color = evaluated
                        .as_ref()
                        .and_then(|evaluated| evaluated.value())
                        .and_then(|value| value.get_as::<library::model::frame::color::Color>())
                        .unwrap_or(library::model::frame::color::Color {
                            r: 255,
                            g: 255,
                            b: 255,
                            a: 255,
                        });
                    let mut edited =
                        Color32::from_rgba_unmultiplied(color.r, color.g, color.b, color.a);
                    ui.horizontal(|ui| {
                        property_label(ui, "Color");
                        if let Some(issue) =
                            evaluated.as_ref().and_then(|evaluated| evaluated.issue())
                        {
                            render_node_property_issue(ui, project_node_id, "color", issue);
                        }
                        let (response, popup_closed) =
                            continuous_color_edit_button(ui, &mut edited);
                        let finished = popup_closed || continuous_response_finished(ui, &response);
                        let edit = response.changed().then(|| NodeEdit::SetProperty {
                            owner: PortOwner::Node(project_node_id),
                            key: "color".into(),
                            time: property_time,
                            value: PropertyValue::Color(library::model::frame::color::Color {
                                r: edited.r(),
                                g: edited.g(),
                                b: edited.b(),
                                a: edited.a(),
                            }),
                        });
                        self.queue_continuous_edit(
                            PortOwner::Node(project_node_id),
                            "color",
                            edit,
                            finished,
                        );
                    });
                }
                NodeContent::PluginOperation(operation) => {
                    ui.horizontal(|ui| {
                        property_label(ui, "Category");
                        bounded_non_selectable_label(
                            ui,
                            &operation.category,
                            INLINE_CONTROL_WIDTH,
                            egui::Align::LEFT,
                        );
                    });
                    ui.horizontal(|ui| {
                        property_label(ui, "Component");
                        bounded_non_selectable_label(
                            ui,
                            &operation.component_id,
                            INLINE_CONTROL_WIDTH,
                            egui::Align::LEFT,
                        );
                    });
                    ui.horizontal(|ui| {
                        property_label(ui, "Operation");
                        bounded_non_selectable_label(
                            ui,
                            &operation.operation,
                            INLINE_CONTROL_WIDTH,
                            egui::Align::LEFT,
                        );
                    });
                }
                NodeContent::Value(value) => {
                    ui.horizontal(|ui| {
                        property_label(ui, "Category");
                        bounded_non_selectable_label(
                            ui,
                            VALUE_NODE_CATEGORY_LABEL,
                            INLINE_CONTROL_WIDTH,
                            egui::Align::LEFT,
                        );
                    });
                    ui.horizontal(|ui| {
                        property_label(ui, "Operation");
                        bounded_non_selectable_label(
                            ui,
                            value_operation_label(*value),
                            INLINE_CONTROL_WIDTH,
                            egui::Align::LEFT,
                        );
                    });
                }
                NodeContent::Media(_) | NodeContent::Reference(_) | NodeContent::Merge => {}
            }
        });
    }

    fn has_node_menu(&mut self, item: &GraphItem) -> bool {
        matches!(
            item,
            GraphItem::Node(_)
                | GraphItem::Container(PortOwner::Track(_))
                | GraphItem::Container(PortOwner::Clip(_))
        )
    }

    fn show_node_menu(
        &mut self,
        node_id: egui_snarl::NodeId,
        _inputs: &[InPin],
        _outputs: &[OutPin],
        ui: &mut egui::Ui,
        snarl: &mut Snarl<GraphItem>,
    ) {
        let Some(item) = snarl.get_node(node_id).copied() else {
            return;
        };
        let delete_target = match item {
            GraphItem::Node(node_id) => Some((PortOwner::Node(node_id), "Delete Node")),
            GraphItem::Container(PortOwner::Track(track_id)) => {
                Some((PortOwner::Track(track_id), "Delete Track"))
            }
            GraphItem::Container(PortOwner::Clip(clip_id)) => {
                Some((PortOwner::Clip(clip_id), "Delete Clip"))
            }
            _ => None,
        };
        if let GraphItem::Node(project_node_id) = item {
            if let Some(node) = self.project.get_node(project_node_id) {
                let enabled = !node.enabled;
                let label = if enabled {
                    "Enable Node"
                } else {
                    "Disable Node"
                };
                let response = ui.button(label);
                crate::qa::register_component_with_metadata(
                    format!("node_editor.menu.toggle_enabled.node:{project_node_id}"),
                    "node_editor_menu_item",
                    response.rect,
                    response.enabled(),
                    Some(serde_json::json!({
                        "action": if enabled { "enable" } else { "disable" },
                        "owner": qa_container_key(PortOwner::Node(project_node_id)),
                        "enabled": enabled,
                    })),
                );
                if response.clicked() {
                    self.edits
                        .push(QueuedNodeEdit::Atomic(NodeEdit::SetEnabled {
                            node_id: project_node_id,
                            enabled,
                        }));
                    ui.close();
                    return;
                }
            }
        }
        if let Some((owner, label)) = delete_target {
            let response = ui.button(label);
            crate::qa::register_component_with_metadata(
                format!("node_editor.menu.delete.{}", qa_container_key(owner)),
                "node_editor_menu_item",
                response.rect,
                response.enabled(),
                Some(serde_json::json!({
                    "action": "delete",
                    "owner": qa_container_key(owner),
                })),
            );
            if response.clicked() {
                self.edits
                    .push(QueuedNodeEdit::Atomic(NodeEdit::Delete { owner }));
                ui.close();
            }
        }
    }

    fn final_node_rect(
        &mut self,
        node_id: egui_snarl::NodeId,
        rect: egui::Rect,
        _ui: &mut egui::Ui,
        snarl: &mut Snarl<GraphItem>,
    ) {
        let Some(item) = snarl.get_node(node_id).copied() else {
            return;
        };
        let graph_rect = rect;
        let unclipped_rect = *self.to_global * graph_rect;
        let rect = clipped_qa_rect(unclipped_rect, *self.canvas_clip);
        match item {
            GraphItem::Node(id) => {
                self.context_menu_exclusion_rects.push(graph_rect);
                if let Ok(mut node_rects) = self.rendered_node_rects.lock() {
                    node_rects.insert(id, graph_rect);
                }
                #[cfg(test)]
                capture_test_rect(&format!("node_editor.node:{id}"), rect);
                crate::qa::register_component_with_metadata(
                    format!("node_editor.node:{id}"),
                    "node",
                    rect,
                    true,
                    Some(serde_json::json!({
                        "node_id": id,
                        "inactive": graph_item_inactive(
                            self.project,
                            GraphItem::Node(id),
                            self.current_time,
                        ),
                        "inactive_reason": graph_item_inactive_reason(
                            self.project,
                            GraphItem::Node(id),
                            self.current_time,
                        ).map(GraphItemInactiveReason::as_str),
                        "unclipped_rect": qa_rect_metadata(unclipped_rect),
                        "visible_in_canvas": rect.is_positive(),
                    })),
                )
            }
            GraphItem::Container(_) => {
                // Only the integrated header/control card is a Snarl item;
                // the separately painted container body remains available to
                // the global Create menu and Node placement.
                self.context_menu_exclusion_rects.push(graph_rect);
            }
            GraphItem::PortAnchor { owner, kind } => {
                // Transparent Snarl anchor frames can be wider than the
                // sockets they carry. Exclude only each projected socket hit
                // from the Create menu so the rest of the edge rail and the
                // entire body remain usable.
                let Some(container) = self
                    .containers
                    .iter()
                    .find(|container| container.owner == owner)
                else {
                    return;
                };
                let pin_count = input_definitions(self.project, item)
                    .len()
                    .max(output_definitions(self.project, item).len());
                for index in 0..pin_count {
                    let socket = egui::Rect::from_center_size(
                        container.embedded_port_center(kind, index),
                        egui::Vec2::splat(PORT_SOCKET_SIZE),
                    );
                    let screen_hit = (*self.to_global * socket).expand(WIRE_PORT_DROP_RADIUS);
                    self.context_menu_exclusion_rects
                        .push(self.to_global.inverse() * screen_hit);
                }
            }
        }
    }

    fn connect(&mut self, from: &OutPin, to: &InPin, snarl: &mut Snarl<GraphItem>) {
        if self.suppress_wire_connect {
            return;
        }
        if let Some(edit) = edit_for_wire(
            self.project,
            snarl,
            from.id.node,
            from.id.output,
            to.id.node,
            to.id.input,
            true,
        ) {
            self.edits.push(QueuedNodeEdit::Atomic(edit));
            snarl.connect(from.id, to.id);
        }
    }

    fn disconnect(&mut self, from: &OutPin, to: &InPin, snarl: &mut Snarl<GraphItem>) {
        let edit = edit_for_wire(
            self.project,
            snarl,
            from.id.node,
            from.id.output,
            to.id.node,
            to.id.input,
            false,
        );
        let context_target = match &edit {
            Some(NodeEdit::Disconnect { from, to }) => self
                .project
                .connections
                .iter()
                .find(|connection| connection.from == *from && connection.to == *to)
                .map(|connection| NodeEditorEditableWire::ProjectConnection {
                    connection_id: connection.id,
                }),
            Some(NodeEdit::SetOutputNode {
                owner,
                node_id: None,
            }) => container_output_node_id(self.project, *owner).map(|node_id| {
                NodeEditorEditableWire::OutputBinding {
                    owner: *owner,
                    node_id,
                }
            }),
            _ => None,
        };
        if let Some(target) = context_target {
            *self.wire_context_request = Some(target);
            return;
        }
        if let Some(edit) = edit {
            self.edits.push(QueuedNodeEdit::Atomic(edit));
        }
        snarl.disconnect(from.id, to.id);
    }

    fn drop_outputs(&mut self, pin: &OutPin, snarl: &mut Snarl<GraphItem>) {
        for remote in &pin.remotes {
            if let Some(edit) = edit_for_wire(
                self.project,
                snarl,
                pin.id.node,
                pin.id.output,
                remote.node,
                remote.input,
                false,
            ) {
                self.edits.push(QueuedNodeEdit::Atomic(edit));
            }
        }
        snarl.drop_outputs(pin.id);
    }

    fn drop_inputs(&mut self, pin: &InPin, snarl: &mut Snarl<GraphItem>) {
        for remote in &pin.remotes {
            if let Some(edit) = edit_for_wire(
                self.project,
                snarl,
                remote.node,
                remote.output,
                pin.id.node,
                pin.id.input,
                false,
            ) {
                self.edits.push(QueuedNodeEdit::Atomic(edit));
            }
        }
        snarl.drop_inputs(pin.id);
    }

    fn draw_background(
        &mut self,
        _background: Option<&BackgroundPattern>,
        viewport: &egui::Rect,
        snarl_style: &SnarlStyle,
        style: &egui::Style,
        painter: &egui::Painter,
        _snarl: &Snarl<GraphItem>,
    ) {
        // `painter.clip_rect()` is the final Snarl viewport in graph space.
        // Preserve its screen-space equivalent for foreground painting, QA
        // geometry and coordinate interactions registered after `show`.
        *self.canvas_clip = *self.to_global * painter.clip_rect();
        let scale = sanitized_node_editor_scale(self.to_global.scaling);
        let mut grid_style = *snarl_style;
        grid_style.bg_pattern_stroke = Some(egui::Stroke::new(
            screen_stroke_in_graph_units(0.7, scale),
            Color32::from_rgba_premultiplied(115, 128, 152, 34),
        ));
        BackgroundPattern::grid(egui::Vec2::splat(adaptive_grid_spacing(scale)), 0.0).draw(
            viewport,
            &grid_style,
            style,
            painter,
        );

        for container in self.containers {
            paint_container_backdrop(
                painter,
                container,
                container_inactive(self.project, container.owner, self.current_time),
            );
        }
    }

    fn current_transform(
        &mut self,
        to_global: &mut egui::emath::TSTransform,
        _snarl: &mut Snarl<GraphItem>,
    ) {
        resolve_node_editor_transform(to_global, self.locked_canvas_transform);
        *self.to_global = *to_global;
    }
}

pub(in crate::ui::panels::node_editor) fn resolve_node_editor_transform(
    transform: &mut egui::emath::TSTransform,
    locked: Option<egui::emath::TSTransform>,
) {
    if let Some(locked) = locked {
        *transform = locked;
    }
    sanitize_node_editor_transform(transform);
}
