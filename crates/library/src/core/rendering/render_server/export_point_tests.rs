use super::*;
use crate::model::authoring::{ModuleConnection, ModuleConnectionId};
use crate::model::node::{Node, NodeContent, ParticleNodeRole, PointNodeRole};

fn grid_export_project() -> Arc<AuthoringProject> {
    let mut project = particle_export_project().as_ref().clone();
    let definition = project.module_definitions.values_mut().next().unwrap();
    let renderer_id = definition
        .graph
        .nodes
        .values()
        .find(|node| {
            matches!(
                node.content(), NodeContent::NativeOperation(content)
                if content.catalog_id == ParticleNodeRole::SpriteRenderer.catalog_id()
            )
        })
        .unwrap()
        .id;
    let grid = Node::new_catalog_node(PointNodeRole::Grid.catalog_id()).unwrap();
    let grid_id = grid.id;
    // Preserve only the shared Sprite/Output; no Particle stage survives in
    // this project. The source is a real Grid, not an inert Particle fixture.
    let output_id = definition.outputs().next().unwrap().node_id;
    definition
        .graph
        .nodes
        .retain(|id, _| *id == renderer_id || *id == output_id);
    definition
        .graph
        .connections
        .retain(|connection| connection.from.node_id == renderer_id);
    definition
        .interface
        .parameters
        .retain(|parameter| parameter.target.node_id == renderer_id);
    definition.graph.nodes.insert(grid_id, grid);
    definition.graph.connections.push(ModuleConnection {
        id: ModuleConnectionId::new(),
        from: ModulePortAddress {
            node_id: grid_id,
            port: "points".into(),
        },
        to: ModulePortAddress {
            node_id: renderer_id,
            port: "particles".into(),
        },
        order: 0,
        blend_mode: crate::model::BlendMode::Normal,
    });
    for instance in project.module_instances.values_mut() {
        instance.parameter_overrides.retain(|id, _| {
            definition
                .interface
                .parameters
                .iter()
                .any(|parameter| parameter.id == *id)
        });
    }
    definition.topology_revision += 1;
    project.validate().unwrap();
    Arc::new(project)
}

#[test]
fn grid_export_preflight_reaches_the_shared_gpu_point_boundary() {
    let project = grid_export_project();
    let plan = RenderPlanCompiler::compile(&project).unwrap();
    assert!(
        preflight_authoring_video_requires_gpu(
            &project,
            &plan,
            &PluginManager::default(),
            project.root_timeline_id,
            None,
            1,
        )
        .unwrap()
    );
}

#[cfg(all(feature = "gl", target_os = "windows"))]
#[test]
#[ignore = "requires an idle desktop OpenGL 4.3 GPU"]
fn authoring_grid_png_export_matches_preview_and_is_nontransparent() {
    assert_point_png_export_matches_preview(grid_export_project());
}
