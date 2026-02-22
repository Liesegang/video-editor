//! Hardcoded node type definitions for the data-flow graph.
//!
//! Reference: node_list.yml (documentation only, not loaded at runtime).

mod color;
mod compositing;
mod data;
mod decorator;
mod effector;
mod effects;
mod generators;
mod image;
mod logic;
mod math;
mod particles;
mod path;
mod scripting;
mod style;
mod text;
mod threed;
mod time;

use crate::plugin::PluginManager;
use crate::plugin::node_types::{NodeCategory, NodeTypeDefinition};
use crate::project::connection::{PinDataType, PinDefinition};

/// Register all built-in node type definitions.
pub(crate) fn register_all_node_types(manager: &PluginManager) {
    for def in all_node_definitions() {
        manager.register_node_type(def);
    }
}

// ---------------------------------------------------------------------------
// Pin helpers
// ---------------------------------------------------------------------------

fn inp(name: &str, display: &str, dt: PinDataType) -> PinDefinition {
    PinDefinition::input(name, display, dt)
}

fn out(name: &str, display: &str, dt: PinDataType) -> PinDefinition {
    PinDefinition::output(name, display, dt)
}

fn node(type_id: &str, name: &str, cat: NodeCategory) -> NodeTypeDefinition {
    NodeTypeDefinition::new(type_id, name, cat)
}

// ---------------------------------------------------------------------------
// Pattern helpers — eliminate repeated boilerplate
// ---------------------------------------------------------------------------

/// Blend node: foreground + background + opacity → image.
fn blend_node(type_id: &str, name: &str) -> NodeTypeDefinition {
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
fn particle_modifier(
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
fn image_filter(
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

fn all_node_definitions() -> Vec<NodeTypeDefinition> {
    [
        scripting::scripting_nodes(),
        data::data_nodes(),
        text::text_nodes(),
        math::math_nodes(),
        logic::logic_nodes(),
        compositing::compositing_nodes(),
        color::color_nodes(),
        generators::generator_nodes(),
        effects::filter_nodes(),
        path::path_nodes(),
        time::time_nodes(),
        image::image_nodes(),
        particles::particle_nodes(),
        threed::threed_nodes(),
        style::style_nodes(),
        effector::effector_nodes(),
        decorator::decorator_nodes(),
    ]
    .concat()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_all_node_definitions_count() {
        let defs = all_node_definitions();
        assert_eq!(defs.len(), 99, "Expected 99 node definitions");
    }

    #[test]
    fn test_no_duplicate_type_ids() {
        let defs = all_node_definitions();
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
        let defs = all_node_definitions();
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
}
