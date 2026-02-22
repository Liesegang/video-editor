//! Backward-compatibility re-exports from `processing::ensemble`.

pub use super::processing::ensemble::config;
pub use super::processing::ensemble::decorators;
pub use super::processing::ensemble::effectors;
pub use super::processing::ensemble::target;
pub use super::processing::ensemble::types;
pub use super::sources::text as text_decompose;

pub use super::processing::ensemble::config::{DecoratorConfig, EffectorConfig, EnsembleData};
pub use super::processing::ensemble::decorators::{
    BackplateDecorator, BackplateShape, BackplateTarget, Decorator,
};
pub use super::processing::ensemble::effectors::{
    Effector, OpacityEffector, RandomizeEffector, StepDelayEffector, TransformEffector,
};
pub use super::processing::ensemble::target::{EffectorEntry, EffectorTarget};
pub use super::processing::ensemble::types::{
    EffectorContext, EnsembleChar, EnsembleLine, EnsembleText, TransformData,
};
