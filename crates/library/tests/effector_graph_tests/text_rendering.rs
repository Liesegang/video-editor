use std::sync::{Arc, RwLock};

use anyhow::{Context, Result as AnyResult, bail};
use library::editor::project_service::ProjectManager;
use library::model::NodeContent;
use library::model::frame::color::Color;
use library::model::frame::entity::FrameContent;
use library::model::frame::runtime_shape::{
    RuntimeShapeGeometry, evaluate_text_element_transforms,
};
use library::model::project::Project;
use library::model::property::{PropertyValue, Vec2};
use library::plugin::{FrameEvaluationContext, PluginManager};
use ordered_float::OrderedFloat;

use super::support::{
    HEIGHT, WIDTH, assert_alpha_inside_preview_bounds, evaluate, first_content, first_object,
    group_effect_time, insert_effector_chain, preview, project_with_graph, render_frame,
    root_transform_id, set_constant,
};

#[test]
fn normal_nonensemble_text_pixels_are_stable_across_project_roundtrip() -> AnyResult<()> {
    let plugins = Arc::new(PluginManager::default());
    let manager = ProjectManager::new(
        Arc::new(RwLock::new(Project::new("factory"))),
        plugins.clone(),
    );
    let graph = manager
        .create_text_graph("PARITY", "Arial", WIDTH, HEIGHT)
        .context("create Text graph")?;
    let (project, _) = project_with_graph(graph, 0.0, 2.0)?;
    let frame = evaluate(&project, &plugins, 0)?;
    let FrameContent::Text { ensemble, .. } =
        first_content(&frame.items).context("rendered Text content is missing")?
    else {
        bail!("plain Style graph did not render Text content");
    };
    assert!(
        ensemble.is_none(),
        "a plain Style branch must stay non-Ensemble"
    );
    let expected = preview(&project, &plugins, 0)?;
    assert!(expected.data.iter().any(|channel| *channel != 0));

    let loaded = Project::load(&project.save()?)?;
    assert_eq!(loaded, project);
    assert_eq!(preview(&loaded, &plugins, 0)?.data, expected.data);
    Ok(())
}

#[test]
fn graph_randomize_char_is_deterministic_and_seeded_by_element_identity() -> AnyResult<()> {
    let plugins = Arc::new(PluginManager::default());
    let manager = ProjectManager::new(
        Arc::new(RwLock::new(Project::new("factory"))),
        plugins.clone(),
    );
    let mut graph = manager
        .create_text_graph("AA\nAA", "Arial", WIDTH, HEIGHT)
        .context("create Text graph")?;
    let text_id = graph
        .nodes
        .iter()
        .find(|node| {
            matches!(
                node.content(),
                NodeContent::Generator(library::model::GeneratorContent::Text)
            )
        })
        .context("Text graph has no Text source")?
        .id;
    let mut random = plugins.create_effector_operation_node("randomize")?;
    set_constant(&mut random, "seed", 7.0.into());
    set_constant(&mut random, "translate_range", 8.0.into());
    set_constant(&mut random, "rotate_range", 12.0.into());
    set_constant(&mut random, "scale_range", 0.25.into());
    set_constant(&mut random, "target", PropertyValue::String("Char".into()));
    let random_id = random.id;
    graph.nodes.push(random);
    insert_effector_chain(&mut graph, &[random_id])?;
    let (project, _) = project_with_graph(graph, 0.0, 2.0)?;

    let image_a = preview(&project, &plugins, 0)?;
    let image_b = preview(&project, &plugins, 0)?;
    assert_eq!(image_a.data, image_b.data);

    let frame = evaluate(&project, &plugins, 0)?;
    let FrameContent::Text {
        ensemble: Some(ensemble),
        ..
    } = first_content(&frame.items).context("rendered Text content is missing")?
    else {
        bail!("Randomize graph did not produce Text EnsembleData");
    };

    let evaluators = plugins.get_property_evaluators();
    let context = FrameEvaluationContext {
        project: &project,
        composition: project
            .compositions
            .first()
            .context("project has no Composition")?,
        property_evaluators: &evaluators,
        plugin_manager: &plugins,
        resolved_inputs: None,
    };
    let RuntimeShapeGeometry::Text(runtime_text) = plugins
        .get_entity_converter("text")
        .context("Text entity converter is missing")?
        .convert_shape(
            &context,
            project
                .get_node(text_id)
                .context("Text source is missing")?,
            0.0,
        )
        .context("Text converter produced no Shape")?
        .geometry
    else {
        bail!("Text converter did not produce runtime text geometry");
    };
    assert_eq!(runtime_text.elements.len(), 4);
    assert_ne!(
        runtime_text.elements[0].line_group_id, runtime_text.elements[2].line_group_id,
        "repeated characters on separate lines need distinct line identities"
    );
    let transforms = evaluate_text_element_transforms(&runtime_text, ensemble, 0.0)?;
    assert_eq!(
        transforms,
        evaluate_text_element_transforms(&runtime_text, ensemble, 0.0)?,
        "the same seed and element identities must reproduce exactly"
    );
    assert!(
        transforms
            .iter()
            .skip(1)
            .any(|transform| { transforms.first().is_some_and(|first| transform != first) }),
        "all character identities reused one seeded transform"
    );
    let loaded = Project::load(&project.save()?)?;
    assert_eq!(image_a.data, preview(&loaded, &plugins, 0)?.data);

    let mut changed_seed = project;
    set_constant(
        changed_seed
            .get_node_mut(random_id)
            .context("Randomize operation is missing")?,
        "seed",
        8.0.into(),
    );
    assert_ne!(image_a.data, preview(&changed_seed, &plugins, 0)?.data);
    Ok(())
}

