//! Reusable Composition authoring must preserve the production render path.

use std::collections::HashMap;
use std::sync::Arc;

use ordered_float::OrderedFloat;

use super::*;
use crate::cache::CacheManager;
use crate::core::render_plan::RenderPlanCompiler;
use crate::editor::{
    AppearanceOperationFactory, AuthoringPropertyOwner, RenderDestination, RenderService,
    TimelineEditorService,
};
use crate::model::authoring::{
    AttachmentOwner, AttachmentStage, CompositionParameterTarget, RationalRate, ShapeKind,
    ShapeSource, TimeMap, TimelineInterval,
};
use crate::model::frame::color::Color;
use crate::model::property::{ColorValue, Property, PropertyValue, Vec2};
use crate::plugin::PluginManager;
use crate::rendering::renderer::RenderOutput;
use crate::rendering::skia_renderer::SkiaRenderer;

struct ShapeFixture {
    service: TimelineEditorService,
    plugins: Arc<PluginManager>,
    item_id: TimelineItemId,
    fill_id: uuid::Uuid,
    original_time_map: TimeMap,
}

fn time(numerator: i64, denominator: u32) -> MediaTime {
    MediaTime::new(numerator, denominator).expect("valid exact time")
}

fn vec2(x: f64, y: f64) -> PropertyValue {
    PropertyValue::Vec2(Vec2 {
        x: OrderedFloat(x),
        y: OrderedFloat(y),
    })
}

fn color(r: u8, g: u8, b: u8, a: u8) -> PropertyValue {
    PropertyValue::ColorValue(ColorValue::from_straight_srgba8(&Color { r, g, b, a }))
}

fn shape_fixture(name: &str) -> ShapeFixture {
    let plugins = Arc::new(PluginManager::default());
    let mut project =
        AuthoringProject::new(name, 96, 64, RationalRate::new(30, 1).unwrap(), time(12, 1))
            .unwrap();
    let timeline_id = project.root_timeline_id;
    project
        .timelines
        .get_mut(&timeline_id)
        .unwrap()
        .background_color = Color {
        r: 0,
        g: 0,
        b: 0,
        a: 0,
    };
    let service = TimelineEditorService::new(project).unwrap();
    let project = service.snapshot().unwrap();
    let track_id = project.timelines[&timeline_id].track_order[0];
    drop(project);

    let mut fill = AppearanceOperationFactory::create(plugins.as_ref(), "fill").unwrap();
    fill.properties.set(
        "color".to_string(),
        Property::constant(color(235, 85, 40, 220)),
    );
    let fill_id = fill.id;
    let mut shadow = AppearanceOperationFactory::create(plugins.as_ref(), "drop_shadow").unwrap();
    shadow.properties.set(
        "distance".to_string(),
        Property::constant(PropertyValue::from(5.0)),
    );
    shadow.properties.set(
        "size".to_string(),
        Property::constant(PropertyValue::from(3.0)),
    );
    let (item_id, _) = service
        .add_item(
            track_id,
            "Styled animated shape".to_string(),
            SourceRef::Shape {
                shape: ShapeSource {
                    shape_kind: ShapeKind::Rectangle,
                    parameters: HashMap::from([
                        ("width".to_string(), PropertyValue::from(28.0)),
                        ("height".to_string(), PropertyValue::from(18.0)),
                    ]),
                    appearance_operations: vec![fill, shadow],
                },
            },
            TimelineInterval::new(time(2, 1), time(4, 1)).unwrap(),
            0,
        )
        .unwrap();
    let owner = AuthoringPropertyOwner::Item(item_id);
    service
        .set_authored_property_keyframe_mode(
            owner,
            "position".to_string(),
            time(1, 2),
            vec2(28.0, 25.0),
        )
        .unwrap();
    service
        .upsert_authored_property_keyframe(
            owner,
            "position".to_string(),
            time(2, 1),
            vec2(58.0, 35.0),
            None,
        )
        .unwrap();
    service
        .set_authored_property_keyframe_mode(
            owner,
            "opacity".to_string(),
            time(1, 2),
            PropertyValue::from(0.9),
        )
        .unwrap();
    service
        .upsert_authored_property_keyframe(
            owner,
            "opacity".to_string(),
            time(2, 1),
            PropertyValue::from(0.55),
            None,
        )
        .unwrap();
    service
        .add_builtin_effect_by_id(
            plugins.as_ref(),
            AttachmentOwner::Item { item_id },
            AttachmentStage::ItemPostTransform,
            "blur",
        )
        .unwrap();

    let original_time_map = TimeMap {
        source_start: time(1, 2),
        playback_rate: RationalRate::new(3, 2).unwrap(),
    };
    let mut project = service.snapshot().unwrap().as_ref().clone();
    project.items.get_mut(&item_id).unwrap().time_map = original_time_map;
    let service = TimelineEditorService::new(project).unwrap();
    ShapeFixture {
        service,
        plugins,
        item_id,
        fill_id,
        original_time_map,
    }
}

