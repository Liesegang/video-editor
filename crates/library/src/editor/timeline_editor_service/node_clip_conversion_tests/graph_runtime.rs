use super::*;

#[test]
fn node_clip_graph_backplate_consumes_its_background_shape_input() {
    let plugins = Arc::new(PluginManager::default());
    let (service, track_id) = small_service("Graph Backplate runtime");
    let (item_id, _) = service
        .add_item(
            track_id,
            "Title".to_string(),
            SourceRef::Text {
                text: "Backplate".to_string(),
                appearance_operations: vec![fill(plugins.as_ref(), Color::white())],
                ensemble_operations: Vec::new(),
            },
            interval(2),
            0,
        )
        .unwrap();
    let conversion = service
        .convert_source_to_node_clip(plugins.as_ref(), item_id)
        .unwrap();

    let project = service.snapshot().unwrap();
    let definition = &project.module_definitions[&conversion.definition_id];
    let text_id = definition
        .graph
        .nodes
        .values()
        .find(|node| {
            matches!(
                node.content(),
                NodeContent::Generator(GeneratorContent::Text)
            )
        })
        .unwrap()
        .id;
    let appearance_stack_id = definition
        .graph
        .nodes
        .values()
        .find(|node| {
            matches!(
                node.content(),
                NodeContent::NativeOperation(operation)
                    if operation.catalog_id == crate::model::node::APPEARANCE_STACK_CATALOG_ID
            )
        })
        .unwrap()
        .id;
    let text_to_appearance = definition
        .graph
        .connections
        .iter()
        .find(|connection| {
            connection.from.node_id == text_id
                && connection.to.node_id == appearance_stack_id
                && connection.to.port == SHAPE_INPUT_PORT
        })
        .unwrap()
        .id;
    drop(project);

    service
        .disconnect_instance_module_connection(conversion.instance_id, text_to_appearance)
        .unwrap();
    let background = AuthoringNodeFactory::create(
        plugins.as_ref(),
        ModuleNodeRequest::Shape {
            path: "M 0 0 H 10 V 10 H 0 Z".to_string(),
            width: 10,
            height: 10,
        },
        96,
        64,
    )
    .unwrap();
    let background_id = background.id;
    service
        .add_instance_module_node(conversion.instance_id, background)
        .unwrap();
    let backplate = plugins
        .create_decorator_operation_node("backplate")
        .unwrap();
    let backplate_id = backplate.id;
    service
        .add_instance_module_node(conversion.instance_id, backplate)
        .unwrap();
    for (from, to) in [
        (
            ModulePortAddress {
                node_id: text_id,
                port: SHAPE_OUTPUT_PORT.to_string(),
            },
            ModulePortAddress {
                node_id: backplate_id,
                port: SHAPE_INPUT_PORT.to_string(),
            },
        ),
        (
            ModulePortAddress {
                node_id: background_id,
                port: SHAPE_OUTPUT_PORT.to_string(),
            },
            ModulePortAddress {
                node_id: backplate_id,
                port: BACKGROUND_SHAPE_INPUT_PORT.to_string(),
            },
        ),
        (
            ModulePortAddress {
                node_id: backplate_id,
                port: SHAPE_OUTPUT_PORT.to_string(),
            },
            ModulePortAddress {
                node_id: appearance_stack_id,
                port: SHAPE_INPUT_PORT.to_string(),
            },
        ),
    ] {
        service
            .connect_instance_module_ports(conversion.instance_id, from, to, 0)
            .unwrap();
    }

    let project = service.snapshot().unwrap();
    let plan = RenderPlanCompiler::compile(&project).unwrap();
    let frame =
        evaluate_render_plan_frame(&project, &plan, plugins.as_ref(), 0, 1.0, None).unwrap();
    let shape = first_shape(&frame.items).expect("Backplate must produce fitted Shape geometry");
    assert_eq!(shape.object.source_node_id, backplate_id);
    assert!(!shape.path.is_empty());
    assert!(
        shape.ensemble.is_none(),
        "graph Backplate must not use paint-time fallback"
    );
    assert!(shape.path_effects.is_empty());
}
