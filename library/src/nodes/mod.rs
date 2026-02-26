//! Co-located node definitions and evaluators.
//!
//! Each node category lives in its own submodule containing both the
//! `NodeTypeDefinition` declarations and the `NodeEvaluator` implementation.

pub mod compositing;
pub mod decorator;
pub mod effect;
pub mod effector;
pub mod source;
pub mod style;

// Definition-only modules (no evaluator yet)
mod color;
mod data;
mod generators;
mod image_defs;
mod logic;
mod math;
mod particles;
mod path;
mod scripting;
mod text_defs;
mod threed;
mod time;

use crate::pipeline::evaluator::NodeEvaluator;
use crate::plugin::node_types::{NodeCategory, NodeTypeDefinition};
use crate::project::connection::{PinDataType, PinDefinition};

// ---------------------------------------------------------------------------
// Pin helpers — used by definition modules
// ---------------------------------------------------------------------------

pub(crate) fn inp(name: &str, display: &str, dt: PinDataType) -> PinDefinition {
    PinDefinition::input(name, display, dt)
}

pub(crate) fn out(name: &str, display: &str, dt: PinDataType) -> PinDefinition {
    PinDefinition::output(name, display, dt)
}

pub(crate) fn node(type_id: &str, name: &str, cat: NodeCategory) -> NodeTypeDefinition {
    NodeTypeDefinition::new(type_id, name, cat)
}

// ---------------------------------------------------------------------------
// Pattern helpers — eliminate repeated boilerplate
// ---------------------------------------------------------------------------

/// Blend node: foreground + background + opacity → image.
pub(crate) fn blend_node(type_id: &str, name: &str) -> NodeTypeDefinition {
    use PinDataType::*;
    node(type_id, name, NodeCategory::Compositing)
        .with_inputs(vec![
            inp("foreground", "Foreground", Image),
            inp("background", "Background", Image),
            inp("opacity", "Opacity", Scalar),
        ])
        .with_outputs(vec![out("image", "Image", Image)])
}

/// Particle modifier: particles in + extra params → particles out.
pub(crate) fn particle_modifier(
    type_id: &str,
    name: &str,
    extra_inputs: Vec<PinDefinition>,
) -> NodeTypeDefinition {
    use PinDataType::*;
    let mut inputs = vec![inp("particles", "Particles", ParticleSystem)];
    inputs.extend(extra_inputs);
    node(type_id, name, NodeCategory::Particles)
        .with_inputs(inputs)
        .with_outputs(vec![out("particles", "Particles", ParticleSystem)])
}

/// Image filter: image in + extra params → image out.
pub(crate) fn image_filter(
    type_id: &str,
    name: &str,
    cat: NodeCategory,
    extra_inputs: Vec<PinDefinition>,
) -> NodeTypeDefinition {
    use PinDataType::*;
    let mut inputs = vec![inp("image", "Image", Image)];
    inputs.extend(extra_inputs);
    node(type_id, name, cat)
        .with_inputs(inputs)
        .with_outputs(vec![out("image", "Image", Image)])
}

// ---------------------------------------------------------------------------
// Aggregation
// ---------------------------------------------------------------------------

/// All built-in node type definitions.
pub fn all_definitions() -> Vec<NodeTypeDefinition> {
    [
        compositing::definitions(),
        effect::definitions(),
        style::definitions(),
        effector::definitions(),
        decorator::definitions(),
        scripting::scripting_nodes(),
        data::data_nodes(),
        text_defs::text_nodes(),
        math::math_nodes(),
        logic::logic_nodes(),
        color::color_nodes(),
        generators::generator_nodes(),
        path::path_nodes(),
        time::time_nodes(),
        image_defs::image_nodes(),
        particles::particle_nodes(),
        threed::threed_nodes(),
    ]
    .concat()
}

/// All built-in node evaluators.
pub fn all_evaluators() -> Vec<Box<dyn NodeEvaluator>> {
    vec![
        Box::new(source::SourceEvaluator),
        Box::new(compositing::TransformEvaluator),
        Box::new(compositing::BlendEvaluator),
        Box::new(compositing::PreviewOutputEvaluator),
        Box::new(effect::EffectEvaluator),
        Box::new(style::StyleEvaluator),
        Box::new(effector::EffectorEvaluator),
        Box::new(decorator::DecoratorEvaluator),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_all_node_definitions_count() {
        let defs = all_definitions();
        // 99 original + blend nodes already included = 99
        // After adding blend evaluator, the count stays the same since
        // blend definitions were already in compositing_nodes()
        assert!(
            defs.len() >= 99,
            "Expected at least 99 node definitions, got {}",
            defs.len()
        );
    }

    #[test]
    fn test_no_duplicate_type_ids() {
        let defs = all_definitions();
        let mut seen = std::collections::HashSet::new();
        for def in &defs {
            assert!(
                seen.insert(&def.type_id),
                "Duplicate type_id: {}",
                def.type_id
            );
        }
    }

    #[test]
    fn test_all_categories_covered() {
        let defs = all_definitions();
        let categories: std::collections::HashSet<_> = defs.iter().map(|d| d.category).collect();

        assert!(categories.contains(&NodeCategory::Scripting));
        assert!(categories.contains(&NodeCategory::Data));
        assert!(categories.contains(&NodeCategory::Text));
        assert!(categories.contains(&NodeCategory::Math));
        assert!(categories.contains(&NodeCategory::Logic));
        assert!(categories.contains(&NodeCategory::Compositing));
        assert!(categories.contains(&NodeCategory::Color));
        assert!(categories.contains(&NodeCategory::Generator));
        assert!(categories.contains(&NodeCategory::Filters));
        assert!(categories.contains(&NodeCategory::Distortion));
        assert!(categories.contains(&NodeCategory::Path));
        assert!(categories.contains(&NodeCategory::Time));
        assert!(categories.contains(&NodeCategory::Image));
        assert!(categories.contains(&NodeCategory::Particles));
        assert!(categories.contains(&NodeCategory::ThreeD));
        assert!(categories.contains(&NodeCategory::Style));
        assert!(categories.contains(&NodeCategory::Effector));
        assert!(categories.contains(&NodeCategory::Decorator));
    }

    #[test]
    fn test_preview_output_definition_exists() {
        let defs = all_definitions();
        let preview = defs
            .iter()
            .find(|d| d.type_id == "compositing.preview_output");
        assert!(
            preview.is_some(),
            "compositing.preview_output definition missing"
        );
        let preview = preview.unwrap();
        assert_eq!(preview.inputs.len(), 1);
        assert_eq!(preview.inputs[0].name, "image_in");
        assert!(preview.outputs.is_empty());
    }

    #[test]
    fn test_all_evaluators_registered() {
        let evaluators = all_evaluators();
        assert_eq!(evaluators.len(), 8);
    }
}
