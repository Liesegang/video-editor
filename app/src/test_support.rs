use library::editor::project_service::{GeneratorNodeRequest, ProjectManager};
use library::model::{Node, Project};
use library::plugin::PluginManager;
use std::sync::{Arc, RwLock};

pub(crate) fn generator_node(name: &str, request: GeneratorNodeRequest) -> Node {
    generator_node_for_canvas(name, request, 1920, 1080, 1920, 1080)
}

pub(crate) fn generator_node_for_canvas(
    name: &str,
    request: GeneratorNodeRequest,
    canvas_width: u64,
    canvas_height: u64,
    clip_width: u64,
    clip_height: u64,
) -> Node {
    let manager = ProjectManager::new(
        Arc::new(RwLock::new(Project::new("app test generator factory"))),
        Arc::new(PluginManager::default()),
    );
    let result = manager.create_generator_node(
        request,
        canvas_width,
        canvas_height,
        clip_width,
        clip_height,
    );
    assert!(
        result.is_ok(),
        "built-in Generator converter must create a complete test Node: {result:?}"
    );
    let mut node = result.unwrap_or_else(|_| Node::new_merge("invalid Generator test fallback"));
    node.name = name.to_string();
    node
}
