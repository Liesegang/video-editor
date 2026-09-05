use super::*;

use crate::editor::TextEnsembleOperationKind;
use crate::editor::timeline_editor_service::node_clip_conversion_tests::{small_service, time};
use crate::model::node::GeneratorContent;
use crate::plugin::DECORATOR_CATEGORY;
use ordered_float::OrderedFloat;

fn vec2(x: f64, y: f64) -> PropertyValue {
    PropertyValue::Vec2(crate::model::property::Vec2 {
        x: OrderedFloat(x),
        y: OrderedFloat(y),
    })
}

#[test]
fn text_conversion_publishes_the_direct_surface_in_stable_semantic_order() {
    let plugins = PluginManager::default();
    let (service, track_id) = small_service("Text published surface");
    let fill = crate::editor::AppearanceOperationFactory::create(&plugins, "fill")
        .expect("Text fixture uses the production explicit Fill");
    let (item_id, _) = service
        .add_item(
            track_id,
            "Title".to_string(),
            SourceRef::Text {
                text: "First line\nSecond line".to_string(),
                appearance_operations: vec![fill],
                ensemble_operations: Vec::new(),
            },
            TimelineInterval::new(MediaTime::zero(), time(2)).unwrap(),
            0,
        )
        .unwrap();
    let (sibling_id, _) = service
        .add_item(
            track_id,
            "Sibling".to_string(),
            SourceRef::Text {
                text: "Untouched".to_string(),
                appearance_operations: Vec::new(),
                ensemble_operations: Vec::new(),
            },
            TimelineInterval::new(time(2), time(2)).unwrap(),
            1,
        )
        .unwrap();
    let (operation_id, _) = service
        .add_text_ensemble_operation_by_id(
            &plugins,
            item_id,
            TextEnsembleOperationKind::Decorator,
            "backplate",
        )
        .unwrap();
    for (key, value) in [
        ("position", vec2(12.0, 18.0)),
        ("scale", vec2(125.0, 80.0)),
        ("anchor", vec2(4.0, 6.0)),
        ("rotation", PropertyValue::from(23.0)),
        ("opacity", PropertyValue::from(72.0)),
    ] {
        service
            .set_authored_property_constant(
                AuthoringPropertyOwner::Item(item_id),
                key.to_string(),
                value,
            )
            .unwrap();
    }
    let before = service.snapshot().unwrap();
    let sibling_before = before.items[&sibling_id].clone();
    let placement_before = before.items[&item_id].authored_properties.clone();

    let result = service
        .convert_source_to_node_clip(&plugins, item_id)
        .unwrap();
    let after = service.snapshot().unwrap();
    assert_eq!(after.items[&sibling_id], sibling_before);
    assert_eq!(after.items[&item_id].authored_properties, placement_before);

    let definition = &after.module_definitions[&result.definition_id];
    let text_node_id = definition
        .graph
        .nodes
        .values()
        .find(|node| {
            matches!(
                node.content(),
                NodeContent::Generator(GeneratorContent::Text)
            )
        })
        .map(|node| node.id)
        .unwrap();
    let fill_node_id = definition
        .graph
        .nodes
        .values()
        .find(|node| {
            matches!(
                node.content(),
                NodeContent::PluginOperation(content)
                    if content.category == STYLE_CATEGORY && content.component_id == "fill"
            )
        })
        .map(|node| node.id)
        .unwrap();
    let descriptor = plugins
        .text_ensemble_operation_descriptor(DECORATOR_CATEGORY, "backplate")
        .unwrap();
    let fill_descriptor = plugins
        .operation_descriptor(STYLE_CATEGORY, "fill", STYLE_APPLY_OPERATION)
        .unwrap();
    let parameters = &definition.interface.parameters;
    let expected_names = ["Content", "Font", "Font Size"]
        .into_iter()
        .map(str::to_string)
        .chain(
            descriptor
                .properties()
                .iter()
                .map(|property| property.label().to_string()),
        )
        .chain(
            fill_descriptor
                .properties()
                .iter()
                .map(|property| format!("Fill {}", property.label())),
        )
        .collect::<Vec<_>>();
    assert_eq!(
        parameters
            .iter()
            .map(|parameter| parameter.name.clone())
            .collect::<Vec<_>>(),
        expected_names
    );
    for (parameter, port) in parameters[..3].iter().zip(["text", "font_family", "size"]) {
        assert_eq!(parameter.target.node_id, text_node_id);
        assert_eq!(parameter.target.port, port);
    }
    assert_eq!(
        parameters[2].default_value,
        PropertyValue::from(crate::plugin::entity_converter::DEFAULT_TIMELINE_TEXT_SIZE)
    );
    let fill_start = 3 + descriptor.properties().len();
    for parameter in &parameters[3..fill_start] {
        assert_eq!(parameter.target.node_id, operation_id);
    }
    for (parameter, property) in parameters[fill_start..]
        .iter()
        .zip(fill_descriptor.properties())
    {
        assert_eq!(parameter.target.node_id, fill_node_id);
        assert_eq!(
            parameter.target.port,
            format!("{PROPERTY_PORT_PREFIX}{}", property.name())
        );
    }

    let instance = &after.module_instances[&result.instance_id];
    let content = parameters
        .iter()
        .find(|parameter| parameter.name == "Content")
        .unwrap();
    assert_eq!(
        instance.parameter_overrides.get(&content.id),
        Some(&PropertyValue::String(
            "First line\nSecond line".to_string()
        ))
    );
}
