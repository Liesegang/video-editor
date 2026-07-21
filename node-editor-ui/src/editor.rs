//! Reusable egui rendering surface backed by borrowed frame descriptors.

use egui::{Color32, CornerRadius, Pos2, Stroke, StrokeKind, Vec2};

use crate::{
    interaction, EditorOutput, GraphFrame, InteractionOptions, InteractionState, ItemId,
    NodeDescriptor, PortDirection,
};

/// Host extension point for domain-specific controls inside a generic Node.
///
/// Headers, selection chrome, ports, wires, and groups remain editor-owned;
/// the host renders only its model-specific body controls.
pub trait NodeBodyRenderer<NodeId> {
    fn show(&mut self, node: &NodeId, ui: &mut egui::Ui);
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
    pub node_header_height: f32,
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
            node_header_height: 28.0,
        }
    }
}

/// Stateless editor entry point. Persistent graph data remains entirely in
/// the host; pass the same [`InteractionState`] back only while gestures live.
pub struct Editor;

impl Editor {
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
        paint(ui, frame, renderer, config);
        interaction::interact(ui, frame, state, InteractionOptions::ALL, false)
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
) where
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
        painter.rect(
            rect,
            CornerRadius::same(8),
            config.group_fill,
            if selected {
                config.selected_stroke
            } else {
                config.normal_stroke
            },
            StrokeKind::Inside,
        );
        if header.is_positive() {
            painter.rect_filled(header, CornerRadius::same(8), config.group_header_fill);
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

    for node in frame.nodes {
        paint_node(ui, frame, node, renderer, config);
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
        painter.circle_filled(center, config.port_radius, color);
        painter.circle_stroke(center, config.port_radius, Stroke::new(1.0, Color32::BLACK));
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
}

fn paint_node<NodeId, PortId, WireId, GroupId, Key, Renderer>(
    ui: &mut egui::Ui,
    frame: &GraphFrame<'_, NodeId, PortId, WireId, GroupId, Key>,
    node: &NodeDescriptor<'_, NodeId, GroupId>,
    renderer: &mut Renderer,
    config: EditorConfig,
) where
    NodeId: Clone + Eq,
    WireId: Clone + Eq,
    GroupId: Clone + Eq,
    Renderer: NodeBodyRenderer<NodeId>,
{
    let rect = frame.screen_rect(node.rect).intersect(frame.viewport);
    if !rect.is_positive() {
        return;
    }
    let selected = frame
        .selection
        .items
        .contains(&ItemId::Node(node.id.clone()));
    let painter = ui.painter().with_clip_rect(frame.viewport);
    painter.rect(
        rect,
        CornerRadius::same(7),
        config.node_fill,
        if selected {
            config.selected_stroke
        } else {
            config.normal_stroke
        },
        StrokeKind::Inside,
    );
    let header_height = config.node_header_height.min(rect.height());
    let header = egui::Rect::from_min_size(rect.min, Vec2::new(rect.width(), header_height));
    painter.rect_filled(header, CornerRadius::same(7), config.node_header_fill);
    painter.text(
        header.left_center() + Vec2::new(8.0, 0.0),
        egui::Align2::LEFT_CENTER,
        node.title,
        egui::TextStyle::Button.resolve(ui.style()),
        if node.enabled {
            ui.visuals().strong_text_color()
        } else {
            ui.visuals().weak_text_color()
        },
    );

    let body =
        egui::Rect::from_min_max(Pos2::new(rect.left(), header.bottom()), rect.right_bottom())
            .shrink2(Vec2::new(7.0, 5.0));
    if body.is_positive() {
        ui.scope_builder(egui::UiBuilder::new().max_rect(body), |ui| {
            ui.set_clip_rect(body.intersect(frame.viewport));
            renderer.show(&node.id, ui);
        });
    }
}
