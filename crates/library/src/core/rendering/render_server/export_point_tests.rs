use super::*;
use crate::model::authoring::{ModuleConnection, ModuleConnectionId};
use crate::model::node::{ColorContent, Node, NodeContent, ParticleNodeRole, PointNodeRole};
use crate::model::point::PointAttributeElementType;

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

fn typed_color_export_project() -> Arc<AuthoringProject> {
    let mut project = grid_export_project().as_ref().clone();
    let definition = project.module_definitions.values_mut().next().unwrap();
    let source_connection = definition
        .graph
        .connections
        .iter()
        .find(|connection| connection.to.port == "particles")
        .unwrap()
        .clone();
    let grid_id = source_connection.from.node_id;
    let renderer_id = source_connection.to.node_id;
    let info = Node::new_catalog_node(PointNodeRole::Info.catalog_id()).unwrap();
    let info_id = info.id;
    let ramp = Node::new_color("Point Color Ramp", ColorContent::ColorRamp);
    let ramp_id = ramp.id;
    let store = Node::new_catalog_node(
        PointNodeRole::StoreAttribute(PointAttributeElementType::Color).catalog_id(),
    )
    .unwrap();
    let store_id = store.id;
    for node in [info, ramp, store] {
        definition.graph.nodes.insert(node.id, node);
    }
    definition
        .graph
        .connections
        .retain(|connection| connection.id != source_connection.id);
    definition.interface.parameters.retain(|parameter| {
        parameter.target.node_id != renderer_id || parameter.target.port != "color"
    });
    for (source, output, target, input) in [
        (grid_id, "points", info_id, "points"),
        (grid_id, "points", store_id, "points"),
        (info_id, "random", ramp_id, "factor"),
        (ramp_id, "color", store_id, "value"),
        (store_id, "points", renderer_id, "particles"),
        (store_id, "attribute", renderer_id, "color"),
    ] {
        definition.graph.connections.push(ModuleConnection {
            id: ModuleConnectionId::new(),
            from: ModulePortAddress {
                node_id: source,
                port: output.into(),
            },
            to: ModulePortAddress {
                node_id: target,
                port: input.into(),
            },
            order: 0,
            blend_mode: crate::model::BlendMode::Normal,
        });
    }
    definition.topology_revision += 1;
    definition.interface_version += 1;
    for instance in project.module_instances.values_mut() {
        instance.parameter_overrides.retain(|id, _| {
            definition
                .interface
                .parameters
                .iter()
                .any(|parameter| parameter.id == *id)
        });
    }
    project.validate().unwrap();
    Arc::new(project)
}

#[test]
fn typed_color_export_compiles_the_attribute_into_the_shared_point_program() {
    let project = typed_color_export_project();
    let plan = RenderPlanCompiler::compile(&project).unwrap();
    let definition = plan.module_definitions.values().next().unwrap();
    let renderer = definition.point_renderers.values().next().unwrap();
    let program = renderer.point_program.as_ref().unwrap();
    assert_eq!(program.schema.attributes().len(), 1);
    assert_eq!(
        program.schema.attributes()[0].element_type(),
        PointAttributeElementType::Color
    );
    assert!(matches!(
        program.instructions[usize::from(program.color_register)],
        crate::core::render_plan::CompiledPointInstruction::LoadAttribute { attribute: 0 }
    ));
}

#[cfg(all(feature = "gl", target_os = "windows"))]
#[test]
#[ignore = "requires an idle desktop OpenGL 4.3 GPU"]
fn authoring_typed_color_point_png_export_matches_preview_and_is_nontransparent() {
    assert_point_png_export_matches_preview(typed_color_export_project());
}

