//! Descriptor-backed appearance evaluation for direct Timeline sources.

use crate::error::LibraryError;
use crate::model::authoring::{AppearanceInputKind, AppearanceOperation, appearance_input_kind};
use crate::model::frame::entity::StyleConfig;
use crate::model::project::EvalOutput;
use crate::plugin::{PluginManager, STYLE_APPLY_OPERATION, STYLE_CATEGORY};

use super::frame_values::evaluate_property_map;

/// Lower the direct Timeline's ordered presentation into ordinary raster,
/// unary image-operation, and Merge commands. This is derived frame data,
/// never a second authored graph or a list reordered by the renderer. Stage
/// evaluation is deferred until its graph input exists, so `NoOutput` follows
/// the same branch-local semantics as the equivalent Node graph.
pub(super) fn appearance_image(
    plugins: &PluginManager,
    operations: &[AppearanceOperation],
    resolution: (u64, u64),
    time: f64,
    fps: f64,
    mut rasterize: impl FnMut(
        StyleConfig,
    ) -> Result<crate::model::frame::entity::FrameItem, LibraryError>,
) -> Result<Option<crate::model::frame::entity::FrameItem>, LibraryError> {
    let mut stages = Vec::with_capacity(operations.len());
    for operation in operations {
        if operation.operation.category != STYLE_CATEGORY
            || operation.operation.operation != STYLE_APPLY_OPERATION
        {
            return Err(LibraryError::Validation(format!(
                "Appearance operation {} has an unsupported contract",
                operation.id
            )));
        }
        let input_kind = appearance_input_kind(&operation.declared_ports).ok_or_else(|| {
            LibraryError::Validation(format!(
                "Appearance operation {} has an unsupported Image contract",
                operation.id
            ))
        })?;
        stages.push((input_kind, operation));
    }
    fold_appearance(
        stages,
        resolution,
        time,
        |operation| evaluate_operation(plugins, operation, time, fps, resolution),
        &mut rasterize,
    )
}

fn fold_appearance<T>(
    stages: impl IntoIterator<Item = (AppearanceInputKind, T)>,
    resolution: (u64, u64),
    time: f64,
    mut evaluate: impl FnMut(&T) -> Result<EvalOutput<StyleConfig>, LibraryError>,
    mut rasterize: impl FnMut(
        StyleConfig,
    ) -> Result<crate::model::frame::entity::FrameItem, LibraryError>,
) -> Result<Option<crate::model::frame::entity::FrameItem>, LibraryError> {
    use crate::model::frame::effect::ImageEffect;
    use crate::model::frame::entity::{FrameGroup, FrameGroupKind, FrameItem};
    let mut current = None;
    for (input_kind, stage) in stages {
        if input_kind == AppearanceInputKind::Image && current.is_none() {
            // The corresponding unary graph Node has no input and therefore
            // does not evaluate its properties/plugin at all.
            continue;
        }
        let EvalOutput::Produced(style) = evaluate(&stage)? else {
            if input_kind == AppearanceInputKind::Image {
                // A failed unary stage consumes its branch. A later Shape
                // raster may start a new branch, exactly like a later Merge.
                current = None;
            }
            // A failed Shape branch contributes nothing to its Merge and
            // therefore leaves any preceding accumulated Image untouched.
            continue;
        };
        let id = style.id;
        let (kind, effects, items) = if input_kind == AppearanceInputKind::Shape {
            let next = rasterize(style)?;
            let Some(previous) = current.take() else {
                current = Some(next);
                continue;
            };
            (FrameGroupKind::Merge, Vec::new(), vec![previous, next])
        } else {
            let Some(previous) = current.take() else {
                // An Image operation cannot manufacture a Shape raster input.
                continue;
            };
            (
                FrameGroupKind::ImageStyle,
                vec![ImageEffect::LayerStyle(style)],
                vec![previous],
            )
        };
        current = Some(FrameItem::Group(FrameGroup {
            source_id: id,
            kind,
            width: resolution.0,
            height: resolution.1,
            background_color: super::frame_values::transparent(),
            transform: Default::default(),
            blend_mode: crate::model::BlendMode::Normal,
            effect_time: ordered_float::OrderedFloat(time),
            effects,
            items,
        }));
    }
    Ok(current)
}

