use super::*;

use std::sync::Arc;

use crate::editor::timeline_editor_service::node_clip_conversion_tests::{
    color, rendered_pixels, small_service, time,
};

#[test]
fn solid_color_automation_moves_to_one_instance_with_pixel_parity_and_one_undo() {
    let plugins = Arc::new(PluginManager::default());
    let (service, track_id) = small_service("Solid source conversion");
    let source = SourceRef::Solid {
        color: color(20, 40, 60, 255),
    };
    let (item_id, _) = service
        .add_item(
            track_id,
            "Animated Solid".to_string(),
            source.clone(),
            TimelineInterval::new(MediaTime::zero(), time(2)).unwrap(),
            0,
        )
        .unwrap();
    let (sibling_id, _) = service
        .add_item(
            track_id,
            "Sibling Solid".to_string(),
            source,
            TimelineInterval::new(time(3), time(4)).unwrap(),
            1,
        )
        .unwrap();
    let mut source_key_ids = Vec::new();
    for (seconds, value) in [(0, color(220, 30, 50, 255)), (1, color(30, 80, 230, 255))] {
        let (key_id, _) = service
            .upsert_authored_property_keyframe(
                AuthoringPropertyOwner::Item(item_id),
                "color".to_string(),
                time(seconds),
                PropertyValue::Color(value),
                None,
            )
            .unwrap();
        source_key_ids.push(key_id);
    }
    let before = service.snapshot().unwrap();
    let sibling_before = before.items[&sibling_id].clone();
    let before_pixels = rendered_pixels(&before, Arc::clone(&plugins), 15);

    let conversion = service
        .convert_source_to_node_clip(plugins.as_ref(), item_id)
        .unwrap();

    let after = service.snapshot().unwrap();
    assert_eq!(after.items[&sibling_id], sibling_before);
    assert!(
        after.items[&item_id]
            .authored_properties
            .get("color")
            .is_none()
    );
    let SourceRef::Module(invocation) = &after.items[&item_id].source else {
        panic!("converted Solid must be a Module invocation");
    };
    let moved_ids = invocation
        .automation_tracks
        .values()
        .flat_map(|track| track.keyframes.iter().map(|keyframe| keyframe.id))
        .collect::<Vec<_>>();
    assert!(source_key_ids.iter().all(|id| moved_ids.contains(id)));
    assert_eq!(
        after.module_definitions[&conversion.definition_id].sharing,
        ModuleDefinitionSharing::Private
    );
    assert_eq!(
        rendered_pixels(&after, Arc::clone(&plugins), 15),
        before_pixels,
        "Solid source color changed during conversion"
    );

    service.undo().unwrap().expect("one conversion undo");
    assert_eq!(service.snapshot().unwrap().as_ref(), before.as_ref());
}
