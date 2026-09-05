use std::collections::HashMap;

use ordered_float::OrderedFloat;
use serde::{Deserialize, Serialize};

use crate::model::BlendMode;
use crate::model::node::{
    Node, NodeContent, TRANSITION_AUDIO_INPUT_NODE_ID, TRANSITION_AUDIO_MIX_NODE_ID,
    TRANSITION_IMAGE_INPUT_NODE_ID, TRANSITION_PROGRESS_INPUT_NODE_ID, transition_input_node_id,
    transition_mix_node_id,
};
use crate::model::project::{
    AUDIO_OUTPUT_PORT, IMAGE_INPUT_PORT, IMAGE_OUTPUT_PORT, NUMBER_RESULT_OUTPUT_PORT,
    PortDataType, SOUND_INPUT_PORT, TRANSITION_FROM_INPUT_PORT, TRANSITION_PROGRESS_INPUT_PORT,
    TRANSITION_TO_INPUT_PORT,
};
use crate::model::property::PropertyValue;

use super::{
    AutomationTrack, MediaInputBinding, ModuleConnection, ModuleConnectionId, ModuleDefinition,
    ModuleDefinitionSharing, ModuleInstanceId, ModuleOutputId, ModulePortAddress,
    PublishedMediaInput, PublishedMediaInputId, PublishedParameter,
    PublishedParameterAutomationCapability, PublishedParameterId, TransitionMediaType,
};

/// Host-specific public contract of a reusable Module Definition.
///
/// A Transition contract protects stable Published Interface IDs while all
/// Module-internal Node identities remain private and freely refactorable.
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug, Default)]
#[serde(
    tag = "kind",
    content = "contract",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum ModuleHostContract {
    #[default]
    General,
    Transition(TransitionModuleInterface),
}

impl ModuleHostContract {
    pub const fn transition(&self) -> Option<&TransitionModuleInterface> {
        match self {
            Self::General => None,
            Self::Transition(contract) => Some(contract),
        }
    }

    pub fn protects_parameter(&self, parameter_id: PublishedParameterId) -> bool {
        self.transition()
            .is_some_and(|contract| contract.progress_parameter_id == parameter_id)
    }

    pub fn protects_media_input(&self, input_id: PublishedMediaInputId) -> bool {
        self.transition().is_some_and(|contract| {
            contract.from_input_id == input_id || contract.to_input_id == input_id
        })
    }

    /// Enforces the processing subset implemented by the active host runtime.
    /// Protected host boundary Nodes are validated separately by their typed
    /// interface contract and must not be passed to this predicate.
    pub fn validate_authored_processing_node(&self, node: &Node) -> Result<(), String> {
        let Some(contract) = self.transition() else {
            return Ok(());
        };
        contract.validate_authored_processing_node(node)
    }

    pub fn validate_plugin_node_creation(&self) -> Result<(), String> {
        let Some(contract) = self.transition() else {
            return Ok(());
        };
        contract.validate_plugin_node_creation()
    }

    pub fn validate_additional_media_input(&self, data_type: PortDataType) -> Result<(), String> {
        let Some(contract) = self.transition() else {
            return Ok(());
        };
        contract.validate_additional_media_input(data_type)
    }
}

/// Stable public identities injected by a Timeline Transition host.
///
/// The targets of these IDs live inside `ModuleDefinition`; a Transition
/// placement stores only its Module instance identity and never an internal
/// Node UUID.
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug)]
#[serde(deny_unknown_fields)]
pub struct TransitionModuleInterface {
    pub media_type: TransitionMediaType,
    pub from_input_id: PublishedMediaInputId,
    pub to_input_id: PublishedMediaInputId,
    pub progress_parameter_id: PublishedParameterId,
    pub output_id: ModuleOutputId,
}

impl TransitionModuleInterface {
    pub const fn output_kind(&self) -> super::MediaOutputKind {
        self.media_type.output_kind()
    }