#[test]
fn style_local_scope_time_drives_ensemble_bounds_and_pixels() -> AnyResult<()> {
    let plugins = Arc::new(PluginManager::default());
    let manager = ProjectManager::new(
        Arc::new(RwLock::new(Project::new("factory"))),
        plugins.clone(),
    );
    let mut graph = manager
        .create_text_graph("ABCD", "Arial", WIDTH, HEIGHT)
        .context("create Text graph")?;
    let source_id = graph
        .nodes
        .iter()
        .find(|node| {
            matches!(
                node.content(),
                NodeContent::Generator(library::model::GeneratorContent::Text)
            )
        })
        .context("Text graph has no Text source")?
        .id;
    let transform_id = root_transform_id(&graph)?;
    let style_id = graph
        .nodes
        .iter()
        .find(|node| {
            matches!(
                node.content(),
                NodeContent::PluginOperation(operation) if operation.category == "style"
            )
        })
        .context("Text graph has no Style operation")?
        .id;
    set_constant(
        graph
            .nodes
            .iter_mut()
            .find(|node| node.id == source_id)
            .context("Text source is missing")?,
        "size",
        18.0.into(),
    );
    let transform = graph
        .nodes
        .iter_mut()
        .find(|node| node.id == transform_id)
        .context("Text root Transform is missing")?;
    set_constant(
        transform,
        "position",
        PropertyValue::Vec2(Vec2 {
            x: OrderedFloat(20.0),
            y: OrderedFloat(12.0),
        }),
    );
    set_constant(
        transform,
        "anchor",
        PropertyValue::Vec2(Vec2 {
            x: OrderedFloat(0.0),
            y: OrderedFloat(0.0),
        }),
    );
    let mut delay = plugins
        .create_effector_operation_node("step_delay")
        .context("create StepDelay operation")?;
    set_constant(&mut delay, "delay", 0.5.into());
    set_constant(&mut delay, "duration", 0.0.into());
    set_constant(&mut delay, "from_opacity", 0.0.into());
    set_constant(&mut delay, "to_opacity", 100.0.into());
    set_constant(&mut delay, "target", PropertyValue::String("Block".into()));
    let delay_id = delay.id;
    graph.nodes.push(delay);
    insert_effector_chain(&mut graph, &[delay_id])?;

    let (mut local_project, _) = project_with_graph(graph.clone(), 2.0, 4.0)?;
    let (mut global_project, _) = project_with_graph(graph, 0.0, 4.0)?;
    for project in [&mut local_project, &mut global_project] {
        project
            .compositions
            .first_mut()
            .context("project has no Composition")?
            .background_color = Color {
            r: 0,
            g: 0,
            b: 0,
            a: 0,
        };
    }

    let local_frame = evaluate(&local_project, &plugins, 21)?;
    let global_frame = evaluate(&global_project, &plugins, 21)?;
    let local_time = group_effect_time(&local_frame.items, style_id)
        .context("local Style effect time is missing")?;
    let global_time = group_effect_time(&global_frame.items, style_id)
        .context("global Style effect time is missing")?;
    assert!((local_time - 0.1).abs() < 1e-9);
    assert!((global_time - 2.1).abs() < 1e-9);

    let local_bounds = first_object(&local_frame.items)
        .context("local frame has no object")?
        .content_bounds
        .context("local object has no Preview bounds")?;
    let global_bounds = first_object(&global_frame.items)
        .context("global frame has no object")?
        .content_bounds
        .context("global object has no Preview bounds")?;
    assert!(
        local_bounds.width < global_bounds.width,
        "bounds must evaluate StepDelay at Style-local time, not global time"
    );
    let local_image = render_frame(&local_frame, &plugins)?;
    let global_image = render_frame(&global_frame, &plugins)?;
    assert_alpha_inside_preview_bounds(&local_frame, &local_image)?;
    assert_alpha_inside_preview_bounds(&global_frame, &global_image)?;
    assert_ne!(local_image.data, global_image.data);
    Ok(())
}
