use ordered_float::OrderedFloat;

use super::particle_tests::{ParticleFixture, connection, particle_fixture, point_scenes};
use super::point_position_tests::{detach_renderer_input, publish, renderer};
use super::{
    CompiledPointInstruction, CompiledPointValueType, RenderPlanCache, evaluate_render_plan_frame,
};
use crate::model::animation::EasingFunction;
use crate::model::authoring::{
    AuthoringProject, AutomationKeyframe, AutomationTrack, MediaTime, SourceRef,
};
use crate::model::node::{
    Node, PARTICLE_SYSTEM_PORT, POINT_ATTRIBUTE_OUTPUT_PORT, POINT_ATTRIBUTE_VALUE_PORT,
    POINT_SCALE_INPUT_PORT, POINT_SELECTION_INPUT_PORT, POINT_SIZE_PORT, POINT_SOURCE_PORT,
    PointNodeRole,
};
use crate::model::point::{PointAttributeElementType, PointInstruction};
use crate::model::project::{IMAGE_INPUT_PORT, IMAGE_OUTPUT_PORT, MERGE_IMAGES_PORT, PortDataType};
use crate::model::property::PropertyValue;
use crate::plugin::PluginManager;

fn install_set_size(fixture: &mut ParticleFixture) -> uuid::Uuid {
    let renderer = renderer(fixture);
    let definition = fixture
        .project
        .module_definitions
        .get_mut(&fixture.definition_id)
        .unwrap();
    let upstream = detach_renderer_input(definition, renderer);
    let set = Node::new_catalog_node(PointNodeRole::SetSize.catalog_id()).unwrap();
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

fn compiled_program(fixture: &ParticleFixture) -> super::CompiledPointProgram {
    let renderer = renderer(fixture);
    super::compiler::compile_module(&fixture.project.module_definitions[&fixture.definition_id])
        .unwrap()
        .point_renderers[&renderer]
        .point_program
        .clone()
        .unwrap()
}

#[test]
fn private_default_set_size_preserves_incoming_size_and_roundtrips_evaluated_program() {
    let mut fixture = particle_fixture(1);
    let set_id = install_set_size(&mut fixture);
    let compiled = compiled_program(&fixture);
    assert!(compiled.position_register.is_none());
    assert!(compiled.size_register.is_some());
    assert!(
        compiled
            .instructions
            .iter()
            .any(|instruction| matches!(instruction, CompiledPointInstruction::Size))
    );
    assert!(!compiled.instructions.iter().any(|instruction| matches!(
        instruction,
        CompiledPointInstruction::Uniform { node_id, port, .. }
            if *node_id == set_id && port == POINT_SIZE_PORT
    )));

    let evaluate = |project: &AuthoringProject| {
        let mut cache = RenderPlanCache::default();
        let (plan, _) = cache.compile(project).unwrap();
        let frame =
            evaluate_render_plan_frame(project, &plan, &PluginManager::default(), 0, 1.0, None)
                .unwrap();
        point_scenes(&frame.items)[0].point_program.clone().unwrap()
    };
    let before = evaluate(&fixture.project);
    assert!(before.has_geometry_output());
    assert_eq!(before.position_register, None);
    assert_eq!(before.size_register, compiled.size_register);
    assert!(
        before
            .instructions
            .iter()
            .any(|instruction| matches!(instruction, PointInstruction::Size))
    );
    let encoded = serde_json::to_string(&fixture.project).unwrap();
    let loaded: AuthoringProject = serde_json::from_str(&encoded).unwrap();
    assert_eq!(evaluate(&loaded), before);
}

#[test]
fn stored_size_field_drives_set_size_through_the_typed_attribute_route() {
    let mut fixture = particle_fixture(1);
    let set_id = install_set_size(&mut fixture);
    let definition = fixture
        .project
        .module_definitions
        .get_mut(&fixture.definition_id)
        .unwrap();
    let point_input = crate::model::authoring::ModulePortAddress {
        node_id: set_id,
        port: POINT_SOURCE_PORT.to_string(),
    };
    let upstream = definition
        .graph
        .connections
        .iter()
        .find(|connection| connection.to == point_input)
        .unwrap()
        .from
        .clone();
    definition
        .graph
        .connections
        .retain(|connection| connection.to != point_input);
    let info = Node::new_catalog_node(PointNodeRole::Info.catalog_id()).unwrap();
    let store = Node::new_catalog_node(
        PointNodeRole::StoreAttribute(PointAttributeElementType::Number).catalog_id(),
    )
    .unwrap();
    let (info_id, store_id) = (info.id, store.id);
    definition
        .graph
        .nodes
        .extend([(info_id, info), (store_id, store)]);
    definition.graph.connections.extend([
        connection(
            upstream.node_id,
            &upstream.port,
            info_id,
            POINT_SOURCE_PORT,
            0,
        ),
        connection(
            upstream.node_id,
            &upstream.port,
            store_id,
            POINT_SOURCE_PORT,
            0,
        ),
        connection(
            info_id,
            POINT_SIZE_PORT,
            store_id,
            POINT_ATTRIBUTE_VALUE_PORT,
            0,
        ),
        connection(store_id, POINT_SOURCE_PORT, set_id, POINT_SOURCE_PORT, 0),
        connection(
            store_id,
            POINT_ATTRIBUTE_OUTPUT_PORT,
            set_id,
            POINT_SIZE_PORT,
            0,
        ),
    ]);
    definition.topology_revision += 1;

    let program = compiled_program(&fixture);
    assert_eq!(
        program.schema.attributes()[0].element_type(),
        PointAttributeElementType::Number
    );
    assert!(program.instructions.iter().any(|instruction| matches!(
        instruction,
        CompiledPointInstruction::StoreAttribute { attribute: 0, .. }
    )));
    assert!(program.instructions.iter().any(|instruction| matches!(
        instruction,
        CompiledPointInstruction::LoadAttribute { attribute: 0 }
    )));
    assert!(program.size_register.is_some());
}

#[test]
fn ordered_set_size_stages_capture_upstream_and_downstream_size_snapshots() {
    let mut fixture = particle_fixture(1);
    let renderer = renderer(&fixture);
    let definition = fixture
        .project
        .module_definitions
        .get_mut(&fixture.definition_id)
        .unwrap();
    let upstream = detach_renderer_input(definition, renderer);
    let before = Node::new_catalog_node(PointNodeRole::Info.catalog_id()).unwrap();
    let first = Node::new_catalog_node(PointNodeRole::SetSize.catalog_id()).unwrap();
    let after = Node::new_catalog_node(PointNodeRole::Info.catalog_id()).unwrap();
    let second = Node::new_catalog_node(PointNodeRole::SetSize.catalog_id()).unwrap();
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
        connection(after_id, POINT_SIZE_PORT, second_id, POINT_SIZE_PORT, 0),
        connection(
            before_id,
            POINT_SIZE_PORT,
            second_id,
            POINT_SCALE_INPUT_PORT,
            0,
        ),
        connection(
            second_id,
            POINT_SOURCE_PORT,
            renderer,
            PARTICLE_SYSTEM_PORT,
            0,
        ),
    ]);
    definition.topology_revision += 1;

    let program = super::compiler::compile_module(definition)
        .unwrap()
        .point_renderers[&renderer]
        .point_program
        .clone()
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
    assert_eq!(program.size_register, selects.last().copied());
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
        panic!("second size must multiply by Scale")
    };
    assert_eq!(*left, selects[0]);
    assert!(matches!(
        program.instructions[usize::from(*right)],
        CompiledPointInstruction::Size
    ));
}

