//! Inspector controls for one bounded Node Clip invocation.
//!
//! The Timeline owns parameter automation and external media bindings. The
//! Module Definition supplies the finite graph and its published interface.

use library::editor::TimelineEditorService;
use library::model::authoring::{
    AuthoringProject, ModuleDefinition, ModuleInvocation, PublishedParameter, PublishedParameterId,
    TimelineItem,
};
use library::model::node::native_node_descriptor_for_node;
use library::plugin::PluginManager;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use crate::state::authoring::AuthoringUiState;
use crate::ui::module_parameter_editor::ModuleParameterContext;

use super::property_authoring::published_parameter_row;

#[expect(
    clippy::too_many_arguments,
    reason = "the Node Clip Inspector borrows the authoritative Project, editor services, preview service, and selected invocation without creating parallel ownership"
)]
pub(super) fn module_parameters(
    ui: &mut egui::Ui,
    project: &Arc<AuthoringProject>,
    state: &mut AuthoringUiState,
    service: &TimelineEditorService,
    plugins: &PluginManager,
    media_previews: &mut crate::ui::media_preview::AuthoringMediaPreviewService,
    item: &TimelineItem,
    invocation: &ModuleInvocation,
) {
    let Some(instance) = project.module_instances.get(&invocation.instance_id) else {
        return;
    };
    let Some(definition) = project.module_definitions.get(&instance.definition_id) else {
        return;
    };
    let structured_ensemble = match service.node_clip_text_ensemble_stack(item.id) {
        Ok(stack) => stack,
        Err(error) => {
            state.error = Some(error.to_string());
            None
        }
    };
    let structured_appearance = match service.node_clip_appearance_stack(item.id) {
        Ok(stack) => stack,
        Err(error) => {
            state.error = Some(error.to_string());
            None
        }
    };
    let structured_parameter_ids = structured_ensemble
        .iter()
        .flat_map(|stack| &stack.operations)
        .flat_map(|operation| operation.parameter_ids.iter().copied())
        .chain(
            structured_appearance
                .iter()
                .flat_map(|stack| &stack.operations)
                .flat_map(|operation| operation.parameter_ids.iter().copied()),
        )
        .collect::<HashSet<_>>();
    let context = ModuleParameterContext {
        project,
        service,
        plugins,
        owner: library::editor::ModuleParameterOwner::Invocation(
            library::editor::ModuleAutomationOwner::Item(item.id),
        ),
        instance,
        definition,
    };
    ui.separator();
    egui::CollapsingHeader::new("Node Clip parameters")
        .default_open(true)
        .show(ui, |ui| {
            if definition.interface.parameters.is_empty() {
                ui.weak("Publish a Node input to expose a reusable control here.");
            }
            for group in parameter_groups(definition, &structured_parameter_ids) {
                let response = egui::CollapsingHeader::new(&group.label)
                    .id_salt(("node-clip-parameter-group", definition.id, group.node_id))
                    .default_open(true)
                    .show(ui, |ui| {
                        for parameter in &group.parameters {
                            published_parameter_row(
                                ui,
                                state,
                                &context,
                                parameter,
                                Some(project),
                                Some(media_previews),
                            );
                        }
                    })
                    .header_response;
                crate::qa::register_component_with_metadata(
                    format!(
                        "inspector.node_clip.parameter_group:{}:{}",
                        definition.id, group.node_id
                    ),
                    "inspector_node_clip_parameter_group",
                    crate::qa::global_response_rect(ui.ctx(), &response),
                    true,
                    Some(serde_json::json!({
                        "definition_id": definition.id,
                        "node_id": group.node_id,
                        "label": group.label,
                        "parameter_count": group.parameters.len(),
                    })),
                );
            }
        });
    if let Some(stack) = structured_appearance.as_ref() {
        super::appearance::node_clip_appearance_section(ui, state, &context, stack);
    }
    if let Some(stack) = structured_ensemble.as_ref() {
        super::text_ensemble::node_clip_text_ensemble_section(ui, state, &context, stack);
    }
    module_media_inputs(ui, project, state, service, item, invocation, definition);
}

struct ParameterGroup<'a> {
    node_id: uuid::Uuid,
    label: String,
    parameters: Vec<&'a PublishedParameter>,
}

fn parameter_groups<'a>(
    definition: &'a ModuleDefinition,
    excluded: &HashSet<PublishedParameterId>,
) -> Vec<ParameterGroup<'a>> {
    let mut groups = Vec::<ParameterGroup<'a>>::new();
    let mut group_indices = HashMap::<uuid::Uuid, usize>::new();
    for parameter in definition
        .interface
        .parameters
        .iter()
        .filter(|parameter| !excluded.contains(&parameter.id))
    {
        let node_id = parameter.target.node_id;
        let index = *group_indices.entry(node_id).or_insert_with(|| {
            let index = groups.len();
            groups.push(ParameterGroup {
                node_id,
                label: parameter_group_label(definition, node_id),
                parameters: Vec::new(),
            });
            index
        });
        groups[index].parameters.push(parameter);
    }
    groups
}

fn parameter_group_label(definition: &ModuleDefinition, node_id: uuid::Uuid) -> String {
    let Some(node) = definition.graph.nodes.get(&node_id) else {
        return format!("Missing Node {node_id}");
    };
    if !node.name.trim().is_empty() {
        return node.name.clone();
    }
    native_node_descriptor_for_node(node)
        .map_or("Node", |descriptor| descriptor.label())
        .to_string()
}

