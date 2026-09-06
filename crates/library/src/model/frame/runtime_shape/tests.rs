use super::*;
use crate::core::ensemble::target::EffectorTarget;
use crate::core::ensemble::types::{EffectorConfig, EnsembleData};
use crate::core::rendering::path_geometry::to_skia_path;
use crate::model::frame::color::Color;
use crate::model::frame::draw_type::DrawStyle;
use crate::model::path::{FillRule, PathContour, PathPoint, PathSegment, PathValue};

#[test]
fn text_ensemble_transform_target_uses_block_line_and_character_pivots() {
    let text =
        crate::core::rendering::text_layout::layout_runtime_text_shape("AB\nCD", "Arial", 42.0);
    assert_eq!(text.lines.len(), 2);
    assert_eq!(text.elements.len(), 4);
    let transforms = |target| {
        evaluate_text_element_transforms(
            &text,
            &EnsembleData {
                enabled: true,
                effector_configs: vec![EffectorConfig::Transform {
                    translate: (13.0, -7.0),
                    rotate: 0.0,
                    scale: (2.0, 0.5),
                    target,
                }],
                decorator_configs: Vec::new(),
                patches: Default::default(),
            },
            0.0,
        )
        .unwrap()
    };

    let block = transforms(EffectorTarget::Block);
    let line = transforms(EffectorTarget::Line);
    let character = transforms(EffectorTarget::Char);
    assert_ne!(block, line);
    assert_ne!(line, character);
    for transform in &character {
        assert_eq!(transform.translate, (13.0, -7.0));
        assert_eq!(transform.scale, (2.0, 0.5));
    }
    for runtime_line in &text.lines {
        let mapped_line = runtime_line
            .element_range
            .clone()
            .map(|index| transformed_text_element_bounds(&text.elements[index], &line[index]))
            .reduce(RuntimeBounds::union)
            .expect("line has visible elements");
        let mapped_center = bounds_center(mapped_line);
        let original_center = bounds_center(runtime_line.bounds);
        assert!((mapped_center.x - (original_center.x + 13.0)).abs() < 0.01);
        assert!((mapped_center.y - (original_center.y - 7.0)).abs() < 0.01);
    }

    let first_center = text_element_center(&text.elements[0]);
    let mapped = transformed_text_element_bounds(&text.elements[0], &block[0]);
    let mapped_center = bounds_center(mapped);
    let block_center = bounds_center(text.block_bounds);
    assert!(
        (mapped_center.x - (block_center.x + 13.0 + (first_center.x - block_center.x) * 2.0)).abs()
            < 0.01
    );
    assert!(
        (mapped_center.y - (block_center.y - 7.0 + (first_center.y - block_center.y) * 0.5)).abs()
            < 0.01
    );
}

#[test]
fn ensemble_decoration_outset_is_applied_after_element_scale() {
    let text = crate::core::rendering::text_layout::layout_runtime_text_shape("A", "Arial", 100.0);
    let patch = TransformData {
        translate: (0.0, 0.0),
        rotate: 0.0,
        scale: (0.1, 0.1),
        opacity: 1.0,
        color_override: None,
    };
    let ensemble = EnsembleData {
        enabled: true,
        effector_configs: Vec::new(),
        decorator_configs: Vec::new(),
        patches: std::collections::HashMap::from([(0, patch)]),
    };
    let styles = vec![
        StyleConfig {
            id: Uuid::new_v4(),
            style: DrawStyle::Fill {
                color: Color::white(),
                offset: 0.0,
            },
        },
        StyleConfig {
            id: Uuid::new_v4(),
            style: DrawStyle::DropShadow {
                color: Color::black(),
                opacity: 1.0,
                blend_mode: crate::model::BlendMode::Normal,
                angle: 0.0,
                distance: 50.0,
                spread: 0.0,
                size: 0.0,
            },
        },
    ];
    let transforms = evaluate_text_element_transforms(&text, &ensemble, 0.0).unwrap();
    let scaled_body = transformed_text_element_bounds(&text.elements[0], &transforms[0]);
    let visual = measure_ensemble_text_visual_bounds(&text, &styles, &ensemble, 0.0)
        .unwrap()
        .expect("scaled Ensemble text has visual bounds");

    assert!(
        visual.right >= scaled_body.right + 49.0,
        "Drop Shadow decoration was incorrectly scaled with the glyph: body={scaled_body:?}, visual={visual:?}"
    );
}

