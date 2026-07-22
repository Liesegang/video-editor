use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

use library::model::frame::color::Color;
use library::model::property::{ColorSpaceRef, ColorValue, ColorValueError, PropertyValue};

fn hash(value: &ColorValue) -> u64 {
    let mut hasher = DefaultHasher::new();
    value.hash(&mut hasher);
    hasher.finish()
}

#[test]
fn graph_color_enforces_only_space_finite_and_straight_alpha_invariants()
-> Result<(), Box<dyn std::error::Error>> {
    assert!(matches!(
        ColorSpaceRef::new("  "),
        Err(ColorValueError::EmptyColorSpace)
    ));
    assert!(matches!(
        ColorValue::new(ColorSpaceRef::srgb(), [f64::NAN, 0.0, 0.0, 1.0]),
        Err(ColorValueError::NonFiniteComponent { component: "r" })
    ));
    assert!(matches!(
        ColorValue::new(ColorSpaceRef::srgb(), [0.0, f64::INFINITY, 0.0, 1.0]),
        Err(ColorValueError::NonFiniteComponent { component: "g" })
    ));
    assert!(matches!(
        ColorValue::new(ColorSpaceRef::srgb(), [0.0, 0.0, 0.0, -0.01]),
        Err(ColorValueError::AlphaOutOfRange)
    ));
    assert!(matches!(
        ColorValue::new(ColorSpaceRef::srgb(), [0.0, 0.0, 0.0, 1.01]),
        Err(ColorValueError::AlphaOutOfRange)
    ));

    let hdr = ColorValue::new(
        ColorSpaceRef::new("scene_linear")?,
        [-0.25, 4.0, 65_504.0, 0.5],
    )?;
    assert_eq!(hdr.rgba(), [-0.25, 4.0, 65_504.0, 0.5]);
    assert_eq!(hdr.color_space().as_str(), "scene_linear");
    Ok(())
}

#[test]
fn tagged_property_value_round_trips_without_map_ambiguity()
-> Result<(), Box<dyn std::error::Error>> {
    let color = ColorValue::new(
        ColorSpaceRef::new("scene_linear")?,
        [-0.125, 2.0, 0.25, 0.75],
    )?;
    let property = PropertyValue::ColorValue(color);
    let json = serde_json::to_value(&property)?;
    assert_eq!(
        json,
        serde_json::json!({
            "$type": "color_value",
            "space": "scene_linear",
            "rgba": [-0.125, 2.0, 0.25, 0.75]
        })
    );
    assert_eq!(serde_json::from_value::<PropertyValue>(json)?, property);

    let partial = serde_json::json!({"$type": "color_value", "note": "ordinary map"});
    let partial_value = serde_json::from_value::<PropertyValue>(partial.clone())?;
    assert!(matches!(partial_value, PropertyValue::Map(_)));
    assert_eq!(serde_json::Value::from(&partial_value), partial);

    let extra = serde_json::json!({
        "$type": "color_value",
        "space": "scene_linear",
        "rgba": [0.0, 0.0, 0.0, 1.0],
        "note": "ordinary map"
    });
    let extra_value = serde_json::from_value::<PropertyValue>(extra.clone())?;
    assert!(matches!(extra_value, PropertyValue::Map(_)));
    assert_eq!(serde_json::Value::from(&extra_value), extra);

    for malformed in [
        serde_json::json!({
            "$type": "color_value",
            "space": "",
            "rgba": [0.0, 0.0, 0.0, 1.0]
        }),
        serde_json::json!({
            "$type": "color_value",
            "space": "scene_linear",
            "rgba": [0.0, 0.0, 0.0, 1.5]
        }),
        serde_json::json!({
            "$type": "color_value",
            "space": "scene_linear",
            "rgba": [0.0, 0.0, 1.0]
        }),
    ] {
        let property = serde_json::from_value::<PropertyValue>(malformed.clone())?;
        assert!(matches!(property, PropertyValue::Map(_)));
        assert_eq!(serde_json::Value::from(&property), malformed);
    }

    let legacy_json = serde_json::json!({"r": 1, "g": 2, "b": 3, "a": 4});
    assert!(matches!(
        serde_json::from_value::<PropertyValue>(legacy_json)?,
        PropertyValue::Color(Color {
            r: 1,
            g: 2,
            b: 3,
            a: 4
        })
    ));
    Ok(())
}

#[test]
fn straight_srgba8_adapter_is_exact_and_never_premultiplies()
-> Result<(), Box<dyn std::error::Error>> {
    for channel in u8::MIN..=u8::MAX {
        let legacy = Color {
            r: channel,
            g: u8::MAX - channel,
            b: channel / 2,
            a: channel,
        };
        let graph = ColorValue::from_straight_srgba8(&legacy);
        let [r, g, b, a] = graph.rgba();
        assert_eq!(graph.color_space(), &ColorSpaceRef::srgb());
        assert_eq!(r, f64::from(legacy.r) / 255.0);
        assert_eq!(g, f64::from(legacy.g) / 255.0);
        assert_eq!(b, f64::from(legacy.b) / 255.0);
        assert_eq!(a, f64::from(legacy.a) / 255.0);
        assert_eq!(graph.try_to_straight_srgba8()?, legacy);
    }

    let transparent_red = Color {
        r: 255,
        g: 0,
        b: 0,
        a: 0,
    };
    assert_eq!(
        ColorValue::from_straight_srgba8(&transparent_red).rgba(),
        [1.0, 0.0, 0.0, 0.0]
    );
    Ok(())
}

#[test]
fn legacy_adapter_rejects_quantization_clipping_and_color_space_guessing()
-> Result<(), Box<dyn std::error::Error>> {
    let sub_byte = ColorValue::new(ColorSpaceRef::srgb(), [0.5, 0.0, 0.0, 1.0])?;
    assert!(matches!(
        sub_byte.try_to_straight_srgba8(),
        Err(ColorValueError::NotExactlyRepresentableAsStraightSrgba8 { component: "r" })
    ));

    let hdr = ColorValue::new(ColorSpaceRef::srgb(), [-0.5, 2.0, 0.0, 1.0])?;
    assert!(hdr.try_to_straight_srgba8().is_err());

    let linear = ColorValue::new(ColorSpaceRef::new("scene_linear")?, [1.0, 0.0, 0.0, 1.0])?;
    assert!(matches!(
        linear.try_to_straight_srgba8(),
        Err(ColorValueError::NotStraightSrgba8ColorSpace)
    ));
    Ok(())
}

#[test]
fn equality_hash_and_serde_share_the_validated_value_semantics()
-> Result<(), Box<dyn std::error::Error>> {
    let positive_zero = ColorValue::new(ColorSpaceRef::srgb(), [0.0, 0.25, 1.0, 1.0])?;
    let negative_zero = ColorValue::new(ColorSpaceRef::srgb(), [-0.0, 0.25, 1.0, 1.0])?;
    assert_eq!(positive_zero, negative_zero);
    assert_eq!(hash(&positive_zero), hash(&negative_zero));

    let other_space = ColorValue::new(ColorSpaceRef::new("scene_linear")?, positive_zero.rgba())?;
    assert_ne!(positive_zero, other_space);

    let encoded = serde_json::to_string(&positive_zero)?;
    let decoded: ColorValue = serde_json::from_str(&encoded)?;
    assert_eq!(decoded, positive_zero);
    assert_eq!(hash(&decoded), hash(&positive_zero));
    Ok(())
}
