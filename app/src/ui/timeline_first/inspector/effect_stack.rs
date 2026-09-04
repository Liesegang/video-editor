use std::collections::HashMap;

use egui_phosphor::regular as icons;
use library::editor::{ModuleAttachmentPlacement, TimelineEditorService};
use library::model::authoring::{
    Attachment, AttachmentOwner, AttachmentProcessor, AttachmentStage, AuthoringProject,
    InstanceLocator, ItemOutputStage, MediaInputBinding, MediaOutputKind, ModuleConnection,
    ModuleConnectionId, ModuleDefinition, ModuleDefinitionId, ModuleDefinitionSharing, ModuleGraph,
    ModuleInterface, ModulePortAddress, PublishedMediaInput, PublishedMediaInputId,
    PublishedMediaOutput, PublishedMediaOutputId, SourceRef, TimelineId, TimelineItem,
};
use library::model::project::asset::AssetKind;
use library::model::project::connection::{PortDataType, IMAGE_OUTPUT_PORT, MERGE_IMAGES_PORT};
use library::model::property::PropertyDefinition;
use library::model::Node;
use library::plugin::PluginManager;

use crate::state::authoring::AuthoringUiState;
use crate::state::module_node_editor::{ModuleEditorHost, ModuleNodeEditorDocument};

use super::{property_control, property_row};

mod module_controls;
use module_controls::module_effect_controls;

struct EffectStackResources<'a> {
    project: &'a AuthoringProject,
    service: &'a TimelineEditorService,
    plugins: &'a PluginManager,
}

#[derive(Clone, Copy)]
struct StackPosition {
    index: usize,
    len: usize,
}

pub(super) fn effect_stack(
    ui: &mut egui::Ui,
    project: &AuthoringProject,
    state: &mut AuthoringUiState,
    service: &TimelineEditorService,
    plugins: &PluginManager,
    owner: AttachmentOwner,
    stages: &[AttachmentStage],
) {
    ui.separator();
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new("Effects").strong());
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let add = ui.menu_button(egui::RichText::new(icons::PLUS).size(16.0), |ui| {
                for stage in stages {
                    ui.menu_button(stage_label(*stage), |ui| {
                        add_stage_menu(ui, project, state, service, plugins, &owner, *stage);
                    });
                }
            });
            crate::qa::register_component_with_metadata(
                "inspector.effects.add",
                "effect_stack_add",
                add.response.rect,
                !stages.is_empty(),
                Some(serde_json::json!({
                    "owner": format!("{owner:?}"),
                    "stage_count": stages.len(),
                })),
            );
            add.response
                .on_hover_text("Add Effect to an evaluation stage");
        });
    });

    if stages.is_empty() {
        ui.weak("This source has no supported Effect stage");
        return;
    }

    for stage in stages {
        let mut attachments = project
            .attachments
            .values()
            .filter(|attachment| attachment.owner == owner && attachment.stage == *stage)
            .collect::<Vec<_>>();
        attachments.sort_by_key(|attachment| (attachment.order, attachment.id));
        let count = attachments.len();
        egui::CollapsingHeader::new(format!("{}  {count}", stage_label(*stage)))
            .id_salt(("effect_stage", &owner, stage))
            .default_open(count > 0)
            .show(ui, |ui| {
                if attachments.is_empty() {
                    ui.weak("No effects at this stage");
                }
                for (index, attachment) in attachments.iter().enumerate() {
                    let resources = EffectStackResources {
                        project,
                        service,
                        plugins,
                    };
                    effect_entry(
                        ui,
                        state,
                        &resources,
                        attachment,
                        StackPosition {
                            index,
                            len: attachments.len(),
                        },
                    );
                }
            });
    }
}

