use super::*;
use crate::animation::EasingFunction;
use crate::editor::{
    AuthoringKeyframeTarget, AuthoringKeyframeUpdate, ModuleAutomationOwner,
    ModuleInterfaceCommand, ModuleParameterOwner,
};
use crate::model::authoring::ModulePortAddress;
use crate::model::authoring::{
    AutomationKeyframe, AutomationTrack, MediaTime, ProjectDocument,
    PublishedParameterAutomationCapability, SourceRef,
};
use crate::model::frame::color::Color;
use crate::model::node::{NativeNodeRuntimeStatus, NodeContent, native_node_descriptor};
use crate::model::project::{
    NUMBER_RESULT_OUTPUT_PORT,
    property::{Property, PropertyValue},
};
use ordered_float::OrderedFloat;

use crate::model::project::asset::{Asset, AssetKind};
use crate::model::property::ImageCollectionValue;

fn seconds(value: i64) -> MediaTime {
    MediaTime::new(value, 1).expect("whole seconds")
}

#[test]
fn factory_builds_one_private_typed_chain_and_mandatory_output() {
    let result = ParticleNodeClipFactory::create("GPU Particles").expect("factory");
    assert_eq!(result.definition.graph.nodes.len(), 9);
    assert_eq!(result.definition.graph.connections.len(), 8);
    assert_eq!(result.definition.outputs().count(), 1);
    assert_eq!(result.definition.interface.parameters.len(), 30);
    assert_eq!(result.definition.sharing, ModuleDefinitionSharing::Private);
    for connection in &result.definition.graph.connections {
        let from = &result.definition.graph.nodes[&connection.from.node_id];
        let to = &result.definition.graph.nodes[&connection.to.node_id];
        assert!(
            from.ui_position[0] + from.ui_size[0] < to.ui_position[0],
            "starter rows must leave space for Vec3 controls and adjacent headers"
        );
        assert_eq!(from.ui_position[1], to.ui_position[1]);
        assert!(from.ui_size[0] >= 560.0);
        assert_eq!(from.ui_size[0], to.ui_size[0]);
    }
    result
        .definition
        .validate()
        .expect("valid particle topology");
    let published = |id| {
        result
            .definition
            .interface
            .parameters
            .iter()
            .find(|parameter| parameter.id == id)
            .expect("published Sprite parameter")
    };
    assert_eq!(
        &published(result.parameters.sprites).default_value,
        &PropertyValue::ImageCollection(Default::default())
    );
    assert_eq!(
        &published(result.parameters.selection_mode).default_value,
        &PropertyValue::String("random".into())
    );
    assert_eq!(
        &published(result.parameters.selection).default_value,
        &PropertyValue::Number(OrderedFloat(0.0))
    );
}

#[test]
fn sprite_collection_asset_removal_is_atomic_and_collection_entry_removal_is_not() {
    let service = TimelineEditorService::create_default("Sprite assets").expect("service");
    let project = service.snapshot().expect("project");
    let track_id = project.timelines[&project.root_timeline_id].track_order[0];
    drop(project);
    let created = service
        .create_particle_node_clip(ParticleNodeClipPlacement {
            track_id,
            name: "Sprites".into(),
            interval: TimelineInterval::new(MediaTime::zero(), seconds(5)).unwrap(),
            layer: 0,
        })
        .unwrap();
    let asset = Asset::new("sprite", "sprite.png", AssetKind::Image);
    let asset_id = asset.id;
    service.add_asset(asset).unwrap();
    service
        .set_module_parameter(
            created.instance_id,
            created.parameters.sprites,
            PropertyValue::ImageCollection(ImageCollectionValue::new(vec![asset_id]).unwrap()),
        )
        .unwrap();

    let revision = service.revision().unwrap();
    assert!(service.remove_asset(asset_id).is_err());
    assert_eq!(service.revision().unwrap(), revision);
    assert!(
        service
            .snapshot()
            .unwrap()
            .assets
            .iter()
            .any(|asset| asset.id == asset_id)
    );

    service
        .set_module_parameter(
            created.instance_id,
            created.parameters.sprites,
            PropertyValue::ImageCollection(ImageCollectionValue::default()),
        )
        .unwrap();
    assert!(
        service
            .snapshot()
            .unwrap()
            .assets
            .iter()
            .any(|asset| asset.id == asset_id)
    );
    service.remove_asset(asset_id).unwrap();
    assert!(
        !service
            .snapshot()
            .unwrap()
            .assets
            .iter()
            .any(|asset| asset.id == asset_id)
    );
}

