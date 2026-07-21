use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

use ordered_float::OrderedFloat;

use super::{
    FillRule, PathContour, PathPoint, PathSegment, PathValidationError, PathValue,
    SvgPathCodecError, SvgPathEnvelope, decode_svg_path, encode_svg_path,
    parse_legacy_svg_path_data, write_legacy_svg_path_data,
};

fn complete_path() -> PathValue {
    PathValue::new(
        FillRule::EvenOdd,
        vec![
            PathContour::new(
                PathPoint::new(1.0, 2.0),
                vec![
                    PathSegment::line(PathPoint::new(3.0, 4.0)),
                    PathSegment::quadratic(PathPoint::new(5.0, 6.0), PathPoint::new(7.0, 8.0)),
                    PathSegment::conic(PathPoint::new(9.0, 10.0), PathPoint::new(11.0, 12.0), 0.75),
                    PathSegment::cubic(
                        PathPoint::new(13.0, 14.0),
                        PathPoint::new(15.0, 16.0),
                        PathPoint::new(17.0, 18.0),
                    ),
                ],
                true,
            ),
            PathContour::new(
                PathPoint::new(-1.0, -2.0),
                vec![PathSegment::line(PathPoint::new(-3.0, -4.0))],
                false,
            ),
        ],
    )
    .unwrap()
}

#[test]
fn canonical_path_serde_round_trip_preserves_hash_and_equality() {
    let path = complete_path();
    let json = serde_json::to_string(&path).unwrap();
    let restored: PathValue = serde_json::from_str(&json).unwrap();
    assert_eq!(restored, path);

    let hash = |value: &PathValue| {
        let mut hasher = DefaultHasher::new();
        value.hash(&mut hasher);
        hasher.finish()
    };
    assert_eq!(hash(&restored), hash(&path));
}

#[test]
fn every_coordinate_and_conic_weight_must_be_finite() {
    let invalid_start = PathValue::new(
        FillRule::NonZero,
        vec![PathContour::new(
            PathPoint::new(f64::NAN, 0.0),
            Vec::new(),
            false,
        )],
    )
    .unwrap_err();
    assert!(matches!(
        invalid_start,
        PathValidationError::NonFiniteCoordinate {
            contour_index: 0,
            axis: "x",
            ..
        }
    ));
    assert!(invalid_start.to_string().contains("start.x"));

    let invalid_weight = PathValue::new(
        FillRule::NonZero,
        vec![PathContour::new(
            PathPoint::new(0.0, 0.0),
            vec![PathSegment::Conic {
                control: PathPoint::new(1.0, 1.0),
                to: PathPoint::new(2.0, 0.0),
                weight: OrderedFloat(f64::INFINITY),
            }],
            false,
        )],
    )
    .unwrap_err();
    assert!(matches!(
        invalid_weight,
        PathValidationError::NonFiniteConicWeight {
            contour_index: 0,
            segment_index: 0,
            ..
        }
    ));
}

#[test]
fn deserialization_runs_canonical_validation() {
    let json = r#"{
        "fill_rule":"non_zero",
        "contours":[{
            "start":{"x":0.0,"y":0.0},
            "segments":[{
                "kind":"conic",
                "control":{"x":1.0,"y":1.0},
                "to":{"x":2.0,"y":0.0},
                "weight":1e400
            }],
            "closed":false
        }]
    }"#;
    let error = serde_json::from_str::<PathValue>(json).unwrap_err();
    assert!(error.to_string().contains("number out of range"));
}

#[test]
fn svg_round_trip_preserves_contours_fill_rule_and_curve_verbs() {
    let source = SvgPathEnvelope::new(
        "M0 0 L20 0 Q30 10 20 20 C15 30 5 30 0 20 Z M40 0 L50 10",
        FillRule::EvenOdd,
    );
    let first = decode_svg_path(&source).unwrap();
    assert_eq!(first.contours().len(), 2);
    assert!(first.contours()[0].is_closed());
    assert!(!first.contours()[1].is_closed());
    assert_eq!(first.fill_rule(), FillRule::EvenOdd);

    let encoded = encode_svg_path(&first).unwrap();
    assert_eq!(encoded.fill_rule(), FillRule::EvenOdd);
    let second = decode_svg_path(&encoded).unwrap();
    assert_eq!(second, first);
}

#[test]
fn svg_arc_conic_weights_survive_the_explicit_boundary() {
    let source = SvgPathEnvelope::new("M0 0 A25 25 0 0 1 25 25", FillRule::NonZero);
    let first = decode_svg_path(&source).unwrap();
    let weights = conic_weights(&first);
    assert!(
        !weights.is_empty(),
        "SVG arc did not decode to conic segments"
    );

    let second = decode_svg_path(&encode_svg_path(&first).unwrap()).unwrap();
    assert_eq!(conic_weights(&second), weights);
    assert_eq!(second, first);

    let serialized = serde_json::to_string(&encode_svg_path(&first).unwrap()).unwrap();
    let restored: SvgPathEnvelope = serde_json::from_str(&serialized).unwrap();
    assert_eq!(decode_svg_path(&restored).unwrap(), first);
}

#[test]
fn legacy_string_adapter_is_explicit_and_keeps_existing_path_data_working() {
    let path = parse_legacy_svg_path_data("M1 2 L3 4 Z").unwrap();
    assert_eq!(path.fill_rule(), FillRule::NonZero);
    let encoded = write_legacy_svg_path_data(&path).unwrap();
    assert_eq!(parse_legacy_svg_path_data(&encoded).unwrap(), path);
}

#[test]
fn legacy_string_adapter_rejects_unrepresentable_conic_weight() {
    let path = decode_svg_path(&SvgPathEnvelope::new(
        "M0 0 A25 25 0 0 1 25 25",
        FillRule::NonZero,
    ))
    .unwrap();
    let error = write_legacy_svg_path_data(&path).unwrap_err();
    assert!(matches!(
        error,
        SvgPathCodecError::LegacyConicUnsupported { .. }
    ));
}

fn conic_weights(path: &PathValue) -> Vec<OrderedFloat<f64>> {
    path.contours()
        .iter()
        .flat_map(PathContour::segments)
        .filter_map(|segment| match segment {
            PathSegment::Conic { weight, .. } => Some(*weight),
            _ => None,
        })
        .collect()
}