fn vector_fields_export_project() -> Arc<AuthoringProject> {
    use crate::model::node::NUMERIC_LENGTH_CATALOG_ID;
    use crate::model::property::{Property, PropertyValue, Vec3};

    let mut project = typed_color_export_project().as_ref().clone();
    let definition = project.module_definitions.values_mut().next().unwrap();
    let find_catalog = |id: &str| {
        definition.graph.nodes.values().find(|node| {
            matches!(node.content(), NodeContent::NativeOperation(operation) if operation.catalog_id == id)
        }).unwrap().id
    };
    let grid = find_catalog(PointNodeRole::Grid.catalog_id());
    let info = find_catalog(PointNodeRole::Info.catalog_id());
    let color =
        find_catalog(PointNodeRole::StoreAttribute(PointAttributeElementType::Color).catalog_id());
    let ramp = definition
        .graph
        .nodes
        .values()
        .find(|node| matches!(node.content(), NodeContent::Color(ColorContent::ColorRamp)))
        .unwrap()
        .id;
    let mut scale = Node::new_multiply("Spatial scale");
    scale
        .set_property(
            "b".into(),
            Property::constant(PropertyValue::Vec3(Vec3 {
                x: 0.01.into(),
                y: 0.02.into(),
                z: 0.03.into(),
            })),
        )
        .unwrap();
    let position = Node::new_catalog_node(
        PointNodeRole::StoreAttribute(PointAttributeElementType::Vec3).catalog_id(),
    )
    .unwrap();
    let length = Node::new_catalog_node(NUMERIC_LENGTH_CATALOG_ID).unwrap();
    let (scale_id, position_id, length_id) = (scale.id, position.id, length.id);
    for node in [scale, position, length] {
        definition.graph.nodes.insert(node.id, node);
    }
    definition.graph.connections.retain(|edge| {
        !(edge.to.node_id == color && edge.to.port == "points"
            || edge.from.node_id == info && edge.to.node_id == ramp)
    });
    for (source, output, target, input) in [
        (grid, "points", position_id, "points"),
        (info, "position", scale_id, "a"),
        (scale_id, "result", position_id, "value"),
        (position_id, "points", color, "points"),
        (position_id, "attribute", length_id, "value"),
        (length_id, "result", ramp, "factor"),
    ] {
        definition.graph.connections.push(ModuleConnection {
            id: ModuleConnectionId::new(),
            from: ModulePortAddress {
                node_id: source,
                port: output.into(),
            },
            to: ModulePortAddress {
                node_id: target,
                port: input.into(),
            },
            order: 0,
            blend_mode: crate::model::BlendMode::Normal,
        });
    }
    definition.topology_revision += 1;
    project.validate().unwrap();
    Arc::new(project)
}

#[test]
fn vector_fields_export_uses_the_shared_numeric_point_program() {
    let project = vector_fields_export_project();
    let plan = RenderPlanCompiler::compile(&project).unwrap();
    let definition = plan.module_definitions.values().next().unwrap();
    let renderer = definition.point_renderers.values().next().unwrap();
    let program = renderer.point_program.as_ref().unwrap();
    assert_eq!(
        program.schema.attributes()[0].element_type(),
        PointAttributeElementType::Vec3
    );
    assert!(program.instructions.iter().any(|instruction| matches!(
        instruction,
        crate::core::render_plan::CompiledPointInstruction::Length { .. }
    )));
}

#[cfg(all(feature = "gl", target_os = "windows"))]
#[test]
#[ignore = "requires an idle desktop OpenGL 4.3 GPU"]
fn authoring_vector_fields_point_png_export_matches_preview_and_is_nontransparent() {
    assert_point_png_export_matches_preview(vector_fields_export_project());
}

fn conditional_fields_export_project() -> Arc<AuthoringProject> {
    use crate::model::conditional::ComparisonOperation;
    use crate::model::node::ConditionalNodeRole;
    use crate::model::property::{Property, PropertyValue};

    let mut project = typed_color_export_project().as_ref().clone();
    let definition = project.module_definitions.values_mut().next().unwrap();
    let store_id = definition.graph.nodes.values().find(|node| {
        matches!(node.content(), NodeContent::NativeOperation(op)
            if op.catalog_id == PointNodeRole::StoreAttribute(PointAttributeElementType::Color).catalog_id())
    }).unwrap().id;
    let color_input = definition
        .graph
        .connections
        .iter()
        .find(|edge| edge.to.node_id == store_id && edge.to.port == "value")
        .unwrap()
        .from
        .clone();
    let random_input = definition
        .graph
        .connections
        .iter()
        .find(|edge| edge.to.node_id == color_input.node_id && edge.to.port == "factor")
        .unwrap()
        .from
        .clone();
    let point_input = definition
        .graph
        .connections
        .iter()
        .find(|edge| edge.to.node_id == store_id && edge.to.port == "points")
        .unwrap()
        .from
        .clone();
    let mut compare = Node::new_catalog_node(
        ConditionalNodeRole::Compare(ComparisonOperation::Greater).catalog_id(),
    )
    .unwrap();
    compare
        .set_property(
            "b".into(),
            Property::constant(PropertyValue::Number(0.5.into())),
        )
        .unwrap();
    let mask = Node::new_catalog_node(
        PointNodeRole::StoreAttribute(PointAttributeElementType::Boolean).catalog_id(),
    )
    .unwrap();
    let mut select =
        Node::new_catalog_node(ConditionalNodeRole::Select(PortDataType::Color).catalog_id())
            .unwrap();
    // False points become red; true points retain their ramp-generated color.
    select
        .set_property(
            "if_false".into(),
            Property::constant(PropertyValue::ColorValue(
                crate::model::property::ColorValue::from_straight_srgba8(&Color {
                    r: 255,
                    g: 0,
                    b: 0,
                    a: 255,
                }),
            )),
        )
        .unwrap();
    let (compare_id, mask_id, select_id) = (compare.id, mask.id, select.id);
    for node in [compare, mask, select] {
        definition.graph.nodes.insert(node.id, node);
    }
    definition.graph.connections.retain(|edge| {
        !(edge.to.node_id == store_id && matches!(edge.to.port.as_str(), "value" | "points"))
    });
    for (source, output, target, input) in [
        (
            point_input.node_id,
            point_input.port.as_str(),
            mask_id,
            "points",
        ),
        (
            random_input.node_id,
            random_input.port.as_str(),
            compare_id,
            "a",
        ),
        (compare_id, "result", mask_id, "value"),
        (mask_id, "points", store_id, "points"),
        (mask_id, "attribute", select_id, "condition"),
        (
            color_input.node_id,
            color_input.port.as_str(),
            select_id,
            "if_true",
        ),
        (select_id, "result", store_id, "value"),
    ] {
        definition.graph.connections.push(ModuleConnection {
            id: ModuleConnectionId::new(),
            from: ModulePortAddress {
                node_id: source,
                port: output.into(),
            },
            to: ModulePortAddress {
                node_id: target,
                port: input.into(),
            },
            order: 0,
            blend_mode: crate::model::BlendMode::Normal,
        });
    }
    definition.topology_revision += 1;
    project.validate().unwrap();
    Arc::new(project)
}