#[test]
fn set_size_rejects_foreign_domains_and_non_number_fields() {
    let mut foreign = particle_fixture(1);
    let set_id = install_set_size(&mut foreign);
    let definition = foreign
        .project
        .module_definitions
        .get_mut(&foreign.definition_id)
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
        connection(info_id, POINT_SIZE_PORT, set_id, POINT_SIZE_PORT, 0),
    ]);
    definition.topology_revision += 1;
    let error = super::compiler::compile_module(definition).unwrap_err();
    assert!(error.contains("different Point domain"), "{error}");

    let mut wrong_type = particle_fixture(1);
    let set_id = install_set_size(&mut wrong_type);
    let definition = wrong_type
        .project
        .module_definitions
        .get_mut(&wrong_type.definition_id)
        .unwrap();
    let upstream = definition
        .graph
        .connections
        .iter()
        .find(|connection| connection.to.node_id == set_id)
        .unwrap()
        .from
        .clone();
    let info = Node::new_catalog_node(PointNodeRole::Info.catalog_id()).unwrap();
    let info_id = info.id;
    definition.graph.nodes.insert(info_id, info);
    definition.graph.connections.extend([
        connection(
            upstream.node_id,
            &upstream.port,
            info_id,
            POINT_SOURCE_PORT,
            0,
        ),
        connection(info_id, "position", set_id, POINT_SIZE_PORT, 0),
    ]);
    definition.topology_revision += 1;
    let error = super::compiler::compile_module(definition).unwrap_err();
    assert!(
        error.contains("cannot connect Vec3 to Number")
            || (error.contains("requires Number") && error.contains("Vec3")),
        "{error}"
    );
}