    fn validate_authored_processing_node(&self, node: &Node) -> Result<(), String> {
        if matches!(
            node.content(),
            NodeContent::NativeOperation(operation)
                if matches!(
                    operation.catalog_id.as_str(),
                    TRANSITION_IMAGE_INPUT_NODE_ID
                        | TRANSITION_AUDIO_INPUT_NODE_ID
                        | TRANSITION_PROGRESS_INPUT_NODE_ID
                )
        ) {
            return Err(format!(
                "Transition host boundary Node '{}' ({}) can only be created by the Transition Module factory",
                node.name, node.id
            ));
        }
        let supported = match self.media_type {
            TransitionMediaType::Image => {
                !matches!(node.content(), NodeContent::SoundMerge)
                    && !matches!(
                        node.content(),
                        NodeContent::NativeOperation(operation)
                            if operation.catalog_id == TRANSITION_AUDIO_MIX_NODE_ID
                    )
            }
            TransitionMediaType::Audio => {
                matches!(
                    node.content(),
                    NodeContent::SoundMerge | NodeContent::Value(_)
                ) || matches!(
                    node.content(),
                    NodeContent::NativeOperation(operation)
                        if operation.catalog_id == TRANSITION_AUDIO_MIX_NODE_ID
                )
            }
        };
        if supported {
            Ok(())
        } else {
            match self.media_type {
                TransitionMediaType::Image => Err(format!(
                    "Image Transition Modules cannot contain Audio processing Nodes; '{}' ({}) is unsupported",
                    node.name, node.id
                )),
                TransitionMediaType::Audio => Err(format!(
                    "Audio Transition Modules currently support only Audio Mix, Audio Merge, and numeric Value processing Nodes; '{}' ({}) has no authoring audio runtime",
                    node.name, node.id
                )),
            }
        }
    }

    fn validate_plugin_node_creation(&self) -> Result<(), String> {
        if self.media_type == TransitionMediaType::Audio {
            Err(
                "Plugin operation Nodes do not yet have an authoring runtime in Audio Transition Modules"
                    .to_string(),
            )
        } else {
            Ok(())
        }
    }

    /// Keeps the Published media surface equal to the active host runtime.
    /// Image transitions can resolve additional Image frames. The Audio mixer
    /// currently receives only host-owned A/B and no host accepts a second
    /// media kind through an Image transition invocation.
    pub fn validate_additional_media_input(&self, data_type: PortDataType) -> Result<(), String> {
        match (self.media_type, data_type) {
            (TransitionMediaType::Image, PortDataType::Image) => Ok(()),
            (TransitionMediaType::Image, unsupported) => Err(format!(
                "Image Transition Modules cannot publish an additional {unsupported:?} media input because the Image Transition runtime accepts only additional Image inputs"
            )),
            (TransitionMediaType::Audio, unsupported) => Err(format!(
                "Audio Transition Modules cannot publish an additional {unsupported:?} media input because the current audio mixer supplies only the host-owned A/B inputs"
            )),
        }
    }

    /// Validates assignment through the chooser that has no controls for
    /// satisfying required additional inputs. Callers with an explicit
    /// binding form use the regular definition/project validation path.
    pub fn validate_atomic_assignment(&self, definition: &ModuleDefinition) -> Result<(), String> {
        self.validate_definition(definition)?;
        if let Some(input) = definition.interface.media_inputs.iter().find(|input| {
            input.required && input.id != self.from_input_id && input.id != self.to_input_id
        }) {
            return Err(format!(
                "Transition Module '{}' has required media input '{}' beyond host-owned A/B and cannot be assigned without controls",
                definition.name, input.name
            ));
        }
        Ok(())
    }