fn add_stage_menu(
    ui: &mut egui::Ui,
    project: &AuthoringProject,
    state: &mut AuthoringUiState,
    service: &TimelineEditorService,
    plugins: &PluginManager,
    owner: &AttachmentOwner,
    stage: AttachmentStage,
) {
    let media_type = stage_media_type(stage);
    let mut effects = plugins
        .get_available_effects()
        .into_iter()
        .filter(|(effect_id, _, _)| {
            service
                .create_builtin_effect(plugins, effect_id)
                .is_ok_and(|effect| effect.contract.input_type == media_type)
        })
        .collect::<Vec<_>>();
    effects.sort_by(|left, right| left.2.cmp(&right.2).then(left.1.cmp(&right.1)));

    ui.weak("Built-in Effects");
    if effects.is_empty() {
        ui.weak("No compatible built-in effects");
    }
    let mut current_category = String::new();
    for (effect_id, name, category) in effects {
        if current_category != category {
            if !current_category.is_empty() {
                ui.separator();
            }
            current_category.clone_from(&category);
            ui.weak(&category);
        }
        if ui.button(name).clicked() {
            match service.add_builtin_effect_by_id(plugins, owner.clone(), stage, &effect_id) {
                Ok(_) => state.status = format!("Added Effect at {}", stage_label(stage)),
                Err(error) => state.error = Some(error.to_string()),
            }
            ui.close();
        }
    }

    ui.separator();
    ui.weak("Node Effects");
    if media_type == PortDataType::Image
        && ui
            .button(format!("{} New Node Effect", icons::SHARE_NETWORK))
            .on_hover_text("Create a bounded, instance-local graph for this Effect")
            .clicked()
    {
        create_and_open_node_effect(state, service, owner.clone(), stage);
        ui.close();
        return;
    }

    let compatible = compatible_module_outputs(project, media_type);
    if compatible.is_empty() {
        if media_type == PortDataType::Audio {
            ui.weak("No compatible audio Module templates");
        }
        return;
    }
    ui.menu_button("From Module Template", |ui| {
        for (definition_id, output_id, label) in &compatible {
            if ui.button(label).clicked() {
                attach_and_open_module(
                    state,
                    service,
                    owner.clone(),
                    stage,
                    *definition_id,
                    *output_id,
                );
                ui.close();
            }
        }
    });
}

fn effect_entry(
    ui: &mut egui::Ui,
    state: &mut AuthoringUiState,
    resources: &EffectStackResources<'_>,
    attachment: &Attachment,
    position: StackPosition,
) {
    let (icon, title) = attachment_title(resources.project, resources.plugins, attachment);
    let response = egui::Frame::group(ui.style()).show(ui, |ui| {
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new(icon).weak());
            ui.add_enabled(
                attachment.enabled && !attachment.bypassed,
                egui::Label::new(egui::RichText::new(&title).strong()),
            );
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.menu_button(icons::DOTS_THREE, |ui| {
                    effect_actions_menu(ui, state, resources, attachment, position);
                })
                .response
                .on_hover_text("Effect actions");
                if matches!(&attachment.processor, AttachmentProcessor::Module(_))
                    && ui
                        .small_button(icons::SHARE_NETWORK)
                        .on_hover_text("Open Node Effect")
                        .clicked()
                {
                    open_module_attachment(resources.project, state, attachment);
                }
                let enabled_icon = if attachment.enabled {
                    icons::EYE
                } else {
                    icons::EYE_SLASH
                };
                if ui
                    .small_button(enabled_icon)
                    .on_hover_text(if attachment.enabled {
                        "Disable Effect"
                    } else {
                        "Enable Effect"
                    })
                    .clicked()
                {
                    update_attachment_state(
                        state,
                        resources.service,
                        attachment,
                        !attachment.enabled,
                        attachment.bypassed,
                    );
                }
            });
        });

        if attachment.bypassed {
            ui.weak("Bypassed");
        } else if !attachment.enabled {
            ui.weak("Disabled");
        }
        ui.add_enabled_ui(attachment.enabled && !attachment.bypassed, |ui| {
            effect_parameters(ui, state, resources, attachment);
        });
    });
    crate::qa::register_component_with_metadata(
        format!("inspector.effect:{}", attachment.id),
        "effect_stack_entry",
        response.response.rect,
        true,
        Some(serde_json::json!({
            "attachment_id": attachment.id,
            "stage": format!("{:?}", attachment.stage),
            "order": attachment.order,
            "enabled": attachment.enabled,
            "bypassed": attachment.bypassed,
            "kind": match &attachment.processor {
                AttachmentProcessor::BuiltinEffect(_) => "builtin",
                AttachmentProcessor::Module(_) => "module",
            },
        })),
    );
    response.response.context_menu(|ui| {
        effect_actions_menu(ui, state, resources, attachment, position);
    });
    ui.add_space(4.0);
}

