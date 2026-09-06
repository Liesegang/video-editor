use ordered_float::OrderedFloat;
use std::collections::HashMap;

use super::*;
use crate::core::render_plan::{RenderPlanCompiler, evaluate_render_plan_frame};
use crate::editor::AppearanceOperationFactory;
use crate::model::authoring::{
    ModuleConnection, ModuleDefinitionSharing, ModulePortAddress, ModuleTemplateOrigin,
    PublishedMediaInput, PublishedParameter, ShapeKind, ShapeSource,
};
use crate::model::frame::color::Color;
use crate::model::frame::entity::FrameItem;
use crate::model::project::{IMAGE_INPUT_PORT, IMAGE_OUTPUT_PORT, PortDataType};
use crate::model::property::{Property, PropertyValue};
use crate::plugin::property_port_key;

fn seconds(value: i64) -> MediaTime {
    MediaTime::new(value, 1).expect("whole seconds")
}

struct EffectDefinition {
    definition: ModuleDefinition,
    output_id: ModuleOutputId,
    sigma_target: ModulePortAddress,
    sigma_parameter_id: Option<PublishedParameterId>,
}

fn blur_effect_definition(
    plugins: &PluginManager,
    sharing: ModuleDefinitionSharing,
    publish_sigma: bool,
) -> EffectDefinition {
    let node = plugins
        .create_effect_operation_node("blur")
        .expect("Blur operation Node");
    let node_id = node.id;
    let sigma_target = ModulePortAddress {
        node_id,
        port: property_port_key("sigma_x"),
    };
    let sigma_default = node
        .properties()
        .get_constant_value("sigma_x")
        .cloned()
        .expect("Blur sigma_x");
    let (mut definition, output_id) = ModuleDefinition::new_image("Blur Effect", sharing);
    let output_target = definition
        .output(output_id)
        .expect("Image Output")
        .target(PortDataType::Image)
        .expect("Image target");
    definition.graph.nodes.insert(node_id, node);
    definition.graph.connections.push(ModuleConnection {
        id: ModuleConnectionId::new(),
        from: ModulePortAddress {
            node_id,
            port: IMAGE_OUTPUT_PORT.to_string(),
        },
        to: output_target,
        order: 0,
        blend_mode: BlendMode::Normal,
    });
    let image_input_id = PublishedMediaInputId::new();
    definition.interface.media_inputs.push(PublishedMediaInput {
        id: image_input_id,
        name: "Image".to_string(),
        data_type: PortDataType::Image,
        target: ModulePortAddress {
            node_id,
            port: IMAGE_INPUT_PORT.to_string(),
        },
        required: true,
        primary: true,
    });
    let sigma_parameter_id = publish_sigma.then(|| {
        let parameter_id = PublishedParameterId::new();
        definition.interface.parameters.push(PublishedParameter {
            id: parameter_id,
            name: "Blur X".to_string(),
            data_type: PortDataType::Number,
            default_value: sigma_default,
            target: sigma_target.clone(),
        });
        parameter_id
    });
    definition.topology_revision += 1;
    definition.interface_version += 1;
    EffectDefinition {
        definition,
        output_id,
        sigma_target,
        sigma_parameter_id,
    }
}

fn add_shape(service: &TimelineEditorService, plugins: &PluginManager) -> TimelineItemId {
    let snapshot = service.snapshot().expect("project");
    let track_id = snapshot.timelines[&snapshot.root_timeline_id].track_order[0];
    drop(snapshot);
    let mut fill = AppearanceOperationFactory::create(plugins, "fill").expect("Fill");
    fill.properties.set(
        "color".to_string(),
        Property::constant(PropertyValue::Color(Color::white())),
    );
    service
        .add_item(
            track_id,
            "Shape".to_string(),
            SourceRef::Shape {
                shape: ShapeSource {
                    shape_kind: ShapeKind::Rectangle,
                    parameters: HashMap::from([
                        ("width".to_string(), PropertyValue::from(48.0)),
                        ("height".to_string(), PropertyValue::from(48.0)),
                    ]),
                    appearance_operations: vec![fill],
                },
            },
            TimelineInterval::new(MediaTime::zero(), seconds(4)).expect("interval"),
            0,
        )
        .expect("Shape item")
        .0
}