    pub fn validate_definition(&self, definition: &ModuleDefinition) -> Result<(), String> {
        if self.from_input_id == self.to_input_id {
            return Err("Transition Module A and B must have distinct Published input IDs".into());
        }
        if definition
            .interface
            .media_inputs
            .iter()
            .any(|input| input.primary)
        {
            return Err(
                "Transition Modules cannot publish a primary media input; A and B are supplied by the Timeline host"
                    .to_string(),
            );
        }
        let expected = self.media_type.port_data_type();
        let require_media_input = |id, role: &str| {
            let input = definition
                .interface
                .media_inputs
                .iter()
                .find(|input| input.id == id)
                .ok_or_else(|| format!("Transition Module has no protected {role} input"))?;
            if input.data_type != expected || !input.required || input.primary {
                return Err(format!(
                    "Transition Module protected {role} input has an invalid contract"
                ));
            }
            let target = definition
                .graph
                .nodes
                .get(&input.target.node_id)
                .ok_or_else(|| format!("Transition Module protected {role} target is missing"))?;
            if !target.enabled || target.bypassed {
                return Err(format!(
                    "Transition Module protected {role} boundary cannot be disabled or bypassed"
                ));
            }
            Ok(())
        };
        require_media_input(self.from_input_id, "A")?;
        require_media_input(self.to_input_id, "B")?;
        let progress = definition
            .interface
            .parameters
            .iter()
            .find(|parameter| parameter.id == self.progress_parameter_id)
            .ok_or_else(|| "Transition Module has no protected Progress parameter".to_string())?;
        if progress.data_type != PortDataType::Number
            || progress.default_value != PropertyValue::Number(OrderedFloat(0.0))
        {
            return Err(
                "Transition Module protected Progress must be a normalized Number with default 0"
                    .to_string(),
            );
        }
        let progress_target = definition
            .graph
            .nodes
            .get(&progress.target.node_id)
            .ok_or_else(|| "Transition Module protected Progress target is missing".to_string())?;
        if progress.target.port != TRANSITION_PROGRESS_INPUT_PORT
            || !matches!(
                progress_target.content(),
                NodeContent::NativeOperation(operation)
                    if operation.catalog_id == TRANSITION_PROGRESS_INPUT_NODE_ID
            )
        {
            return Err(
                "Transition Module protected Progress must target the canonical Progress boundary"
                    .to_string(),
            );
        }
        if !matches!(
            definition.parameter_automation_capability(self.progress_parameter_id)?,
            PublishedParameterAutomationCapability::FrameSampled
        ) {
            return Err(
                "Transition Module protected Progress boundary must be frame-sampled".to_string(),
            );
        }
        if !progress_target.enabled || progress_target.bypassed {
            return Err(
                "Transition Module protected Progress boundary cannot be disabled or bypassed"
                    .to_string(),
            );
        }
        let output = definition
            .output(self.output_id)
            .ok_or_else(|| "Transition Module has no protected Output".to_string())?;
        let output_target = output.target(expected).ok_or_else(|| {
            "Transition Module protected Output has the wrong media type".to_string()
        })?;
        let has_non_contract_connection = definition.graph.connections.iter().any(|connection| {
            connection.to.node_id == output.node_id && connection.to != output_target
        });
        let publishes_non_contract_input =
            definition.interface.media_inputs.iter().any(|input| {
                input.target.node_id == output.node_id && input.target != output_target
            });
        if has_non_contract_connection || publishes_non_contract_input {
            return Err(format!(
                "Transition Module protected Output may accept only its {:?} contract input",
                self.media_type
            ));
        }
        for input in definition
            .interface
            .media_inputs
            .iter()
            .filter(|input| input.id != self.from_input_id && input.id != self.to_input_id)
        {
            self.validate_additional_media_input(input.data_type)?;
        }
        for node in definition
            .graph
            .nodes
            .values()
            .filter(|node| !definition.is_protected_host_boundary_node(node.id))
        {
            self.validate_authored_processing_node(node)?;
        }
        Ok(())
    }
}

impl TransitionMediaType {
    pub const fn port_data_type(self) -> PortDataType {
        match self {
            Self::Image => PortDataType::Image,
            Self::Audio => PortDataType::Audio,
        }
    }
}

impl ModuleDefinition {
    /// One authority for Node Editor lock presentation and edit guards. It
    /// covers A, B, Progress, and the dedicated Transition Output boundary.
    pub fn is_protected_host_boundary_node(&self, node_id: uuid::Uuid) -> bool {
        let Some(contract) = self.host_contract.transition() else {
            return false;
        };
        let public_input_targets = self.interface.media_inputs.iter().any(|input| {
            (input.id == contract.from_input_id || input.id == contract.to_input_id)
                && input.target.node_id == node_id
        });
        let progress_target = self.interface.parameters.iter().any(|parameter| {
            parameter.id == contract.progress_parameter_id && parameter.target.node_id == node_id
        });
        let output_target = self
            .output(contract.output_id)
            .is_some_and(|output| output.node_id == node_id);
        public_input_targets || progress_target || output_target
    }