fn effect_parameters(
    ui: &mut egui::Ui,
    state: &mut AuthoringUiState,
    resources: &EffectStackResources<'_>,
    attachment: &Attachment,
) {
    match &attachment.processor {
        AttachmentProcessor::BuiltinEffect(effect) => {
            let definitions = resources
                .plugins
                .get_effect_properties(&effect.operation.component_id);
            for contract in &effect.contract.parameters {
                let Some(parameter) = effect.parameters.get(&contract.key) else {
                    continue;
                };
                let definition = definitions
                    .iter()
                    .find(|definition| definition.name() == contract.key);
                let label = definition.map_or(contract.key.as_str(), PropertyDefinition::label);
                let (finished, edited_value) = {
                    let draft = state
                        .inspector
                        .effect_values
                        .entry((attachment.id, contract.key.clone()))
                        .or_insert_with(|| parameter.value.clone());
                    let (finished, _) = property_row(ui, label, draft, definition, "", 0.1, false);
                    (finished, draft.clone())
                };
                if finished {
                    if let Err(error) = resources.service.set_builtin_effect_parameter(
                        attachment.id,
                        &contract.key,
                        edited_value,
                    ) {
                        state.error = Some(error.to_string());
                    }
                }
            }
        }
        AttachmentProcessor::Module(invocation) => {
            module_effect_controls(ui, state, resources, attachment, invocation)
        }
    }
}

fn effect_actions_menu(
    ui: &mut egui::Ui,
    state: &mut AuthoringUiState,
    resources: &EffectStackResources<'_>,
    attachment: &Attachment,
    position: StackPosition,
) {
    if matches!(&attachment.processor, AttachmentProcessor::Module(_))
        && ui
            .button(format!("{} Open Node Effect", icons::SHARE_NETWORK))
            .clicked()
    {
        open_module_attachment(resources.project, state, attachment);
        ui.close();
    }

    let mut enabled = attachment.enabled;
    if ui.checkbox(&mut enabled, "Enabled").changed() {
        update_attachment_state(
            state,
            resources.service,
            attachment,
            enabled,
            attachment.bypassed,
        );
        ui.close();
    }
    let mut bypassed = attachment.bypassed;
    if ui.checkbox(&mut bypassed, "Bypass").changed() {
        update_attachment_state(
            state,
            resources.service,
            attachment,
            attachment.enabled,
            bypassed,
        );
        ui.close();
    }
    ui.separator();
    if ui
        .add_enabled(
            position.index > 0,
            egui::Button::new(format!("{} Move up", icons::ARROW_UP)),
        )
        .clicked()
    {
        if let Err(error) = resources
            .service
            .reorder_attachment(attachment.id, position.index - 1)
        {
            state.error = Some(error.to_string());
        }
        ui.close();
    }
    if ui
        .add_enabled(
            position.index + 1 < position.len,
            egui::Button::new(format!("{} Move down", icons::ARROW_DOWN)),
        )
        .clicked()
    {
        if let Err(error) = resources
            .service
            .reorder_attachment(attachment.id, position.index + 1)
        {
            state.error = Some(error.to_string());
        }
        ui.close();
    }
    ui.separator();
    if ui
        .button(format!("{} Remove Effect", icons::TRASH))
        .clicked()
    {
        if let Err(error) = resources.service.remove_attachment(attachment.id) {
            state.error = Some(error.to_string());
        }
        ui.close();
    }
}

fn update_attachment_state(
    state: &mut AuthoringUiState,
    service: &TimelineEditorService,
    attachment: &Attachment,
    enabled: bool,
    bypassed: bool,
) {
    match service.set_attachment_state(attachment.id, enabled, bypassed) {
        Ok(_) => {
            state.status = if !enabled {
                "Effect disabled".to_string()
            } else if bypassed {
                "Effect bypassed".to_string()
            } else {
                "Effect enabled".to_string()
            };
        }
        Err(error) => state.error = Some(error.to_string()),
    }
}

