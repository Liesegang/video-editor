use super::*;

fn pointer_button(position: Pos2, pressed: bool) -> egui::Event {
    egui::Event::PointerButton {
        pos: position,
        button: egui::PointerButton::Primary,
        pressed,
        modifiers: egui::Modifiers::NONE,
    }
}

fn nested_fixture() -> Vec<SearchableItem<u8>> {
    let mut leaf = SearchableItem::new("Gaussian Blur", 1);
    leaf.category = Some("Effect / Blur & Sharpen".to_owned());
    leaf.qa_id = Some("nested-menu.item:gaussian-blur".to_owned());
    vec![leaf]
}

fn render_nested_menu_frame(
    context: &egui::Context,
    events: Vec<egui::Event>,
    time: f64,
    items: &[SearchableItem<u8>],
) -> Rect {
    let mut button_rect = Rect::NOTHING;
    reset_searchable_test_components();
    drop(context.run(
        egui::RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(640.0, 480.0))),
            events,
            time: Some(time),
            ..Default::default()
        },
        |context| {
            egui::CentralPanel::default().show(context, |ui| {
                button_rect = searchable_menu_button(ui, "Add Effect", |ui| {
                    ui.set_min_width(290.0);
                    ui.set_min_height(240.0);
                    show_searchable_items_with_qa(
                        ui,
                        "nested-menu",
                        Some("nested-menu.query"),
                        items,
                    )
                })
                .response
                .rect;
            });
        },
    ));
    button_rect
}

#[test]
fn nested_category_real_pointer_click_keeps_menu_and_leaf_visible() {
    let context = egui::Context::default();
    let items = nested_fixture();
    let mut frame = 0_u64;
    let mut run = |events| {
        frame += 1;
        render_nested_menu_frame(&context, events, frame as f64 / 60.0, &items)
    };

    let mut add_button = Rect::NOTHING;
    for _ in 0..3 {
        add_button = run(Vec::new());
    }
    let _ = run(vec![egui::Event::PointerMoved(add_button.center())]);
    let _ = run(vec![pointer_button(add_button.center(), true)]);
    let _ = run(vec![pointer_button(add_button.center(), false)]);
    let _ = run(Vec::new());

    let top_id = "nested-menu.query.category:Effect";
    let (mut top, enabled, _) =
        searchable_test_component(top_id).expect("top category is visible after opening menu");
    assert!(top.is_positive() && enabled);
    let _ = run(vec![egui::Event::PointerMoved(top.center())]);
    top = searchable_test_component(top_id)
        .expect("top category remains available after pointer move")
        .0;
    let _ = run(vec![pointer_button(top.center(), true)]);
    top = searchable_test_component(top_id)
        .expect("top category remains available after pointer press")
        .0;
    let _ = run(vec![pointer_button(top.center(), false)]);

    let nested_id = "nested-menu.query.category:Effect/Blur & Sharpen";
    let (mut nested, enabled, _) = searchable_test_component(nested_id)
        .expect("opening the top category exposes its nested category");
    assert!(nested.is_positive() && enabled);
    let _ = run(vec![egui::Event::PointerMoved(nested.center())]);
    nested = searchable_test_component(nested_id)
        .expect("nested category remains available after pointer move")
        .0;
    let _ = run(vec![pointer_button(nested.center(), true)]);
    nested = searchable_test_component(nested_id)
        .expect("nested category remains available after pointer press")
        .0;
    let _ = run(vec![pointer_button(nested.center(), false)]);
    // Let the native collapsing animation expose the first child row.
    for _ in 0..12 {
        let _ = run(Vec::new());
    }

    let (leaf, enabled, _) = searchable_test_component("nested-menu.item:gaussian-blur")
        .expect("nested click keeps the outer menu open and exposes its leaf");
    assert!(leaf.is_positive() && enabled);
    assert!(
        searchable_test_component(top_id).is_some_and(|(rect, _, _)| rect.is_positive()),
        "the outer category remains rendered while its nested leaf is visible"
    );
    assert!(
        searchable_test_component(nested_id).is_some_and(|(rect, _, _)| rect.is_positive()),
        "the nested category remains rendered with its leaf"
    );
}
