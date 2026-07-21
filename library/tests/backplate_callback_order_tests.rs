use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, RwLock};

use anyhow::Result;
use library::core::ensemble::decorators::{BackplateFit, BackplateShape, BackplateTarget};
use library::core::ensemble::types::DecoratorConfig;
use library::editor::project_service::ProjectManager;
use library::framing::get_frame_from_project;
use library::model::Clip;
use library::model::frame::color::Color;
use library::model::project::{
    BACKGROUND_SHAPE_INPUT_PORT, Composition, NodeContainer, NodeGraphBundle, PortAddress,
    PortOwner, Project, ProjectConnection, SHAPE_INPUT_PORT, SHAPE_OUTPUT_PORT,
};
use library::model::property::{PropertyDefinition, PropertyMap};
use library::plugin::{
    DecoratorPlugin, FrameEvaluationContext, OperationDescriptor, OperationDescriptorError, Plugin,
    PluginManager,
};
use uuid::Uuid;

const WIDTH: u64 = 180;
const HEIGHT: u64 = 100;
const FPS: f64 = 10.0;

struct CountingDecoratorPlugin {
    id: &'static str,
    calls: Arc<AtomicUsize>,
    legacy: bool,
}

impl Plugin for CountingDecoratorPlugin {
    fn id(&self) -> &str {
        self.id
    }

    fn name(&self) -> String {
        self.id.to_string()
    }

    fn category(&self) -> String {
        "Test".to_string()
    }

    fn version(&self) -> (u32, u32, u32) {
        (0, 0, 0)
    }
}

impl DecoratorPlugin for CountingDecoratorPlugin {
    fn properties(&self) -> Vec<PropertyDefinition> {
        Vec::new()
    }

    fn descriptor(&self) -> Result<OperationDescriptor, OperationDescriptorError> {
        if self.legacy {
            OperationDescriptor::decorator(self.id(), self.name(), self.properties())
        } else {
            OperationDescriptor::backplate(self.id(), self.name(), self.properties())
        }
    }

    fn evaluate_source(
        &self,
        _context: &FrameEvaluationContext,
        _source_id: Uuid,
        _properties: &PropertyMap,
        _eval_time: f64,
    ) -> Option<DecoratorConfig> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        if self.legacy {
            Some(DecoratorConfig::LegacyBackplate {
                target: BackplateTarget::Block,
                shape: BackplateShape::Rect,
                color: Color {
                    r: 0,
                    g: 0,
                    b: 0,
                    a: 255,
                },
                padding: (0.0, 0.0, 0.0, 0.0),
                corner_radius: 0.0,
            })
        } else {
            Some(DecoratorConfig::Backplate {
                target: BackplateTarget::Block,
                padding: (0.0, 0.0, 0.0, 0.0),
                offset: (0.0, 0.0),
                fit: BackplateFit::Stretch,
            })
        }
    }
}

fn graph_project(
    plugins: &Arc<PluginManager>,
    component_id: &str,
    with_target: bool,
    with_background: bool,
) -> Result<Project> {
    let factory = ProjectManager::new(
        Arc::new(RwLock::new(Project::new("Backplate callback factory"))),
        plugins.clone(),
    );
    let target = factory.create_shape_node("M 10 10 H 50 V 30 H 10 Z", WIDTH, HEIGHT, 1, 1)?;
    let background = factory.create_shape_node("M 0 0 L 10 0 L 5 10 Z", WIDTH, HEIGHT, 1, 1)?;
    let backplate = plugins.create_decorator_operation_node(component_id)?;
    let style = plugins.create_style_operation_node("fill")?;
    let target_id = target.id;
    let background_id = background.id;
    let backplate_id = backplate.id;
    let style_id = style.id;
    let mut connections = vec![ProjectConnection::new(
        PortAddress::new(PortOwner::Node(backplate_id), SHAPE_OUTPUT_PORT),
        PortAddress::new(PortOwner::Node(style_id), SHAPE_INPUT_PORT),
        0,
    )];
    if with_target {
        connections.push(ProjectConnection::new(
            PortAddress::new(PortOwner::Node(target_id), SHAPE_OUTPUT_PORT),
            PortAddress::new(PortOwner::Node(backplate_id), SHAPE_INPUT_PORT),
            0,
        ));
    }
    if with_background {
        connections.push(ProjectConnection::new(
            PortAddress::new(PortOwner::Node(background_id), SHAPE_OUTPUT_PORT),
            PortAddress::new(PortOwner::Node(backplate_id), BACKGROUND_SHAPE_INPUT_PORT),
            0,
        ));
    }

    let graph = NodeGraphBundle::new(
        vec![target, background, backplate, style],
        connections,
        Some(style_id),
    );
    let mut project = Project::new("Backplate callback order");
    let (composition, track) = Composition::new("main", WIDTH, HEIGHT, FPS, 2.0);
    let track_id = track.id;
    project.add_track(track)?;
    project.add_composition(composition)?;
    let clip = Clip::new("shape", 0.0, 2.0);
    let clip_id = clip.id;
    project.add_clip(clip);
    project.attach_clip_to_track(track_id, clip_id)?;
    project.insert_node_graph(NodeContainer::Clip(clip_id), graph)?;
    Ok(project)
}

fn evaluate(project: &Project, plugins: &Arc<PluginManager>) -> Result<bool> {
    Ok(!get_frame_from_project(
        project,
        0,
        0,
        1.0,
        None,
        &plugins.get_property_evaluators(),
        plugins,
    )?
    .items
    .is_empty())
}

#[test]
fn backplate_resolves_required_shapes_before_callback_without_changing_v1() -> Result<()> {
    let calls = Arc::new(AtomicUsize::new(0));
    let plugins = Arc::new(PluginManager::default());
    plugins.register_decorator_plugin(Arc::new(CountingDecoratorPlugin {
        id: "counting_backplate_v2",
        calls: calls.clone(),
        legacy: false,
    }));

    for (with_target, with_background) in [(false, true), (true, false)] {
        let project = graph_project(
            &plugins,
            "counting_backplate_v2",
            with_target,
            with_background,
        )?;
        assert!(!evaluate(&project, &plugins)?);
        assert_eq!(
            calls.load(Ordering::SeqCst),
            0,
            "a missing required Shape must prevent the plugin callback"
        );
    }

    let project = graph_project(&plugins, "counting_backplate_v2", true, true)?;
    assert!(evaluate(&project, &plugins)?);
    assert_eq!(
        calls.load(Ordering::SeqCst),
        1,
        "a valid two-Shape Backplate invokes its callback exactly once"
    );

    let legacy_calls = Arc::new(AtomicUsize::new(0));
    plugins.register_decorator_plugin(Arc::new(CountingDecoratorPlugin {
        id: "counting_backplate_v1",
        calls: legacy_calls.clone(),
        legacy: true,
    }));
    let project = graph_project(&plugins, "counting_backplate_v1", true, false)?;
    assert!(evaluate(&project, &plugins)?);
    assert_eq!(
        legacy_calls.load(Ordering::SeqCst),
        1,
        "the frozen one-Shape v1 contract must not require a background input"
    );
    Ok(())
}
