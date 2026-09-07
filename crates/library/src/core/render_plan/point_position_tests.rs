use ordered_float::OrderedFloat;

use super::particle_tests::{
    ParticleFixture, connection, particle_fixture, particle_node_id, particle_renderer_and_output,
    point_scenes,
};
use super::{
    CompiledPointInstruction, CompiledPointValueType, RenderPlanCache, evaluate_render_plan_frame,
};
use crate::model::animation::EasingFunction;
use crate::model::authoring::{
    AutomationKeyframe, AutomationTrack, MediaTime, ModuleDefinition, ModulePortAddress,
    PublishedParameter, PublishedParameterId, SourceRef,
};
use crate::model::node::{
    Node, PARTICLE_SYSTEM_PORT, POINT_ATTRIBUTE_OUTPUT_PORT, POINT_OFFSET_INPUT_PORT,
    POINT_POSITION_INPUT_PORT, POINT_SELECTION_INPUT_PORT, POINT_SOURCE_PORT, PointNodeRole,
};
use crate::model::point::{PointAttributeElementType, PointInstruction};
use crate::model::project::{IMAGE_INPUT_PORT, IMAGE_OUTPUT_PORT, MERGE_IMAGES_PORT, PortDataType};
use crate::model::property::{PropertyValue, Vec3};
use crate::plugin::PluginManager;

fn vec3(x: f64, y: f64, z: f64) -> PropertyValue {
    PropertyValue::Vec3(Vec3 {
        x: OrderedFloat(x),
        y: OrderedFloat(y),
        z: OrderedFloat(z),
    })
}

fn renderer(fixture: &ParticleFixture) -> uuid::Uuid {
    particle_node_id(
        fixture,
        crate::model::node::ParticleNodeRole::SpriteRenderer.catalog_id(),
    )
}

fn detach_renderer_input(
    definition: &mut ModuleDefinition,
    renderer: uuid::Uuid,
) -> ModulePortAddress {
    let upstream = definition
        .graph
        .connections
        .iter()
        .find(|connection| {
            connection.to.node_id == renderer && connection.to.port == PARTICLE_SYSTEM_PORT
        })
        .unwrap()
        .from
        .clone();
    definition.graph.connections.retain(|connection| {
        !(connection.to.node_id == renderer && connection.to.port == PARTICLE_SYSTEM_PORT)
    });
    upstream
}

fn install_set_position(fixture: &mut ParticleFixture) -> uuid::Uuid {
    let renderer = renderer(fixture);
    let definition = fixture
        .project
        .module_definitions
        .get_mut(&fixture.definition_id)
        .unwrap();
    let upstream = detach_renderer_input(definition, renderer);
    let set = Node::new_catalog_node(PointNodeRole::SetPosition.catalog_id()).unwrap();
    let set_id = set.id;
    definition.graph.nodes.insert(set_id, set);
    definition.graph.connections.extend([
        connection(
            upstream.node_id,
            &upstream.port,
            set_id,
            POINT_SOURCE_PORT,
            0,
        ),
        connection(set_id, POINT_SOURCE_PORT, renderer, PARTICLE_SYSTEM_PORT, 0),
    ]);
    definition.topology_revision += 1;
    set_id
}

#[test]
fn unconnected_unpublished_position_uses_the_incoming_stream_snapshot() {
    let mut fixture = particle_fixture(1);
    let renderer = renderer(&fixture);
    let set_id = install_set_position(&mut fixture);
    let definition = &fixture.project.module_definitions[&fixture.definition_id];
    let compiled = super::compiler::compile_module(definition).unwrap();
    let program = compiled.point_renderers[&renderer]
        .point_program
        .as_ref()
        .unwrap();
    assert!(program.position_register.is_some());
    assert!(
        program
            .instructions
            .iter()
            .any(|instruction| matches!(instruction, CompiledPointInstruction::Position))
    );
    assert!(!program.instructions.iter().any(|instruction| matches!(
        instruction,
        CompiledPointInstruction::Uniform { node_id, port, .. }
            if *node_id == set_id && port == POINT_POSITION_INPUT_PORT
    )));
}

fn publish(
    definition: &mut ModuleDefinition,
    node_id: uuid::Uuid,
    port: &str,
    data_type: PortDataType,
    default_value: PropertyValue,
) -> PublishedParameterId {
    let id = PublishedParameterId::new();
    definition.interface.parameters.push(PublishedParameter {
        id,
        name: format!("Set Position {port}"),
        data_type,
        default_value,
        target: ModulePortAddress {
            node_id,
            port: port.to_string(),
        },
    });
    definition.interface_version += 1;
    id
}

