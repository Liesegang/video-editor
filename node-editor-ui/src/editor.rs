//! Reusable egui rendering surface backed by borrowed frame descriptors.

use egui::{Color32, CornerRadius, Pos2, Stroke, StrokeKind, Vec2};

use crate::{
    chrome, interaction, EditorOutput, GraphFrame, GroupChrome, InteractionOptions,
    InteractionState, ItemId, NodeDescriptor, NodeHeader, NodePalette, NodeVisualStyle,
    PortDirection, PortLabel, PortVisualStyle,
};

/// Host extension point for domain-specific controls inside a generic Node.
///
/// Headers, selection chrome, ports, wires, and groups remain editor-owned;
/// the host renders only its model-specific body controls.
pub trait NodeBodyRenderer<NodeId> {
    /// Render host controls and report which interactive child owns the
    /// current pointer. Use [`NodeBodyResponse::from_response`] for each
    /// slider, drag value, button, or other interactive response and combine
    /// them with [`NodeBodyResponse::union`].
    fn show(&mut self, node: &NodeId, ui: &mut egui::Ui) -> NodeBodyResponse;
}

/// Frame-local pointer ownership reported by host controls in a Node body.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct NodeBodyResponse {
    pointer_owned: bool,
}

impl NodeBodyResponse {
    /// No body control owns the current pointer.
    pub const NONE: Self = Self {
        pointer_owned: false,
    };

    /// Convert one real egui widget response into body pointer ownership.
    pub fn from_response(response: &egui::Response) -> Self {
        Self {
            pointer_owned: response.enabled()
                && response.sense.interactive()
                && (response.contains_pointer()
                    || response.dragged()
                    || response.is_pointer_button_down_on()),
        }
    }

    /// Combine several child widget responses without exposing their IDs.
    #[must_use]
    pub const fn union(self, other: Self) -> Self {
        Self {
            pointer_owned: self.pointer_owned || other.pointer_owned,
        }
    }

    const fn pointer_owned(self) -> bool {
        self.pointer_owned
    }
}

/// Visual policy for the standalone surface.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct EditorConfig {
    pub background: Color32,
    pub group_fill: Color32,
    pub group_header_fill: Color32,
    pub node_fill: Color32,
    pub node_header_fill: Color32,
    pub selected_stroke: Stroke,
    pub normal_stroke: Stroke,
    pub wire_stroke: Stroke,
    pub port_radius: f32,
}

impl Default for EditorConfig {
    fn default() -> Self {
        Self {
            background: Color32::from_rgb(24, 25, 30),
            group_fill: Color32::from_rgba_premultiplied(45, 50, 63, 90),
            group_header_fill: Color32::from_rgb(47, 52, 66),
            node_fill: Color32::from_rgb(38, 40, 49),
            node_header_fill: Color32::from_rgb(55, 60, 74),
            selected_stroke: Stroke::new(2.0, Color32::from_rgb(105, 165, 255)),
            normal_stroke: Stroke::new(1.0, Color32::from_rgb(79, 84, 100)),
            wire_stroke: Stroke::new(2.5, Color32::from_rgb(145, 151, 170)),
            port_radius: 5.0,
        }
    }
}

/// Stateless editor entry point. Persistent graph data remains entirely in
/// the host; pass the same [`InteractionState`] back only while gestures live.
pub struct Editor;

impl Editor {
    /// Resolve a Node palette against generic selection/inactive state.
    ///
    /// Host adapters use this before handing frames to an external layout
    /// engine. The returned style is also the shell contract used by the
    /// editor-owned renderer.
    pub fn node_visual_style(
        palette: NodePalette,
        inactive: bool,
        selected: bool,
        scale: f32,
    ) -> NodeVisualStyle {
        chrome::node_visual_style(palette, inactive, selected, scale)
    }

    /// Build the generic Node body shell for an external layout engine.
    pub fn node_frame(style: NodeVisualStyle) -> egui::Frame {
        chrome::node_frame(style)
    }

    /// Build the generic Node header shell for an external layout engine.
    pub fn node_header_frame(style: NodeVisualStyle) -> egui::Frame {
        chrome::node_header_frame(style)
    }

    /// Render generic header content inside a host-owned Node layout.
    pub fn show_node_header(ui: &mut egui::Ui, header: NodeHeader<'_>) -> egui::Response {
        chrome::show_node_header(ui, header)
    }

    /// Render a non-selectable, bounded port label inside a host layout.
    pub fn show_port_label(ui: &mut egui::Ui, label: PortLabel<'_>) -> egui::Response {
        chrome::show_port_label(ui, label)
    }