fn attach(
    service: &TimelineEditorService,
    owner: AttachmentOwner,
    definition_id: ModuleDefinitionId,
    output_id: ModuleOutputId,
) -> (AttachmentId, ModuleInstanceId) {
    let (attachment_id, instance_id, _) = service
        .attach_module(ModuleAttachmentPlacement {
            owner,
            stage: AttachmentStage::ItemPostTransform,
            definition_id,
            output_id,
            parameter_overrides: HashMap::new(),
            input_bindings: HashMap::new(),
        })
        .expect("Module Effect");
    (attachment_id, instance_id)
}

fn attachment_invocation(
    project: &AuthoringProject,
    attachment_id: AttachmentId,
) -> &ModuleInvocation {
    let AttachmentProcessor::Module(invocation) = &project.attachments[&attachment_id].processor
    else {
        panic!("expected Module Effect")
    };
    invocation
}

fn blur_sigma(items: &[FrameItem]) -> Option<f64> {
    items.iter().find_map(|item| match item {
        FrameItem::Group(group) => group
            .effects
            .iter()
            .find(|effect| effect.effect_type == "blur")
            .and_then(|effect| effect.properties.get("sigma_x"))
            .and_then(|value| value.get_as::<f64>())
            .or_else(|| blur_sigma(&group.items)),
        FrameItem::Object(_) | FrameItem::Transition(_) => None,
    })
}