#[test]
fn ordered_set_position_stages_use_upstream_and_downstream_position_snapshots() {
    let mut fixture = particle_fixture(1);
    let renderer = renderer(&fixture);
    let definition = fixture
        .project
        .module_definitions
        .get_mut(&fixture.definition_id)
        .unwrap();
    let upstream = detach_renderer_input(definition, renderer);
    let before = Node::new_catalog_node(PointNodeRole::Info.catalog_id()).unwrap();
    let first = Node::new_catalog_node(PointNodeRole::SetPosition.catalog_id()).unwrap();
    let after = Node::new_catalog_node(PointNodeRole::Info.catalog_id()).unwrap();
    let second = Node::new_catalog_node(PointNodeRole::SetPosition.catalog_id()).unwrap();
    let (before_id, first_id, after_id, second_id) = (before.id, first.id, after.id, second.id);
    definition.graph.nodes.extend([
        (before_id, before),
        (first_id, first),
        (after_id, after),
        (second_id, second),
    ]);
    definition.graph.connections.extend([
        connection(
            upstream.node_id,
            &upstream.port,
            before_id,
            POINT_SOURCE_PORT,
            0,
        ),
        connection(
            upstream.node_id,
            &upstream.port,
            first_id,
            POINT_SOURCE_PORT,
            0,
        ),
        connection(first_id, POINT_SOURCE_PORT, after_id, POINT_SOURCE_PORT, 0),
        connection(first_id, POINT_SOURCE_PORT, second_id, POINT_SOURCE_PORT, 0),
        connection(
            after_id,
            "position",
            second_id,
            POINT_POSITION_INPUT_PORT,
            0,
        ),
        connection(before_id, "position", second_id, POINT_OFFSET_INPUT_PORT, 0),
        connection(
            second_id,
            POINT_SOURCE_PORT,
            renderer,
            PARTICLE_SYSTEM_PORT,
            0,
        ),
    ]);
    definition.topology_revision += 1;

    let compiled = super::compiler::compile_module(definition).unwrap();
    let program = compiled.point_renderers[&renderer]
        .point_program
        .as_ref()
        .unwrap();
    let selects = program
        .instructions
        .iter()
        .enumerate()
        .filter_map(|(index, instruction)| {
            matches!(instruction, CompiledPointInstruction::Select { .. }).then_some(index as u16)
        })
        .collect::<Vec<_>>();
    assert_eq!(selects.len(), 2);
    assert_eq!(program.position_register, selects.last().copied());
    let (when_true, when_false) = match &program.instructions[usize::from(selects[1])] {
        CompiledPointInstruction::Select {
            when_true,
            when_false,
            ..
        } => (*when_true, *when_false),
        instruction => panic!("expected final Select, got {instruction:?}"),
    };
    assert_eq!(when_false, selects[0]);
    let CompiledPointInstruction::Binary { left, right, .. } =
        &program.instructions[usize::from(when_true)]
    else {
        panic!("second position must add its Offset")
    };
    assert_eq!(
        *left, selects[0],
        "downstream Point Info observes the first Set result"
    );
    assert!(
        matches!(
            program.instructions[usize::from(*right)],
            CompiledPointInstruction::Position
        ),
        "an upstream Point Info first used after Set must retain its original snapshot"
    );
    assert_ne!(*right, selects[0]);
}

