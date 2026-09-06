//! Clip Prefab operations preserve the authoritative audio schedule.

use super::*;
use crate::editor::{AuthoringPropertyOwner, TimelineEditorService};

fn render_audio(project: &AuthoringProject, start: u64, count: usize) -> Vec<f32> {
    let cache = CacheManager::new();
    AuthoringAudioMixer::root(project, &cache)
        .unwrap()
        .render_window(start, count)
        .unwrap()
}

fn assert_exact_samples(label: &str, expected: &[f32], actual: &[f32]) {
    assert_eq!(
        actual.len(),
        expected.len(),
        "{label}: sample count changed"
    );
    if let Some(index) = expected
        .iter()
        .zip(actual)
        .position(|(expected, actual)| expected != actual)
    {
        panic!(
            "{label}: first differing sample {index}: expected {}, actual {}",
            expected[index], actual[index]
        );
    }
}

fn assert_samples_differ(label: &str, before: &[f32], after: &[f32]) {
    assert_eq!(after.len(), before.len(), "{label}: sample count changed");
    assert!(
        before
            .iter()
            .zip(after)
            .any(|(before, after)| before != after),
        "{label}: samples did not change"
    );
}

fn only_inner_item(project: &AuthoringProject, timeline_id: TimelineId) -> TimelineItemId {
    let mut items = project
        .items
        .values()
        .filter(|item| project.tracks[&item.track_id].timeline_id == timeline_id);
    let item_id = items.next().expect("one extracted audio item").id;
    assert!(items.next().is_none(), "extracted Timeline has one item");
    item_id
}

#[test]
fn extraction_and_unique_preserve_exact_audio_samples_and_definition_isolation() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("prefab.wav");
    let samples = (0..80)
        .map(|index| {
            let value = (index as f32 + 1.0) / 100.0;
            [value, -value]
        })
        .collect::<Vec<_>>();
    write_stereo_wave(&path, &samples);
    let mut project = project_with_audio_track(60);
    let timeline_id = project.root_timeline_id;
    let track_id = project.timelines[&timeline_id].track_order[0];
    let asset_id = add_audio_asset(&mut project, &path, samples.len());
    let item_id = add_asset_item(&mut project, track_id, asset_id, 5, 20, 3);
    set_gain(
        &mut project.items.get_mut(&item_id).unwrap().authored_properties,
        0.8,
    );
    project.validate().unwrap();
    let service = TimelineEditorService::new(project).unwrap();

    let before = service.snapshot().unwrap();
    let before_samples = render_audio(&before, 0, 60);
    let (definition_id, _) = service
        .extract_item_to_composition(item_id, "Reusable audio".to_string())
        .unwrap();
    let extracted = service.snapshot().unwrap();
    assert_exact_samples(
        "audio extraction",
        &before_samples,
        &render_audio(&extracted, 0, 60),
    );
    let inner_id = only_inner_item(&extracted, definition_id);

    let (linked_id, _) = service.duplicate_item(item_id, frame_time(30), 1).unwrap();
    let linked_before = service.snapshot().unwrap();
    let first_before = render_audio(&linked_before, 5, 20);
    let second_before = render_audio(&linked_before, 30, 20);
    assert_exact_samples("linked definition", &first_before, &second_before);
    service
        .set_authored_property_constant(
            AuthoringPropertyOwner::Item(inner_id),
            "gain".to_string(),
            PropertyValue::Number(OrderedFloat(0.5)),
        )
        .unwrap();
    let linked_after = service.snapshot().unwrap();
    let first_linked = render_audio(&linked_after, 5, 20);
    let second_linked = render_audio(&linked_after, 30, 20);
    assert_samples_differ("first linked audio edit", &first_before, &first_linked);
    assert_samples_differ("second linked audio edit", &second_before, &second_linked);
    assert_exact_samples("linked audio edit", &first_linked, &second_linked);

    let before_unique = render_audio(&linked_after, 0, 60);
    let (unique_timeline_id, _) = service.make_composition_unique(linked_id).unwrap();
    assert_ne!(unique_timeline_id, definition_id);
    let unique = service.snapshot().unwrap();
    assert_exact_samples(
        "unique audio copy",
        &before_unique,
        &render_audio(&unique, 0, 60),
    );

    service
        .set_authored_property_constant(
            AuthoringPropertyOwner::Item(inner_id),
            "gain".to_string(),
            PropertyValue::Number(OrderedFloat(0.2)),
        )
        .unwrap();
    let isolated = service.snapshot().unwrap();
    let first_isolated = render_audio(&isolated, 5, 20);
    let second_isolated = render_audio(&isolated, 30, 20);
    assert_samples_differ(
        "original audio definition edit",
        &first_linked,
        &first_isolated,
    );
    assert_exact_samples(
        "unique audio definition stays independent",
        &second_linked,
        &second_isolated,
    );
}
