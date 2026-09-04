use super::target::EffectorTarget;
use super::types::EffectorConfig;
use super::types::{EffectorContext, TransformData};
use crate::error::LibraryError;
use skia_safe::Point;

#[derive(Clone, Copy, Debug)]
pub struct EffectorElementContext {
    pub global_index: usize,
    pub stable_id: u64,
    pub block_group_id: u64,
    pub line_group_id: u64,
    pub line_index: usize,
    pub line_char_index: usize,
    pub total_chars: usize,
    pub line_char_count: usize,
    pub line_count: usize,
    pub char_center: Point,
    pub line_center: Point,
    pub block_center: Point,
}

/// Evaluates serialized effector configuration for one laid-out character.
/// This is the single runtime path used by the renderer and the unit tests.
pub fn evaluate_configured_transform(
    configs: &[EffectorConfig],
    time: f32,
    element: EffectorElementContext,
) -> Result<TransformData, LibraryError> {
    let mut transform = TransformData::identity();
    for config in configs {
        let target = match config {
            EffectorConfig::Transform { target, .. }
            | EffectorConfig::StepDelay { target, .. }
            | EffectorConfig::Opacity { target, .. }
            | EffectorConfig::Randomize { target, .. } => *target,
        };
        let (
            group_index,
            group_total,
            sequence_index,
            sequence_total,
            random_identity,
            target_center,
        ) = match target {
            EffectorTarget::Block => (
                0,
                1,
                element.global_index,
                element.total_chars,
                element.block_group_id,
                element.block_center,
            ),
            EffectorTarget::Line => (
                element.line_index,
                element.line_count,
                element.line_char_index,
                element.line_char_count,
                element.line_group_id,
                element.line_center,
            ),
            EffectorTarget::Char => (
                element.global_index,
                element.total_chars,
                0,
                1,
                element.stable_id,
                element.char_center,
            ),
            EffectorTarget::Parts => {
                return Err(LibraryError::Render(
                    "Ensemble EffectorTarget::Parts is not supported".to_string(),
                ));
            }
        };
        // Target grouping and Step Delay sequencing are related but distinct.
        // A Block sequences every character, a Line restarts that sequence for
        // each line, and a Char is an independent one-element sequence. Other
        // effectors use the addressed group itself for their context.
        let (index, total) = if matches!(config, EffectorConfig::StepDelay { .. }) {
            (sequence_index, sequence_total)
        } else {
            (group_index, group_total)
        };
        let context = EffectorContext {
            time,
            index,
            total,
            element_index: element.global_index,
            element_identity: random_identity,
            block_group_id: element.block_group_id,
            line_group_id: element.line_group_id,
            line_index: element.line_index,
            char_center: element.char_center,
        };
        match config {
            EffectorConfig::Transform {
                translate,
                rotate,
                scale,
                ..
            } => TransformEffector::new(transform_about_target(
                *translate,
                *rotate,
                *scale,
                target_center,
                element.char_center,
            ))
            .apply(&context, &mut transform),
            EffectorConfig::StepDelay {
                delay_per_element,
                duration,
                from_opacity,
                to_opacity,
                ..
            } => StepDelayEffector::linear(
                *delay_per_element,
                TransformData {
                    opacity: *from_opacity / 100.0,
                    ..TransformData::identity()
                },
                TransformData {
                    opacity: *to_opacity / 100.0,
                    ..TransformData::identity()
                },
                *duration,
            )
            .apply(&context, &mut transform),
            EffectorConfig::Opacity {
                target_opacity,
                mode,
                ..
            } => {
                OpacityEffector::new(*target_opacity / 100.0, *mode).apply(&context, &mut transform)
            }
            EffectorConfig::Randomize {
                translate_range,
                rotate_range,
                scale_range,
                seed,
                ..
            } => RandomizeEffector::new(*translate_range, *rotate_range, *scale_range, *seed)
                .apply(&context, &mut transform),
        }
    }
    Ok(transform)
}