#[test]
fn bypassed_point_stages_alias_the_stream_without_storing_or_losing_position() {
    let mut fixture = particle_fixture(1);
    let renderer = renderer(&fixture);
    let definition = fixture
        .project
        .module_definitions
        .get_mut(&fixture.definition_id)
        .unwrap();
    let upstream = detach_renderer_input(definition, renderer);
    let mut store = Node::new_catalog_node(
        PointNodeRole::StoreAttribute(PointAttributeElementType::Vec3).catalog_id(),
    )
    .unwrap();
    store.bypassed = true;
    let mut bypassed_set = Node::new_catalog_node(PointNodeRole::SetPosition.catalog_id()).unwrap();
    bypassed_set.bypassed = true;
    let info = Node::new_catalog_node(PointNodeRole::Info.catalog_id()).unwrap();
    let active_set = Node::new_catalog_node(PointNodeRole::SetPosition.catalog_id()).unwrap();
    let (store_id, bypassed_id, info_id, active_id) =
        (store.id, bypassed_set.id, info.id, active_set.id);
    definition.graph.nodes.extend([
        (store_id, store),
        (bypassed_id, bypassed_set),
        (info_id, info),
        (active_id, active_set),
    ]);
    definition.graph.connections.extend([
        connection(
            upstream.node_id,
            &upstream.port,
            store_id,
            POINT_SOURCE_PORT,
            0,
        ),
        connection(
            store_id,
            POINT_SOURCE_PORT,
            bypassed_id,
            POINT_SOURCE_PORT,
            0,
        ),
        connection(
            bypassed_id,
            POINT_SOURCE_PORT,
            info_id,
            POINT_SOURCE_PORT,
            0,
        ),
        connection(
            bypassed_id,
            POINT_SOURCE_PORT,
            active_id,
            POINT_SOURCE_PORT,
            0,
        ),
        connection(info_id, "position", active_id, POINT_POSITION_INPUT_PORT, 0),
        connection(
            store_id,
            POINT_ATTRIBUTE_OUTPUT_PORT,
            active_id,
            POINT_OFFSET_INPUT_PORT,
            0,
        ),
        connection(
            active_id,
            POINT_SOURCE_PORT,
            renderer,
            PARTICLE_SYSTEM_PORT,
            0,
        ),
    ]);
    definition.topology_revision += 1;

    let compiled = super::compiler::compile_module(definition).unwrap();
    let program = compiled.point_renderers[&renderer]
        .point_program
        .as_ref()
        .unwrap();
    assert!(program.schema.attributes().is_empty());
    assert!(
        !program.instructions.iter().any(|instruction| matches!(
            instruction,
            CompiledPointInstruction::StoreAttribute { .. }
        ))
    );
    assert!(program.position_register.is_some());
}

#[test]
fn set_position_rejects_a_position_field_from_an_independent_point_domain() {
    let mut fixture = particle_fixture(1);
    let set_id = install_set_position(&mut fixture);
    let definition = fixture
        .project
        .module_definitions
        .get_mut(&fixture.definition_id)
        .unwrap();
    let grid = Node::new_catalog_node(PointNodeRole::Grid.catalog_id()).unwrap();
    let info = Node::new_catalog_node(PointNodeRole::Info.catalog_id()).unwrap();
    let (grid_id, info_id) = (grid.id, info.id);
    definition
        .graph
        .nodes
        .extend([(grid_id, grid), (info_id, info)]);
    definition.graph.connections.extend([
        connection(grid_id, POINT_SOURCE_PORT, info_id, POINT_SOURCE_PORT, 0),
        connection(info_id, "position", set_id, POINT_POSITION_INPUT_PORT, 0),
    ]);
    definition.topology_revision += 1;

    let error = super::compiler::compile_module(definition).unwrap_err();
    assert!(error.contains("different Point domain"), "{error}");
}

#[test]
fn set_position_is_scoped_to_its_sprite_branch() {
    let mut fixture = particle_fixture(1);
    let (plain_renderer, output) = particle_renderer_and_output(&fixture);
    let definition = fixture
        .project
        .module_definitions
        .get_mut(&fixture.definition_id)
        .unwrap();
    let upstream = definition
        .graph
        .connections
        .iter()
        .find(|connection| {
            connection.to.node_id == plain_renderer && connection.to.port == PARTICLE_SYSTEM_PORT
        })
        .unwrap()
        .from
        .clone();
    definition.graph.connections.retain(|connection| {
        !(connection.from.node_id == plain_renderer
            && connection.from.port == IMAGE_OUTPUT_PORT
            && connection.to.node_id == output)
    });
    let set = Node::new_catalog_node(PointNodeRole::SetPosition.catalog_id()).unwrap();
    let moved_renderer =
        Node::new_catalog_node(crate::model::node::ParticleNodeRole::SpriteRenderer.catalog_id())
            .unwrap();
    let merge = Node::new_merge("Plain and moved Points");
    let (set_id, moved_id, merge_id) = (set.id, moved_renderer.id, merge.id);
    definition
        .graph
        .nodes
        .extend([(set_id, set), (moved_id, moved_renderer), (merge_id, merge)]);
    definition.graph.connections.extend([
        connection(
            upstream.node_id,
            &upstream.port,
            set_id,
            POINT_SOURCE_PORT,
            0,
        ),
        connection(set_id, POINT_SOURCE_PORT, moved_id, PARTICLE_SYSTEM_PORT, 0),
        connection(
            plain_renderer,
            IMAGE_OUTPUT_PORT,
            merge_id,
            MERGE_IMAGES_PORT,
            0,
        ),
        connection(moved_id, IMAGE_OUTPUT_PORT, merge_id, MERGE_IMAGES_PORT, 1),
        connection(merge_id, IMAGE_OUTPUT_PORT, output, IMAGE_INPUT_PORT, 0),
    ]);
    definition.topology_revision += 1;

    let compiled = super::compiler::compile_module(definition).unwrap();
    assert!(
        compiled.point_renderers[&plain_renderer]
            .point_program
            .is_none()
    );
    assert!(
        compiled.point_renderers[&moved_id]
            .point_program
            .as_ref()
            .unwrap()
            .position_register
            .is_some()
    );
}