    /// Creates an editable starter graph with protected A, B, Progress, and
    /// Output boundaries. The production Node Editor edits this same graph.
    pub fn new_transition(
        name: impl Into<String>,
        sharing: ModuleDefinitionSharing,
        media_type: TransitionMediaType,
    ) -> Result<(Self, TransitionModuleInterface), String> {
        let (mut definition, output_id) = Self::new_image(name, sharing);
        let is_audio = media_type == TransitionMediaType::Audio;
        let media_input_port = if is_audio {
            SOUND_INPUT_PORT
        } else {
            IMAGE_INPUT_PORT
        };
        let media_output_port = if is_audio {
            AUDIO_OUTPUT_PORT
        } else {
            IMAGE_OUTPUT_PORT
        };

        let mut from = Node::new_catalog_node(transition_input_node_id(is_audio))?;
        from.name = "A".to_string();
        from.ui_position = [40.0, 60.0];
        let from_node_id = from.id;
        let mut to = Node::new_catalog_node(transition_input_node_id(is_audio))?;
        to.name = "B".to_string();
        to.ui_position = [40.0, 220.0];
        let to_node_id = to.id;
        let mut progress = Node::new_catalog_node(TRANSITION_PROGRESS_INPUT_NODE_ID)?;
        progress.name = "Progress".to_string();
        progress.ui_position = [40.0, 380.0];
        let progress_node_id = progress.id;
        let mut mix = Node::new_catalog_node(transition_mix_node_id(is_audio))?;
        mix.name = if is_audio {
            "Audio Crossfade".to_string()
        } else {
            "Cross Dissolve".to_string()
        };
        mix.ui_position = [360.0, 150.0];
        let mix_node_id = mix.id;
        let output_node_id = definition
            .output(output_id)
            .ok_or_else(|| "Transition Module factory lost its Output".to_string())?
            .node_id;
        definition
            .graph
            .nodes
            .get_mut(&output_node_id)
            .ok_or_else(|| "Transition Module factory lost its Output Node".to_string())?
            .ui_position = [920.0, 170.0];
        definition.graph.nodes.extend([
            (from_node_id, from),
            (to_node_id, to),
            (progress_node_id, progress),
            (mix_node_id, mix),
        ]);

        let address = |node_id, port: &str| ModulePortAddress {
            node_id,
            port: port.to_string(),
        };
        let output_target = definition
            .output(output_id)
            .and_then(|output| output.target(media_type.port_data_type()))
            .ok_or_else(|| "Transition Module Output has no matching media boundary".to_string())?;
        definition.graph.connections.extend([
            ModuleConnection {
                id: ModuleConnectionId::new(),
                from: address(from_node_id, media_output_port),
                to: address(mix_node_id, TRANSITION_FROM_INPUT_PORT),
                order: 0,
                blend_mode: BlendMode::Normal,
            },
            ModuleConnection {
                id: ModuleConnectionId::new(),
                from: address(to_node_id, media_output_port),
                to: address(mix_node_id, TRANSITION_TO_INPUT_PORT),
                order: 0,
                blend_mode: BlendMode::Normal,
            },
            ModuleConnection {
                id: ModuleConnectionId::new(),
                from: address(progress_node_id, NUMBER_RESULT_OUTPUT_PORT),
                to: address(mix_node_id, TRANSITION_PROGRESS_INPUT_PORT),
                order: 0,
                blend_mode: BlendMode::Normal,
            },
            ModuleConnection {
                id: ModuleConnectionId::new(),
                from: address(mix_node_id, media_output_port),
                to: output_target,
                order: 0,
                blend_mode: BlendMode::Normal,
            },
        ]);

        let from_input_id = PublishedMediaInputId::new();
        let to_input_id = PublishedMediaInputId::new();
        let progress_parameter_id = PublishedParameterId::new();
        definition.interface.media_inputs.extend([
            PublishedMediaInput {
                id: from_input_id,
                name: "A".to_string(),
                data_type: media_type.port_data_type(),
                target: address(from_node_id, media_input_port),
                required: true,
                primary: false,
            },
            PublishedMediaInput {
                id: to_input_id,
                name: "B".to_string(),
                data_type: media_type.port_data_type(),
                target: address(to_node_id, media_input_port),
                required: true,
                primary: false,
            },
        ]);
        definition.interface.parameters.push(PublishedParameter {
            id: progress_parameter_id,
            name: "Progress".to_string(),
            data_type: PortDataType::Number,
            default_value: PropertyValue::Number(OrderedFloat(0.0)),
            target: address(progress_node_id, TRANSITION_PROGRESS_INPUT_PORT),
        });
        let contract = TransitionModuleInterface {
            media_type,
            from_input_id,
            to_input_id,
            progress_parameter_id,
            output_id,
        };
        definition.host_contract = ModuleHostContract::Transition(contract.clone());
        definition.topology_revision = definition
            .topology_revision
            .checked_add(1)
            .ok_or_else(|| "Transition Module topology revision overflow".to_string())?;
        definition.interface_version = definition
            .interface_version
            .checked_add(1)
            .ok_or_else(|| "Transition Module interface version overflow".to_string())?;
        definition.validate()?;
        Ok((definition, contract))
    }
}

