use super::*;

fn number(value: f64) -> PropertyValue {
    PropertyValue::Number(OrderedFloat(value))
}

fn zero_vec3() -> Vec3 {
    Vec3 {
        x: OrderedFloat(0.0),
        y: OrderedFloat(0.0),
        z: OrderedFloat(0.0),
    }
}

#[test]
fn missing_property_is_promoted_from_the_supplied_default_value() {
    let mut properties = PropertyMap::new();

    let id = properties
        .upsert_keyframe_with_id("opacity", 1.25, number(100.0), None)
        .expect("a missing direct property should be keyframeable");

    let property = properties
        .get("opacity")
        .expect("property should be created");
    assert_eq!(property.evaluator, "keyframe");
    assert_eq!(
        property.keyframes(),
        vec![Keyframe {
            id,
            time: OrderedFloat(1.25),
            value: number(100.0),
            easing: EasingFunction::Linear,
        }]
    );
}

#[test]
fn tolerance_upsert_updates_one_key_and_preserves_identity_and_easing() {
    let mut property = Property::constant(number(10.0));
    let first_id = property
        .upsert_keyframe_with_id(1.0, number(20.0), Some(EasingFunction::EaseInQuad))
        .expect("constant should promote");

    let matched_id = property
        .upsert_keyframe_with_id(1.0005, number(30.0), None)
        .expect("keyframe should update");
    assert_eq!(matched_id, first_id);
    assert_eq!(property.keyframes().len(), 1);
    assert_eq!(property.keyframes()[0].value, number(30.0));
    assert_eq!(property.keyframes()[0].easing, EasingFunction::EaseInQuad);

    let distinct_id = property
        .upsert_keyframe_with_id(1.002, number(40.0), None)
        .expect("time outside tolerance should insert");
    assert_ne!(distinct_id, first_id);
    assert_eq!(property.keyframes().len(), 2);
}

#[test]
fn removing_the_last_keyframe_restores_its_typed_value_as_a_constant() {
    let value = PropertyValue::Vec2(Vec2 {
        x: OrderedFloat(12.0),
        y: OrderedFloat(34.0),
    });
    let mut property = Property::constant(value.clone());
    let id = property
        .upsert_keyframe_with_id(2.0, value.clone(), None)
        .expect("constant should promote");

    assert!(property.remove_keyframe_by_id(id));
    assert_eq!(property.evaluator, "constant");
    assert_eq!(property.value(), Some(&value));
    assert!(property.keyframes().is_empty());
}

#[test]
fn stable_identity_survives_crossing_and_continues_to_edit_the_same_key() {
    let mut property = Property::constant(number(0.0));
    let moving_id = property
        .upsert_keyframe_with_id(1.0, number(10.0), None)
        .expect("first key should insert");
    let stationary_id = property
        .upsert_keyframe_with_id(2.0, number(20.0), None)
        .expect("second key should insert");

    assert!(property.update_keyframe_by_id(
        moving_id,
        KeyframeUpdate {
            time: Some(3.0),
            ..Default::default()
        }
    ));
    assert_eq!(
        property
            .keyframes()
            .iter()
            .map(|keyframe| keyframe.id)
            .collect::<Vec<_>>(),
        vec![stationary_id, moving_id]
    );

    assert!(property.update_keyframe_by_id(
        moving_id,
        KeyframeUpdate {
            value: Some(number(99.0)),
            easing: Some(EasingFunction::Constant),
            ..Default::default()
        }
    ));
    let moved = property
        .keyframe_by_id(moving_id)
        .expect("moving key should still exist");
    let stationary = property
        .keyframe_by_id(stationary_id)
        .expect("stationary key should still exist");
    assert_eq!(moved.time, OrderedFloat(3.0));
    assert_eq!(moved.value, number(99.0));
    assert_eq!(moved.easing, EasingFunction::Constant);
    assert_eq!(stationary.time, OrderedFloat(2.0));
    assert_eq!(stationary.value, number(20.0));
}

#[test]
fn easing_and_keyframe_identity_survive_serialization_roundtrip() {
    let first = Keyframe::new(0.0, number(0.0), EasingFunction::EaseInQuad);
    let second = Keyframe::new(1.0, number(10.0), EasingFunction::Linear);
    let property = Property::keyframe(vec![second.clone(), first.clone()]);

    assert_eq!(property.evaluate_at(0.5).unwrap(), number(2.5));
    let json = serde_json::to_string(&property).expect("property should serialize");
    assert!(json.contains("\"id\""));
    let loaded: Property = serde_json::from_str(&json).expect("property should deserialize");

    assert_eq!(loaded, property);
    assert_eq!(
        loaded
            .keyframes()
            .iter()
            .map(|keyframe| keyframe.id)
            .collect::<Vec<_>>(),
        vec![first.id, second.id]
    );
    assert_eq!(loaded.evaluate_at(0.5).unwrap(), number(2.5));
}

#[test]
fn value_edits_preserve_expression_and_plugin_evaluator_modes() {
    let mut properties = PropertyMap::new();
    properties.set(
        "expression".to_string(),
        Property::expression("value * 2".to_string(), number(3.0)),
    );
    properties.set(
        "plugin".to_string(),
        Property {
            evaluator: "third-party".to_string(),
            properties: HashMap::from([
                ("value".to_string(), number(4.0)),
                ("configuration".to_string(), PropertyValue::Boolean(true)),
            ]),
        },
    );

    properties.update_property_or_keyframe("expression", 0.0, number(5.0), None);
    properties.update_property_or_keyframe("plugin", 0.0, number(6.0), None);

    let expression = properties.get("expression").unwrap();
    assert_eq!(expression.evaluator, "expression");
    assert_eq!(expression.expression_text(), Some("value * 2"));
    assert_eq!(expression.value(), Some(&number(5.0)));
    let plugin = properties.get("plugin").unwrap();
    assert_eq!(plugin.evaluator, "third-party");
    assert_eq!(plugin.value(), Some(&number(6.0)));
    assert_eq!(
        plugin.properties.get("configuration"),
        Some(&PropertyValue::Boolean(true))
    );
}

#[test]
fn vector_validation_enforces_finite_and_hard_bounds_componentwise() {
    let hard = PropertyDefinition::new(
        "force",
        PropertyUiType::vec3_with_range(-10.0, 10.0, 0.1, "", true, true),
        "Force",
        PropertyValue::Vec3(zero_vec3()),
    );
    let outside = PropertyValue::Vec3(Vec3 {
        x: OrderedFloat(0.0),
        y: OrderedFloat(11.0),
        z: OrderedFloat(0.0),
    });
    assert!(hard.validate_value(&outside).is_err());

    let soft = PropertyDefinition::new(
        "position",
        PropertyUiType::vec3_with_range(-10.0, 10.0, 0.1, "", false, false),
        "Position",
        PropertyValue::Vec3(zero_vec3()),
    );
    soft.validate_value(&outside)
        .expect("soft UI bounds must not reject an authored vector");
    assert!(
        hard.validate_value(&PropertyValue::Vec3(Vec3 {
            x: OrderedFloat(f64::NAN),
            y: OrderedFloat(0.0),
            z: OrderedFloat(0.0),
        }))
        .is_err()
    );
}