#[test]
fn factory_inserts_a_neutral_turbulence_force_in_order_and_publishes_every_control() {
    let result = ParticleNodeClipFactory::create("GPU Particles").expect("factory");
    let parameter = |id| {
        result
            .definition
            .interface
            .parameters
            .iter()
            .find(|parameter| parameter.id == id)
            .expect("published parameter")
    };
    let gravity_node = parameter(result.parameters.gravity).target.node_id;
    let turbulence_node = parameter(result.parameters.turbulence_strength)
        .target
        .node_id;
    let drag_node = parameter(result.parameters.drag).target.node_id;
    assert!(
        result
            .definition
            .graph
            .connections
            .iter()
            .any(|connection| {
                connection.from.node_id == gravity_node && connection.to.node_id == turbulence_node
            })
    );
    assert!(
        result
            .definition
            .graph
            .connections
            .iter()
            .any(|connection| {
                connection.from.node_id == turbulence_node && connection.to.node_id == drag_node
            })
    );

    for (id, port, default) in [
        (
            result.parameters.turbulence_strength,
            "strength",
            PropertyValue::Number(OrderedFloat(0.0)),
        ),
        (
            result.parameters.turbulence_frequency,
            "frequency",
            PropertyValue::Number(OrderedFloat(0.01)),
        ),
        (
            result.parameters.turbulence_octaves,
            "octaves",
            PropertyValue::Integer(1),
        ),
        (
            result.parameters.turbulence_evolution,
            "evolution",
            PropertyValue::Number(OrderedFloat(0.0)),
        ),
        (
            result.parameters.turbulence_seed,
            "seed",
            PropertyValue::Integer(1),
        ),
    ] {
        let parameter = parameter(id);
        assert_eq!(parameter.target.node_id, turbulence_node);
        assert_eq!(parameter.target.port, port);
        assert_eq!(parameter.default_value, default);
    }
}

#[test]
fn factory_inserts_one_inactive_collision_between_drag_and_sprite() {
    let result = ParticleNodeClipFactory::create("GPU Particles").expect("factory");
    let parameter = |id| {
        result
            .definition
            .interface
            .parameters
            .iter()
            .find(|parameter| parameter.id == id)
            .expect("published parameter")
    };
    let collision_node = parameter(result.parameters.collision_active).target.node_id;
    assert_eq!(
        parameter(result.parameters.collision_active).default_value,
        PropertyValue::Boolean(false)
    );
    for (id, port) in [
        (result.parameters.collision_active, "active"),
        (result.parameters.collision_plane_point, "plane_point"),
        (result.parameters.collision_plane_normal, "plane_normal"),
        (result.parameters.collision_radius, "radius"),
        (result.parameters.collision_bounce, "bounce"),
        (result.parameters.collision_friction, "friction"),
    ] {
        let published = parameter(id);
        assert_eq!(published.target.node_id, collision_node);
        assert_eq!(published.target.port, port);
    }
    let drag_node = parameter(result.parameters.drag).target.node_id;
    let renderer_node = parameter(result.parameters.color).target.node_id;
    for (from, to) in [(drag_node, collision_node), (collision_node, renderer_node)] {
        assert!(
            result
                .definition
                .graph
                .connections
                .iter()
                .any(|connection| {
                    connection.from.node_id == from
                        && connection.from.port == PARTICLE_SYSTEM_PORT
                        && connection.to.node_id == to
                        && connection.to.port == PARTICLE_SYSTEM_PORT
                })
        );
    }
}