/// Convert a group-pivot transform into the equivalent translation for the
/// per-element drawing boundary. The renderer deliberately owns one glyph
/// draw at a time; compensating its character-center pivot here lets Block,
/// Line, and Char retain their authored grouping semantics without a second
/// Ensemble renderer.
fn transform_about_target(
    translate: (f32, f32),
    rotate_degrees: f32,
    scale: (f32, f32),
    target_center: Point,
    char_center: Point,
) -> TransformData {
    let delta = Point::new(
        target_center.x - char_center.x,
        target_center.y - char_center.y,
    );
    let radians = rotate_degrees.to_radians();
    let (sin, cos) = radians.sin_cos();
    let scaled_x = delta.x * scale.0;
    let scaled_y = delta.y * scale.1;
    let mapped_x = scaled_x * cos - scaled_y * sin;
    let mapped_y = scaled_x * sin + scaled_y * cos;
    TransformData {
        translate: (
            translate.0 + delta.x - mapped_x,
            translate.1 + delta.y - mapped_y,
        ),
        rotate: rotate_degrees,
        scale,
        opacity: 1.0,
        color_override: None,
    }
}

/// Effector（集団制御モディファイア）のトレイト
pub trait Effector: Send + Sync {
    fn apply(&self, ctx: &EffectorContext, transform: &mut TransformData);
    fn name(&self) -> &str;
}

/// 全要素に一様な変形を適用するEffector
pub struct TransformEffector {
    pub transform: TransformData,
}

impl TransformEffector {
    pub fn new(transform: TransformData) -> Self {
        Self { transform }
    }
}

impl Effector for TransformEffector {
    fn apply(&self, _ctx: &EffectorContext, transform: &mut TransformData) {
        *transform = transform.combine(&self.transform);
    }

    fn name(&self) -> &str {
        "Transform"
    }
}

/// 要素ごとに時間差をつけてアニメーションするEffector
pub struct StepDelayEffector {
    pub delay_per_element: f32,
    pub from: TransformData,
    pub to: TransformData,
    pub duration: f32,
    pub easing_fn: fn(f32) -> f32,
}

impl StepDelayEffector {
    pub fn new(
        delay_per_element: f32,
        from: TransformData,
        to: TransformData,
        duration: f32,
        easing_fn: fn(f32) -> f32,
    ) -> Self {
        Self {
            delay_per_element,
            from,
            to,
            duration,
            easing_fn,
        }
    }

    pub fn linear(
        delay_per_element: f32,
        from: TransformData,
        to: TransformData,
        duration: f32,
    ) -> Self {
        Self::new(delay_per_element, from, to, duration, |t| t)
    }
}

impl Effector for StepDelayEffector {
    fn apply(&self, ctx: &EffectorContext, transform: &mut TransformData) {
        // effective_time = global_time - (index * delay)
        let effective_time = ctx.time - (ctx.index as f32 * self.delay_per_element);

        let progress = if self.duration <= 0.0 {
            if effective_time < 0.0 { 0.0 } else { 1.0 }
        } else if effective_time < 0.0 {
            0.0
        } else if effective_time > self.duration {
            1.0
        } else {
            effective_time / self.duration
        };

        let eased = (self.easing_fn)(progress);

        let interpolated = TransformData {
            translate: (
                self.from.translate.0 + (self.to.translate.0 - self.from.translate.0) * eased,
                self.from.translate.1 + (self.to.translate.1 - self.from.translate.1) * eased,
            ),
            rotate: self.from.rotate + (self.to.rotate - self.from.rotate) * eased,
            scale: (
                self.from.scale.0 + (self.to.scale.0 - self.from.scale.0) * eased,
                self.from.scale.1 + (self.to.scale.1 - self.from.scale.1) * eased,
            ),
            opacity: self.from.opacity + (self.to.opacity - self.from.opacity) * eased,
            color_override: if eased >= 1.0 {
                self.to.color_override.clone()
            } else {
                self.from.color_override.clone()
            },
        };

        *transform = transform.combine(&interpolated);
    }

    fn name(&self) -> &str {
        "Step Delay"
    }
}

