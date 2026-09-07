use super::*;
use crate::model::NodeContent;
use crate::model::authoring::PublishedParameterAutomationCapability;

fn catalog_node<'a>(
    plexus: &'a PlexusNodeClipDefinition,
    catalog_id: &str,
) -> &'a crate::model::node::Node {
    let nodes = plexus
        .definition
        .graph
        .nodes
        .values()
        .filter(|node| {
            matches!(
                node.content(),
                NodeContent::NativeOperation(operation) if operation.catalog_id == catalog_id
            )
        })
        .collect::<Vec<_>>();
    assert_eq!(nodes.len(), 1, "expected one {catalog_id} Node");
    nodes[0]
}

#[test]
fn factory_builds_reusable_finite_plexus_graph_and_published_interface() {
    let plexus = PlexusNodeClipFactory::create("Plexus Network").expect("factory");
    let definition = &plexus.definition;
    assert_eq!(
        definition.sharing,
        ModuleDefinitionSharing::ReusableTemplate(ModuleTemplateOrigin::Project)
    );
    assert_eq!(definition.graph.nodes.len(), 4);
    assert_eq!(definition.graph.connections.len(), 3);
    assert_eq!(definition.interface.parameters.len(), 11);
    definition.validate().expect("valid Plexus definition");

    let grid = catalog_node(&plexus, "native.point.grid");
    let connect = catalog_node(&plexus, "native.point.connect-points");
    let renderer = catalog_node(&plexus, "native.point.line-renderer");
    let output = definition.output(plexus.output_id).expect("Image Output");
    let routes = definition
        .graph
        .connections
        .iter()
        .map(|route| {
            (
                route.from.node_id,
                route.from.port.as_str(),
                route.to.node_id,
                route.to.port.as_str(),
            )
        })
        .collect::<Vec<_>>();
    assert!(routes.contains(&(grid.id, "points", connect.id, "points")));
    assert!(routes.contains(&(connect.id, "connections", renderer.id, "connections")));
    assert!(routes.contains(&(renderer.id, "image", output.node_id, "image_in")));

    for (key, expected) in [
        ("count_x", PropertyValue::Integer(10)),
        ("count_y", PropertyValue::Integer(7)),
        ("count_z", PropertyValue::Integer(1)),
    ] {
        assert_eq!(
            grid.properties().get(key).expect("Grid property").value(),
            Some(&expected)
        );
    }
    assert_eq!(
        grid.properties()
            .get("spacing")
            .expect("Grid Spacing")
            .value(),
        Some(&PropertyValue::Vec3(Vec3 {
            x: OrderedFloat(40.0),
            y: OrderedFloat(40.0),
            z: OrderedFloat(40.0),
        }))
    );

    let expected = [
        ("Count X", grid.id, "count_x", PortDataType::Integer),
        ("Count Y", grid.id, "count_y", PortDataType::Integer),
        ("Count Z", grid.id, "count_z", PortDataType::Integer),
        ("Spacing", grid.id, "spacing", PortDataType::Vec3),
        ("Center", grid.id, "center", PortDataType::Vec3),
        (
            "Min Distance",
            connect.id,
            "min_distance",
            PortDataType::Number,
        ),
        (
            "Max Distance",
            connect.id,
            "max_distance",
            PortDataType::Number,
        ),
        (
            "Max Neighbors",
            connect.id,
            "max_neighbors",
            PortDataType::Integer,
        ),
        ("Color", renderer.id, "color", PortDataType::Color),
        ("Width", renderer.id, "width", PortDataType::Number),
        ("Fade", renderer.id, "fade", PortDataType::Number),
    ];
    for (parameter, (name, node_id, port, data_type)) in
        definition.interface.parameters.iter().zip(expected)
    {
        assert_eq!(parameter.name, name);
        assert_eq!(parameter.data_type, data_type);
        assert_eq!(parameter.target.node_id, node_id);
        assert_eq!(parameter.target.port, port);
        assert_eq!(
            definition
                .parameter_automation_capability(parameter.id)
                .expect("published input capability"),
            PublishedParameterAutomationCapability::FrameSampled
        );
    }
}
