pub mod config;
pub mod decorators;
pub mod effectors;
pub mod target;
pub mod types;

pub use config::{DecoratorConfig, EffectorConfig, EnsembleData};
pub use decorators::{BackplateDecorator, BackplateShape, BackplateTarget, Decorator};
pub use effectors::{
    Effector, OpacityEffector, RandomizeEffector, StepDelayEffector, TransformEffector,
};
pub use target::{EffectorEntry, EffectorTarget};
pub use types::{EffectorContext, EnsembleChar, EnsembleLine, EnsembleText, TransformData};
