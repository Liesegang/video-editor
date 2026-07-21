//! Read-only projection of one connected Node input for authoring UI.
//!
//! Scalar values use the same scope, bypass, arithmetic, and property
//! traversal as frame rendering. Media and complex domains intentionally stop
//! at their validated typed connections so inspecting a Node never decodes a
//! frame, mixes audio, or computes an FFT.

use std::collections::HashSet;

use super::evaluator::FrameEvaluator;
use crate::error::LibraryError;
use crate::model::project::{
    EvalOutput, PortAddress, PortDataType, PortDirection, PortMultiplicity,
};
use crate::model::property::PropertyValue;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum InputValuePreview {
    Value {
        value: PropertyValue,
        source: PortAddress,
        declared_type: PortDataType,
    },
    TypeSummary {
        data_type: PortDataType,
        sources: Vec<PortAddress>,
    },
    NoOutput {
        declared_type: PortDataType,
        source: Option<PortAddress>,
    },
}

impl FrameEvaluator<'_> {
    /// Resolve one authored input at Composition timeline time without
    /// creating or persisting an intermediate graph model.
    pub fn evaluate_input_preview(
        &self,
        target: &PortAddress,
        timeline_time: f64,
    ) -> Result<InputValuePreview, LibraryError> {
        let definition = self
            .project
            .port_definition(target, PortDirection::Input)
            .ok_or_else(|| LibraryError::Validation(format!("Missing input port {target:?}")))?;
        if definition.multiplicity == PortMultiplicity::Variadic {
            let connections = self.validated_connections_to(target)?;
            if connections.is_empty() {
                return Ok(InputValuePreview::NoOutput {
                    declared_type: definition.data_type,
                    source: None,
                });
            }
            return Ok(InputValuePreview::TypeSummary {
                data_type: summary_type(self, definition.data_type, &connections),
                sources: connections
                    .into_iter()
                    .map(|connection| connection.from.clone())
                    .collect(),
            });
        }
        let connection = match self.single_connection_to(target)? {
            EvalOutput::Produced(connection) => connection,
            EvalOutput::NoOutput => {
                return Ok(InputValuePreview::NoOutput {
                    declared_type: definition.data_type,
                    source: None,
                });
            }
        };
        let source = connection.from.clone();
        if !is_scalar_preview_type(definition.data_type) {
            return Ok(InputValuePreview::TypeSummary {
                data_type: summary_type(self, definition.data_type, &[connection]),
                sources: vec![source],
            });
        }
        match self.resolve_metadata_value(&source, timeline_time, &mut HashSet::new())? {
            EvalOutput::Produced(value) => Ok(InputValuePreview::Value {
                value,
                source,
                declared_type: definition.data_type,
            }),
            EvalOutput::NoOutput => Ok(InputValuePreview::NoOutput {
                declared_type: definition.data_type,
                source: Some(source),
            }),
        }
    }
}

fn is_scalar_preview_type(data_type: PortDataType) -> bool {
    matches!(
        data_type,
        PortDataType::Numeric
            | PortDataType::Number
            | PortDataType::Integer
            | PortDataType::Boolean
            | PortDataType::String
            | PortDataType::Color
            | PortDataType::Vec2
            | PortDataType::Vec3
            | PortDataType::Vec4
    )
}

fn summary_type(
    evaluator: &FrameEvaluator<'_>,
    declared_type: PortDataType,
    connections: &[&crate::model::project::ProjectConnection],
) -> PortDataType {
    if declared_type != PortDataType::Any {
        return declared_type;
    }
    let mut source_types = connections.iter().filter_map(|connection| {
        evaluator
            .project
            .port_definition(&connection.from, PortDirection::Output)
            .map(|definition| definition.data_type)
    });
    let Some(first) = source_types.next() else {
        return PortDataType::Any;
    };
    if source_types.all(|data_type| data_type == first) {
        first
    } else {
        PortDataType::Any
    }
}

#[cfg(test)]
mod tests {
    use ordered_float::OrderedFloat;

    use super::*;
    use crate::model::project::{
        FMOD_X_INPUT_PORT, IMAGE_INPUT_PORT, IMAGE_OUTPUT_PORT, NUMBER_RESULT_OUTPUT_PORT,
        PortOwner, ProjectConnection, TIME_PORT,
    };
    use crate::model::{Composition, Node, NodeContainer, Project};
    use crate::plugin::PluginManager;

    struct Fixture {
        project: Project,
        composition_id: uuid::Uuid,
        value_target: PortAddress,
        image_source: PortAddress,
        image_target: PortAddress,
        source_id: uuid::Uuid,
    }