fn render_pixels(
    project: &AuthoringProject,
    plugins: Arc<PluginManager>,
    frame_number: u64,
) -> Vec<u8> {
    let timeline = &project.timelines[&project.root_timeline_id];
    let plan = RenderPlanCompiler::compile(project).expect("production RenderPlan");
    let frame =
        evaluate_render_plan_frame(project, &plan, plugins.as_ref(), frame_number, 1.0, None)
            .expect("production FrameInfo");
    let cache = Arc::new(CacheManager::new());
    let renderer = SkiaRenderer::new(
        u32::try_from(timeline.width).unwrap(),
        u32::try_from(timeline.height).unwrap(),
        timeline.background_color.clone(),
        false,
        None,
        Some(Arc::clone(&cache)),
    )
    .expect("CPU renderer");
    let RenderOutput::Image(image) = RenderService::new(renderer, plugins, cache)
        .render_authoring_frame(project, &frame, RenderDestination::Preview)
        .expect("authoring Preview")
    else {
        panic!("Preview must terminate to encoded pixels")
    };
    image.data
}

fn assert_exact_pixels(label: &str, expected: &[u8], actual: &[u8]) {
    assert_eq!(actual.len(), expected.len(), "{label}: image size changed");
    if let Some(index) = expected
        .iter()
        .zip(actual)
        .position(|(expected, actual)| expected != actual)
    {
        panic!(
            "{label}: first differing byte {index}: expected {}, actual {}",
            expected[index], actual[index]
        );
    }
}

fn assert_pixels_differ(label: &str, before: &[u8], after: &[u8]) {
    assert_eq!(after.len(), before.len(), "{label}: image size changed");
    assert!(
        before
            .iter()
            .zip(after)
            .any(|(before, after)| before != after),
        "{label}: pixels did not change"
    );
}

fn only_inner_item(project: &AuthoringProject, timeline_id: TimelineId) -> TimelineItemId {
    let mut items = project
        .items
        .values()
        .filter(|item| project.tracks[&item.track_id].timeline_id == timeline_id);
    let item_id = items.next().expect("one extracted item").id;
    assert!(items.next().is_none(), "extracted Timeline has one item");
    item_id
}

#[test]
fn extraction_preserves_exact_styled_animated_pixels_and_external_placement() {
    let fixture = shape_fixture("Extract render parity");
    let before_project = fixture.service.snapshot().unwrap();
    let before_item = before_project.items[&fixture.item_id].clone();
    let frames = [59, 60, 75, 90, 119, 179, 180];
    let before_pixels =
        frames.map(|frame| render_pixels(&before_project, Arc::clone(&fixture.plugins), frame));
    for (frame, pixels) in frames.into_iter().zip(&before_pixels) {
        let visible = pixels.chunks_exact(4).any(|pixel| pixel[3] != 0);
        assert_eq!(visible, (60..180).contains(&frame), "frame {frame}");
    }

    let (timeline_id, _) = fixture
        .service
        .extract_item_to_composition(fixture.item_id, "Reusable badge".to_string())
        .unwrap();
    let after_project = fixture.service.snapshot().unwrap();
    let outer = &after_project.items[&fixture.item_id];
    let SourceRef::Composition(instance) = &outer.source else {
        panic!("original placement must become a Composition")
    };
    assert_eq!(instance.timeline_id, timeline_id);
    assert_eq!(outer.name, before_item.name);
    assert_eq!(outer.track_id, before_item.track_id);
    assert_eq!(outer.interval, before_item.interval);
    assert_eq!(outer.layer, before_item.layer);
    assert_eq!(outer.parent, before_item.parent);
    assert_eq!(outer.blend_mode, before_item.blend_mode);
    assert_eq!(outer.time_map, TimeMap::default());
    assert!(outer.authored_properties.iter().next().is_none());

    let inner_id = only_inner_item(&after_project, timeline_id);
    let inner = &after_project.items[&inner_id];
    assert_eq!(inner.interval.start, MediaTime::zero());
    assert_eq!(inner.interval.duration, before_item.interval.duration);
    assert_eq!(inner.time_map, fixture.original_time_map);
    assert_eq!(inner.source, before_item.source);
    assert_eq!(inner.authored_properties, before_item.authored_properties);
    assert!(
        after_project.attachments.values().any(|attachment| {
            attachment.owner == (AttachmentOwner::Item { item_id: inner_id })
        })
    );
    assert!(!after_project.attachments.values().any(|attachment| {
        attachment.owner
            == (AttachmentOwner::Item {
                item_id: fixture.item_id,
            })
    }));

    for (frame, before_pixels) in frames.into_iter().zip(&before_pixels) {
        let after_pixels = render_pixels(&after_project, Arc::clone(&fixture.plugins), frame);
        assert_exact_pixels(
            &format!("Extract to Composition at frame {frame}"),
            before_pixels,
            &after_pixels,
        );
    }
}

