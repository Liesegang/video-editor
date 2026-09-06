//! Typed clipboard payload and one-transaction paste for Module selections.

use std::collections::{HashMap, HashSet};

use serde::{Deserialize, Serialize};

use super::output::require_insertable_processing_node;
use super::{bump_interface_version, bump_topology_revision, private_definition_for_instance};
use crate::model::authoring::{
    AttachmentProcessor, AutomationTrack, ChangeSet, ModuleConnection, ModuleInstanceId,
    PublishedAction, PublishedActionId, PublishedParameter, PublishedParameterId, PublishedSignal,
    PublishedSignalId, SourceRef, TransitionId,
};
use crate::model::node::Node;
use crate::model::node::NodeContent;
use crate::model::property::PropertyValue;

use super::super::module_asset::require_project_asset;
use super::super::*;

const CLIPBOARD_VERSION: u32 = 1;

#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ClipboardParameter {
    published: PublishedParameter,
    current_override: Option<PropertyValue>,
    automation: Option<AutomationTrack>,
}

/// Serialized processing-node selection. Host media inputs and their external
/// bindings are deliberately outside this payload; parameters, signals, and
/// actions directly owned by selected Nodes remain part of the selection.
#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ModuleSelectionClipboard {
    version: u32,
    nodes: Vec<Node>,
    connections: Vec<ModuleConnection>,
    parameters: Vec<ClipboardParameter>,
    signals: Vec<PublishedSignal>,
    actions: Vec<PublishedAction>,
}

#[derive(Clone, PartialEq, Debug)]
pub struct ModuleSelectionPasteReceipt {
    pub definition_id: ModuleDefinitionId,
    pub node_ids: Vec<uuid::Uuid>,
    pub changes: ChangeSet,
}

#[derive(Clone, Copy)]
enum InvocationOwner {
    Module(ModuleAutomationOwner),
    Transition(TransitionId),
}

impl ModuleSelectionClipboard {
    pub fn capture(
        project: &AuthoringProject,
        instance_id: ModuleInstanceId,
        selected_node_ids: &[uuid::Uuid],
    ) -> Result<Self, String> {
        project.validate()?;
        let instance = project
            .module_instances
            .get(&instance_id)
            .ok_or_else(|| format!("Missing Module instance {instance_id}"))?;
        let definition = project
            .module_definitions
            .get(&instance.definition_id)
            .ok_or_else(|| format!("Missing Module definition {}", instance.definition_id))?;
        let requested = selected_node_ids.iter().copied().collect::<HashSet<_>>();
        if requested.len() != selected_node_ids.len() {
            return Err("A Module clipboard selection contains duplicate Node IDs".to_string());
        }
        for node_id in &requested {
            if !definition.graph.nodes.contains_key(node_id) {
                return Err(format!("Missing Module Node {node_id}"));
            }
        }
        let selected = requested
            .iter()
            .copied()
            .filter(|node_id| {
                let node = &definition.graph.nodes[node_id];
                require_insertable_processing_node(node).is_ok()
                    && !definition.is_protected_host_boundary_node(*node_id)
            })
            .collect::<HashSet<_>>();
        if selected.is_empty() {
            return Err("A Module clipboard selection must contain a processing Node".to_string());
        }
        let mut nodes = selected
            .iter()
            .map(|node_id| definition.graph.nodes[node_id].clone())
            .collect::<Vec<_>>();
        nodes.sort_by_key(|node| node.id);
        let connections = definition
            .graph
            .connections
            .iter()
            .filter(|connection| {
                selected.contains(&connection.from.node_id)
                    && selected.contains(&connection.to.node_id)
            })
            .cloned()
            .collect();
        let invocation = invocation(project, instance_id)?;
        let parameters = definition
            .interface
            .parameters
            .iter()
            .filter(|parameter| selected.contains(&parameter.target.node_id))
            .map(|parameter| ClipboardParameter {
                published: parameter.clone(),
                current_override: instance.parameter_overrides.get(&parameter.id).cloned(),
                automation: invocation.automation_tracks().get(&parameter.id).cloned(),
            })
            .collect();
        let signals = definition
            .interface
            .signals
            .iter()
            .filter(|signal| selected.contains(&signal.source.node_id))
            .cloned()
            .collect();
        let actions = definition
            .interface
            .actions
            .iter()
            .filter(|action| selected.contains(&action.target.node_id))
            .cloned()
            .collect();
        Ok(Self {
            version: CLIPBOARD_VERSION,
            nodes,
            connections,
            parameters,
            signals,
            actions,
        })
    }
}

impl TimelineEditorService {
    pub fn paste_instance_module_selection(
        &self,
        instance_id: ModuleInstanceId,
        clipboard: &ModuleSelectionClipboard,
        origin: [f32; 2],
    ) -> Result<ModuleSelectionPasteReceipt, LibraryError> {
        validate_clipboard(clipboard, origin).map_err(LibraryError::Validation)?;
        let mut session = self.write_session()?;
        let ((definition_id, node_ids), changes) = session
            .transact(
                vec![ProjectInvalidation::ModuleInstance { instance_id }],
                |project| paste(project, instance_id, clipboard, origin),
            )
            .map_err(LibraryError::Validation)?;
        Ok(ModuleSelectionPasteReceipt {
            definition_id,
            node_ids,
            changes,
        })
    }
}