#[test]
fn attachment_parameter_automation_is_projected_committed_and_rendered_by_the_same_owner() {
    let plugins = PluginManager::default();
    let service =
        TimelineEditorService::create_default("Module Effect automation").expect("service");
    let item_id = add_shape(&service, &plugins);
    let effect = blur_effect_definition(
        &plugins,
        ModuleDefinitionSharing::ReusableTemplate(ModuleTemplateOrigin::Project),
        true,
    );
    let parameter_id = effect.sigma_parameter_id.expect("published sigma");
    let definition_id = effect.definition.id;
    service
        .add_module_definition(effect.definition)
        .expect("definition");
    let (attachment_id, instance_id) = attach(
        &service,
        AttachmentOwner::Item { item_id },
        definition_id,
        effect.output_id,
    );
    let module_owner = ModuleAutomationOwner::Attachment(attachment_id);
    let owner = ModuleParameterOwner::Invocation(module_owner);
    let baseline = service.snapshot().expect("baseline");
    assert_eq!(
        module_owner
            .invocation(&baseline)
            .expect("invocation")
            .instance_id,
        instance_id
    );
    assert_eq!(
        module_owner.timeline_id(&baseline).expect("Timeline"),
        baseline.root_timeline_id
    );

    service
        .upsert_module_parameter_keyframe(
            &owner,
            parameter_id,
            MediaTime::zero(),
            PropertyValue::Number(OrderedFloat(0.0)),
            Some(EasingFunction::Linear),
        )
        .expect("first key");
    service
        .upsert_module_parameter_keyframe(
            &owner,
            parameter_id,
            seconds(1),
            PropertyValue::Number(OrderedFloat(12.0)),
            Some(EasingFunction::Linear),
        )
        .expect("second key");
    let before_projection = service.snapshot().expect("automated");
    let insertion_id = KeyframeId::new();
    let target = AuthoringPropertyValueTarget::Keyframe {
        local_time: MediaTime::new(1, 2).expect("half second"),
        insertion_id,
    };
    let projected = TimelineEditorService::project_module_parameter_value(
        &before_projection,
        &owner,
        instance_id,
        parameter_id,
        PropertyValue::Number(OrderedFloat(8.0)),
        target,
    )
    .expect("projection");
    service
        .apply_module_parameter_value(
            &owner,
            instance_id,
            parameter_id,
            PropertyValue::Number(OrderedFloat(8.0)),
            target,
        )
        .expect("commit");
    assert_eq!(service.snapshot().expect("committed").as_ref(), &projected);
    service.undo().expect("undo").expect("change");
    assert_eq!(service.snapshot().expect("restored"), before_projection);

    let second_key_id = attachment_invocation(&before_projection, attachment_id).automation_tracks
        [&parameter_id]
        .keyframes[1]
        .id;
    service
        .update_keyframe(
            &AuthoringKeyframeTarget::ModuleParameter {
                owner: owner.clone(),
                parameter_id,
            },
            second_key_id,
            AuthoringKeyframeUpdate {
                time: Some(MediaTime::new(3, 2).expect("updated time")),
                value: Some(PropertyValue::Number(OrderedFloat(10.0))),
                easing: Some(EasingFunction::EaseInOutQuad),
            },
        )
        .expect("update attachment key");
    let updated = service.snapshot().expect("updated");
    let updated_key = attachment_invocation(&updated, attachment_id).automation_tracks
        [&parameter_id]
        .keyframes
        .iter()
        .find(|keyframe| keyframe.id == second_key_id)
        .expect("stable updated key");
    assert_eq!(
        updated_key.time,
        MediaTime::new(3, 2).expect("updated time")
    );
    assert_eq!(updated_key.easing, EasingFunction::EaseInOutQuad);
    service.undo().expect("undo update").expect("change");
    assert_eq!(service.snapshot().expect("restored"), before_projection);
    service
        .remove_module_parameter_keyframe(&owner, parameter_id, second_key_id)
        .expect("remove attachment key");
    assert_eq!(
        attachment_invocation(&service.snapshot().expect("removed"), attachment_id)
            .automation_tracks[&parameter_id]
            .keyframes
            .len(),
        1
    );
    service.undo().expect("undo removal").expect("change");
    assert_eq!(service.snapshot().expect("restored"), before_projection);
    service
        .set_module_parameter_constant(
            &owner,
            parameter_id,
            PropertyValue::Number(OrderedFloat(4.0)),
        )
        .expect("switch to constant");
    assert!(
        !attachment_invocation(&service.snapshot().expect("constant"), attachment_id)
            .automation_tracks
            .contains_key(&parameter_id)
    );
    service.undo().expect("undo constant").expect("change");
    assert_eq!(service.snapshot().expect("restored"), before_projection);

    let automated = service.snapshot().expect("render project");
    let plan = RenderPlanCompiler::compile(&automated).expect("render plan");
    let compiled = plan
        .module_invocations
        .iter()
        .find(|invocation| {
            invocation.host == crate::core::render_plan::ModuleHost::Attachment(attachment_id)
        })
        .expect("compiled attachment invocation");
    assert_eq!(
        compiled.automation_tracks,
        attachment_invocation(&automated, attachment_id).automation_tracks
    );

    let sigma_at = |frame_index| {
        let frame = evaluate_render_plan_frame(&automated, &plan, &plugins, frame_index, 1.0, None)
            .expect("evaluated frame");
        blur_sigma(&frame.items).expect("evaluated Blur effect")
    };
    assert_eq!(sigma_at(0), 0.0);
    assert_eq!(sigma_at(30), 12.0);
}

