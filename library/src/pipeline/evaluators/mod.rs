//! Backward-compatibility re-exports from pipeline stage submodules.

pub use super::compositing::transform_evaluator as transform;
pub use super::effects::effect_evaluator as effect;
pub use super::processing::decorator_evaluator as decorator;
pub use super::processing::effector_evaluator as effector;
pub use super::processing::style_evaluator as style;
pub use super::processing::svg_builder;
pub use super::sources::clip_evaluator as clip;