fn validate_clipboard(
    clipboard: &ModuleSelectionClipboard,
    origin: [f32; 2],
) -> Result<(), String> {
    if clipboard.version != CLIPBOARD_VERSION {
        return Err(format!(
            "Unsupported Module clipboard version {}",
            clipboard.version
        ));
    }
    if !origin.into_iter().all(f32::is_finite) {
        return Err("Module paste origin must be finite".to_string());
    }
    if clipboard.nodes.is_empty() {
        return Err("A Module clipboard selection must contain a processing Node".to_string());
    }
    let node_ids = clipboard
        .nodes
        .iter()
        .map(|node| {
            require_insertable_processing_node(node)?;
            Ok(node.id)
        })
        .collect::<Result<HashSet<_>, String>>()?;
    if node_ids.len() != clipboard.nodes.len() {
        return Err("A Module clipboard selection contains duplicate Node IDs".to_string());
    }
    let mut connection_ids = HashSet::new();
    for connection in &clipboard.connections {
        if !connection_ids.insert(connection.id) {
            return Err(
                "A Module clipboard selection contains duplicate Connection IDs".to_string(),
            );
        }
        if !node_ids.contains(&connection.from.node_id)
            || !node_ids.contains(&connection.to.node_id)
        {
            return Err("A Module clipboard Connection escapes the selected Nodes".to_string());
        }
    }
    let mut interface_ids = HashSet::new();
    for parameter in &clipboard.parameters {
        if !interface_ids.insert(parameter.published.id.as_uuid())
            || !node_ids.contains(&parameter.published.target.node_id)
        {
            return Err(
                "A Module clipboard parameter is duplicated or targets another Node".to_string(),
            );
        }
    }
    for signal in &clipboard.signals {
        if !interface_ids.insert(signal.id.as_uuid()) || !node_ids.contains(&signal.source.node_id)
        {
            return Err(
                "A Module clipboard signal is duplicated or sourced by another Node".to_string(),
            );
        }
    }
    for action in &clipboard.actions {
        if !interface_ids.insert(action.id.as_uuid()) || !node_ids.contains(&action.target.node_id)
        {
            return Err(
                "A Module clipboard action is duplicated or targets another Node".to_string(),
            );
        }
    }
    Ok(())
}

fn paste(
    project: &mut AuthoringProject,
    instance_id: ModuleInstanceId,
    clipboard: &ModuleSelectionClipboard,
    origin: [f32; 2],
) -> Result<(ModuleDefinitionId, Vec<uuid::Uuid>), String> {
    for node in &clipboard.nodes {
        if let NodeContent::Media(media) = node.content() {
            require_project_asset(project, media.asset_id)?;
        }
    }
    let invocation_owner = invocation_owner(project, instance_id)?;
    let definition_id = private_definition_for_instance(project, instance_id)?;
    let minimum = clipboard
        .nodes
        .iter()
        .fold([f32::INFINITY; 2], |minimum, node| {
            [
                minimum[0].min(node.ui_position[0]),
                minimum[1].min(node.ui_position[1]),
            ]
        });
    let node_ids = clipboard
        .nodes
        .iter()
        .map(|node| (node.id, uuid::Uuid::new_v4()))
        .collect::<HashMap<_, _>>();
    let parameter_ids = clipboard
        .parameters
        .iter()
        .map(|parameter| (parameter.published.id, PublishedParameterId::new()))
        .collect::<HashMap<_, _>>();
    {
        let definition = project
            .module_definitions
            .get_mut(&definition_id)
            .ok_or_else(|| format!("Missing Module definition {definition_id}"))?;
        for source in &clipboard.nodes {
            let mut node = source.clone();
            node.id = node_ids[&source.id];
            node.ui_position = [
                origin[0] + source.ui_position[0] - minimum[0],
                origin[1] + source.ui_position[1] - minimum[1],
            ];
            definition
                .host_contract
                .validate_authored_processing_node(&node)?;
            if definition.graph.nodes.insert(node.id, node).is_some() {
                return Err("Generated Module Node ID collided during paste".to_string());
            }
        }
        for source in &clipboard.connections {
            let mut connection = source.clone();
            connection.id = ModuleConnectionId::new();
            connection.from.node_id = node_ids[&source.from.node_id];
            connection.to.node_id = node_ids[&source.to.node_id];
            definition.graph.connections.push(connection);
        }
        for source in &clipboard.parameters {
            let mut parameter = source.published.clone();
            parameter.id = parameter_ids[&source.published.id];
            parameter.target.node_id = node_ids[&source.published.target.node_id];
            definition.interface.parameters.push(parameter);
        }
        for source in &clipboard.signals {
            let mut signal = source.clone();
            signal.id = PublishedSignalId::new();
            signal.source.node_id = node_ids[&source.source.node_id];
            definition.interface.signals.push(signal);
        }
        for source in &clipboard.actions {
            let mut action = source.clone();
            action.id = PublishedActionId::new();
            action.target.node_id = node_ids[&source.target.node_id];
            definition.interface.actions.push(action);
        }
        bump_topology_revision(definition)?;
        if !clipboard.parameters.is_empty()
            || !clipboard.signals.is_empty()
            || !clipboard.actions.is_empty()
        {
            bump_interface_version(definition)?;
        }
    }
    {
        let instance = project
            .module_instances
            .get_mut(&instance_id)
            .ok_or_else(|| format!("Missing Module instance {instance_id}"))?;
        for source in &clipboard.parameters {
            if let Some(value) = &source.current_override {
                instance
                    .parameter_overrides
                    .insert(parameter_ids[&source.published.id], value.clone());
            }
        }
    }
    let mut invocation = invocation_mut(project, invocation_owner)?;
    let tracks = invocation.automation_tracks_mut();
    for source in &clipboard.parameters {
        if let Some(mut track) = source.automation.clone() {
            for keyframe in &mut track.keyframes {
                keyframe.id = KeyframeId::new();
            }
            tracks.insert(parameter_ids[&source.published.id], track);
        }
    }
    let mut pasted_ids = clipboard
        .nodes
        .iter()
        .map(|node| node_ids[&node.id])
        .collect::<Vec<_>>();
    pasted_ids.sort();
    Ok((definition_id, pasted_ids))
}