#[test]
fn canonical_conic_bounds_do_not_use_the_quadratic_svg_fallback() -> Result<(), LibraryError> {
    let value = PathValue::new(
        FillRule::NonZero,
        vec![PathContour::new(
            PathPoint::new(0.0, 0.0),
            vec![PathSegment::conic(
                PathPoint::new(50.0, 100.0),
                PathPoint::new(100.0, 0.0),
                0.2,
            )],
            false,
        )],
    )
    .map_err(|error| LibraryError::Render(error.to_string()))?;
    let direct = to_skia_path(&value)?;
    let direct_bounds = direct.compute_tight_bounds();
    let fallback = crate::model::path::encode_svg_path(&value)
        .map_err(|error| LibraryError::Render(error.to_string()))?
        .into_path_data();
    let fallback_path = skia_safe::Path::from_svg(&fallback)
        .ok_or_else(|| LibraryError::Render("invalid test SVG fallback".to_string()))?;
    let fallback_bounds = fallback_path.compute_tight_bounds();
    assert!(
        (direct_bounds.bottom - fallback_bounds.bottom).abs() > 1.0,
        "weighted conic and quadratic fallback unexpectedly share bounds"
    );

    let runtime_bounds = RuntimeBounds::new(
        direct_bounds.left,
        direct_bounds.top,
        direct_bounds.right,
        direct_bounds.bottom,
    );
    let source_id = Uuid::new_v4();
    let shape = RuntimeShape {
        source_id,
        geometry: RuntimeShapeGeometry::Path(RuntimePathShape {
            path: fallback.clone(),
            canonical_path: Some(value.clone()),
            bounds: runtime_bounds,
            path_effects: Vec::new(),
            parts: vec![RuntimePathPart {
                path: fallback,
                canonical_path: Some(value.clone()),
                bounds: runtime_bounds,
                stable_id: 7,
                block_group_id: 7,
                line_group_id: 7,
                line_index: 0,
                opacity: 0.4,
            }],
        }),
        spatial_transform_node_id: None,
        spatial_transform: Default::default(),
        modulation_transform: Default::default(),
        transform: Default::default(),
        effects: Vec::new(),
        effector_configs: Vec::new(),
        decorator_configs: Vec::new(),
    };
    let style = StyleConfig {
        id: Uuid::new_v4(),
        style: DrawStyle::Fill {
            color: Color::white(),
            offset: 0.0,
        },
    };
    let object = shape.into_styled_object(style, 0.0)?;
    let bounds = object
        .content_bounds
        .ok_or_else(|| LibraryError::Render("styled conic has no bounds".to_string()))?;
    let (_, _, _, rendered_height) = bounds.as_tuple();
    assert!((rendered_height - (direct_bounds.height() + 2.0)).abs() <= f32::EPSILON);
    let FrameContent::Shape {
        canonical_path: Some(rendered),
        parts,
        ..
    } = &object.content
    else {
        return Err(LibraryError::Render(
            "styled conic dropped canonical geometry".to_string(),
        ));
    };
    assert_eq!(rendered, &value);
    assert!(matches!(
        parts.as_slice(),
        [FramePathPart { opacity, .. }] if opacity.into_inner() == 0.4
    ));
    let FrameContent::Shape { styles, .. } = &object.content else {
        return Err(LibraryError::Render(
            "styled conic changed content type".to_string(),
        ));
    };
    assert!(matches!(
        styles.first().map(|style| &style.style),
        Some(DrawStyle::Fill { color, .. }) if color.a == 255
    ));
    Ok(())
}