    fn fixture() -> Fixture {
        let mut project = Project::new("input preview");
        let (composition, track) = Composition::new("Main", 640, 360, 30.0, 5.0);
        let composition_id = composition.id;
        project.add_track(track).unwrap();
        project.add_composition(composition).unwrap();
        let container = NodeContainer::Composition(composition_id);

        let mut source = Node::new_fmod("Source");
        source
            .set_property(
                "divisor".to_string(),
                crate::model::property::Property::constant(PropertyValue::Number(OrderedFloat(
                    10.0,
                ))),
            )
            .unwrap();
        let source_id = source.id;
        project.add_node(source);
        project
            .attach_node_to_container(container, source_id)
            .unwrap();
        project
            .connect_ports(
                PortAddress::new(PortOwner::Composition(composition_id), TIME_PORT),
                PortAddress::new(PortOwner::Node(source_id), FMOD_X_INPUT_PORT),
            )
            .unwrap();

        let target = Node::new_fmod("Target");
        let target_id = target.id;
        project.add_node(target);
        project
            .attach_node_to_container(container, target_id)
            .unwrap();
        let value_target = PortAddress::new(PortOwner::Node(target_id), FMOD_X_INPUT_PORT);
        project
            .connect_ports(
                PortAddress::new(PortOwner::Node(source_id), NUMBER_RESULT_OUTPUT_PORT),
                value_target.clone(),
            )
            .unwrap();

        let image_source = Node::new_merge("Image Source");
        let image_source_id = image_source.id;
        project.add_node(image_source);
        project
            .attach_node_to_container(container, image_source_id)
            .unwrap();
        let image_target_node = PluginManager::default()
            .create_image_transform_operation_node()
            .unwrap();
        let image_target_id = image_target_node.id;
        project.add_node(image_target_node);
        project
            .attach_node_to_container(container, image_target_id)
            .unwrap();
        let image_target = PortAddress::new(PortOwner::Node(image_target_id), IMAGE_INPUT_PORT);
        let image_source = PortAddress::new(PortOwner::Node(image_source_id), IMAGE_OUTPUT_PORT);
        project
            .connect_ports(image_source.clone(), image_target.clone())
            .unwrap();

        Fixture {
            project,
            composition_id,
            value_target,
            image_source,
            image_target,
            source_id,
        }
    }

    fn evaluate(
        fixture: &Fixture,
        target: &PortAddress,
        time: f64,
    ) -> Result<InputValuePreview, LibraryError> {
        let plugins = PluginManager::default();
        let composition = fixture
            .project
            .get_composition(fixture.composition_id)
            .unwrap();
        FrameEvaluator::new(
            &fixture.project,
            composition,
            plugins.get_property_evaluators(),
            &plugins,
        )
        .evaluate_input_preview(target, time)
    }

    #[test]
    fn scalar_preview_uses_authoritative_time_and_arithmetic_graph() {
        let fixture = fixture();
        assert_eq!(
            evaluate(&fixture, &fixture.value_target, 2.25).unwrap(),
            InputValuePreview::Value {
                value: PropertyValue::Number(OrderedFloat(2.25)),
                source: PortAddress::new(
                    PortOwner::Node(fixture.source_id),
                    NUMBER_RESULT_OUTPUT_PORT,
                ),
                declared_type: PortDataType::Numeric,
            }
        );
    }

    #[test]
    fn disabled_scalar_is_no_output_and_media_is_only_a_type_summary() {
        let mut fixture = fixture();
        fixture
            .project
            .get_node_mut(fixture.source_id)
            .unwrap()
            .enabled = false;
        assert!(matches!(
            evaluate(&fixture, &fixture.value_target, 1.0).unwrap(),
            InputValuePreview::NoOutput {
                declared_type: PortDataType::Numeric,
                source: Some(_),
            }
        ));
        assert!(matches!(
            evaluate(&fixture, &fixture.image_target, 1.0).unwrap(),
            InputValuePreview::TypeSummary {
                data_type: PortDataType::Image,
                sources,
            } if sources.len() == 1
        ));
    }

    #[test]
    fn malformed_single_input_is_an_error_without_mutation() {
        let mut fixture = fixture();
        let before = fixture.project.clone();
        fixture.project.connections.push(ProjectConnection::new(
            PortAddress::new(PortOwner::Composition(fixture.composition_id), TIME_PORT),
            fixture.value_target.clone(),
            1,
        ));
        let malformed = fixture.project.clone();
        assert!(evaluate(&fixture, &fixture.value_target, 1.0).is_err());
        assert_eq!(fixture.project, malformed);
        assert_ne!(fixture.project, before);

        fixture.project.connections.push(ProjectConnection::new(
            fixture.image_source.clone(),
            fixture.image_target.clone(),
            1,
        ));
        let malformed_media = fixture.project.clone();
        assert!(evaluate(&fixture, &fixture.image_target, 1.0).is_err());
        assert_eq!(fixture.project, malformed_media);
    }
}