enum InvocationMut<'a> {
    Ordinary(&'a mut ModuleInvocation),
    Transition(&'a mut crate::model::authoring::TransitionModuleProcessor),
}

impl InvocationMut<'_> {
    fn automation_tracks_mut(&mut self) -> &mut HashMap<PublishedParameterId, AutomationTrack> {
        match self {
            Self::Ordinary(invocation) => &mut invocation.automation_tracks,
            Self::Transition(invocation) => &mut invocation.automation_tracks,
        }
    }
}

fn invocation_owner(
    project: &AuthoringProject,
    instance_id: ModuleInstanceId,
) -> Result<InvocationOwner, String> {
    let mut owners =
        project
            .items
            .values()
            .filter_map(|item| match &item.source {
                SourceRef::Module(invocation) if invocation.instance_id == instance_id => Some(
                    InvocationOwner::Module(ModuleAutomationOwner::Item(item.id)),
                ),
                _ => None,
            })
            .chain(project.attachments.values().filter_map(
                |attachment| match &attachment.processor {
                    AttachmentProcessor::Module(invocation)
                        if invocation.instance_id == instance_id =>
                    {
                        Some(InvocationOwner::Module(ModuleAutomationOwner::Attachment(
                            attachment.id,
                        )))
                    }
                    _ => None,
                },
            ))
            .chain(project.transitions.values().filter_map(|transition| {
                transition
                    .processor
                    .module_processor()
                    .filter(|invocation| invocation.instance_id == instance_id)
                    .map(|_| InvocationOwner::Transition(transition.id))
            }));
    let owner = owners
        .next()
        .ok_or_else(|| format!("Module instance {instance_id} has no invocation owner"))?;
    if owners.next().is_some() {
        return Err(format!(
            "Module instance {instance_id} has more than one invocation owner"
        ));
    }
    Ok(owner)
}

fn invocation(
    project: &AuthoringProject,
    instance_id: ModuleInstanceId,
) -> Result<InvocationRef<'_>, String> {
    match invocation_owner(project, instance_id)? {
        InvocationOwner::Module(owner) => owner.invocation(project).map(InvocationRef::Ordinary),
        InvocationOwner::Transition(transition_id) => project.transitions[&transition_id]
            .processor
            .module_processor()
            .map(InvocationRef::Transition)
            .ok_or_else(|| format!("Transition {transition_id} is not a Module Transition")),
    }
}

enum InvocationRef<'a> {
    Ordinary(&'a ModuleInvocation),
    Transition(&'a crate::model::authoring::TransitionModuleProcessor),
}

impl InvocationRef<'_> {
    fn automation_tracks(&self) -> &HashMap<PublishedParameterId, AutomationTrack> {
        match self {
            Self::Ordinary(invocation) => &invocation.automation_tracks,
            Self::Transition(invocation) => &invocation.automation_tracks,
        }
    }
}

fn invocation_mut(
    project: &mut AuthoringProject,
    owner: InvocationOwner,
) -> Result<InvocationMut<'_>, String> {
    match owner {
        InvocationOwner::Module(owner) => {
            owner.invocation_mut(project).map(InvocationMut::Ordinary)
        }
        InvocationOwner::Transition(transition_id) => project
            .transitions
            .get_mut(&transition_id)
            .ok_or_else(|| format!("Missing Transition {transition_id}"))?
            .processor
            .module_processor_mut()
            .map(InvocationMut::Transition)
            .ok_or_else(|| format!("Transition {transition_id} is not a Module Transition")),
    }
}

#[cfg(test)]
mod tests;