fn evaluate_operation(
    plugins: &PluginManager,
    operation: &AppearanceOperation,
    time: f64,
    fps: f64,
    resolution: (u64, u64),
) -> Result<EvalOutput<StyleConfig>, LibraryError> {
    let descriptor = match plugins.operation_descriptor(
        STYLE_CATEGORY,
        &operation.operation.component_id,
        STYLE_APPLY_OPERATION,
    ) {
        Ok(descriptor) => descriptor,
        Err(error) => {
            log::warn!(
                "Appearance operation {} is unavailable: {error}; producing NoOutput",
                operation.id
            );
            return Ok(EvalOutput::NoOutput);
        }
    };
    if !descriptor.is_execution_compatible_with_ports(&operation.declared_ports) {
        log::warn!(
            "Appearance operation {} descriptor no longer matches its persisted contract; producing NoOutput",
            operation.id
        );
        return Ok(EvalOutput::NoOutput);
    }
    let values = evaluate_property_map(
        &operation.properties,
        time,
        &format!("Appearance operation {}", operation.id),
    )?;
    Ok(plugins.evaluate_style_operation_values(
        &operation.operation.component_id,
        operation.id,
        &values,
        time,
        fps,
        resolution,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::frame::draw_type::DrawStyle;
    use crate::model::frame::effect::ImageEffect;
    use crate::model::frame::entity::{FrameGroup, FrameGroupKind, FrameItem};

    fn style(style: DrawStyle) -> StyleConfig {
        StyleConfig {
            id: uuid::Uuid::new_v4(),
            style,
        }
    }

    fn lower(
        stages: Vec<(AppearanceInputKind, Option<StyleConfig>)>,
    ) -> Result<Option<FrameItem>, LibraryError> {
        fold_appearance(
            stages,
            (128, 96),
            0.0,
            |style| {
                Ok(style
                    .clone()
                    .map_or(EvalOutput::NoOutput, EvalOutput::Produced))
            },
            raster,
        )
    }

    fn raster(style: StyleConfig) -> Result<FrameItem, LibraryError> {
        Ok(FrameItem::Group(FrameGroup {
            source_id: style.id,
            kind: FrameGroupKind::Node,
            width: 128,
            height: 96,
            background_color: super::super::frame_values::transparent(),
            transform: Default::default(),
            blend_mode: crate::model::BlendMode::Normal,
            effect_time: ordered_float::OrderedFloat(0.0),
            effects: vec![],
            items: vec![],
        }))
    }

    fn group(item: &FrameItem) -> &FrameGroup {
        let FrameItem::Group(group) = item else {
            panic!("expected group")
        };
        group
    }

    #[test]
    fn image_stages_keep_authored_order_and_raster_branches_merge_at_their_position() {
        let fill = style(DrawStyle::Fill {
            paint: (crate::model::frame::color::Color::white()).into(),
            opacity: 1.0,
            offset: 0.0,
        });
        let alpha = style(DrawStyle::Opacity { opacity: 0.5 });
        let later_fill = fill.clone();
        let output = lower(vec![
            (AppearanceInputKind::Shape, Some(fill.clone())),
            (AppearanceInputKind::Image, Some(alpha.clone())),
            (AppearanceInputKind::Shape, Some(later_fill)),
        ])
        .unwrap()
        .unwrap();
        let merge = group(&output);
        assert_eq!(merge.kind, FrameGroupKind::Merge);
        assert_eq!(merge.items.len(), 2);
        let image_op = group(&merge.items[0]);
        assert_eq!(image_op.kind, FrameGroupKind::ImageStyle);
        assert_eq!(image_op.effects, vec![ImageEffect::LayerStyle(alpha)]);
        assert_eq!(group(&image_op.items[0]).source_id, fill.id);
        assert_eq!(group(&merge.items[1]).kind, FrameGroupKind::Node);
    }

    #[test]
    fn leading_image_operation_is_not_evaluated_and_later_shape_starts_the_image() {
        let alpha = style(DrawStyle::Opacity { opacity: 0.5 });
        let fill = style(DrawStyle::Fill {
            paint: (crate::model::frame::color::Color::white()).into(),
            opacity: 1.0,
            offset: 0.0,
        });
        let mut evaluated = Vec::new();
        let mut raster_calls = 0;
        let output = fold_appearance(
            vec![
                (AppearanceInputKind::Image, alpha),
                (AppearanceInputKind::Shape, fill.clone()),
            ],
            (128, 96),
            0.0,
            |style| {
                evaluated.push(style.id);
                Ok(EvalOutput::Produced(style.clone()))
            },
            |style| {
                raster_calls += 1;
                raster(style)
            },
        )
        .unwrap()
        .unwrap();
        assert_eq!(evaluated, vec![fill.id]);
        assert_eq!(raster_calls, 1);
        assert_eq!(group(&output).source_id, fill.id);
    }

    #[test]
    fn failed_image_branch_is_empty_until_a_later_shape_restarts_it() {
        let first = style(DrawStyle::Fill {
            paint: (crate::model::frame::color::Color::white()).into(),
            opacity: 1.0,
            offset: 0.0,
        });
        let later = style(DrawStyle::Fill {
            paint: (crate::model::frame::color::Color::white()).into(),
            opacity: 1.0,
            offset: 1.0,
        });
        let output = lower(vec![
            (AppearanceInputKind::Shape, Some(first)),
            (AppearanceInputKind::Image, None),
            (AppearanceInputKind::Shape, Some(later.clone())),
        ])
        .unwrap()
        .unwrap();
        assert_eq!(group(&output).kind, FrameGroupKind::Node);
        assert_eq!(group(&output).source_id, later.id);
    }

    #[test]
    fn failed_shape_branch_preserves_the_preceding_image() {
        let fill = style(DrawStyle::Fill {
            paint: (crate::model::frame::color::Color::white()).into(),
            opacity: 1.0,
            offset: 0.0,
        });
        let alpha = style(DrawStyle::Opacity { opacity: 0.5 });
        let output = lower(vec![
            (AppearanceInputKind::Shape, Some(fill)),
            (AppearanceInputKind::Image, Some(alpha.clone())),
            (AppearanceInputKind::Shape, None),
        ])
        .unwrap()
        .unwrap();
        assert_eq!(group(&output).kind, FrameGroupKind::ImageStyle);
        assert_eq!(group(&output).source_id, alpha.id);
    }

    #[test]
    fn descriptor_input_kind_not_the_returned_draw_style_owns_the_stage_boundary() {
        // A malformed Image plugin returning Fill must not turn into a
        // geometry producer on the direct path while remaining Image in Nodes.
        let fill = style(DrawStyle::Fill {
            paint: (crate::model::frame::color::Color::white()).into(),
            opacity: 1.0,
            offset: 0.0,
        });
        let malformed = style(DrawStyle::Fill {
            paint: (crate::model::frame::color::Color::white()).into(),
            opacity: 1.0,
            offset: 0.0,
        });
        let mut raster_calls = 0;
        let output = fold_appearance(
            vec![
                (AppearanceInputKind::Shape, fill),
                (AppearanceInputKind::Image, malformed.clone()),
            ],
            (128, 96),
            0.0,
            |style| Ok(EvalOutput::Produced(style.clone())),
            |style| {
                raster_calls += 1;
                raster(style)
            },
        )
        .unwrap()
        .unwrap();
        assert_eq!(raster_calls, 1);
        assert_eq!(group(&output).kind, FrameGroupKind::ImageStyle);
        assert_eq!(group(&output).source_id, malformed.id);
    }
}