#[test]
fn only_the_executable_particle_slice_is_enabled_in_the_catalog() {
    for catalog_id in [
        "native.particle.emitter",
        "native.particle.shape-location",
        "native.particle.initialize",
        "native.particle.gravity-force",
        "native.particle.turbulence",
        "native.particle.drag-force",
        "native.particle.vortex-force",
        "native.particle.point-force",
        "native.particle.collision-plane",
        "native.particle.sprite-renderer",
    ] {
        assert_eq!(
            native_node_descriptor(catalog_id)
                .expect("descriptor")
                .runtime_status(),
            NativeNodeRuntimeStatus::Implemented
        );
    }
    assert_eq!(
        native_node_descriptor("native.particle.mesh-renderer")
            .expect("placeholder")
            .runtime_status(),
        NativeNodeRuntimeStatus::DesignNeeded
    );
}

#[test]
fn particle_published_parameter_capabilities_follow_their_native_target_ports() {
    let particle = ParticleNodeClipFactory::create("GPU Particles").expect("factory");
    for parameter_id in [
        particle.parameters.capacity,
        particle.parameters.emission_rate,
        particle.parameters.lifetime,
        particle.parameters.seed,
        particle.parameters.emitter_shape,
        particle.parameters.emitter_position,
        particle.parameters.emitter_radius,
        particle.parameters.emitter_size,
        particle.parameters.emitter_surface_only,
        particle.parameters.velocity_min,
        particle.parameters.velocity_max,
        particle.parameters.size_min,
        particle.parameters.size_max,
        particle.parameters.gravity,
        particle.parameters.turbulence_strength,
        particle.parameters.turbulence_frequency,
        particle.parameters.turbulence_octaves,
        particle.parameters.turbulence_evolution,
        particle.parameters.turbulence_seed,
        particle.parameters.drag,
        particle.parameters.collision_active,
        particle.parameters.collision_plane_point,
        particle.parameters.collision_plane_normal,
        particle.parameters.collision_radius,
        particle.parameters.collision_bounce,
        particle.parameters.collision_friction,
    ] {
        assert!(matches!(
            particle
                .definition
                .parameter_automation_capability(parameter_id)
                .expect("published parameter"),
            PublishedParameterAutomationCapability::ConstantOnly { .. }
        ));
    }
    assert_eq!(
        particle
            .definition
            .parameter_automation_capability(particle.parameters.color)
            .expect("published color"),
        PublishedParameterAutomationCapability::FrameSampled
    );
}

#[test]
fn particle_capability_rejects_missing_and_output_targets() {
    let particle = ParticleNodeClipFactory::create("GPU Particles").expect("factory");
    let parameter = particle
        .definition
        .interface
        .parameters
        .iter()
        .find(|parameter| parameter.id == particle.parameters.emission_rate)
        .expect("Emission Rate");

    let mut missing = particle.definition.clone();
    missing
        .interface
        .parameters
        .iter_mut()
        .find(|candidate| candidate.id == parameter.id)
        .unwrap()
        .target
        .port = "missing".to_string();
    let error = missing
        .parameter_automation_capability(parameter.id)
        .unwrap_err();
    assert!(error.contains("invalid target"), "{error}");
    assert!(error.contains("no Input port 'missing'"), "{error}");

    let mut output = particle.definition.clone();
    output
        .interface
        .parameters
        .iter_mut()
        .find(|candidate| candidate.id == parameter.id)
        .unwrap()
        .target
        .port = PARTICLE_SYSTEM_PORT.to_string();
    let error = output
        .parameter_automation_capability(parameter.id)
        .unwrap_err();
    assert!(error.contains("invalid target"), "{error}");
    assert!(error.contains("no Input port 'particles'"), "{error}");
}