#[test]
fn published_set_position_inputs_sample_local_keys_and_sibling_overrides_without_recompile() {
    let mut fixture = particle_fixture(2);
    let renderer = renderer(&fixture);
    let set_id = install_set_position(&mut fixture);
    let definition = fixture
        .project
        .module_definitions
        .get_mut(&fixture.definition_id)
        .unwrap();
    let position = publish(
        definition,
        set_id,
        POINT_POSITION_INPUT_PORT,
        PortDataType::Vec3,
        vec3(0.0, 0.0, 0.0),
    );
    let offset = publish(
        definition,
        set_id,
        POINT_OFFSET_INPUT_PORT,
        PortDataType::Vec3,
        vec3(1.0, 2.0, 3.0),
    );
    let selection = publish(
        definition,
        set_id,
        POINT_SELECTION_INPUT_PORT,
        PortDataType::Boolean,
        PropertyValue::Boolean(true),
    );
    let mut cache = RenderPlanCache::default();
    cache.compile(&fixture.project).unwrap();

    let SourceRef::Module(invocation) = &mut fixture
        .project
        .items
        .get_mut(&fixture.item_ids[0])
        .unwrap()
        .source
    else {
        panic!("Module item")
    };
    invocation.automation_tracks.insert(
        position,
        AutomationTrack {
            keyframes: vec![
                AutomationKeyframe::new(
                    MediaTime::zero(),
                    vec3(10.0, 20.0, 30.0),
                    EasingFunction::Linear,
                ),
                AutomationKeyframe::new(
                    MediaTime::new(1, 1).unwrap(),
                    vec3(30.0, 40.0, 50.0),
                    EasingFunction::Linear,
                ),
            ],
        },
    );
    let sibling = fixture
        .project
        .module_instances
        .get_mut(&fixture.instance_ids[1])
        .unwrap();
    sibling
        .parameter_overrides
        .insert(position, vec3(70.0, 80.0, 90.0));
    sibling
        .parameter_overrides
        .insert(offset, vec3(7.0, 8.0, 9.0));
    sibling
        .parameter_overrides
        .insert(selection, PropertyValue::Boolean(false));

    let (plan, stats) = cache.compile(&fixture.project).unwrap();
    assert_eq!(stats.compiled_definitions, 0);
    assert_eq!(stats.reused_definitions, 1);
    let compiled_program = plan.module_definitions[&fixture.definition_id].point_renderers
        [&renderer]
        .point_program
        .as_ref()
        .unwrap();
    let register = |port: &str| {
        compiled_program
            .instructions
            .iter()
            .position(|instruction| {
                matches!(
                    instruction,
                    CompiledPointInstruction::Uniform { node_id, port: key, .. }
                        if *node_id == set_id && key == port
                )
            })
            .unwrap()
    };
    let position_register = register(POINT_POSITION_INPUT_PORT);
    let offset_register = register(POINT_OFFSET_INPUT_PORT);
    let selection_register = register(POINT_SELECTION_INPUT_PORT);
    assert_eq!(
        compiled_program.instructions[position_register],
        CompiledPointInstruction::Uniform {
            node_id: set_id,
            port: POINT_POSITION_INPUT_PORT.to_string(),
            value_type: CompiledPointValueType::Exact(PointAttributeElementType::Vec3),
        }
    );

    let frame = evaluate_render_plan_frame(
        &fixture.project,
        &plan,
        &PluginManager::default(),
        15,
        1.0,
        None,
    )
    .unwrap();
    let scenes = point_scenes(&frame.items);
    assert_eq!(scenes.len(), 2);
    for scene in scenes {
        let program = scene.point_program.as_ref().unwrap();
        assert_eq!(
            program.position_register,
            compiled_program.position_register
        );
        let expected = if scene.invocation.module_instance_id == fixture.instance_ids[0] {
            (
                vec3(20.0, 30.0, 40.0),
                vec3(1.0, 2.0, 3.0),
                PropertyValue::Boolean(true),
            )
        } else {
            (
                vec3(70.0, 80.0, 90.0),
                vec3(7.0, 8.0, 9.0),
                PropertyValue::Boolean(false),
            )
        };
        for (register, expected) in [
            (position_register, expected.0),
            (offset_register, expected.1),
            (selection_register, expected.2),
        ] {
            assert_eq!(
                program.instructions[register],
                PointInstruction::Constant { value: expected }
            );
        }
    }
}
