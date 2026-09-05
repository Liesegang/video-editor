use super::*;

impl GraphBuilder<'_> {
    pub(super) fn add_ensemble_operation(
        &mut self,
        operation: &TextEnsembleOperation,
    ) -> Result<(), LibraryError> {
        let mut node = self.plugins.create_text_ensemble_operation_node(
            &operation.operation.category,
            &operation.operation.component_id,
        )?;
        let NodeContent::PluginOperation(content) = node.content() else {
            return Err(LibraryError::Validation(format!(
                "Text Ensemble operation {} did not create a Plugin Node",
                operation.id
            )));
        };
        if content.declared_ports != operation.declared_ports {
            return Err(LibraryError::Validation(format!(
                "Text Ensemble operation {} no longer matches its plugin port contract",
                operation.id
            )));
        }
        node.id = operation.id;
        self.position_node(&mut node);
        self.connect_current_to(&node, SHAPE_INPUT_PORT, SHAPE_OUTPUT_PORT)?;

        let descriptor = self.plugins.text_ensemble_operation_descriptor(
            &operation.operation.category,
            &operation.operation.component_id,
        )?;
        for definition in descriptor.properties() {
            let key = definition.name();
            let property = operation.properties.get(key).ok_or_else(|| {
                LibraryError::Validation(format!(
                    "Text Ensemble operation {} is missing property '{key}'",
                    operation.id
                ))
            })?;
            self.move_property_to_node_parameter(
                &node,
                key,
                &format!("{PROPERTY_PORT_PREFIX}{key}"),
                definition.label(),
                property,
            )?;
        }
        Ok(())
    }
}
