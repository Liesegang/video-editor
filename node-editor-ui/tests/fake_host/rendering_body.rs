use std::cell::Cell;
use std::cell::RefCell;

use super::support::*;

#[test]
fn fake_host_renders_nested_groups_nodes_wires_ports_and_host_bodies() {
    let context = egui::Context::default();
    let graph = FakeGraph::new();
    let mut state = State::default();

    let (outputs, full, rendered) = run_frame(
        &context,
        &graph,
        &mut state,
        &[],
        None,
        Vec::new(),
        Modifiers::NONE,
    );

    assert!(outputs.is_empty());
    assert_eq!(rendered, [1, 2]);
    assert!(!full.shapes.is_empty());
    assert_eq!(graph.groups[1].parent, Some(graph.groups[0].id));
}

#[test]
fn fake_external_layout_consumes_the_same_chrome_descriptors() {
    let context = egui::Context::default();
    let response_rects = RefCell::new(Vec::new());
    let full = context.run(
        RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(500.0, 300.0))),
            ..Default::default()
        },
        |context| {
            egui::CentralPanel::default()
                .frame(egui::Frame::NONE)
                .show(context, |ui| {
                    let palette = NodePalette {
                        body: egui::Color32::from_rgb(30, 40, 50),
                        header: egui::Color32::from_rgb(60, 70, 80),
                        accent: egui::Color32::from_rgb(150, 160, 170),
                    };
                    let visual = Editor::node_visual_style(palette, false, true, 0.5);
                    Editor::node_frame(visual).show(ui, |ui| {
                        Editor::node_header_frame(visual).show(ui, |ui| {
                            let response = Editor::show_node_header(
                                ui,
                                NodeHeader {
                                    title: "Host source",
                                    title_color: None,
                                    leading: Some(HeaderGlyph {
                                        glyph: "S",
                                        tooltip: "Source",
                                    }),
                                    trailing: None,
                                    accent: palette.accent,
                                    min_width: 180.0,
                                    title_width: 140.0,
                                    row_height: 24.0,
                                    details_visible: true,
                                },
                            );
                            response_rects.borrow_mut().push(response.rect);
                        });
                        let response = Editor::show_port_label(
                            ui,
                            PortLabel {
                                text: "Image",
                                width: 90.0,
                                row_height: 22.0,
                                align: egui::Align::RIGHT,
                                details_visible: true,
                            },
                        );
                        response_rects.borrow_mut().push(response.rect);
                    });
                    let group = Rect::from_min_size(pos2(240.0, 40.0), vec2(220.0, 180.0));
                    let chrome = GroupChrome {
                        body_fill: egui::Color32::from_gray(30),
                        header_fill: egui::Color32::from_gray(50),
                        outline: egui::Stroke::new(1.5, egui::Color32::LIGHT_BLUE),
                        divider: egui::Stroke::new(1.0, egui::Color32::GRAY),
                        header_height: 28.0,
                        corner_radius: 8,
                        details_visible: true,
                    };
                    Editor::paint_group_backdrop(ui.painter(), group, chrome);
                    Editor::paint_group_foreground(ui.painter(), group, chrome);
                });
        },
    );

    assert_eq!(response_rects.into_inner().len(), 2);
    assert!(!full.shapes.is_empty());
}

