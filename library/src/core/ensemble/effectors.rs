use super::types::{EffectorContext, TransformData};

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

        let progress = if effective_time < 0.0 {
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

    /// 簡易的な疑似乱数生成（LCG）
    fn random(&self, index: usize, component: u32) -> f32 {
        let seed = self
            .seed
            .wrapping_add(index as u64)
            .wrapping_add(component as u64);
        let a = 1664525u64;
        let c = 1013904223u64;
        let m = 2u64.pow(32);
        let value = (a.wrapping_mul(seed).wrapping_add(c)) % m;
        (value as f32) / (m as f32)
    }
}

impl Effector for RandomizeEffector {
    fn apply(&self, ctx: &EffectorContext, transform: &mut TransformData) {
        let tx = self.random(ctx.index, 0) * 2.0 - 1.0; // -1.0 ~ 1.0
        let ty = self.random(ctx.index, 1) * 2.0 - 1.0;
        let rot = self.random(ctx.index, 2) * 2.0 - 1.0;
        let sx = self.random(ctx.index, 3) * 2.0 - 1.0;
        let sy = self.random(ctx.index, 4) * 2. - 1.0;

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
    use skia_safe::Point;

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
            line_index: 0,
            char_center: Point::new(0.0, 0.0),
        };

        effector.apply(&ctx, &mut transform);
        // ランダム性があるので、値がidentityから変化していることを確認
        assert_ne!(transform.translate, (0.0, 0.0));
    }
}
