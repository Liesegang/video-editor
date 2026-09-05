//! Conversion of an authored Appearance stack into a bounded style branch.

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

        let style_column = self.next_column;
        let mut outputs = Vec::with_capacity(operations.len());
        for (index, authored) in operations.iter().enumerate() {
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
                || !appearance_direct_contract_is_compatible(&authored.declared_ports)
            {
                return Err(LibraryError::Validation(format!(
                    "Appearance operation {} no longer matches its Style contract",
                    authored.id
                )));
            }
            node.id = authored.id;
            node.ui_position = [style_column, 40.0 + index as f32 * 190.0];
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
                            "Style {} has no default for Property '{}'",
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
            outputs.push(ModulePortAddress {
                node_id: node.id,
                port: STYLE_OUTPUT_PORT.to_string(),
            });
        }
        self.next_column += 300.0;

        let mut stack = Node::new_catalog_node(APPEARANCE_STACK_CATALOG_ID)
            .map_err(LibraryError::Validation)?;
        self.position_node(&mut stack);
        let stack_id = stack.id;
        if self
            .definition
            .graph
            .nodes
            .insert(stack_id, stack)
            .is_some()
        {
            return Err(LibraryError::Validation(format!(
                "Appearance Stack {stack_id} collides with an existing Node"
            )));
        }
        self.definition.graph.connections.push(ModuleConnection {
            id: ModuleConnectionId::new(),
            from: shape_source,
            to: ModulePortAddress {
                node_id: stack_id,
                port: SHAPE_INPUT_PORT.to_string(),
            },
            order: 0,
            blend_mode: BlendMode::Normal,
        });
        for (order, from) in outputs.into_iter().enumerate() {
            self.definition.graph.connections.push(ModuleConnection {
                id: ModuleConnectionId::new(),
                from,
                to: ModulePortAddress {
                    node_id: stack_id,
                    port: APPEARANCE_STYLES_PORT.to_string(),
                },
                order: order as i64,
                blend_mode: BlendMode::Normal,
            });
        }
        self.current = Some(ModulePortAddress {
            node_id: stack_id,
            port: IMAGE_OUTPUT_PORT.to_string(),
        });
        Ok(())
    }
}
