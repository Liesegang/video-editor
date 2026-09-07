//! Conversion of an authored Appearance stack into its canonical Image graph.

use super::*;

impl GraphBuilder<'_> {
    pub(super) fn add_appearance_operations(
        &mut self,
        operations: &[AppearanceOperation],
    ) -> Result<(), LibraryError> {
        let shape_source = self.current.take().ok_or_else(|| {
            LibraryError::Validation("Appearance stack has no Shape source".to_string())
        })?;
        if operations.is_empty() {
            return Err(LibraryError::Validation(
                "A Text or Shape without an Appearance cannot be converted to an Image Node Clip"
                    .to_string(),
            ));
        }

        let first_column = self.next_column;
        let mut operation_ids = Vec::with_capacity(operations.len());
        for authored in operations {
            let mut node = self
                .plugins
                .create_style_operation_node(&authored.operation.component_id)?;
            let NodeContent::PluginOperation(content) = node.content() else {
                return Err(LibraryError::Validation(format!(
                    "Appearance operation {} did not create a Plugin operation Node",
                    authored.id
                )));
            };
            if authored.operation.category != STYLE_CATEGORY
                || authored.operation.operation != STYLE_APPLY_OPERATION
                || content.category != authored.operation.category
                || content.component_id != authored.operation.component_id
                || content.operation != authored.operation.operation
                || content.declared_ports != authored.declared_ports
                || crate::model::authoring::appearance_input_kind(&authored.declared_ports)
                    .is_none()
            {
                return Err(LibraryError::Validation(format!(
                    "Appearance operation {} no longer matches its Image contract",
                    authored.id
                )));
            }
            node.id = authored.id;
            if self
                .definition
                .graph
                .nodes
                .insert(node.id, node.clone())
                .is_some()
            {
                return Err(LibraryError::Validation(format!(
                    "Appearance Node {} has a duplicate stable identity",
                    node.id
                )));
            }
            let descriptor = self.plugins.operation_descriptor(
                STYLE_CATEGORY,
                &authored.operation.component_id,
                STYLE_APPLY_OPERATION,
            )?;
            for definition in descriptor.properties() {
                let property = authored.properties.get(definition.name()).ok_or_else(|| {
                    LibraryError::Validation(format!(
                        "Appearance operation {} is missing Property '{}'",
                        authored.id,
                        definition.name()
                    ))
                })?;
                let default = node
                    .properties()
                    .get(definition.name())
                    .and_then(Property::value)
                    .cloned()
                    .ok_or_else(|| {
                        LibraryError::Validation(format!(
                            "Appearance {} has no default for Property '{}'",
                            authored.operation.component_id,
                            definition.name()
                        ))
                    })?;
                let parameter_id = self.add_parameter(
                    node.id,
                    format!("{PROPERTY_PORT_PREFIX}{}", definition.name()),
                    &format!("{} {}", descriptor.label(), definition.label()),
                    default,
                )?;
                self.move_property_value(parameter_id, property)?;
            }
            operation_ids.push(node.id);
        }

        let (image, next_column) = super::super::appearance::node_clip::build_appearance_chain(
            &mut self.definition,
            &shape_source,
            &operation_ids,
            first_column,
        )
        .map_err(LibraryError::Validation)?;
        self.next_column = next_column;
        self.current = Some(image);
        Ok(())
    }
}