#[test]
fn service_creates_only_the_explicit_particle_item_in_one_undo_step() {
    let service = TimelineEditorService::create_default("Particle authoring").expect("service");
    let project = service.snapshot().expect("project");
    let track_id = project.timelines[&project.root_timeline_id].track_order[0];
    drop(project);
    let (ordinary_item_id, _) = service
        .add_item(
            track_id,
            "Ordinary".to_string(),
            SourceRef::Solid {
                color: Color::white(),
            },
            TimelineInterval::new(seconds(0), seconds(2)).expect("interval"),
            0,
        )
        .expect("ordinary item");
    let before = service.snapshot().expect("before");

    let created = service
        .create_particle_node_clip(ParticleNodeClipPlacement {
            track_id,
            name: "GPU Particles".to_string(),
            interval: TimelineInterval::new(seconds(1), seconds(5)).expect("interval"),
            layer: 1,
        })
        .expect("particle item");
    let project = service.snapshot().expect("created project");
    assert!(matches!(
        project.items[&ordinary_item_id].source,
        SourceRef::Solid { .. }
    ));
    let SourceRef::Module(invocation) = &project.items[&created.item_id].source else {
        panic!("Particle item must be an explicit Node Clip");
    };
    assert_eq!(invocation.instance_id, created.instance_id);
    assert_eq!(invocation.output_id, created.output_id);
    assert!(invocation.automation_tracks.is_empty());
    assert!(invocation.input_bindings.is_empty());
    assert_eq!(
        project.module_instances[&created.instance_id].definition_id,
        created.definition_id
    );
    assert_eq!(
        project.module_definitions[&created.definition_id].sharing,
        ModuleDefinitionSharing::Private
    );
    drop(project);

    service.undo().expect("undo").expect("creation transaction");
    assert_eq!(
        service.snapshot().expect("restored").as_ref(),
        before.as_ref()
    );
}

#[test]
fn particle_instance_override_survives_project_file_round_trip() {
    let service = TimelineEditorService::create_default("Particle persistence").expect("service");
    let project = service.snapshot().expect("project");
    let track_id = project.timelines[&project.root_timeline_id].track_order[0];
    drop(project);
    let created = service
        .create_particle_node_clip(ParticleNodeClipPlacement {
            track_id,
            name: "Particle System".to_string(),
            interval: TimelineInterval::new(MediaTime::zero(), seconds(5)).expect("interval"),
            layer: 0,
        })
        .expect("particle item");
    let persisted_seed = PropertyValue::Integer(7_919);
    service
        .set_module_parameter(
            created.instance_id,
            created.parameters.seed,
            persisted_seed.clone(),
        )
        .expect("instance Seed override");

    let directory = tempfile::tempdir().expect("temporary project directory");
    let path = directory.path().join("particle.ruvie");
    service.save_as(&path).expect("save Particle project");
    let reopened = TimelineEditorService::open(&path).expect("reopen Particle project");
    let reopened_project = reopened.snapshot().expect("reopened Particle project");

    assert_eq!(
        reopened_project.module_instances[&created.instance_id]
            .parameter_overrides
            .get(&created.parameters.seed),
        Some(&persisted_seed)
    );
    assert_eq!(
        reopened.document().expect("reopened document"),
        service.document().expect("saved document")
    );
}