/// Public-only processor reference persisted by one Timeline Transition.
#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
#[serde(deny_unknown_fields)]
pub struct TransitionModuleProcessor {
    pub instance_id: ModuleInstanceId,
    /// Timeline-owned external media routes keyed by stable Published
    /// Interface identity. Protected A/B are supplied by the host.
    pub input_bindings: HashMap<PublishedMediaInputId, MediaInputBinding>,
    /// Timeline-local keyframes for ordinary Published parameters. Protected
    /// Progress always comes from the Transition interval.
    pub automation_tracks: HashMap<PublishedParameterId, AutomationTrack>,
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::super::ModuleInputPortOwnership;
    use super::*;
    use crate::model::node::NodeContent;
    use crate::model::project::{MERGE_IMAGES_PORT, MERGE_SOUNDS_PORT};

    #[test]
    fn starter_definitions_expose_stable_typed_boundaries() {
        for media_type in [TransitionMediaType::Image, TransitionMediaType::Audio] {
            let (definition, contract) = ModuleDefinition::new_transition(
                "Editable Transition",
                ModuleDefinitionSharing::ReusableTemplate(
                    super::super::ModuleTemplateOrigin::Project,
                ),
                media_type,
            )
            .expect("Transition Module");

            definition.validate().expect("valid Transition Module");
            assert_eq!(definition.host_contract.transition(), Some(&contract));
            assert_eq!(contract.media_type, media_type);
            assert_eq!(definition.graph.nodes.len(), 5);
            assert_eq!(definition.graph.connections.len(), 4);
            assert_eq!(definition.outputs().count(), 1);
            let published_ids = [
                contract.from_input_id.as_uuid(),
                contract.to_input_id.as_uuid(),
                contract.progress_parameter_id.as_uuid(),
                contract.output_id.as_uuid(),
            ]
            .into_iter()
            .collect::<HashSet<_>>();
            assert_eq!(published_ids.len(), 4);
            assert!(definition.graph.nodes.values().any(|node| {
                matches!(node.content(), NodeContent::ModuleOutput(output) if output.id == contract.output_id)
            }));
            let output_node = definition
                .output(contract.output_id)
                .and_then(|output| definition.graph.nodes.get(&output.node_id))
                .expect("protected Output Node");
            assert_eq!(output_node.ui_position, [920.0, 170.0]);
            let protected = definition
                .graph
                .nodes
                .values()
                .filter(|node| definition.is_protected_host_boundary_node(node.id))
                .count();
            assert_eq!(protected, 4);

            for input in definition.interface.media_inputs.iter().filter(|input| {
                input.id == contract.from_input_id || input.id == contract.to_input_id
            }) {
                assert_eq!(
                    definition.input_port_ownership(&input.target),
                    ModuleInputPortOwnership::HostProtected
                );
            }
            let progress = definition
                .interface
                .parameters
                .iter()
                .find(|parameter| parameter.id == contract.progress_parameter_id)
                .expect("protected Progress");
            assert_eq!(
                definition.input_port_ownership(&progress.target),
                ModuleInputPortOwnership::HostProtected
            );
            let output_target = definition
                .output(contract.output_id)
                .expect("protected Output")
                .target(media_type.port_data_type())
                .expect("typed Output target");
            assert_eq!(
                definition.input_port_ownership(&output_target),
                ModuleInputPortOwnership::Internal,
                "the protected Output Node still needs an internal graph input"
            );
        }
    }