fn create_and_open_node_effect(
    state: &mut AuthoringUiState,
    service: &TimelineEditorService,
    owner: AttachmentOwner,
    stage: AttachmentStage,
) {
    let (definition, output_id) = image_effect_module_definition("Node Effect");
    let definition_id = definition.id;
    let placement = ModuleAttachmentPlacement {
        owner,
        stage,
        definition_id,
        output_id,
        parameter_overrides: HashMap::new(),
        input_bindings: HashMap::new(),
    };
    match service.create_private_module_attachment(definition, placement) {
        Ok((attachment_id, instance_id, _)) => {
            state
                .node_editor
                .request_document(ModuleNodeEditorDocument::ModuleDefinition {
                    definition_id,
                    host: ModuleEditorHost::Attachment {
                        attachment_id,
                        instance_path: state.active_instance_path.clone(),
                        module_instance_id: instance_id,
                    },
                });
            state.status = "Created Node Effect".to_string();
        }
        Err(error) => state.error = Some(error.to_string()),
    }
}

fn attach_and_open_module(
    state: &mut AuthoringUiState,
    service: &TimelineEditorService,
    owner: AttachmentOwner,
    stage: AttachmentStage,
    definition_id: ModuleDefinitionId,
    output_id: PublishedMediaOutputId,
) {
    let placement = ModuleAttachmentPlacement {
        owner,
        stage,
        definition_id,
        output_id,
        parameter_overrides: HashMap::new(),
        input_bindings: HashMap::new(),
    };
    match service.attach_module(placement) {
        Ok((attachment_id, instance_id, _)) => {
            state
                .node_editor
                .request_document(ModuleNodeEditorDocument::ModuleDefinition {
                    definition_id,
                    host: ModuleEditorHost::Attachment {
                        attachment_id,
                        instance_path: state.active_instance_path.clone(),
                        module_instance_id: instance_id,
                    },
                });
            state.status = "Added Module as an Effect".to_string();
        }
        Err(error) => state.error = Some(error.to_string()),
    }
}

fn open_module_attachment(
    project: &AuthoringProject,
    state: &mut AuthoringUiState,
    attachment: &Attachment,
) {
    let AttachmentProcessor::Module(invocation) = &attachment.processor else {
        return;
    };
    let Some(instance) = project.module_instances.get(&invocation.instance_id) else {
        state.error = Some("The Node Effect instance is missing".to_string());
        return;
    };
    state
        .node_editor
        .request_document(ModuleNodeEditorDocument::ModuleDefinition {
            definition_id: instance.definition_id,
            host: ModuleEditorHost::Attachment {
                attachment_id: attachment.id,
                instance_path: state.active_instance_path.clone(),
                module_instance_id: invocation.instance_id,
            },
        });
    state.status = "Opened Node Effect".to_string();
}

fn compatible_module_outputs(
    project: &AuthoringProject,
    media_type: PortDataType,
) -> Vec<(ModuleDefinitionId, PublishedMediaOutputId, String)> {
    let mut compatible = project
        .module_definitions
        .values()
        .filter(|definition| {
            matches!(
                &definition.sharing,
                ModuleDefinitionSharing::ReusableTemplate(_)
            ) && definition
                .interface
                .media_inputs
                .iter()
                .any(|input| input.primary && input.data_type == media_type)
        })
        .flat_map(|definition| {
            definition
                .interface
                .media_outputs
                .iter()
                .filter(move |output| output.data_type == media_type)
                .map(move |output| {
                    (
                        definition.id,
                        output.id,
                        format!("{} / {}", definition.name, output.name),
                    )
                })
        })
        .collect::<Vec<_>>();
    compatible.sort_by(|left, right| left.2.cmp(&right.2));
    compatible
}