#[test]
fn linked_instances_share_definition_edits_until_one_is_made_unique() {
    let fixture = shape_fixture("Linked and unique Composition renders");
    let (timeline_id, _) = fixture
        .service
        .extract_item_to_composition(fixture.item_id, "Shared badge".to_string())
        .unwrap();
    let (linked_id, _) = fixture
        .service
        .duplicate_item(fixture.item_id, time(7, 1), 1)
        .unwrap();
    let project = fixture.service.snapshot().unwrap();
    let inner_id = only_inner_item(&project, timeline_id);
    drop(project);

    let (opacity_id, _) = fixture
        .service
        .publish_composition_parameter(
            timeline_id,
            "Opacity".to_string(),
            CompositionParameterTarget::ItemProperty {
                item_id: inner_id,
                property_key: "opacity".to_string(),
            },
            PropertyValue::from(1.0),
        )
        .unwrap();
    fixture
        .service
        .set_composition_parameter_override(fixture.item_id, opacity_id, PropertyValue::from(0.72))
        .unwrap();
    fixture
        .service
        .set_composition_parameter_override(linked_id, opacity_id, PropertyValue::from(0.38))
        .unwrap();
    let linked_before = fixture.service.snapshot().unwrap();
    let first_before = render_pixels(&linked_before, Arc::clone(&fixture.plugins), 75);
    let second_before = render_pixels(&linked_before, Arc::clone(&fixture.plugins), 225);

    fixture
        .service
        .set_appearance_property(
            fixture.plugins.as_ref(),
            inner_id,
            fixture.fill_id,
            "color",
            MediaTime::zero(),
            color(40, 170, 235, 220),
        )
        .unwrap();
    let linked_after = fixture.service.snapshot().unwrap();
    let first_linked = render_pixels(&linked_after, Arc::clone(&fixture.plugins), 75);
    let second_linked = render_pixels(&linked_after, Arc::clone(&fixture.plugins), 225);
    assert_pixels_differ("first linked definition edit", &first_before, &first_linked);
    assert_pixels_differ(
        "second linked definition edit",
        &second_before,
        &second_linked,
    );

    let unique_baseline = second_linked;
    let (unique_timeline_id, _) = fixture.service.make_composition_unique(linked_id).unwrap();
    assert_ne!(unique_timeline_id, timeline_id);
    let unique_project = fixture.service.snapshot().unwrap();
    let SourceRef::Composition(first_instance) = &unique_project.items[&fixture.item_id].source
    else {
        panic!("first linked instance")
    };
    let SourceRef::Composition(unique_instance) = &unique_project.items[&linked_id].source else {
        panic!("unique instance")
    };
    assert_eq!(first_instance.timeline_id, timeline_id);
    assert_eq!(unique_instance.timeline_id, unique_timeline_id);
    let unique_opacity_id = unique_project.timelines[&unique_timeline_id]
        .published_parameters
        .iter()
        .find(|parameter| parameter.name == "Opacity")
        .expect("copied Opacity parameter")
        .id;
    assert_ne!(unique_opacity_id, opacity_id);
    assert_eq!(
        unique_instance.parameter_overrides.get(&unique_opacity_id),
        Some(&PropertyValue::from(0.38))
    );
    assert!(
        !unique_instance
            .parameter_overrides
            .contains_key(&opacity_id)
    );
    assert_exact_pixels(
        "Make Composition Unique",
        &unique_baseline,
        &render_pixels(&unique_project, Arc::clone(&fixture.plugins), 225),
    );

    fixture
        .service
        .set_appearance_property(
            fixture.plugins.as_ref(),
            inner_id,
            fixture.fill_id,
            "color",
            MediaTime::zero(),
            color(120, 235, 70, 220),
        )
        .unwrap();
    let edited_original = fixture.service.snapshot().unwrap();
    let first_edited = render_pixels(&edited_original, Arc::clone(&fixture.plugins), 75);
    let unique_after = render_pixels(&edited_original, Arc::clone(&fixture.plugins), 225);
    assert_pixels_differ(
        "original definition remains editable",
        &first_linked,
        &first_edited,
    );
    assert_exact_pixels(
        "unique copy is independent from the original definition",
        &unique_baseline,
        &unique_after,
    );
}