#[test]
fn bypass_and_sprite_branches_preserve_their_own_size_geometry() {
    let mut fixture = particle_fixture(1);
    let (plain_renderer, output) = super::particle_tests::particle_renderer_and_output(&fixture);
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
    let mut bypassed = Node::new_catalog_node(PointNodeRole::SetSize.catalog_id()).unwrap();
    bypassed.bypassed = true;
    let active = Node::new_catalog_node(PointNodeRole::SetSize.catalog_id()).unwrap();
    let altered_renderer =
        Node::new_catalog_node(crate::model::node::ParticleNodeRole::SpriteRenderer.catalog_id())
            .unwrap();
    let merge = Node::new_merge("Plain and resized Points");
    let (bypassed_id, active_id, altered_id, merge_id) =
        (bypassed.id, active.id, altered_renderer.id, merge.id);
    definition.graph.nodes.extend([
        (bypassed_id, bypassed),
        (active_id, active),
        (altered_id, altered_renderer),
        (merge_id, merge),
    ]);
    definition.graph.connections.extend([
        connection(
            upstream.node_id,
            &upstream.port,
            bypassed_id,
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
        connection(
            active_id,
            POINT_SOURCE_PORT,
            altered_id,
            PARTICLE_SYSTEM_PORT,
            0,
        ),
        connection(
            plain_renderer,
            IMAGE_OUTPUT_PORT,
            merge_id,
            MERGE_IMAGES_PORT,
            0,
        ),
        connection(
            altered_id,
            IMAGE_OUTPUT_PORT,
            merge_id,
            MERGE_IMAGES_PORT,
            1,
        ),
        connection(merge_id, IMAGE_OUTPUT_PORT, output, IMAGE_INPUT_PORT, 0),
    ]);
    definition.topology_revision += 1;

    let compiled = super::compiler::compile_module(definition).unwrap();
    assert!(
        compiled.point_renderers[&plain_renderer]
            .point_program
            .is_none()
    );
    let resized = compiled.point_renderers[&altered_id]
        .point_program
        .as_ref()
        .unwrap();
    assert!(resized.position_register.is_none());
    assert!(resized.size_register.is_some());
    assert_eq!(
        resized
            .instructions
            .iter()
            .filter(|instruction| matches!(instruction, CompiledPointInstruction::Size))
            .count(),
        1
    );
}

#[test]
fn published_scale_and_selection_sample_keys_and_sibling_overrides_without_recompile() {
    let mut fixture = particle_fixture(2);
    let set_id = install_set_size(&mut fixture);
    let definition = fixture
        .project
        .module_definitions
        .get_mut(&fixture.definition_id)
        .unwrap();
    let scale = publish(
        definition,
        set_id,
        POINT_SCALE_INPUT_PORT,
        PortDataType::Number,
        PropertyValue::Number(OrderedFloat(1.0)),
    );
    let selection = publish(
        definition,
        set_id,
        POINT_SELECTION_INPUT_PORT,
        PortDataType::Boolean,
        PropertyValue::Boolean(false),
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
        scale,
        AutomationTrack {
            keyframes: vec![
                AutomationKeyframe::new(
                    MediaTime::zero(),
                    PropertyValue::Number(OrderedFloat(1.0)),
                    EasingFunction::Linear,
                ),
                AutomationKeyframe::new(
                    MediaTime::new(1, 1).unwrap(),
                    PropertyValue::Number(OrderedFloat(3.0)),
                    EasingFunction::Linear,
                ),
            ],
        },
    );
    invocation.automation_tracks.insert(
        selection,
        AutomationTrack {
            keyframes: vec![AutomationKeyframe::new(
                MediaTime::new(1, 2).unwrap(),
                PropertyValue::Boolean(true),
                EasingFunction::Linear,
            )],
        },
    );
    let sibling = fixture
        .project
        .module_instances
        .get_mut(&fixture.instance_ids[1])
        .unwrap();
    sibling
        .parameter_overrides
        .insert(scale, PropertyValue::Number(OrderedFloat(0.5)));
    sibling
        .parameter_overrides
        .insert(selection, PropertyValue::Boolean(false));

    let (plan, stats) = cache.compile(&fixture.project).unwrap();
    assert_eq!(stats.compiled_definitions, 0);
    assert_eq!(stats.reused_definitions, 1);
    let compiled = compiled_program(&fixture);
    let uniform_register = |port: &str| {
        compiled
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
    let scale_register = uniform_register(POINT_SCALE_INPUT_PORT);
    let selection_register = uniform_register(POINT_SELECTION_INPUT_PORT);
    assert_eq!(
        compiled.instructions[scale_register],
        CompiledPointInstruction::Uniform {
            node_id: set_id,
            port: POINT_SCALE_INPUT_PORT.to_string(),
            value_type: CompiledPointValueType::Exact(PointAttributeElementType::Number),
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
        assert_eq!(program.size_register, compiled.size_register);
        let expected = if scene.invocation.module_instance_id == fixture.instance_ids[0] {
            (2.0, true)
        } else {
            (0.5, false)
        };
        assert_eq!(
            program.instructions[scale_register],
            PointInstruction::Constant {
                value: PropertyValue::Number(OrderedFloat(expected.0)),
            }
        );
        assert_eq!(
            program.instructions[selection_register],
            PointInstruction::Constant {
                value: PropertyValue::Boolean(expected.1),
            }
        );
    }
}
