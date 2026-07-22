use super::ProjectNodeViewer;
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
use node_editor_ui::{Editor, HeaderGlyph, NodeHeader, PortLabel};
use std::sync::Arc;

mod bypass_menu;

impl SnarlViewer<GraphItem> for ProjectNodeViewer<'_> {
    fn node_layout(
        &mut self,
        _default: NodeLayout,
        _node_id: egui_snarl::NodeId,
        _inputs: &[InPin],
        _outputs: &[OutPin],
        _snarl: &Snarl<GraphItem>,
    ) -> NodeLayout {
        // Coil keeps inputs left and outputs right in top-down lists. Pins remain
        // one-per-row without changing graph flow; label/body helpers bound width
        // without changing pin sides.
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
                let style = super::selection::node_selection_presentation(
                    self.project,
                    self.selected_node_ids,
                    project_node_id,
                    self.current_time,
                    self.to_global.scaling,
                )
                .visual;
                Editor::node_frame(style)
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
                let style = super::selection::node_selection_presentation(
                    self.project,
                    self.selected_node_ids,
                    project_node_id,
                    self.current_time,
                    self.to_global.scaling,
                )
                .visual;
                Editor::node_header_frame(style)
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
                let selection = super::selection::node_selection_presentation(
                    self.project,
                    self.selected_node_ids,
                    project_node_id,
                    self.current_time,
                    self.to_global.scaling,
                );
                let (inactive, selected, visual) =
                    (selection.inactive, selection.selected, selection.visual);
                let bypassed = bypass_menu::is_bypassed(self.project.get_node(project_node_id));
                let icon = node_icon(self.project, project_node_id);
                let (status, status_label) = bypass_menu::status(bypassed, inactive);
                let title = node_title(self.project, project_node_id);
                let response = Editor::show_node_header(
                    ui,
                    NodeHeader {
                        title: &title,
                        title_color: None,
                        leading: Some(HeaderGlyph {
                            glyph: icon.glyph,
                            tooltip: icon.label,
                        }),
                        trailing: Some(HeaderGlyph {
                            glyph: status,
                            tooltip: status_label,
                        }),
                        accent: palette.accent,
                        min_width: NODE_HEADER_WIDTH,
                        title_width: NODE_HEADER_WIDTH - 48.0,
                        row_height: PORT_ROW_HEIGHT,
                        details_visible: node_editor_details_visible(self.to_global.scaling),
                    },
                );
                let response = graph_item_inactive_reason(
                    self.project,
                    GraphItem::Node(project_node_id),
                    self.current_time,
                )
                .map_or(response.clone(), |reason| {
                    response.on_hover_text(reason.tooltip())
                });
                let header_content_rect = response.rect;
                let visual_header_rect =
                    Editor::node_header_frame(visual).outer_rect(response.rect);
                let unclipped_content_rect = *self.to_global * header_content_rect;
                let unclipped_header_rect = *self.to_global * visual_header_rect;
                let header_rect = clipped_qa_rect(unclipped_header_rect, *self.canvas_clip);
                if let Ok(mut capture) = self.surface_capture.lock() {
                    capture.record_node_header(project_node_id, visual_header_rect);
                }
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
                let highlight_metadata = super::selection::node_highlight_metadata(visual);
                #[cfg(test)]
                {
                    capture_test_rect(&component_id, header_rect);
                    capture_test_metadata(
                        &component_id,
                        &serde_json::json!({
                            "node_id": project_node_id,
                            "selected": selected,
                            "highlight_style": highlight_metadata,
                            "content_rect": qa_rect_metadata(unclipped_content_rect),
                        }),
                    );
                }
                crate::qa::register_component_with_metadata(
                    component_id,
                    "node_header",
                    header_rect,
                    response.enabled(),
                    Some(serde_json::json!({
                        "node_id": project_node_id,
                        "selected": selected,
                        "highlight_style": highlight_metadata,
                        "hovered": response.hovered(),
                        "content_rect": qa_rect_metadata(unclipped_content_rect),
                        "unclipped_rect": qa_rect_metadata(unclipped_header_rect),
                        "visible_in_canvas": header_rect.is_positive(),
                    })),
                );
                if coordinate_double_clicked {
                    if let Some(node) = self.project.get_node(project_node_id) {
                        if let NodeContent::CompositionInstance(instance) = node.content() {
                            *self.pending_navigation = Some(instance.composition_id);
                        }
                    }
                }
            }
            GraphItem::Container(owner) => {
                let collapsed = container_collapsed(self.project, owner).unwrap_or(false);
                let selection = super::selection::container_selection_presentation(
                    self.project,
                    self.containers,
                    self.selected_container_owners,
                    owner,
                    self.current_time,
                    self.to_global.scaling,
                );
                let selected = selection.selected;
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
                let highlight_metadata = selection.visual.map(container_highlight_metadata);
                #[cfg(test)]
                {
                    capture_test_rect(&component_id, header_rect);
                    capture_test_metadata(
                        &component_id,
                        &serde_json::json!({
                            "owner": qa_container_key(owner),
                            "selected": selected,
                            "highlight_style": highlight_metadata,
                        }),
                    );
                }
                crate::qa::register_component_with_metadata(
                    component_id,
                    "node_container_header",
                    header_rect,
                    response.enabled(),
                    Some(serde_json::json!({
                        "owner": qa_container_key(owner),
                        "selected": selected,
                        "highlight_style": highlight_metadata,
                        "unclipped_rect": qa_rect_metadata(unclipped_header_rect),
                        "visible_in_canvas": header_rect.is_positive(),
                    })),
                );
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
        match item {
            GraphItem::Node(node_id)
                if super::selection::is_physical_merge_node(self.project, *node_id) =>
            {
                merge_input_slots(self.project, *node_id).len()
            }
            GraphItem::Node(_) | GraphItem::Container(_) | GraphItem::PortAnchor { .. } => {
                input_definitions(self.project, *item).len()
            }
        }
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
        let merge_slot = match item {
            Some(GraphItem::Node(node_id))
                if super::selection::is_physical_merge_node(self.project, node_id) =>
            {
                merge_input_slots(self.project, node_id)
                    .get(pin.id.input)
                    .cloned()
            }
            Some(GraphItem::Node(_) | GraphItem::Container(_) | GraphItem::PortAnchor { .. })
            | None => None,
        };
        let definition = merge_slot
            .as_ref()
            .map(|slot| slot.definition.clone())
            .or_else(|| {
                snarl.get_node(pin.id.node).and_then(|item| {
                    input_definitions(self.project, *item)
                        .get(pin.id.input)
                        .cloned()
                })
            })
            .unwrap_or_else(|| PinDefinition {
                key: "missing".to_string(),
                name: "Input".to_string(),
                data_type: PortDataType::Any,
            });
        let connected = !pin.remotes.is_empty();
        let merge_connection_id =
            if let (Some(GraphItem::Node(node_id)), Some(slot)) = (item, merge_slot.as_ref()) {
                match slot.role {
                    MergeInputSlotRole::Connected(_) | MergeInputSlotRole::Vacant(_) => {
                        self.show_merge_input_slot(node_id, slot, ui)
                    }
                    MergeInputSlotRole::Canonical => None,
                }
            } else {
                None
            };
        let merge_slot_rendered = merge_slot.as_ref().is_some_and(|slot| {
            matches!(
                slot.role,
                MergeInputSlotRole::Connected(_) | MergeInputSlotRole::Vacant(_)
            )
        });
        if !merge_slot_rendered {
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
        }
        let address = item
            .and_then(graph_item_owner)
            .map(|owner| PortAddress::new(owner, definition.key.clone()));
        QaPin {
            info: pin_info(definition.data_type, connected),
            component_id: merge_connection_id.map_or_else(
                || qa_port_id(self.project, item, "input", &definition.key),
                |connection_id| format!("node_editor.port.input.merge_connection:{connection_id}"),
            ),
            to_global: *self.to_global,
            graph_center: embedded_pin_center(
                self.containers,
                item,
                PortDirection::Input,
                pin.id.input,
            ),
            address,
            data_type: definition.data_type,
            direction: PortDirection::Input,
            connected,
            connection_id: merge_connection_id,
            canvas_clip: *self.canvas_clip,
            rendered_ports: Arc::clone(&self.rendered_ports),
            surface_capture: Arc::clone(&self.surface_capture),
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
            Editor::show_port_label(
                ui,
                PortLabel {
                    text: &definition.name,
                    width: port_label_width(item),
                    row_height: PORT_ROW_HEIGHT,
                    align: egui::Align::RIGHT,
                    details_visible: node_editor_details_visible(self.to_global.scaling),
                },
            );
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
            data_type: definition.data_type,
            direction: PortDirection::Output,
            connected,
            connection_id: None,
            canvas_clip: *self.canvas_clip,
            rendered_ports: Arc::clone(&self.rendered_ports),
            surface_capture: Arc::clone(&self.surface_capture),
        }
    }

    fn has_body(&mut self, item: &GraphItem) -> bool {
        matches!(
            item,
            GraphItem::Node(node_id)
                if self.project.get_node(*node_id).is_some_and(|node| matches!(
                    node.content(),
                    NodeContent::Color(library::model::ColorContent::Compose)
                ))
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
                    let property_time = node_property_time(
                        self.project,
                        self.plugin_manager,
                        project_node_id,
                        self.current_time,
                    );
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
                NodeContent::Value(value) => self.show_value_body(ui, *value),
                NodeContent::Color(operation) => {
                    self.show_color_body(ui, project_node_id, *operation)
                }
                NodeContent::Data(_) => self.show_data_body(ui, project_node_id),
                NodeContent::List(operation) => self.show_list_body(ui, *operation),
                NodeContent::Path(operation) => self.show_path_body(ui, *operation),
                NodeContent::NativeOperation(_) => self.show_native_body(ui, project_node_id),
                NodeContent::Media(_)
                | NodeContent::CompositionInstance(_)
                | NodeContent::Merge
                | NodeContent::SoundMerge
                | NodeContent::SoundAnalysis(_) => {}
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
                if bypass_menu::show_toggle(ui, node, project_node_id, self.edits) {
                    return;
                }
            }
        }
        if let Some((owner, label)) = delete_target {
            let blocked_reason = match owner {
                PortOwner::Node(node_id)
                    if self.project.compositions.iter().any(|composition| {
                        composition.structural_merge_node_id == node_id
                    })
                        || self.project.tracks.values().any(|track| {
                            track.structural_merge_node_id == node_id
                        }) =>
                {
                    Some(
                        "Structural Merge nodes belong to their Timeline container and cannot be deleted directly",
                    )
                }
                _ => None,
            };
            let response = ui
                .add_enabled(blocked_reason.is_none(), egui::Button::new(label))
                .on_disabled_hover_text(blocked_reason.unwrap_or_default());
            crate::qa::register_component_with_metadata(
                format!("node_editor.menu.delete.{}", qa_container_key(owner)),
                "node_editor_menu_item",
                response.rect,
                response.enabled(),
                Some(serde_json::json!({
                    "action": "delete",
                    "owner": qa_container_key(owner),
                    "blocked_reason": blocked_reason,
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
                if let Ok(mut capture) = self.surface_capture.lock() {
                    capture
                        .record_selectable(crate::state::context_types::SelectionTarget::Node(id));
                }
                if let Ok(mut node_rects) = self.rendered_node_rects.lock() {
                    node_rects.insert(id, graph_rect);
                }
                let selection = super::selection::node_selection_presentation(
                    self.project,
                    self.selected_node_ids,
                    id,
                    self.current_time,
                    self.to_global.scaling,
                );
                let (inactive, selected, visual) =
                    (selection.inactive, selection.selected, selection.visual);
                let highlight_metadata = super::selection::node_highlight_metadata(visual);
                let component_id = format!("node_editor.node:{id}");
                #[cfg(test)]
                {
                    capture_test_rect(&component_id, rect);
                    capture_test_metadata(
                        &component_id,
                        &serde_json::json!({
                            "node_id": id,
                            "selected": selected,
                            "highlight_style": highlight_metadata,
                        }),
                    );
                }
                crate::qa::register_component_with_metadata(
                    component_id,
                    "node",
                    rect,
                    true,
                    Some(serde_json::json!({
                        "node_id": id,
                        "selected": selected,
                        "highlight_style": highlight_metadata,
                        "inactive": inactive,
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
            GraphItem::Container(owner) => {
                // Only the integrated header/control card is a Snarl item;
                // the separately painted container body remains available to
                // the global Create menu and Node placement.
                self.context_menu_exclusion_rects.push(graph_rect);
                if let Ok(mut capture) = self.surface_capture.lock() {
                    capture.record_selectable(super::super::selection_target_for_owner(owner));
                }
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
        let context_target = edit
            .as_ref()
            .and_then(|edit| disconnect_context_target(self.project, edit));
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
        _snarl_style: &SnarlStyle,
        _style: &egui::Style,
        painter: &egui::Painter,
        _snarl: &Snarl<GraphItem>,
    ) {
        // `painter.clip_rect()` is the final Snarl viewport in graph space.
        // Preserve its screen-space equivalent for foreground painting, QA
        // geometry and coordinate interactions registered after `show`.
        *self.canvas_clip = *self.to_global * painter.clip_rect();
        paint_node_editor_canvas_grid(painter, *viewport, *self.canvas_clip, *self.to_global);

        for container in self.containers {
            let selected = self.selected_container_owners.contains(&container.owner);
            paint_container_backdrop(
                painter,
                container,
                container_inactive(self.project, container.owner, self.current_time),
                selected,
                self.to_global.scaling,
            );
        }
    }

    fn current_transform(
        &mut self,
        to_global: &mut egui::emath::TSTransform,
        _snarl: &mut Snarl<GraphItem>,
    ) {
        resolve_node_editor_transform(
            to_global,
            self.locked_canvas_transform,
            self.previous_canvas_transform,
        );
        self.previous_canvas_transform = Some(*to_global);
        *self.to_global = *to_global;
    }
}