/// 不透明度を制御するEffector
pub struct OpacityEffector {
    pub target_opacity: f32,
    pub mode: OpacityMode,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize, Hash)]
pub enum OpacityMode {
    Set,      // 直接設定
    Multiply, // 乗算
    Add,      // 加算
}

impl OpacityEffector {
    pub fn new(target_opacity: f32, mode: OpacityMode) -> Self {
        Self {
            target_opacity,
            mode,
        }
    }

    pub fn fade_in(delay_per_element: f32, duration: f32) -> StepDelayEffector {
        let from = TransformData {
            opacity: 0.0,
            ..TransformData::identity()
        };
        let to = TransformData {
            opacity: 1.0,
            ..TransformData::identity()
        };
        StepDelayEffector::linear(delay_per_element, from, to, duration)
    }
}

impl Effector for OpacityEffector {
    fn apply(&self, _ctx: &EffectorContext, transform: &mut TransformData) {
        match self.mode {
            OpacityMode::Set => {
                transform.opacity = self.target_opacity;
            }
            OpacityMode::Multiply => {
                transform.opacity *= self.target_opacity;
            }
            OpacityMode::Add => {
                transform.opacity += self.target_opacity;
            }
        }
    }

    fn name(&self) -> &str {
        "Opacity"
    }
}

/// ランダムな変形を適用するEffector
pub struct RandomizeEffector {
    pub translate_range: (f32, f32),
    pub rotate_range: f32,
    pub scale_range: (f32, f32),
    pub seed: u64,
}

impl RandomizeEffector {
    pub fn new(
        translate_range: (f32, f32),
        rotate_range: f32,
        scale_range: (f32, f32),
        seed: u64,
    ) -> Self {
        Self {
            translate_range,
            rotate_range,
            scale_range,
            seed,
        }
    }