fn attachment_title(
    project: &AuthoringProject,
    plugins: &PluginManager,
    attachment: &Attachment,
) -> (&'static str, String) {
    match &attachment.processor {
        AttachmentProcessor::BuiltinEffect(effect) => (
            icons::MAGIC_WAND,
            plugins
                .get_available_effects()
                .into_iter()
                .find(|(id, _, _)| *id == effect.operation.component_id)
                .map_or_else(
                    || effect.operation.component_id.clone(),
                    |(_, name, _)| name,
                ),
        ),
        AttachmentProcessor::Module(invocation) => (
            icons::SHARE_NETWORK,
            project
                .module_instances
                .get(&invocation.instance_id)
                .and_then(|instance| project.module_definitions.get(&instance.definition_id))
                .map_or_else(
                    || "Node Effect".to_string(),
                    |definition| definition.name.clone(),
                ),
        ),
    }
}

fn image_effect_module_definition(
    name: impl Into<String>,
) -> (ModuleDefinition, PublishedMediaOutputId) {
    let mut input = Node::new_merge("Effect Input");
    input.ui_position = [80.0, 120.0];
    let input_node_id = input.id;
    let mut output = Node::new_merge("Effect Output");
    output.ui_position = [520.0, 120.0];
    let output_node_id = output.id;
    let input_id = PublishedMediaInputId::new();
    let output_id = PublishedMediaOutputId::new();
    (
        ModuleDefinition {
            id: ModuleDefinitionId::new(),
            name: name.into(),
            sharing: ModuleDefinitionSharing::Private,
            graph: ModuleGraph {
                nodes: HashMap::from([(input_node_id, input), (output_node_id, output)]),
                connections: vec![ModuleConnection {
                    id: ModuleConnectionId::new(),
                    from: ModulePortAddress {
                        node_id: input_node_id,
                        port: IMAGE_OUTPUT_PORT.to_string(),
                    },
                    to: ModulePortAddress {
                        node_id: output_node_id,
                        port: MERGE_IMAGES_PORT.to_string(),
                    },
                    order: 0,
                }],
            },
            interface: ModuleInterface {
                media_inputs: vec![PublishedMediaInput {
                    id: input_id,
                    name: "Input".to_string(),
                    data_type: PortDataType::Image,
                    target: ModulePortAddress {
                        node_id: input_node_id,
                        port: MERGE_IMAGES_PORT.to_string(),
                    },
                    required: true,
                    primary: true,
                }],
                media_outputs: vec![PublishedMediaOutput {
                    id: output_id,
                    name: "Image".to_string(),
                    data_type: PortDataType::Image,
                    source: ModulePortAddress {
                        node_id: output_node_id,
                        port: IMAGE_OUTPUT_PORT.to_string(),
                    },
                }],
                ..ModuleInterface::default()
            },
            topology_revision: 1,
            interface_version: 1,
        },
        output_id,
    )
}

const fn stage_media_type(stage: AttachmentStage) -> PortDataType {
    match stage {
        AttachmentStage::ItemPreTransform
        | AttachmentStage::ItemPostTransform
        | AttachmentStage::TrackPostComposite
        | AttachmentStage::TimelinePostComposite => PortDataType::Image,
        AttachmentStage::AudioPreFader
        | AttachmentStage::AudioPostFader
        | AttachmentStage::TrackPostMix
        | AttachmentStage::TimelinePostMix => PortDataType::Audio,
        AttachmentStage::ItemTimeMap => PortDataType::Number,
    }
}