#[test]
fn service_rejects_particle_simulation_keyframes_but_accepts_sprite_color() {
    let service = TimelineEditorService::create_default("Particle automation").expect("service");
    let project = service.snapshot().expect("project");
    let track_id = project.timelines[&project.root_timeline_id].track_order[0];
    drop(project);
    let created = service
        .create_particle_node_clip(ParticleNodeClipPlacement {
            track_id,
            name: "GPU Particles".to_string(),
            interval: TimelineInterval::new(MediaTime::zero(), seconds(5)).expect("interval"),
            layer: 0,
        })
        .expect("particle item");
    let revision = service.revision().expect("revision");
    let error = service
        .upsert_module_parameter_keyframe(
            &ModuleParameterOwner::Invocation(ModuleAutomationOwner::Item(created.item_id)),
            created.parameters.emission_rate,
            MediaTime::zero(),
            PropertyValue::Number(OrderedFloat(240.0)),
            Some(EasingFunction::Linear),
        )
        .expect_err("simulation automation must fail before mutation");
    assert!(error.to_string().contains("constant-only"));
    assert!(error.to_string().contains("fixed-step parameter schedule"));
    assert_eq!(service.revision().expect("unchanged revision"), revision);

    let update_error = service
        .update_keyframe(
            &AuthoringKeyframeTarget::ModuleParameter {
                owner: ModuleParameterOwner::Invocation(ModuleAutomationOwner::Item(
                    created.item_id,
                )),
                parameter_id: created.parameters.emission_rate,
            },
            crate::model::project::property::KeyframeId::new(),
            AuthoringKeyframeUpdate {
                time: Some(MediaTime::zero()),
                value: Some(PropertyValue::Number(OrderedFloat(360.0))),
                easing: Some(EasingFunction::Linear),
            },
        )
        .expect_err("update must use the same capability guard");
    assert!(update_error.to_string().contains("constant-only"));

    service
        .upsert_module_parameter_keyframe(
            &ModuleParameterOwner::Invocation(ModuleAutomationOwner::Item(created.item_id)),
            created.parameters.color,
            MediaTime::zero(),
            PropertyValue::Color(Color::white()),
            Some(EasingFunction::Linear),
        )
        .expect("Sprite Renderer color is frame-sampled");
    service
        .snapshot()
        .expect("automated project")
        .validate()
        .expect("valid project");
}

#[test]
fn service_rejects_invalid_native_particle_properties_atomically() {
    let service =
        TimelineEditorService::create_default("Particle property contract").expect("service");
    let project = service.snapshot().expect("project");
    let track_id = project.timelines[&project.root_timeline_id].track_order[0];
    drop(project);
    let created = service
        .create_particle_node_clip(ParticleNodeClipPlacement {
            track_id,
            name: "GPU Particles".to_string(),
            interval: TimelineInterval::new(MediaTime::zero(), seconds(5)).expect("interval"),
            layer: 0,
        })
        .expect("particle item");
    let before = service.snapshot().expect("before invalid edit");
    let emitter_id = before.module_definitions[&created.definition_id]
        .graph
        .nodes
        .values()
        .find(|node| {
            matches!(
                node.content(),
                NodeContent::NativeOperation(operation)
                    if operation.catalog_id == "native.particle.emitter"
            )
        })
        .expect("Particle Emitter")
        .id;
    let shape_location_id = before.module_definitions[&created.definition_id]
        .graph
        .nodes
        .values()
        .find(|node| {
            matches!(
                node.content(),
                NodeContent::NativeOperation(operation)
                    if operation.catalog_id == "native.particle.shape-location"
            )
        })
        .expect("Emitter Shape")
        .id;
    let revision = service.revision().expect("revision");

    let error = service
        .set_instance_module_node_property(
            created.instance_id,
            emitter_id,
            "rate".to_string(),
            Property::constant(PropertyValue::String("fast".to_string())),
        )
        .expect_err("typed descriptor must reject the edit");
    assert!(error.to_string().contains("Property 'rate' expects"));
    assert_eq!(service.revision().expect("unchanged revision"), revision);
    assert_eq!(service.snapshot().expect("unchanged project"), before);

    let error = service
        .set_instance_module_node_property(
            created.instance_id,
            shape_location_id,
            "shape".to_string(),
            Property::constant(PropertyValue::String("Cone".to_string())),
        )
        .expect_err("unsupported emitter shape must fail before mutation");
    assert!(
        error
            .to_string()
            .contains("dropdown value \"Cone\" is not an option")
    );
    assert_eq!(service.revision().expect("unchanged revision"), revision);
    assert_eq!(service.snapshot().expect("unchanged project"), before);
}

