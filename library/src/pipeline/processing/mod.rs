//! Processing stage — shape chain evaluation (style, effector, decorator).

pub mod decorator_evaluator;
pub mod effector_evaluator;
pub mod ensemble;
pub mod style_evaluator;
pub mod svg_builder;

pub use decorator_evaluator::DecoratorEvaluator;
pub use effector_evaluator::EffectorEvaluator;
pub use style_evaluator::StyleEvaluator;