#[test]
fn conditional_fields_export_uses_boolean_capture_and_color_selection() {
    let project = conditional_fields_export_project();
    let plan = RenderPlanCompiler::compile(&project).unwrap();
    let definition = plan.module_definitions.values().next().unwrap();
    let renderer = definition.point_renderers.values().next().unwrap();
    let program = renderer.point_program.as_ref().unwrap();
    assert_eq!(
        program.schema.attributes()[0].element_type(),
        PointAttributeElementType::Boolean
    );
    assert!(program.instructions.iter().any(|instruction| matches!(
        instruction,
        crate::core::render_plan::CompiledPointInstruction::Select { .. }
    )));
}

#[cfg(all(feature = "gl", target_os = "windows"))]
#[test]
#[ignore = "requires an idle desktop OpenGL 4.3 GPU"]
fn authoring_conditional_fields_point_png_export_matches_preview_and_is_nontransparent() {
    assert_point_png_export_matches_preview(conditional_fields_export_project());
}

fn positioned_points_export_project() -> Arc<AuthoringProject> {
    use crate::model::property::{Property, PropertyValue, Vec3};

    let mut project = vector_fields_export_project().as_ref().clone();
    let definition = project.module_definitions.values_mut().next().unwrap();
    let position_attribute = definition
        .graph
        .nodes
        .values()
        .find(|node| {
            matches!(node.content(), NodeContent::NativeOperation(operation)
                if operation.catalog_id == PointNodeRole::StoreAttribute(PointAttributeElementType::Vec3).catalog_id())
        })
        .unwrap()
        .id;
    let renderer_input = definition
        .graph
        .connections
        .iter()
        .find(|edge| edge.to.port == "particles")
        .unwrap()
        .clone();
    let mut position = Node::new_catalog_node(PointNodeRole::SetPosition.catalog_id()).unwrap();
    position
        .set_property(
            "offset".into(),
            Property::constant(PropertyValue::Vec3(Vec3 {
                x: 36.0.into(),
                y: (-20.0).into(),
                z: 8.0.into(),
            })),
        )
        .unwrap();
    let position_id = position.id;
    definition.graph.nodes.insert(position_id, position);
    definition
        .graph
        .connections
        .retain(|edge| edge.id != renderer_input.id);
    for (from, output, to, input) in [
        (renderer_input.from.node_id, "points", position_id, "points"),
        (position_attribute, "attribute", position_id, "position"),
        (
            position_id,
            "points",
            renderer_input.to.node_id,
            "particles",
        ),
    ] {
        definition.graph.connections.push(ModuleConnection {
            id: ModuleConnectionId::new(),
            from: ModulePortAddress {
                node_id: from,
                port: output.into(),
            },
            to: ModulePortAddress {
                node_id: to,
                port: input.into(),
            },
            order: 0,
            blend_mode: crate::model::BlendMode::Normal,
        });
    }
    definition.topology_revision += 1;
    project.validate().unwrap();
    Arc::new(project)
}