fn module_media_inputs(
    ui: &mut egui::Ui,
    project: &AuthoringProject,
    state: &mut AuthoringUiState,
    service: &TimelineEditorService,
    item: &TimelineItem,
    invocation: &ModuleInvocation,
    definition: &ModuleDefinition,
) {
    if definition.interface.media_inputs.is_empty() {
        return;
    }
    let Some(host_track) = project.tracks.get(&item.track_id) else {
        return;
    };

    egui::CollapsingHeader::new("Node Clip inputs")
        .default_open(true)
        .show(ui, |ui| {
            for input in &definition.interface.media_inputs {
                let control_id = format!("module_item:{}:module_input:{}", item.id, input.id);
                let action = crate::ui::module_media_input::media_input_picker(
                    ui,
                    crate::ui::module_media_input::MediaInputPicker {
                        control_id: &control_id,
                        project,
                        timeline_id: host_track.timeline_id,
                        input,
                        current: invocation.input_bindings.get(&input.id),
                        excluded_items: std::slice::from_ref(&item.id),
                        required_coverage: None,
                        can_inherit: false,
                    },
                );
                match action {
                    Some(crate::ui::module_media_input::MediaInputPickerAction::Bind(binding)) => {
                        match service.bind_module_input(item.id, input.id, binding) {
                            Ok(_) => state.status = format!("Bound {}", input.name),
                            Err(error) => state.error = Some(error.to_string()),
                        }
                    }
                    Some(crate::ui::module_media_input::MediaInputPickerAction::Unbind) => {
                        match service.unbind_module_input(item.id, input.id) {
                            Ok(_) => state.status = format!("Unbound {}", input.name),
                            Err(error) => state.error = Some(error.to_string()),
                        }
                    }
                    Some(crate::ui::module_media_input::MediaInputPickerAction::Inherit) | None => {
                    }
                }
            }
            ui.weak("Inputs reference clip outputs, not internal Node UUIDs.");
        });
}

#[cfg(test)]
mod tests {
    use crate::ui::property_metadata::published_parameter_keyframe_capability;
    use library::editor::ParticleNodeClipFactory;
    use std::collections::HashSet;

    #[test]
    fn particle_inspector_offers_only_runtime_supported_keyframes() {
        let particle = ParticleNodeClipFactory::create("Particle").expect("Particle Node Clip");

        let (rate_allowed, rate_reason) = published_parameter_keyframe_capability(
            &particle.definition,
            particle.parameters.emission_rate,
        );
        assert!(!rate_allowed);
        assert!(rate_reason.is_some_and(|reason| reason.contains("fixed-step")));

        for parameter_id in [
            particle.parameters.color,
            particle.parameters.sprites,
            particle.parameters.selection_mode,
            particle.parameters.selection,
        ] {
            assert_eq!(
                published_parameter_keyframe_capability(&particle.definition, parameter_id),
                (true, None)
            );
        }
    }

    #[test]
    fn published_parameters_group_by_target_in_first_seen_order_without_duplicates() {
        let particle = ParticleNodeClipFactory::create("Particle").expect("Particle Node Clip");
        let excluded = HashSet::from([
            particle.parameters.collision_bounce,
            particle.parameters.color,
        ]);
        let groups = super::parameter_groups(&particle.definition, &excluded);

        let expected_nodes = particle
            .definition
            .interface
            .parameters
            .iter()
            .filter(|parameter| !excluded.contains(&parameter.id))
            .map(|parameter| parameter.target.node_id)
            .fold(Vec::new(), |mut nodes, node_id| {
                if !nodes.contains(&node_id) {
                    nodes.push(node_id);
                }
                nodes
            });
        assert_eq!(
            groups.iter().map(|group| group.node_id).collect::<Vec<_>>(),
            expected_nodes
        );
        let rendered = groups
            .iter()
            .flat_map(|group| group.parameters.iter().map(|parameter| parameter.id))
            .collect::<Vec<_>>();
        let expected = particle
            .definition
            .interface
            .parameters
            .iter()
            .filter(|parameter| !excluded.contains(&parameter.id))
            .map(|parameter| parameter.id)
            .collect::<HashSet<_>>();
        assert_eq!(rendered.iter().copied().collect::<HashSet<_>>(), expected);
        assert_eq!(rendered.len(), expected.len());
        for group in groups {
            let source_positions = group
                .parameters
                .iter()
                .map(|parameter| {
                    particle
                        .definition
                        .interface
                        .parameters
                        .iter()
                        .position(|candidate| candidate.id == parameter.id)
                        .expect("grouped parameter belongs to the interface")
                })
                .collect::<Vec<_>>();
            assert!(source_positions.windows(2).all(|pair| pair[0] < pair[1]));
        }
    }

    #[test]
    fn parameter_group_label_tracks_node_rename_and_falls_back_to_catalog_label() {
        let mut particle = ParticleNodeClipFactory::create("Particle").expect("Particle Node Clip");
        let node_id = particle.definition.interface.parameters[0].target.node_id;
        let node = particle
            .definition
            .graph
            .nodes
            .get_mut(&node_id)
            .expect("published target Node");
        node.name = "Renamed Emitter".to_string();
        assert_eq!(
            super::parameter_group_label(&particle.definition, node_id),
            "Renamed Emitter"
        );
        particle
            .definition
            .graph
            .nodes
            .get_mut(&node_id)
            .expect("published target Node")
            .name = "  ".to_string();
        assert_eq!(
            super::parameter_group_label(&particle.definition, node_id),
            "Particle Emitter"
        );
    }
}
