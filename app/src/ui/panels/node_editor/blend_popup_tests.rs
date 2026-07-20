use super::*;
use crate::ui::widgets::searchable_context_menu::{
    reset_searchable_test_components, searchable_test_component,
};

fn pointer_button(position: egui::Pos2, pressed: bool) -> egui::Event {
    egui::Event::PointerButton {
        pos: position,
        button: egui::PointerButton::Primary,
        pressed,
        modifiers: egui::Modifiers::NONE,
    }
}

#[allow(
    clippy::too_many_arguments,
    reason = "the egui harness keeps screen, canvas, input, and authored state explicit"
)]
fn render_frame(
    context: &egui::Context,
    screen: egui::Rect,
    canvas_clip: egui::Rect,
    events: Vec<egui::Event>,
    time: f64,
    merge_id: Uuid,
    connection_id: Uuid,
    selected_blend: &mut BlendMode,
) -> egui::Rect {
    let mut selector_rect = egui::Rect::NOTHING;
    reset_searchable_test_components();
    drop(context.run(
        egui::RawInput {
            screen_rect: Some(screen),
            events,
            time: Some(time),
            ..Default::default()
        },
        |context| {
            egui::Area::new(egui::Id::new("merge-popup-production-helper-test"))
                .fixed_pos(egui::pos2(80.0, 370.0))
                .show(context, |ui| {
                    ui.set_clip_rect(canvas_clip);
                    selector_rect = egui::ComboBox::from_id_salt((
                        "merge-popup-production-helper-combo",
                        merge_id,
                        connection_id,
                    ))
                    .selected_text(blend_mode_label(*selected_blend))
                    .width(178.0)
                    .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
                    .show_ui(ui, |ui| {
                        let search_id = format!(
                            "node_editor.merge_layer.blend_search:{merge_id}:{connection_id}"
                        );
                        let mut items = blend_mode_searchable_items(*selected_blend);
                        assert_eq!(items.len(), BlendMode::ALL.len());
                        for item in &mut items {
                            let blend_mode = item.value;
                            item.qa_id = Some(format!(
                                "node_editor.merge_layer.blend.{}:{merge_id}:{connection_id}",
                                blend_mode_qa_key(blend_mode)
                            ));
                            item.qa_metadata = Some(serde_json::json!({
                                "action": "set_authored_blend",
                                "connection_id": connection_id,
                                "blend_mode": blend_mode_qa_key(blend_mode),
                                "blend_group": blend_mode.group().qa_key(),
                                "selected": !item.enabled,
                                "coordinate_space": "screen_points",
                            }));
                        }
                        if let Some(blend_mode) = show_searchable_items_with_qa(
                            ui,
                            &format!("merge_layer_blend_menu:{merge_id}:{connection_id}"),
                            Some(&search_id),
                            &items,
                        ) {
                            *selected_blend = blend_mode;
                        }
                    })
                    .response
                    .rect;
                });
        },
    ));
    selector_rect
}

#[test]
fn merge_blend_popup_uses_production_catalog_search_and_real_screen_clicks() {
    let context = egui::Context::default();
    let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(500.0, 400.0));
    let canvas_clip = egui::Rect::from_min_max(egui::pos2(20.0, 360.0), egui::pos2(420.0, 395.0));
    let merge_id = Uuid::from_u128(0xface);
    let connection_id = Uuid::from_u128(0xcafe);
    let search_id = format!("node_editor.merge_layer.blend_search:{merge_id}:{connection_id}");
    let option_id = format!("node_editor.merge_layer.blend.overlay:{merge_id}:{connection_id}");
    let mut selected_blend = BlendMode::Normal;
    let mut frame = 0_u64;
    let mut run = |events, selected_blend: &mut BlendMode| {
        frame += 1;
        render_frame(
            &context,
            screen,
            canvas_clip,
            events,
            frame as f64 / 60.0,
            merge_id,
            connection_id,
            selected_blend,
        )
    };

    let mut selector = egui::Rect::NOTHING;
    for _ in 0..3 {
        selector = run(Vec::new(), &mut selected_blend);
    }
    assert!(canvas_clip.contains(selector.center()));
    selector = run(
        vec![egui::Event::PointerMoved(selector.center())],
        &mut selected_blend,
    );
    selector = run(
        vec![pointer_button(selector.center(), true)],
        &mut selected_blend,
    );
    let _ = run(
        vec![pointer_button(selector.center(), false)],
        &mut selected_blend,
    );

    for group in [
        "Normal",
        "Darken",
        "Lighten",
        "Contrast",
        "Comparative",
        "HSL",
    ] {
        let category_id = format!("{search_id}.category:{group}");
        let (rect, _enabled, metadata) = searchable_test_component(&category_id)
            .unwrap_or_else(|| panic!("missing production {group} accordion"));
        assert!(rect.is_positive(), "{group} category is clipped: {rect:?}");
        assert_eq!(
            metadata.expect("category metadata")["category_path"],
            serde_json::json!([group])
        );
    }

    let (mut search_rect, _enabled, _) =
        searchable_test_component(&search_id).expect("production search field");
    assert!(search_rect.is_positive());
    let _ = run(
        vec![egui::Event::PointerMoved(search_rect.center())],
        &mut selected_blend,
    );
    search_rect = searchable_test_component(&search_id)
        .expect("search remains open after pointer move")
        .0;
    let _ = run(
        vec![pointer_button(search_rect.center(), true)],
        &mut selected_blend,
    );
    search_rect = searchable_test_component(&search_id)
        .expect("search remains open after pointer press")
        .0;
    let _ = run(
        vec![pointer_button(search_rect.center(), false)],
        &mut selected_blend,
    );
    let _ = run(
        vec![egui::Event::Text("overlay".to_owned())],
        &mut selected_blend,
    );

    let (mut option, enabled, metadata) =
        searchable_test_component(&option_id).expect("filtered Overlay option");
    assert!(option.is_positive() && enabled);
    assert!(screen.contains(option.center()));
    assert!(option.center().y < canvas_clip.top());
    let metadata = metadata.expect("Overlay metadata");
    assert_eq!(metadata["action"], "set_authored_blend");
    assert_eq!(metadata["blend_mode"], "overlay");
    assert_eq!(metadata["blend_group"], "contrast");

    let _ = run(
        vec![egui::Event::PointerMoved(option.center())],
        &mut selected_blend,
    );
    option = searchable_test_component(&option_id)
        .expect("Overlay remains open after pointer move")
        .0;
    let _ = run(
        vec![pointer_button(option.center(), true)],
        &mut selected_blend,
    );
    option = searchable_test_component(&option_id)
        .expect("Overlay remains open after pointer press")
        .0;
    let _ = run(
        vec![pointer_button(option.center(), false)],
        &mut selected_blend,
    );
    assert_eq!(selected_blend, BlendMode::Overlay);
}