#[test]
fn body_drag_value_owns_pointer_while_header_still_moves_node() {
    let context = egui::Context::default();
    let graph = FakeGraph::new();
    let mut state = State::default();
    let response_rect = RefCell::new(Rect::NOTHING);
    let mut value = 0.0;

    {
        let mut render = |events: Vec<Event>| {
            let outputs = RefCell::new(Vec::new());
            let mut renderer = DragValueBodyRenderer {
                value: &mut value,
                response_rect: &response_rect,
            };
            drop(context.run(
                RawInput {
                    screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(800.0, 500.0))),
                    events,
                    ..Default::default()
                },
                |context| {
                    egui::CentralPanel::default()
                        .frame(egui::Frame::NONE)
                        .show(context, |ui| {
                            outputs.borrow_mut().extend(Editor::show(
                                ui,
                                &graph.frame(&[ItemId::Node(1)], Some(ItemId::Node(1))),
                                &mut state,
                                &mut renderer,
                                EditorConfig::default(),
                            ));
                        });
                },
            ));
            outputs.into_inner()
        };
        assert!(render(Vec::new()).is_empty());
        let control = response_rect.borrow().center();
        assert!(graph.nodes[0].rect.contains(control));
        assert!(!graph.nodes[0].header_rect.contains(control));

        let pressed = render(vec![
            Event::PointerMoved(control),
            pointer_button(control, true, Modifiers::NONE),
        ]);
        let dragged = render(vec![Event::PointerMoved(control + vec2(35.0, 0.0))]);
        let released = render(vec![pointer_button(
            control + vec2(35.0, 0.0),
            false,
            Modifiers::NONE,
        )]);
        assert!(pressed
            .iter()
            .all(|output| !matches!(output, EditorOutput::Move { .. })));
        assert!(dragged
            .iter()
            .all(|output| !matches!(output, EditorOutput::Move { .. })));
        assert!(released
            .iter()
            .all(|output| !matches!(output, EditorOutput::Move { .. })));
        let header = graph.nodes[0].header_rect.center();
        let _ = render(vec![
            Event::PointerMoved(header),
            pointer_button(header, true, Modifiers::NONE),
        ]);
        let moved = render(vec![Event::PointerMoved(header + vec2(24.0, 12.0))]);
        assert!(moved.iter().any(|output| matches!(
            output,
            EditorOutput::Move { items, delta, .. }
                if items == &[ItemId::Node(1)] && *delta == vec2(24.0, 12.0)
        )));
    }
    assert_ne!(value, 0.0, "the real DragValue must receive the drag");
}

#[test]
fn batched_header_press_crossing_body_control_remains_a_header_move() {
    let context = egui::Context::default();
    let graph = FakeGraph::new();
    let mut state = State::default();
    let response_rect = RefCell::new(Rect::NOTHING);
    let mut value = 0.0;

    let mut render = |events: Vec<Event>| {
        let outputs = RefCell::new(Vec::new());
        let mut renderer = DragValueBodyRenderer {
            value: &mut value,
            response_rect: &response_rect,
        };
        drop(context.run(
            RawInput {
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(800.0, 500.0))),
                events,
                ..Default::default()
            },
            |context| {
                egui::CentralPanel::default()
                    .frame(egui::Frame::NONE)
                    .show(context, |ui| {
                        outputs.borrow_mut().extend(Editor::show(
                            ui,
                            &graph.frame(&[ItemId::Node(1)], Some(ItemId::Node(1))),
                            &mut state,
                            &mut renderer,
                            EditorConfig::default(),
                        ));
                    });
            },
        ));
        outputs.into_inner()
    };
    assert!(render(Vec::new()).is_empty());
    let start = graph.nodes[0].header_rect.center();
    let end = response_rect.borrow().center();
    assert!(!graph.nodes[0].header_rect.contains(end));
    let outputs = render(vec![
        Event::PointerMoved(start),
        pointer_button(start, true, Modifiers::NONE),
        Event::PointerMoved(end),
    ]);
    let expected = end - start;
    assert!(outputs.iter().any(|output| matches!(
        output,
        EditorOutput::Move {
            grabbed: ItemId::Node(1),
            delta,
            ..
        } if *delta == expected
    )));
}

#[test]
fn body_response_maps_global_press_origin_into_a_transformed_layer() {
    let context = egui::Context::default();
    let area_id = egui::Id::new("transformed_body_control");
    let layer_id = egui::LayerId::new(egui::Order::Middle, area_id);
    let transform = egui::emath::TSTransform::new(vec2(140.0, 80.0), 1.6);
    let local_rect = RefCell::new(Rect::NOTHING);
    let owned = Cell::new(false);
    let render = |events: Vec<Event>| {
        drop(context.run(
            RawInput {
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(800.0, 500.0))),
                events,
                ..Default::default()
            },
            |context| {
                context.set_transform_layer(layer_id, transform);
                egui::Area::new(area_id)
                    .fixed_pos(pos2(40.0, 30.0))
                    .show(context, |ui| {
                        let response = ui.button("host body control");
                        local_rect.replace(response.rect);
                        owned.set(NodeBodyResponse::from_response(&response).owns_pointer());
                    });
            },
        ));
    };

    render(Vec::new());
    let global_control = transform * local_rect.borrow().center();
    render(vec![
        Event::PointerMoved(global_control),
        pointer_button(global_control, true, Modifiers::NONE),
    ]);
    assert!(owned.get());
}