#[test]
fn attachment_publish_and_first_key_are_atomic_cow_and_reject_stale_state() {
    let plugins = PluginManager::default();
    let service = TimelineEditorService::create_default("Module Effect publish").expect("service");
    let item_id = add_shape(&service, &plugins);
    let effect = blur_effect_definition(
        &plugins,
        ModuleDefinitionSharing::ReusableTemplate(ModuleTemplateOrigin::Project),
        false,
    );
    let reusable_definition_id = effect.definition.id;
    service
        .add_module_definition(effect.definition)
        .expect("definition");
    let (attachment_id, instance_id) = attach(
        &service,
        AttachmentOwner::Item { item_id },
        reusable_definition_id,
        effect.output_id,
    );
    let (sibling_attachment_id, sibling_instance_id) = attach(
        &service,
        AttachmentOwner::Item { item_id },
        reusable_definition_id,
        effect.output_id,
    );
    let before = service.snapshot().expect("before publication");
    let revision = service.revision().expect("revision");
    let publication = service
        .publish_module_parameter_keyframe(
            &ModuleParameterOwner::Invocation(ModuleAutomationOwner::Attachment(attachment_id)),
            instance_id,
            effect.sigma_target,
            seconds(1),
            revision,
        )
        .expect("publish and key");
    let after = service.snapshot().expect("published");
    assert_ne!(publication.definition_id, reusable_definition_id);
    assert_eq!(
        after.module_instances[&instance_id].definition_id,
        publication.definition_id
    );
    assert_eq!(
        after.module_instances[&sibling_instance_id].definition_id,
        reusable_definition_id
    );
    assert!(
        attachment_invocation(&after, attachment_id)
            .automation_tracks
            .contains_key(&publication.parameter_id)
    );
    assert!(
        attachment_invocation(&after, sibling_attachment_id)
            .automation_tracks
            .is_empty()
    );
    service.undo().expect("undo").expect("publication");
    assert_eq!(service.snapshot().expect("restored"), before);

    let stale_before = service.snapshot().expect("stale baseline");
    let error = service
        .publish_module_parameter_keyframe(
            &ModuleParameterOwner::Invocation(ModuleAutomationOwner::Attachment(attachment_id)),
            instance_id,
            ModulePortAddress {
                node_id: uuid::Uuid::new_v4(),
                port: "missing".to_string(),
            },
            seconds(1),
            revision,
        )
        .expect_err("stale publication");
    assert!(error.to_string().contains("stale"));
    assert_eq!(service.snapshot().expect("unchanged"), stale_before);
}

#[test]
fn attachment_owner_resolves_item_track_and_timeline_invalidations() {
    let plugins = PluginManager::default();
    let service = TimelineEditorService::create_default("Module Effect owners").expect("service");
    let item_id = add_shape(&service, &plugins);
    let snapshot = service.snapshot().expect("project");
    let timeline_id = snapshot.root_timeline_id;
    let track_id = snapshot.items[&item_id].track_id;
    drop(snapshot);
    let effect = blur_effect_definition(
        &plugins,
        ModuleDefinitionSharing::ReusableTemplate(ModuleTemplateOrigin::Project),
        true,
    );
    let definition_id = effect.definition.id;
    let parameter_id = effect.sigma_parameter_id.expect("parameter");
    service
        .add_module_definition(effect.definition)
        .expect("definition");
    for attachment_owner in [
        AttachmentOwner::Item { item_id },
        AttachmentOwner::Track { track_id },
        AttachmentOwner::Timeline { timeline_id },
    ] {
        let stage = match attachment_owner {
            AttachmentOwner::Item { .. } => AttachmentStage::ItemPostTransform,
            AttachmentOwner::Track { .. } => AttachmentStage::TrackPostComposite,
            AttachmentOwner::Timeline { .. } => AttachmentStage::TimelinePostComposite,
        };
        let (attachment_id, _, _) = service
            .attach_module(ModuleAttachmentPlacement {
                owner: attachment_owner,
                stage,
                definition_id,
                output_id: effect.output_id,
                parameter_overrides: HashMap::new(),
                input_bindings: HashMap::new(),
            })
            .expect("attachment");
        let module_owner = ModuleAutomationOwner::Attachment(attachment_id);
        assert_eq!(
            module_owner
                .timeline_id(&service.snapshot().expect("project"))
                .expect("Timeline"),
            timeline_id
        );
        let (_, changes) = service
            .upsert_module_parameter_keyframe(
                &ModuleParameterOwner::Invocation(module_owner),
                parameter_id,
                MediaTime::zero(),
                PropertyValue::Number(OrderedFloat(2.0)),
                None,
            )
            .expect("attachment automation");
        assert!(!changes.invalidations.is_empty());
    }
    let project = service.snapshot().expect("project");
    let plan = RenderPlanCompiler::compile(&project).expect("render plan");
    assert_eq!(
        plan.module_invocations
            .iter()
            .filter(|invocation| {
                matches!(
                    invocation.host,
                    crate::core::render_plan::ModuleHost::Attachment(_)
                ) && invocation.automation_tracks.contains_key(&parameter_id)
            })
            .count(),
        3,
        "Item, Track, and Timeline Module Effects retain their Timeline automation in the render plan"
    );
}