const fn stage_label(stage: AttachmentStage) -> &'static str {
    match stage {
        AttachmentStage::ItemTimeMap => "Time Map",
        AttachmentStage::ItemPreTransform => "Before Transform",
        AttachmentStage::ItemPostTransform => "After Transform",
        AttachmentStage::TrackPostComposite => "Track Composite",
        AttachmentStage::TimelinePostComposite => "Composition Output",
        AttachmentStage::AudioPreFader => "Before Fader",
        AttachmentStage::AudioPostFader => "After Fader",
        AttachmentStage::TrackPostMix => "Track Mix",
        AttachmentStage::TimelinePostMix => "Composition Mix",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use library::model::authoring::{ModuleTemplateOrigin, RationalRate};

    #[test]
    fn new_node_effect_has_an_implicit_primary_input_and_image_output() {
        let (definition, output_id) = image_effect_module_definition("Effect");

        assert!(matches!(
            definition.sharing,
            ModuleDefinitionSharing::Private
        ));
        assert!(definition.interface.media_inputs.iter().any(|input| {
            input.primary && input.required && input.data_type == PortDataType::Image
        }));
        assert!(definition
            .interface
            .media_outputs
            .iter()
            .any(|output| { output.id == output_id && output.data_type == PortDataType::Image }));
        let primary = definition
            .interface
            .media_inputs
            .iter()
            .find(|input| input.primary)
            .expect("primary input");
        let output = definition
            .interface
            .media_outputs
            .iter()
            .find(|output| output.id == output_id)
            .expect("published output");
        assert_ne!(primary.target.node_id, output.source.node_id);
        assert!(definition.graph.connections.iter().any(|connection| {
            connection.from.node_id == primary.target.node_id
                && connection.to.node_id == output.source.node_id
        }));
        definition.validate().expect("valid effect Module");
    }

    #[test]
    fn node_effect_graph_can_process_the_implicit_host_before_published_output() {
        use library::core::render_plan::{evaluate_render_plan_frame, RenderPlanCompiler};
        use library::editor::ModuleNodeRequest;
        use library::model::frame::color::Color;
        use library::model::frame::entity::FrameItem;
        use library::model::project::connection::IMAGE_INPUT_PORT;
        use library::model::property::{Property, PropertyValue};
        use library::plugin::{EFFECT_APPLY_OPERATION, EFFECT_CATEGORY};
        use ordered_float::OrderedFloat;

        fn contains_nonzero_blur(items: &[FrameItem]) -> bool {
            items.iter().any(|item| match item {
                FrameItem::Group(group) => {
                    group.effects.iter().any(|effect| {
                        effect.effect_type == "blur"
                            && matches!(
                                effect.properties.get("sigma_x"),
                                Some(PropertyValue::Number(value)) if value.into_inner() > 0.0
                            )
                    }) || contains_nonzero_blur(&group.items)
                }
                FrameItem::Object(_) => false,
            })
        }

        let plugins = PluginManager::default();
        let service = TimelineEditorService::create_default("Effect runtime").expect("service");
        let initial = service.snapshot().expect("project");
        let timeline_id = initial.root_timeline_id;
        let track_id = *initial.tracks.keys().next().expect("default track");
        drop(initial);
        service
            .add_item(
                track_id,
                "Host".to_string(),
                SourceRef::Solid {
                    color: Color::white(),
                },
                library::model::authoring::TimelineInterval::new(
                    library::model::authoring::MediaTime::zero(),
                    library::model::authoring::MediaTime::new(5, 1).expect("duration"),
                )
                .expect("interval"),
                0,
            )
            .expect("host item");

        let (mut definition, output_id) = image_effect_module_definition("Blur graph");
        let definition_id = definition.id;
        let input_node_id = definition.interface.media_inputs[0].target.node_id;
        let output_node_id = definition.interface.media_outputs[0].source.node_id;
        let mut blur = service
            .create_module_node(
                &plugins,
                ModuleNodeRequest::PluginOperation {
                    category: EFFECT_CATEGORY.to_string(),
                    component_id: "blur".to_string(),
                    operation: EFFECT_APPLY_OPERATION.to_string(),
                },
                1920,
                1080,
            )
            .expect("Blur Node");
        blur.set_property(
            "sigma_x".to_string(),
            Property::constant(PropertyValue::Number(OrderedFloat(4.0))),
        )
        .expect("non-zero Blur radius");
        blur.ui_position = [300.0, 120.0];
        let blur_node_id = blur.id;
        definition.graph.nodes.insert(blur_node_id, blur);
        definition.graph.connections = vec![
            ModuleConnection {
                id: ModuleConnectionId::new(),
                from: ModulePortAddress {
                    node_id: input_node_id,
                    port: IMAGE_OUTPUT_PORT.to_string(),
                },
                to: ModulePortAddress {
                    node_id: blur_node_id,
                    port: IMAGE_INPUT_PORT.to_string(),
                },
                order: 0,
            },
            ModuleConnection {
                id: ModuleConnectionId::new(),
                from: ModulePortAddress {
                    node_id: blur_node_id,
                    port: IMAGE_OUTPUT_PORT.to_string(),
                },
                to: ModulePortAddress {
                    node_id: output_node_id,
                    port: MERGE_IMAGES_PORT.to_string(),
                },
                order: 0,
            },
        ];
        definition.topology_revision += 1;
        definition.validate().expect("processed graph");
        service
            .create_private_module_attachment(
                definition,
                ModuleAttachmentPlacement {
                    owner: AttachmentOwner::Timeline { timeline_id },
                    stage: AttachmentStage::TimelinePostComposite,
                    definition_id,
                    output_id,
                    parameter_overrides: HashMap::new(),
                    input_bindings: HashMap::new(),
                },
            )
            .expect("Node Effect");

        let project = service.snapshot().expect("project");
        let plan = RenderPlanCompiler::compile(&project).expect("RenderPlan");
        let frame =
            evaluate_render_plan_frame(&project, &plan, 0, 1.0, None).expect("evaluated frame");
        assert!(contains_nonzero_blur(&frame.items));
    }

    #[test]
    fn template_choices_require_a_compatible_primary_input() {
        let mut project = AuthoringProject::new(
            "Project",
            1920,
            1080,
            RationalRate::new(30, 1).expect("rate"),
            library::model::authoring::MediaTime::new(10, 1).expect("duration"),
        )
        .expect("project");
        let (mut compatible, output_id) = image_effect_module_definition("Compatible");
        compatible.sharing =
            ModuleDefinitionSharing::ReusableTemplate(ModuleTemplateOrigin::Project);
        let compatible_id = compatible.id;
        project.module_definitions.insert(compatible.id, compatible);
        let (mut generator, _) = image_effect_module_definition("Generator only");
        generator.sharing =
            ModuleDefinitionSharing::ReusableTemplate(ModuleTemplateOrigin::Project);
        generator.interface.media_inputs.clear();
        project.module_definitions.insert(generator.id, generator);

        assert_eq!(
            compatible_module_outputs(&project, PortDataType::Image),
            vec![(compatible_id, output_id, "Compatible / Image".to_string())]
        );
    }

    #[test]
    fn node_effect_attachment_and_state_changes_are_atomic_and_undoable() {
        let service = TimelineEditorService::create_default("Project").expect("service");
        let timeline_id = service.snapshot().expect("project").root_timeline_id;
        let (definition, output_id) = image_effect_module_definition("Effect");
        let definition_id = definition.id;
        let (attachment_id, instance_id, _) = service
            .create_private_module_attachment(
                definition,
                ModuleAttachmentPlacement {
                    owner: AttachmentOwner::Timeline { timeline_id },
                    stage: AttachmentStage::TimelinePostComposite,
                    definition_id,
                    output_id,
                    parameter_overrides: HashMap::new(),
                    input_bindings: HashMap::new(),
                },
            )
            .expect("Node Effect");
        let project = service.snapshot().expect("project");
        assert!(project.attachments.contains_key(&attachment_id));
        assert_eq!(
            project.module_instances[&instance_id].definition_id,
            definition_id
        );
        project.validate().expect("valid project");
        drop(project);

        service
            .set_attachment_state(attachment_id, false, true)
            .expect("state");
        let disabled = service.snapshot().expect("project");
        assert!(!disabled.attachments[&attachment_id].enabled);
        assert!(disabled.attachments[&attachment_id].bypassed);
        drop(disabled);
        service.undo().expect("undo state").expect("state change");
        let restored = service.snapshot().expect("project");
        assert!(restored.attachments[&attachment_id].enabled);
        assert!(!restored.attachments[&attachment_id].bypassed);
        drop(restored);

        service.undo().expect("undo create").expect("creation");
        let empty = service.snapshot().expect("project");
        assert!(!empty.attachments.contains_key(&attachment_id));
        assert!(!empty.module_instances.contains_key(&instance_id));
        assert!(!empty.module_definitions.contains_key(&definition_id));
    }
}