#[test]
fn published_particle_values_reject_any_payloads_before_compile() {
    let service = TimelineEditorService::create_default("Particle values").expect("service");
    let project = service.snapshot().expect("project");
    let track_id = project.timelines[&project.root_timeline_id].track_order[0];
    drop(project);
    let created = service
        .create_particle_node_clip(ParticleNodeClipPlacement {
            track_id,
            name: "GPU Particles".to_string(),
            interval: TimelineInterval::new(MediaTime::zero(), seconds(5)).expect("interval"),
            layer: 0,
        })
        .expect("particle item");
    let base = service.snapshot().expect("particle project");

    let mut invalid_number = base.as_ref().clone();
    invalid_number
        .module_instances
        .get_mut(&created.instance_id)
        .unwrap()
        .parameter_overrides
        .insert(
            created.parameters.emission_rate,
            PropertyValue::Map(HashMap::new()),
        );
    let error = invalid_number.validate().unwrap_err();
    assert!(error.contains("incompatible value"), "{error}");

    let mut non_finite_number = base.as_ref().clone();
    non_finite_number
        .module_instances
        .get_mut(&created.instance_id)
        .unwrap()
        .parameter_overrides
        .insert(
            created.parameters.emission_rate,
            PropertyValue::Number(OrderedFloat(f64::NAN)),
        );
    let error = non_finite_number.validate().unwrap_err();
    assert!(error.contains("must be finite"), "{error}");

    let mut out_of_range_number = base.as_ref().clone();
    out_of_range_number
        .module_instances
        .get_mut(&created.instance_id)
        .unwrap()
        .parameter_overrides
        .insert(
            created.parameters.emission_rate,
            PropertyValue::Number(OrderedFloat(-1.0)),
        );
    let error = out_of_range_number.validate().unwrap_err();
    assert!(error.contains("cannot be less than 0"), "{error}");

    let mut invalid_default = base.as_ref().clone();
    invalid_default
        .module_definitions
        .get_mut(&created.definition_id)
        .unwrap()
        .interface
        .parameters
        .iter_mut()
        .find(|parameter| parameter.id == created.parameters.emission_rate)
        .unwrap()
        .default_value = PropertyValue::Number(OrderedFloat(-1.0));
    let error = invalid_default.validate().unwrap_err();
    assert!(error.contains("cannot be less than 0"), "{error}");

    let mut invalid_range_default = base.as_ref().clone();
    invalid_range_default
        .module_definitions
        .get_mut(&created.definition_id)
        .unwrap()
        .interface
        .parameters
        .iter_mut()
        .find(|parameter| parameter.id == created.parameters.size_min)
        .unwrap()
        .default_value = PropertyValue::Number(OrderedFloat(100.0));
    let error = invalid_range_default.validate().unwrap_err();
    assert!(error.contains("size range"), "{error}");

    let mut invalid_vector = base.as_ref().clone();
    invalid_vector
        .module_instances
        .get_mut(&created.instance_id)
        .unwrap()
        .parameter_overrides
        .insert(
            created.parameters.gravity,
            PropertyValue::Vec3(crate::model::property::Vec3 {
                x: OrderedFloat(1_000_001.0),
                y: OrderedFloat(0.0),
                z: OrderedFloat(0.0),
            }),
        );
    let error = invalid_vector.validate().unwrap_err();
    assert!(error.contains("cannot be greater than 1000000"), "{error}");

    let mut invalid_color = base.as_ref().clone();
    let SourceRef::Module(invocation) = &mut invalid_color
        .items
        .get_mut(&created.item_id)
        .unwrap()
        .source
    else {
        panic!("Particle item must be a Node Clip");
    };
    invocation.automation_tracks.insert(
        created.parameters.color,
        AutomationTrack::new(AutomationKeyframe::new(
            MediaTime::zero(),
            PropertyValue::OpaqueJson(serde_json::Value::Null),
            EasingFunction::Linear,
        ))
        .expect("structurally valid track"),
    );
    let error = invalid_color.validate().unwrap_err();
    assert!(error.contains("incompatible Keyframe value"), "{error}");
}