#[test]
fn positioned_points_export_uses_attribute_driven_geometry_in_the_shared_program() {
    let project = positioned_points_export_project();
    let plan = RenderPlanCompiler::compile(&project).unwrap();
    let definition = plan.module_definitions.values().next().unwrap();
    let program = definition
        .point_renderers
        .values()
        .next()
        .unwrap()
        .point_program
        .as_ref()
        .unwrap();
    assert!(program.position_register.is_some());
    assert!(program.instructions.iter().any(|instruction| matches!(
        instruction,
        crate::core::render_plan::CompiledPointInstruction::LoadAttribute { attribute: 0 }
    )));
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
fn authoring_positioned_points_png_export_matches_preview_and_is_nontransparent() {
    assert_point_png_export_matches_preview(positioned_points_export_project());
}

fn sized_points_export_project() -> Arc<AuthoringProject> {
    use crate::model::node::NUMERIC_LENGTH_CATALOG_ID;
    use crate::model::property::{Property, PropertyValue};

    let mut project = positioned_points_export_project().as_ref().clone();
    let definition = project.module_definitions.values_mut().next().unwrap();
    let find_catalog = |id: &str| {
        definition
            .graph
            .nodes
            .values()
            .find(|node| {
                matches!(node.content(), NodeContent::NativeOperation(operation)
                    if operation.catalog_id == id)
            })
            .unwrap()
            .id
    };
    let position_store =
        find_catalog(PointNodeRole::StoreAttribute(PointAttributeElementType::Vec3).catalog_id());
    let color_store =
        find_catalog(PointNodeRole::StoreAttribute(PointAttributeElementType::Color).catalog_id());
    let length = find_catalog(NUMERIC_LENGTH_CATALOG_ID);
    let renderer_input = definition
        .graph
        .connections
        .iter()
        .find(|edge| edge.to.port == "particles")
        .unwrap()
        .clone();
    let position_to_color = definition
        .graph
        .connections
        .iter()
        .find(|edge| {
            edge.from.node_id == position_store
                && edge.to.node_id == color_store
                && edge.to.port == "points"
        })
        .unwrap()
        .clone();
    let number_store = Node::new_catalog_node(
        PointNodeRole::StoreAttribute(PointAttributeElementType::Number).catalog_id(),
    )
    .unwrap();
    let mut size = Node::new_catalog_node(PointNodeRole::SetSize.catalog_id()).unwrap();
    size.set_property(
        "scale".into(),
        Property::constant(PropertyValue::Number(12.0.into())),
    )
    .unwrap();
    let (number_store_id, size_id) = (number_store.id, size.id);
    definition.graph.nodes.insert(number_store_id, number_store);
    definition.graph.nodes.insert(size_id, size);
    definition
        .graph
        .connections
        .retain(|edge| edge.id != position_to_color.id && edge.id != renderer_input.id);
    for (source, output, target, input) in [
        (position_store, "points", number_store_id, "points"),
        (length, "result", number_store_id, "value"),
        (number_store_id, "points", color_store, "points"),
        (renderer_input.from.node_id, "points", size_id, "points"),
        (number_store_id, "attribute", size_id, "size"),
        (size_id, "points", renderer_input.to.node_id, "particles"),
    ] {
        definition.graph.connections.push(ModuleConnection {
            id: ModuleConnectionId::new(),
            from: ModulePortAddress {
                node_id: source,
                port: output.into(),
            },
            to: ModulePortAddress {
                node_id: target,
                port: input.into(),
            },
            order: 0,
            blend_mode: crate::model::BlendMode::Normal,
        });
    }
    definition.topology_revision += 1;
    project.validate().unwrap();
    Arc::new(project)
}

#[test]
fn sized_points_export_keeps_position_and_attribute_driven_size_in_one_program() {
    let project = sized_points_export_project();
    let plan = RenderPlanCompiler::compile(&project).unwrap();
    let program = plan
        .module_definitions
        .values()
        .next()
        .unwrap()
        .point_renderers
        .values()
        .next()
        .unwrap()
        .point_program
        .as_ref()
        .unwrap();
    assert!(program.position_register.is_some());
    assert!(program.size_register.is_some());
    assert!(
        program
            .schema
            .attributes()
            .iter()
            .any(|attribute| attribute.element_type() == PointAttributeElementType::Number)
    );
    assert!(program.instructions.iter().any(|instruction| matches!(
        instruction,
        crate::core::render_plan::CompiledPointInstruction::LoadAttribute { .. }
    )));
}

#[cfg(all(feature = "gl", target_os = "windows"))]
#[test]
#[ignore = "requires an idle desktop OpenGL 4.3 GPU"]
fn authoring_sized_points_png_export_matches_preview_and_is_nontransparent() {
    assert_point_png_export_matches_preview(sized_points_export_project());
}