    /// Resolve connected/unconnected socket presentation from a host type color.
    pub fn port_visual_style(color: Color32, connected: bool) -> PortVisualStyle {
        chrome::port_visual_style(color, connected)
    }

    /// Paint the fill/header portion of a nested group shell.
    pub fn paint_group_backdrop(painter: &egui::Painter, rect: egui::Rect, chrome: GroupChrome) {
        crate::chrome::paint_group_backdrop(painter, rect, chrome);
    }

    /// Paint the outline/header divider portion of a nested group shell.
    pub fn paint_group_foreground(painter: &egui::Painter, rect: egui::Rect, chrome: GroupChrome) {
        crate::chrome::paint_group_foreground(painter, rect, chrome);
    }

    /// Render a complete reusable editor and produce host mutation intents.
    pub fn show<NodeId, PortId, WireId, GroupId, Key, Renderer>(
        ui: &mut egui::Ui,
        frame: &GraphFrame<'_, NodeId, PortId, WireId, GroupId, Key>,
        state: &mut InteractionState<NodeId, PortId, WireId, GroupId>,
        renderer: &mut Renderer,
        config: EditorConfig,
    ) -> Vec<EditorOutput<NodeId, PortId, WireId, GroupId>>
    where
        NodeId: Clone + Eq,
        PortId: Clone + Eq,
        WireId: Clone + Eq,
        GroupId: Clone + Eq,
        Key: Copy + Eq,
        Renderer: NodeBodyRenderer<NodeId>,
    {
        let body_pointer_owned = paint(ui, frame, renderer, config);
        interaction::interact(
            ui,
            frame,
            state,
            InteractionOptions::ALL,
            body_pointer_owned,
        )
    }

    /// Run the same descriptor and interaction pipeline over host-rendered
    /// chrome. This is the migration seam used by RuViE's current Snarl
    /// adapter; it does not create a second interaction implementation.
    pub fn interact<NodeId, PortId, WireId, GroupId, Key>(
        ui: &mut egui::Ui,
        frame: &GraphFrame<'_, NodeId, PortId, WireId, GroupId, Key>,
        state: &mut InteractionState<NodeId, PortId, WireId, GroupId>,
        options: InteractionOptions,
        pointer_blocked: bool,
    ) -> Vec<EditorOutput<NodeId, PortId, WireId, GroupId>>
    where
        NodeId: Clone + Eq,
        PortId: Clone + Eq,
        WireId: Clone + Eq,
        GroupId: Clone + Eq,
        Key: Copy + Eq,
    {
        interaction::interact(ui, frame, state, options, pointer_blocked)
    }
}

fn paint<NodeId, PortId, WireId, GroupId, Key, Renderer>(
    ui: &mut egui::Ui,
    frame: &GraphFrame<'_, NodeId, PortId, WireId, GroupId, Key>,
    renderer: &mut Renderer,
    config: EditorConfig,
) -> bool
where
    NodeId: Clone + Eq,
    PortId: Clone + Eq,
    WireId: Clone + Eq,
    GroupId: Clone + Eq,
    Renderer: NodeBodyRenderer<NodeId>,
{
    ui.allocate_rect(frame.viewport, egui::Sense::hover());
    let painter = ui.painter().with_clip_rect(frame.viewport);
    painter.rect_filled(frame.viewport, 0.0, config.background);

    for group in frame.groups {
        let rect = frame.screen_rect(group.rect).intersect(frame.viewport);
        let header = frame
            .screen_rect(group.header_rect)
            .intersect(frame.viewport);
        if !rect.is_positive() {
            continue;
        }
        let selected = frame
            .selection
            .items
            .contains(&ItemId::Group(group.id.clone()));
        let group_chrome = GroupChrome {
            body_fill: config.group_fill,
            header_fill: config.group_header_fill,
            outline: if selected {
                config.selected_stroke
            } else {
                config.normal_stroke
            },
            divider: config.normal_stroke,
            header_height: header.height(),
            corner_radius: 8,
            details_visible: true,
        };
        chrome::paint_group_backdrop(&painter, rect, group_chrome);
        chrome::paint_group_foreground(&painter, rect, group_chrome);
        if header.is_positive() {
            painter.text(
                header.left_center() + Vec2::new(9.0, 0.0),
                egui::Align2::LEFT_CENTER,
                group.title,
                egui::TextStyle::Button.resolve(ui.style()),
                ui.visuals().strong_text_color(),
            );
        }
    }

    for wire in frame.wires {
        let selected = frame
            .selection
            .items
            .contains(&ItemId::Wire(wire.id.clone()));
        let stroke = if selected {
            config.selected_stroke
        } else {
            config.wire_stroke
        };
        let points = (0..=24)
            .map(|sample| frame.screen_position(wire.curve.point(sample as f32 / 24.0)))
            .collect::<Vec<_>>();
        painter.add(egui::Shape::line(points, stroke));
    }

    let mut body_pointer_owned = false;
    for node in frame.nodes {
        body_pointer_owned |= paint_node(ui, frame, node, renderer, config);
    }

    for port in frame.ports {
        let center = frame.screen_position(port.center);
        if !frame.viewport.contains(center) {
            continue;
        }
        let color = match port.direction {
            PortDirection::Input => Color32::from_rgb(104, 171, 255),
            PortDirection::Output => Color32::from_rgb(244, 172, 74),
        };
        let visual = chrome::port_visual_style(color, true);
        painter.circle_filled(center, config.port_radius, visual.fill);
        painter.circle_stroke(center, config.port_radius, visual.stroke);
        if !port.label.is_empty() {
            let (position, anchor) = match port.direction {
                PortDirection::Input => (
                    center + Vec2::new(config.port_radius + 4.0, 0.0),
                    egui::Align2::LEFT_CENTER,
                ),
                PortDirection::Output => (
                    center - Vec2::new(config.port_radius + 4.0, 0.0),
                    egui::Align2::RIGHT_CENTER,
                ),
            };
            painter.text(
                position,
                anchor,
                port.label,
                egui::TextStyle::Small.resolve(ui.style()),
                ui.visuals().text_color(),
            );
        }
    }
    body_pointer_owned
}

