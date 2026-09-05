use library::editor::TimelineEditorService;
use library::model::authoring::{MediaTime, ModuleDefinitionSharing, SourceRef, TimelineInterval};

use crate::state::authoring::AuthoringLibraryDrag;

use super::interaction::place_payload;

#[test]
fn particle_library_drag_uses_the_authoritative_private_module_factory() {
    let service = TimelineEditorService::create_default("Particle UI routing").expect("service");
    let initial = service.snapshot().expect("initial project");
    let timeline_id = initial.root_timeline_id;
    let track_id = initial.timelines[&timeline_id].track_order[0];
    drop(initial);

    let (ordinary_item_id, _) = service
        .add_item(
            track_id,
            "Ordinary clip".to_string(),
            SourceRef::Text {
                text: "ordinary".to_string(),
                ensemble_operations: Vec::new(),
            },
            TimelineInterval::new(
                MediaTime::new(0, 1).expect("start"),
                MediaTime::new(2, 1).expect("duration"),
            )
            .expect("interval"),
            0,
        )
        .expect("ordinary item");
    let before = service.snapshot().expect("before Particle placement");

    let item_id = place_payload(
        before.as_ref(),
        timeline_id,
        AuthoringLibraryDrag::NewParticleNodeClip,
        track_id,
        1,
        MediaTime::new(4, 1).expect("placement start"),
        &service,
    )
    .expect("Particle placement");
    let after = service.snapshot().expect("after Particle placement");

    assert_eq!(after.items.len(), before.items.len() + 1);
    assert_eq!(
        after.module_definitions.len(),
        before.module_definitions.len() + 1
    );
    assert_eq!(
        after.module_instances.len(),
        before.module_instances.len() + 1
    );
    assert_eq!(
        after.items[&ordinary_item_id],
        before.items[&ordinary_item_id]
    );

    let item = &after.items[&item_id];
    assert_eq!(item.name, "Particle System");
    assert_eq!(item.track_id, track_id);
    assert_eq!(item.layer, 1);
    assert_eq!(item.interval.start, MediaTime::new(4, 1).unwrap());
    assert_eq!(item.interval.duration, MediaTime::new(5, 1).unwrap());
    let SourceRef::Module(invocation) = &item.source else {
        panic!("Particle Assets payload must create one Module invocation");
    };
    let instance = &after.module_instances[&invocation.instance_id];
    let definition = &after.module_definitions[&instance.definition_id];
    assert_eq!(definition.sharing, ModuleDefinitionSharing::Private);
    assert_eq!(definition.graph.nodes.len(), 6);
    assert_eq!(definition.graph.connections.len(), 5);
    assert_eq!(definition.interface.parameters.len(), 11);

    service.undo().expect("undo").expect("one creation edit");
    assert_eq!(
        service.snapshot().expect("restored").as_ref(),
        before.as_ref()
    );
}
