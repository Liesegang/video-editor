use super::*;

use crate::model::authoring::AppearanceOperation;
use crate::model::property::PropertyValue;

fn text_item(service: &TimelineEditorService) -> TimelineItemId {
    let project = service.snapshot().expect("snapshot");
    let track_id = project.timelines[&project.root_timeline_id].track_order[0];
    service
        .add_item(
            track_id,
            "Text".to_string(),
            SourceRef::Text {
                text: "Appearance".to_string(),
                appearance_operations: Vec::new(),
                ensemble_operations: Vec::new(),
            },
            TimelineInterval::new(MediaTime::zero(), MediaTime::new(5, 1).expect("duration"))
                .expect("interval"),
            0,
        )
        .expect("add Text")
        .0
}

fn operations(project: &AuthoringProject, item_id: TimelineItemId) -> &[AppearanceOperation] {
    let SourceRef::Text {
        appearance_operations,
        ..
    } = &project.items[&item_id].source
    else {
        panic!("Text source")
    };
    appearance_operations
}

#[test]
fn appearance_stack_add_reorder_remove_are_atomic_and_do_not_touch_source_or_siblings() {
    let plugins = PluginManager::default();
    let service = TimelineEditorService::create_default("Appearance stack").expect("service");
    let item_id = text_item(&service);
    let sibling_id = text_item(&service);

    let before_add = service.revision().expect("revision");
    let (fill_id, _) = service
        .add_appearance_operation(&plugins, item_id, "fill", 0)
        .expect("add Fill");
    assert_eq!(
        service.revision().expect("revision").get(),
        before_add.get() + 1,
        "one stack command is one Undo transaction"
    );
    let (stroke_id, _) = service
        .add_appearance_operation(&plugins, item_id, "stroke", 1)
        .expect("add Stroke");
    service
        .reorder_appearance_operation(item_id, stroke_id, 0)
        .expect("reorder");
    let reordered = service.snapshot().expect("snapshot");
    assert_eq!(
        operations(&reordered, item_id)
            .iter()
            .map(|operation| operation.id)
            .collect::<Vec<_>>(),
        [stroke_id, fill_id]
    );
    assert!(operations(&reordered, sibling_id).is_empty());
    assert!(matches!(
        &reordered.items[&item_id].source,
        SourceRef::Text { text, .. } if text == "Appearance"
    ));

    service
        .remove_appearance_operation(item_id, fill_id)
        .expect("remove");
    assert_eq!(
        operations(&service.snapshot().expect("snapshot"), item_id).len(),
        1
    );
    service.undo().expect("undo").expect("remove Undo");
    assert_eq!(
        operations(&service.snapshot().expect("snapshot"), item_id).len(),
        2
    );
}

#[test]
fn appearance_property_uses_the_operation_property_owner_and_preserves_stack_identity() {
    let plugins = PluginManager::default();
    let service = TimelineEditorService::create_default("Appearance property").expect("service");
    let item_id = text_item(&service);
    let (fill_id, _) = service
        .add_appearance_operation(&plugins, item_id, "fill", 0)
        .expect("add Fill");
    let authored = crate::model::property::ColorValue::from_straight_srgba8(
        &crate::model::frame::color::Color {
            r: 12,
            g: 34,
            b: 56,
            a: 200,
        },
    );
    service
        .set_appearance_property(
            &plugins,
            item_id,
            fill_id,
            "paint",
            MediaTime::zero(),
            PropertyValue::Paint(crate::model::property::Paint::Solid(authored.clone())),
        )
        .expect("set Fill paint");

    let snapshot = service.snapshot().expect("snapshot");
    let appearance = operations(&snapshot, item_id);
    assert_eq!(appearance.len(), 1);
    assert_eq!(appearance[0].id, fill_id);
    assert_eq!(
        appearance[0]
            .properties
            .get("paint")
            .and_then(crate::model::property::Property::value),
        Some(&PropertyValue::Paint(crate::model::property::Paint::Solid(
            authored
        )))
    );
    assert!(
        snapshot.items[&item_id]
            .authored_properties
            .get("color")
            .is_none()
    );
}

#[test]
fn appearance_factory_accepts_image_stages_without_inventing_a_raster_input() {
    let plugins = PluginManager::default();
    let service = TimelineEditorService::create_default("Appearance contract").expect("service");
    let item_id = text_item(&service);
    let (operation_id, _) = service
        .add_appearance_operation(
            &plugins,
            item_id,
            crate::plugin::IMAGE_OPACITY_STYLE_COMPONENT_ID,
            0,
        )
        .expect("Image -> Image is an ordered appearance stage");
    let snapshot = service.snapshot().expect("snapshot");
    let authored = operations(&snapshot, item_id);
    assert_eq!(authored.len(), 1);
    assert_eq!(authored[0].id, operation_id);
    assert_eq!(
        crate::model::authoring::appearance_input_kind(&authored[0].declared_ports),
        Some(crate::model::authoring::AppearanceInputKind::Image),
    );
    service.undo().expect("one undo");
    assert!(operations(&service.snapshot().expect("snapshot"), item_id).is_empty());
}