    #[test]
    fn transition_output_rejects_the_non_contract_media_port() {
        let (mut definition, contract) = ModuleDefinition::new_transition(
            "Image Transition",
            ModuleDefinitionSharing::Private,
            TransitionMediaType::Image,
        )
        .expect("Transition Module");
        let output = definition
            .output(contract.output_id)
            .expect("protected Output");
        let hidden_audio = Node::new_sound_merge("Hidden Audio");
        let hidden_audio_id = hidden_audio.id;
        definition.graph.nodes.insert(hidden_audio_id, hidden_audio);
        definition.graph.connections.push(ModuleConnection {
            id: ModuleConnectionId::new(),
            from: ModulePortAddress {
                node_id: hidden_audio_id,
                port: AUDIO_OUTPUT_PORT.to_string(),
            },
            to: ModulePortAddress {
                node_id: output.node_id,
                port: SOUND_INPUT_PORT.to_string(),
            },
            order: 0,
            blend_mode: BlendMode::Normal,
        });

        let error = definition
            .validate()
            .expect_err("non-contract Audio terminal must stay unavailable");
        assert!(error.contains("Image contract input"), "{error}");
    }

    #[test]
    fn transition_processor_persists_no_internal_node_address() {
        let instance_id = ModuleInstanceId::new();
        let processor =
            super::super::TransitionProcessor::module(instance_id, TransitionMediaType::Image);
        let encoded = serde_json::to_value(processor).expect("serialize processor");

        assert_eq!(encoded["implementation"]["kind"], "module");
        assert_eq!(
            encoded["implementation"]["value"]["instance_id"],
            instance_id.to_string()
        );
        assert!(encoded.to_string().find("node_id").is_none());
    }

    #[test]
    fn transition_contract_rejects_primary_input_because_timeline_supplies_a_and_b() {
        let (mut primary, contract) = ModuleDefinition::new_transition(
            "Restricted Inputs",
            ModuleDefinitionSharing::Private,
            TransitionMediaType::Image,
        )
        .unwrap();
        primary
            .interface
            .media_inputs
            .iter_mut()
            .find(|input| input.id == contract.from_input_id)
            .unwrap()
            .primary = true;
        assert!(
            contract
                .validate_definition(&primary)
                .unwrap_err()
                .contains("cannot publish a primary")
        );
    }

    #[test]
    fn transition_progress_cannot_retarget_a_constant_only_particle_input() {
        let (mut definition, contract) = ModuleDefinition::new_transition(
            "Protected Progress",
            ModuleDefinitionSharing::Private,
            TransitionMediaType::Image,
        )
        .unwrap();
        let emitter = Node::new_catalog_node("native.particle.emitter").expect("Particle Emitter");
        let emitter_id = emitter.id;
        definition.graph.nodes.insert(emitter_id, emitter);
        definition
            .interface
            .parameters
            .iter_mut()
            .find(|parameter| parameter.id == contract.progress_parameter_id)
            .expect("protected Progress")
            .target = ModulePortAddress {
            node_id: emitter_id,
            port: "rate".to_string(),
        };

        let error = definition
            .validate()
            .expect_err("the host clock must not drive Particle simulation state");
        assert!(error.contains("canonical Progress boundary"), "{error}");
    }

    #[test]
    fn audio_transition_definition_rejects_nodes_without_an_audio_authoring_runtime() {
        let (mut definition, _) = ModuleDefinition::new_transition(
            "Restricted Audio",
            ModuleDefinitionSharing::Private,
            TransitionMediaType::Audio,
        )
        .unwrap();
        let unsupported = Node::new_merge("Image Merge");
        definition.graph.nodes.insert(unsupported.id, unsupported);

        let error = definition
            .validate()
            .expect_err("Image processing cannot be persisted in an Audio Transition Module");
        assert!(error.contains("has no authoring audio runtime"), "{error}");
    }

    #[test]
    fn transition_definition_rejects_a_second_host_boundary_node() {
        let (mut definition, _) = ModuleDefinition::new_transition(
            "Image Transition",
            ModuleDefinitionSharing::Private,
            TransitionMediaType::Image,
        )
        .unwrap();
        let duplicate = crate::model::native_node_descriptor(TRANSITION_IMAGE_INPUT_NODE_ID)
            .expect("Image boundary descriptor")
            .create_detached_node()
            .expect("detached boundary Node");
        definition.graph.nodes.insert(duplicate.id, duplicate);

        let error = definition
            .validate()
            .expect_err("host boundaries may only come from the starter factory");
        assert!(error.contains("can only be created by"), "{error}");
    }