#[test]
fn service_rejects_unsafe_particle_parameter_combinations_atomically() {
    let service = TimelineEditorService::create_default("Particle range").expect("service");
    let project = service.snapshot().expect("project");
    let track_id = project.timelines[&project.root_timeline_id].track_order[0];
    drop(project);
    let created = service
        .create_particle_node_clip(ParticleNodeClipPlacement {
            track_id,
            name: "GPU Particles".to_string(),
            interval: TimelineInterval::new(MediaTime::zero(), seconds(5)).expect("interval"),
            layer: 0,
        })
        .expect("particle item");
    let before = service.snapshot().expect("before invalid range");
    let revision = service.revision().expect("revision");

    let error = service
        .set_module_parameter(
            created.instance_id,
            created.parameters.size_min,
            PropertyValue::Number(OrderedFloat(100.0)),
        )
        .expect_err("size min above the effective max must fail");
    assert!(error.to_string().contains("size range"), "{error}");
    assert_eq!(service.revision().expect("unchanged revision"), revision);
    assert_eq!(service.snapshot().expect("unchanged project"), before);

    let error = service
        .set_module_parameter(
            created.instance_id,
            created.parameters.lifetime,
            PropertyValue::Number(OrderedFloat(120.0)),
        )
        .expect_err("a synchronous cold seek cannot exceed its work budget");
    assert!(error.to_string().contains("cold seek"), "{error}");
    assert_eq!(service.revision().expect("unchanged revision"), revision);
    assert_eq!(service.snapshot().expect("unchanged project"), before);
}

#[test]
fn service_rejects_constant_only_particle_connections_atomically() {
    let service = TimelineEditorService::create_default("Particle connection").expect("service");
    let project = service.snapshot().expect("project");
    let track_id = project.timelines[&project.root_timeline_id].track_order[0];
    drop(project);
    let created = service
        .create_particle_node_clip(ParticleNodeClipPlacement {
            track_id,
            name: "GPU Particles".to_string(),
            interval: TimelineInterval::new(MediaTime::zero(), seconds(5)).expect("interval"),
            layer: 0,
        })
        .expect("particle item");
    service
        .edit_instance_module_interface(
            created.instance_id,
            ModuleInterfaceCommand::UnpublishParameter {
                parameter_id: created.parameters.emission_rate,
            },
        )
        .expect("make rate an internal input");

    let project = service.snapshot().expect("unpublished project");
    let definition = &project.module_definitions[&created.definition_id];
    let emitter_id = definition
        .graph
        .nodes
        .values()
        .find(|node| {
            matches!(
                node.content(),
                NodeContent::NativeOperation(operation)
                    if operation.catalog_id == "native.particle.emitter"
            )
        })
        .expect("Emitter")
        .id;
    drop(project);

    let before_driver = service.snapshot().expect("before driver");
    let driver = Node::new_add("Rate Driver");
    let driver_id = driver.id;
    service
        .add_instance_module_node(created.instance_id, driver)
        .expect("driver node");
    let before_connection = service.snapshot().expect("before connection");
    let revision = service.revision().expect("revision");

    let error = service
        .connect_instance_module_ports(
            created.instance_id,
            ModulePortAddress {
                node_id: driver_id,
                port: NUMBER_RESULT_OUTPUT_PORT.to_string(),
            },
            ModulePortAddress {
                node_id: emitter_id,
                port: "rate".to_string(),
            },
            0,
        )
        .expect_err("constant-only input must reject graph wiring");
    assert!(error.to_string().contains("constant-only input"));
    assert!(error.to_string().contains("fixed-step parameter schedule"));
    assert_eq!(service.revision().expect("unchanged revision"), revision);
    assert_eq!(
        service.snapshot().expect("unchanged project").as_ref(),
        before_connection.as_ref()
    );

    service.undo().expect("undo").expect("driver transaction");
    assert_eq!(
        service.snapshot().expect("driver removed").as_ref(),
        before_driver.as_ref()
    );
}