    /// Deterministically mix user seed, stable element identity, and transform
    /// component. SplitMix64's avalanche avoids the nearly identical adjacent
    /// values produced by running one LCG step on `seed + index + component`.
    fn random(&self, element_identity: u64, component: u32) -> f32 {
        let mut value = self
            .seed
            .wrapping_add(element_identity.wrapping_mul(0x9E37_79B9_7F4A_7C15))
            .wrapping_add(
                u64::from(component)
                    .wrapping_add(1)
                    .wrapping_mul(0xD1B5_4A32_D192_ED03),
            );
        value = value.wrapping_add(0x9E37_79B9_7F4A_7C15);
        value = (value ^ (value >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        value = (value ^ (value >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        value ^= value >> 31;
        let mantissa = (value >> 40) as u32;
        mantissa as f32 / (1_u32 << 24) as f32
    }
}

impl Effector for RandomizeEffector {
    fn apply(&self, ctx: &EffectorContext, transform: &mut TransformData) {
        let tx = self.random(ctx.element_identity, 0) * 2.0 - 1.0; // -1.0 ~ 1.0
        let ty = self.random(ctx.element_identity, 1) * 2.0 - 1.0;
        let rot = self.random(ctx.element_identity, 2) * 2.0 - 1.0;
        let sx = self.random(ctx.element_identity, 3) * 2.0 - 1.0;
        let sy = self.random(ctx.element_identity, 4) * 2.0 - 1.0;

        transform.translate.0 += tx * self.translate_range.0;
        transform.translate.1 += ty * self.translate_range.1;
        transform.rotate += rot * self.rotate_range;
        transform.scale.0 += sx * self.scale_range.0;
        transform.scale.1 += sy * self.scale_range.1;
    }

    fn name(&self) -> &str {
        "Randomize"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::ensemble::target::EffectorTarget;
    use crate::core::ensemble::types::EffectorConfig;
    use skia_safe::Point;

    fn element(global_index: usize, line_char_index: usize) -> EffectorElementContext {
        EffectorElementContext {
            global_index,
            stable_id: 0x1000 + global_index as u64,
            block_group_id: 0x10,
            line_group_id: 0x11,
            line_index: 1,
            line_char_index,
            total_chars: 6,
            line_char_count: 3,
            line_count: 2,
            char_center: Point::new(5.0, 5.0),
            line_center: Point::new(15.0, 10.0),
            block_center: Point::new(30.0, 20.0),
        }
    }

    #[test]
    fn test_transform_effector() {
        let effector = TransformEffector::new(TransformData {
            translate: (10.0, 20.0),
            rotate: 0.5,
            scale: (1.0, 1.0),
            opacity: 1.0,
            color_override: None,
        });

        let mut transform = TransformData::identity();
        let ctx = EffectorContext {
            time: 0.0,
            index: 0,
            total: 10,
            element_index: 0,
            element_identity: 0x1000,
            block_group_id: 0x10,
            line_group_id: 0x11,
            line_index: 0,
            char_center: Point::new(0.0, 0.0),
        };

        effector.apply(&ctx, &mut transform);
        assert_eq!(transform.translate, (10.0, 20.0));
        assert_eq!(transform.rotate, 0.5);
    }

    #[test]
    fn test_step_delay_effector() {
        let from = TransformData {
            opacity: 0.0,
            ..TransformData::identity()
        };
        let to = TransformData {
            opacity: 1.0,
            ..TransformData::identity()
        };
        let effector = StepDelayEffector::linear(0.1, from, to, 1.0);

        let mut transform = TransformData::identity();
        let ctx = EffectorContext {
            time: 0.5,
            index: 0,
            total: 10,
            element_index: 0,
            element_identity: 0x1000,
            block_group_id: 0x10,
            line_group_id: 0x11,
            line_index: 0,
            char_center: Point::new(0.0, 0.0),
        };

        effector.apply(&ctx, &mut transform);
        assert!((transform.opacity - 0.5).abs() < 0.01);
    }

    #[test]
    fn test_opacity_effector() {
        let effector = OpacityEffector::new(0.5, OpacityMode::Set);
        let mut transform = TransformData::identity();
        let ctx = EffectorContext {
            time: 0.0,
            index: 0,
            total: 10,
            element_index: 0,
            element_identity: 0x1000,
            block_group_id: 0x10,
            line_group_id: 0x11,
            line_index: 0,
            char_center: Point::new(0.0, 0.0),
        };

        effector.apply(&ctx, &mut transform);
        assert_eq!(transform.opacity, 0.5);
    }

    #[test]
    fn test_randomize_effector() {
        let effector = RandomizeEffector::new((10.0, 10.0), 0.5, (0.2, 0.2), 12345);
        let mut transform = TransformData::identity();
        let ctx = EffectorContext {
            time: 0.0,
            index: 0,
            total: 10,
            element_index: 0,
            element_identity: 0x1000,
            block_group_id: 0x10,
            line_group_id: 0x11,
            line_index: 0,
            char_center: Point::new(0.0, 0.0),
        };

        effector.apply(&ctx, &mut transform);
        // ランダム性があるので、値がidentityから変化していることを確認
        assert_ne!(transform.translate, (0.0, 0.0));
    }

    #[test]
    fn configured_step_delay_uses_authored_target_sequence_scopes() {
        let config = |target| EffectorConfig::StepDelay {
            delay_per_element: 0.25,
            duration: 1.0,
            from_opacity: 0.0,
            to_opacity: 100.0,
            target,
        };
        let block_start =
            evaluate_configured_transform(&[config(EffectorTarget::Block)], 0.0, element(3, 0))
                .unwrap();
        let block_middle =
            evaluate_configured_transform(&[config(EffectorTarget::Block)], 1.25, element(3, 0))
                .unwrap();
        let block_end =
            evaluate_configured_transform(&[config(EffectorTarget::Block)], 2.0, element(3, 0))
                .unwrap();
        assert_eq!(block_start.opacity, 0.0);
        assert!((block_middle.opacity - 0.5).abs() < f32::EPSILON);
        assert_eq!(block_end.opacity, 1.0);

        let line =
            evaluate_configured_transform(&[config(EffectorTarget::Line)], 0.5, element(3, 0))
                .unwrap();
        let character =
            evaluate_configured_transform(&[config(EffectorTarget::Char)], 0.5, element(3, 2))
                .unwrap();
        assert_eq!(line.opacity, 0.5);
        assert_eq!(character.opacity, 0.5);

        let scoped_element = element(3, 2);
        let block =
            evaluate_configured_transform(&[config(EffectorTarget::Block)], 0.75, scoped_element)
                .unwrap();
        let line =
            evaluate_configured_transform(&[config(EffectorTarget::Line)], 0.75, scoped_element)
                .unwrap();
        let character =
            evaluate_configured_transform(&[config(EffectorTarget::Char)], 0.75, scoped_element)
                .unwrap();
        assert_eq!(block.opacity, 0.0);
        assert_eq!(line.opacity, 0.25);
        assert_eq!(character.opacity, 0.75);
    }

    #[test]
    fn transform_target_uses_group_pivot_and_independent_axes() {
        let evaluate = |target, translate, scale, rotate| {
            evaluate_configured_transform(
                &[EffectorConfig::Transform {
                    translate,
                    rotate,
                    scale,
                    target,
                }],
                0.0,
                element(0, 0),
            )
            .unwrap()
        };

        let block = evaluate(EffectorTarget::Block, (3.0, 7.0), (2.0, 1.0), 0.0);
        let line = evaluate(EffectorTarget::Line, (3.0, 7.0), (2.0, 1.0), 0.0);
        let character = evaluate(EffectorTarget::Char, (3.0, 7.0), (2.0, 1.0), 0.0);
        assert_eq!(block.translate, (-22.0, 7.0));
        assert_eq!(line.translate, (-7.0, 7.0));
        assert_eq!(character.translate, (3.0, 7.0));
        assert_eq!(block.scale, (2.0, 1.0));

        let vertical = evaluate(EffectorTarget::Block, (3.0, 7.0), (1.0, 2.0), 0.0);
        assert_eq!(vertical.translate, (3.0, -8.0));
        assert_eq!(vertical.scale, (1.0, 2.0));

        let rotated = evaluate(EffectorTarget::Block, (0.0, 0.0), (1.0, 1.0), 90.0);
        assert!((rotated.translate.0 - 40.0).abs() < 0.001);
        assert!((rotated.translate.1 + 10.0).abs() < 0.001);
        assert_eq!(rotated.rotate, 90.0);
    }

    #[test]
    fn configured_randomize_is_seeded_and_applies_translate_rotate_and_scale() {
        let config = EffectorConfig::Randomize {
            translate_range: (20.0, 30.0),
            rotate_range: 45.0,
            scale_range: (0.5, 0.25),
            seed: 42,
            target: EffectorTarget::Block,
        };
        let first =
            evaluate_configured_transform(std::slice::from_ref(&config), 0.0, element(2, 2))
                .unwrap();
        let repeated = evaluate_configured_transform(&[config], 1.0, element(2, 2)).unwrap();
        assert_eq!(first, repeated);
        assert_ne!(first.translate, (0.0, 0.0));
        assert_ne!(first.rotate, 0.0);
        assert_ne!(first.scale, (1.0, 1.0));
    }

    #[test]
    fn char_randomize_is_stable_well_distributed_and_seeded_per_element() {
        let config = |seed| EffectorConfig::Randomize {
            translate_range: (20.0, 30.0),
            rotate_range: 45.0,
            scale_range: (0.5, 0.25),
            seed,
            // Char uses both the character's stable identity and its sequence
            // index, independently of transient draw order.
            target: EffectorTarget::Char,
        };

        let transforms: Vec<_> = (0..8)
            .map(|index| {
                evaluate_configured_transform(&[config(42)], 0.0, element(index, index % 3))
                    .unwrap()
            })
            .collect();

        for (index, transform) in transforms.iter().enumerate() {
            let repeated =
                evaluate_configured_transform(&[config(42)], 99.0, element(index, index % 3))
                    .unwrap();
            assert_eq!(
                *transform, repeated,
                "Randomize changed with time for element {index}"
            );
        }

        let mean_x = transforms
            .iter()
            .map(|transform| transform.translate.0)
            .sum::<f32>()
            / transforms.len() as f32;
        let mean_y = transforms
            .iter()
            .map(|transform| transform.translate.1)
            .sum::<f32>()
            / transforms.len() as f32;
        let translation_variance = transforms
            .iter()
            .map(|transform| {
                (transform.translate.0 - mean_x).powi(2) + (transform.translate.1 - mean_y).powi(2)
            })
            .sum::<f32>()
            / transforms.len() as f32;
        assert!(
            translation_variance > 100.0,
            "per-character translations are insufficiently distributed: {translation_variance}"
        );

        let largest_pair_distance = transforms
            .iter()
            .enumerate()
            .flat_map(|(left_index, left)| {
                transforms.iter().skip(left_index + 1).map(move |right| {
                    ((left.translate.0 - right.translate.0).powi(2)
                        + (left.translate.1 - right.translate.1).powi(2))
                    .sqrt()
                })
            })
            .fold(0.0_f32, f32::max);
        assert!(
            largest_pair_distance > 25.0,
            "characters remained visually clustered: {largest_pair_distance}"
        );

        let changed_seed =
            evaluate_configured_transform(&[config(43)], 0.0, element(3, 0)).unwrap();
        let seed_distance = ((transforms[3].translate.0 - changed_seed.translate.0).powi(2)
            + (transforms[3].translate.1 - changed_seed.translate.1).powi(2))
        .sqrt();
        assert!(
            seed_distance > 2.0,
            "changing the seed barely changed the character: {seed_distance}"
        );
    }

    #[test]
    fn randomize_uses_block_line_and_character_index_scopes() {
        let config = |target| EffectorConfig::Randomize {
            translate_range: (20.0, 30.0),
            rotate_range: 45.0,
            scale_range: (0.5, 0.25),
            seed: 42,
            target,
        };
        let scoped_element =
            |global_index, line_index, line_char_index, stable_id| EffectorElementContext {
                global_index,
                stable_id,
                block_group_id: 0x10,
                line_group_id: 0x11 + line_index as u64,
                line_index,
                line_char_index,
                total_chars: 4,
                line_char_count: 2,
                line_count: 2,
                char_center: Point::new(5.0, 5.0),
                line_center: Point::new(10.0, 5.0 + line_index as f32 * 20.0),
                block_center: Point::new(10.0, 15.0),
            };
        let elements = [
            scoped_element(0, 0, 0, 0x1000),
            scoped_element(1, 0, 1, 0x1001),
            scoped_element(2, 1, 0, 0x2000),
            scoped_element(3, 1, 1, 0x2001),
        ];
        let transforms = |target| {
            elements
                .iter()
                .map(|element| {
                    evaluate_configured_transform(&[config(target)], 0.0, *element).unwrap()
                })
                .collect::<Vec<_>>()
        };

        let block = transforms(EffectorTarget::Block);
        assert!(block.iter().all(|transform| transform == &block[0]));

        let line = transforms(EffectorTarget::Line);
        assert_eq!(line[0], line[1]);
        assert_eq!(line[2], line[3]);
        assert_ne!(line[0], line[2]);

        let character = transforms(EffectorTarget::Char);
        for (index, transform) in character.iter().enumerate() {
            assert!(
                character
                    .iter()
                    .skip(index + 1)
                    .all(|other| other != transform),
                "character {index} reused another character's random transform"
            );
        }
        assert_eq!(character, transforms(EffectorTarget::Char));

        assert_ne!(block[0], line[0]);
        assert_ne!(line[0], character[0]);
        assert_ne!(block[0], character[0]);
    }

    #[test]
    fn parts_target_is_an_explicit_error() {
        let result = evaluate_configured_transform(
            &[EffectorConfig::Opacity {
                target_opacity: 50.0,
                mode: OpacityMode::Set,
                target: EffectorTarget::Parts,
            }],
            0.0,
            element(0, 0),
        );
        assert!(matches!(result, Err(LibraryError::Render(message)) if message.contains("Parts")));
    }
}