    #[test]
    fn audio_transition_definition_accepts_the_finite_runtime_node_set() {
        let (mut definition, _) = ModuleDefinition::new_transition(
            "Supported Audio",
            ModuleDefinitionSharing::Private,
            TransitionMediaType::Audio,
        )
        .unwrap();
        let merge = Node::new_sound_merge("Audio Merge");
        let value = Node::new_add("Progress Offset");
        definition
            .graph
            .nodes
            .extend([(merge.id, merge), (value.id, value)]);

        definition.validate().unwrap();
    }

    #[test]
    fn audio_transition_definition_rejects_additional_published_media_inputs() {
        let (mut definition, _) = ModuleDefinition::new_transition(
            "No Additional Audio",
            ModuleDefinitionSharing::Private,
            TransitionMediaType::Audio,
        )
        .unwrap();
        let merge = Node::new_sound_merge("Additional Input Target");
        let input_id = PublishedMediaInputId::new();
        definition.interface.media_inputs.push(PublishedMediaInput {
            id: input_id,
            name: "Sidechain".to_string(),
            data_type: PortDataType::Audio,
            target: ModulePortAddress {
                node_id: merge.id,
                port: MERGE_SOUNDS_PORT.to_string(),
            },
            required: false,
            primary: false,
        });
        definition.graph.nodes.insert(merge.id, merge);

        let error = definition
            .validate()
            .expect_err("the audio mixer cannot resolve additional Published media inputs");
        assert!(
            error.contains("supplies only the host-owned A/B"),
            "{error}"
        );
    }

    #[test]
    fn image_transition_keeps_additional_image_inputs() {
        let (mut definition, _) = ModuleDefinition::new_transition(
            "Image Matte",
            ModuleDefinitionSharing::Private,
            TransitionMediaType::Image,
        )
        .unwrap();
        let merge = Node::new_merge("Matte Input Target");
        definition.interface.media_inputs.push(PublishedMediaInput {
            id: PublishedMediaInputId::new(),
            name: "Matte".to_string(),
            data_type: PortDataType::Image,
            target: ModulePortAddress {
                node_id: merge.id,
                port: MERGE_IMAGES_PORT.to_string(),
            },
            required: false,
            primary: false,
        });
        definition.graph.nodes.insert(merge.id, merge);

        definition.validate().unwrap();
    }

    #[test]
    fn image_transition_rejects_an_additional_audio_input() {
        let (mut definition, _) = ModuleDefinition::new_transition(
            "Image Transition",
            ModuleDefinitionSharing::Private,
            TransitionMediaType::Image,
        )
        .unwrap();
        let merge = Node::new_sound_merge("Audio Input Target");
        definition.interface.media_inputs.push(PublishedMediaInput {
            id: PublishedMediaInputId::new(),
            name: "Audio".to_string(),
            data_type: PortDataType::Audio,
            target: ModulePortAddress {
                node_id: merge.id,
                port: MERGE_SOUNDS_PORT.to_string(),
            },
            required: false,
            primary: false,
        });
        definition.graph.nodes.insert(merge.id, merge);

        let error = definition
            .validate()
            .expect_err("the Image runtime cannot resolve an Audio input");
        assert!(
            error.contains("accepts only additional Image inputs"),
            "{error}"
        );
    }

    #[test]
    fn atomic_assignment_rejects_a_required_additional_input() {
        let (mut definition, contract) = ModuleDefinition::new_transition(
            "Required Matte",
            ModuleDefinitionSharing::Private,
            TransitionMediaType::Image,
        )
        .unwrap();
        let merge = Node::new_merge("Required Matte Target");
        definition.interface.media_inputs.push(PublishedMediaInput {
            id: PublishedMediaInputId::new(),
            name: "Matte".to_string(),
            data_type: PortDataType::Image,
            target: ModulePortAddress {
                node_id: merge.id,
                port: MERGE_IMAGES_PORT.to_string(),
            },
            required: true,
            primary: false,
        });
        definition.graph.nodes.insert(merge.id, merge);

        let error = contract
            .validate_atomic_assignment(&definition)
            .expect_err("an empty-controls chooser cannot satisfy required inputs");
        assert!(
            error.contains("cannot be assigned without controls"),
            "{error}"
        );
    }
}