fn paint_node<NodeId, PortId, WireId, GroupId, Key, Renderer>(
    ui: &mut egui::Ui,
    frame: &GraphFrame<'_, NodeId, PortId, WireId, GroupId, Key>,
    node: &NodeDescriptor<'_, NodeId, GroupId>,
    renderer: &mut Renderer,
    config: EditorConfig,
) -> bool
where
    NodeId: Clone + Eq,
    WireId: Clone + Eq,
    GroupId: Clone + Eq,
    Renderer: NodeBodyRenderer<NodeId>,
{
    let rect = frame.screen_rect(node.rect).intersect(frame.viewport);
    if !rect.is_positive() {
        return false;
    }
    let selected = frame
        .selection
        .items
        .contains(&ItemId::Node(node.id.clone()));
    let painter = ui.painter().with_clip_rect(frame.viewport);
    let visual = NodeVisualStyle {
        body_fill: config.node_fill,
        header_fill: config.node_header_fill,
        outer_stroke: if selected {
            config.selected_stroke
        } else {
            config.normal_stroke
        },
        highlight_state: if selected { "selected" } else { "none" },
        highlight_screen_width: if selected {
            config.selected_stroke.width * frame.transform.scaling
        } else {
            config.normal_stroke.width * frame.transform.scaling
        },
    };
    painter.rect(
        rect,
        CornerRadius::same(7),
        visual.body_fill,
        visual.outer_stroke,
        StrokeKind::Inside,
    );
    let header = frame
        .screen_rect(node.header_rect)
        .intersect(rect)
        .intersect(frame.viewport);
    painter.rect_filled(header, CornerRadius::same(7), visual.header_fill);
    ui.scope_builder(
        egui::UiBuilder::new().max_rect(header.shrink2(Vec2::new(8.0, 0.0))),
        |ui| {
            ui.set_clip_rect(header.intersect(frame.viewport));
            chrome::show_node_header(
                ui,
                NodeHeader {
                    title: node.title,
                    title_color: Some(if node.enabled {
                        ui.visuals().strong_text_color()
                    } else {
                        ui.visuals().weak_text_color()
                    }),
                    leading: None,
                    trailing: None,
                    accent: ui.visuals().weak_text_color(),
                    min_width: (header.width() - 16.0).max(0.0),
                    title_width: (header.width() - 24.0).max(0.0),
                    row_height: header.height(),
                    details_visible: true,
                },
            );
        },
    );

    let body =
        egui::Rect::from_min_max(Pos2::new(rect.left(), header.bottom()), rect.right_bottom())
            .shrink2(Vec2::new(7.0, 5.0));
    if body.is_positive() {
        return ui
            .scope_builder(egui::UiBuilder::new().max_rect(body), |ui| {
                ui.set_clip_rect(body.intersect(frame.viewport));
                renderer.show(&node.id, ui)
            })
            .inner
            .pointer_owned();
    }
    false
}