#[test]
fn project_import_rejects_persisted_constant_only_particle_automation() {
    let service = TimelineEditorService::create_default("Imported Particle").expect("service");
    let project = service.snapshot().expect("project");
    let track_id = project.timelines[&project.root_timeline_id].track_order[0];
    drop(project);
    let created = service
        .create_particle_node_clip(ParticleNodeClipPlacement {
            track_id,
            name: "GPU Particles".to_string(),
            interval: TimelineInterval::new(MediaTime::zero(), seconds(5)).expect("interval"),
            layer: 0,
        })
        .expect("particle item");
    let mut imported = service.snapshot().expect("project").as_ref().clone();
    let SourceRef::Module(invocation) = &mut imported
        .items
        .get_mut(&created.item_id)
        .expect("particle item")
        .source
    else {
        panic!("Particle item must be a Node Clip");
    };
    invocation.automation_tracks.insert(
        created.parameters.gravity,
        AutomationTrack::new(AutomationKeyframe::new(
            MediaTime::zero(),
            PropertyValue::Vec3(crate::model::property::Vec3 {
                x: OrderedFloat(0.0),
                y: OrderedFloat(180.0),
                z: OrderedFloat(0.0),
            }),
            EasingFunction::Linear,
        ))
        .expect("automation fixture"),
    );
    let validation_error = imported
        .validate()
        .expect_err("invalid imported automation");
    assert!(validation_error.contains("constant-only"));

    let source = serde_json::to_string(&ProjectDocument::new(imported)).expect("raw fixture");
    let import_error = ProjectDocument::from_json(&source).expect_err("import must validate");
    assert!(import_error.contains("constant-only"));
    assert!(import_error.contains("fixed-step parameter schedule"));
}

#[test]
fn project_import_rejects_incomplete_native_particle_schema() {
    let service =
        TimelineEditorService::create_default("Imported Particle schema").expect("service");
    let project = service.snapshot().expect("project");
    let track_id = project.timelines[&project.root_timeline_id].track_order[0];
    drop(project);
    let created = service
        .create_particle_node_clip(ParticleNodeClipPlacement {
            track_id,
            name: "GPU Particles".to_string(),
            interval: TimelineInterval::new(MediaTime::zero(), seconds(5)).expect("interval"),
            layer: 0,
        })
        .expect("particle item");
    let mut imported = service.snapshot().expect("project").as_ref().clone();
    let definition = imported
        .module_definitions
        .get_mut(&created.definition_id)
        .expect("Particle definition");
    let emitter_id = definition
        .graph
        .nodes
        .values()
        .find(|node| {
            matches!(
                node.content(),
                NodeContent::NativeOperation(operation)
                    if operation.catalog_id == "native.particle.emitter"
            )
        })
        .expect("Particle Emitter")
        .id;
    let mut encoded = serde_json::to_value(&definition.graph.nodes[&emitter_id])
        .expect("serialize Particle Emitter");
    encoded["properties"]
        .as_object_mut()
        .expect("Property map")
        .remove("rate");
    definition.graph.nodes.insert(
        emitter_id,
        serde_json::from_value(encoded).expect("deserialize malformed fixture"),
    );

    let validation_error = imported.validate().expect_err("invalid imported schema");
    assert!(
        validation_error.contains("missing required Property 'rate'"),
        "{validation_error}"
    );
    let source = serde_json::to_string(&ProjectDocument::new(imported)).expect("raw fixture");
    let import_error = ProjectDocument::from_json(&source).expect_err("import must validate");
    assert!(
        import_error.contains("missing required Property 'rate'"),
        "{import_error}"
    );
}
