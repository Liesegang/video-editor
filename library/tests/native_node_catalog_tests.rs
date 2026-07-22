#![allow(
    clippy::expect_used,
    clippy::panic,
    clippy::unreachable,
    reason = "integration-test parser helpers fail with contract-specific diagnostics"
)]

use std::collections::{BTreeMap, HashMap, HashSet};

use library::model::project::{
    PortDataType, PortDefinition, PortDirection, PortExposure, PortMultiplicity, PortSide,
};
use library::model::{NativeNodeRuntimeStatus, NodeContent, native_node_catalog};
use library::plugin::{OperationDescriptor, PluginManager};

#[path = "native_node_catalog/property_contract.rs"]
mod property_contract;

use property_contract::{NodeListPropertyMetadata, PropertyContract};

const NODE_LIST: &str = include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/../node_list.yml"));

#[derive(Clone, Debug, Default)]
struct NodeListPort {
    name: Option<String>,
    key: Option<String>,
    label: Option<String>,
    data_type: Option<String>,
    side: Option<String>,
    exposure: Option<String>,
    multiplicity: Option<String>,
    variadic: bool,
    property: Option<String>,
    property_metadata: Option<NodeListPropertyMetadata>,
}

impl NodeListPort {
    fn definition(&self, direction: PortDirection, node_label: &str) -> PortDefinition {
        let name = self
            .name
            .as_deref()
            .unwrap_or_else(|| panic!("{node_label}: catalog port is missing name"));
        let data_type = self
            .data_type
            .as_deref()
            .unwrap_or_else(|| panic!("{node_label}.{name}: catalog port is missing type"));
        let (data_type, list_is_variadic) = parse_data_type(data_type, node_label, name);
        let side = match self.side.as_deref() {
            None | Some("Left") if direction == PortDirection::Input => PortSide::Left,
            None | Some("Right") if direction == PortDirection::Output => PortSide::Right,
            Some("Left") => PortSide::Left,
            Some("Right") => PortSide::Right,
            Some(other) => panic!("{node_label}.{name}: unsupported side {other}"),
            None => unreachable!(),
        };
        let exposure = match self.exposure.as_deref() {
            None | Some("Graph") => PortExposure::Graph,
            Some("Internal") => PortExposure::Internal,
            Some("External") => PortExposure::External,
            Some(other) => panic!("{node_label}.{name}: unsupported exposure {other}"),
        };
        let multiplicity = match self.multiplicity.as_deref() {
            None if self.variadic || list_is_variadic => PortMultiplicity::Variadic,
            None | Some("Single") => PortMultiplicity::Single,
            Some("Variadic") => PortMultiplicity::Variadic,
            Some(other) => panic!("{node_label}.{name}: unsupported multiplicity {other}"),
        };
        PortDefinition {
            key: self.key.clone().unwrap_or_else(|| name.to_string()),
            label: self
                .label
                .clone()
                .unwrap_or_else(|| humanize_port_name(name)),
            direction,
            side,
            exposure,
            data_type,
            multiplicity,
        }
    }
}

#[derive(Clone, Debug, Default)]
struct NodeListEntry {
    label: String,
    category: Option<String>,
    catalog_status: Option<String>,
    catalog_id: Option<String>,
    runtime_status: Option<String>,
    operation_category: Option<String>,
    component_id: Option<String>,
    operation: Option<String>,
    inputs: Vec<NodeListPort>,
    outputs: Vec<NodeListPort>,
}

impl NodeListEntry {
    fn port_definitions(&self) -> Vec<PortDefinition> {
        self.inputs
            .iter()
            .map(|port| port.definition(PortDirection::Input, &self.label))
            .chain(
                self.outputs
                    .iter()
                    .map(|port| port.definition(PortDirection::Output, &self.label)),
            )
            .collect()
    }

    fn property_contracts(&self) -> Result<Vec<PropertyContract>, String> {
        let mut contracts = Vec::new();
        for port in &self.inputs {
            let key = port.key.as_deref().unwrap_or_else(|| {
                port.name
                    .as_deref()
                    .expect("catalog input must have a name")
            });
            let is_property_port = key.starts_with("property:");
            let Some(property_key) = port.property.as_deref() else {
                if is_property_port {
                    return Err(format!(
                        "{}.{}: property input is missing its property key",
                        self.label, key
                    ));
                }
                if port.property_metadata.is_some() {
                    return Err(format!(
                        "{}.{}: non-property input cannot declare property_metadata",
                        self.label, key
                    ));
                }
                continue;
            };
            if key != format!("property:{property_key}") {
                return Err(format!(
                    "{}.{}: property {:?} does not match its port key",
                    self.label, key, property_key
                ));
            }
            let label = port
                .label
                .as_deref()
                .ok_or_else(|| format!("{}.{}: property port is missing label", self.label, key))?;
            let metadata = port.property_metadata.as_ref().ok_or_else(|| {
                format!(
                    "{}.{}: property port is missing complete property_metadata",
                    self.label, key
                )
            })?;
            contracts.push(metadata.contract(
                property_key,
                label,
                &format!("{}.{}", self.label, key),
            )?);
        }
        for port in &self.outputs {
            if port.property.is_some() || port.property_metadata.is_some() {
                return Err(format!(
                    "{}: output ports cannot declare operation properties",
                    self.label
                ));
            }
        }
        Ok(contracts)
    }
}

#[derive(Clone, Copy)]
enum PortSection {
    Inputs,
    Outputs,
}

#[derive(Default)]
struct PendingPort {
    definition: NodeListPort,
    anchor: Option<String>,
}

fn parse_node_list() -> BTreeMap<String, NodeListEntry> {
    let mut entries = BTreeMap::new();
    let mut current = None::<NodeListEntry>;
    let mut section = None::<PortSection>;
    let mut pending = None::<PendingPort>;
    let mut anchors = HashMap::<String, NodeListPort>::new();

    for (line_index, line) in NODE_LIST.lines().enumerate() {
        let line_number = line_index + 1;
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        let indentation = line.len() - line.trim_start_matches(' ').len();
        if indentation == 0 {
            finish_port(&mut current, section, &mut pending, &mut anchors);
            finish_entry(&mut entries, &mut current);
            let Some(label) = trimmed.strip_suffix(':') else {
                panic!("node_list.yml:{line_number}: expected a top-level Node label");
            };
            current = Some(NodeListEntry {
                label: label.to_string(),
                ..NodeListEntry::default()
            });
            section = None;
            continue;
        }

        if indentation == 2 {
            finish_port(&mut current, section, &mut pending, &mut anchors);
            let entry = current
                .as_mut()
                .unwrap_or_else(|| panic!("node_list.yml:{line_number}: field before Node label"));
            match trimmed {
                "inputs:" => section = Some(PortSection::Inputs),
                "outputs:" => section = Some(PortSection::Outputs),
                _ => {
                    section = None;
                    if let Some((key, value)) = yaml_field(trimmed) {
                        let target = match key {
                            "category" => &mut entry.category,
                            "catalog_status" => &mut entry.catalog_status,
                            "catalog_id" => &mut entry.catalog_id,
                            "runtime_status" => &mut entry.runtime_status,
                            "operation_category" => &mut entry.operation_category,
                            "component_id" => &mut entry.component_id,
                            "operation" => &mut entry.operation,
                            _ => continue,
                        };
                        assert!(
                            target.replace(value.to_string()).is_none(),
                            "node_list.yml:{line_number}: duplicate {key} on {}",
                            entry.label
                        );
                    }
                }
            }
            continue;
        }

        if indentation == 4 && trimmed.starts_with("- ") {
            finish_port(&mut current, section, &mut pending, &mut anchors);
            let section = section.unwrap_or_else(|| {
                panic!("node_list.yml:{line_number}: port outside inputs/outputs")
            });
            let mut item = trimmed.trim_start_matches("- ").trim();
            let mut next = PendingPort::default();
            if let Some(alias) = item.strip_prefix('*') {
                next.definition = anchors.get(alias).cloned().unwrap_or_else(|| {
                    panic!("node_list.yml:{line_number}: unknown port alias {alias}")
                });
                pending = Some(next);
                let _ = section;
                continue;
            }
            if let Some(anchor_item) = item.strip_prefix('&') {
                let (anchor, remainder) = anchor_item.split_once(' ').unwrap_or((anchor_item, ""));
                next.anchor = Some(anchor.to_string());
                item = remainder.trim();
            }
            if !item.is_empty() {
                apply_port_field(&mut next.definition, item, line_number);
            }
            pending = Some(next);
            continue;
        }

        if indentation == 6
            && let Some(port) = pending.as_mut()
        {
            if let Some(value) = trimmed.strip_prefix("property_metadata:") {
                assert!(
                    port.definition.property_metadata.is_none(),
                    "node_list.yml:{line_number}: duplicate property_metadata"
                );
                let mut metadata = NodeListPropertyMetadata::default();
                let value = value.trim();
                if !value.is_empty() && !value.starts_with('&') {
                    apply_inline_property_metadata(&mut metadata, value, line_number);
                }
                port.definition.property_metadata = Some(metadata);
            } else {
                apply_port_field(&mut port.definition, trimmed, line_number);
            }
            continue;
        }

        if indentation == 8
            && let Some(metadata) = pending
                .as_mut()
                .and_then(|port| port.definition.property_metadata.as_mut())
        {
            apply_property_metadata_field(metadata, trimmed, line_number);
        }
    }

    finish_port(&mut current, section, &mut pending, &mut anchors);
    finish_entry(&mut entries, &mut current);
    entries
}

fn finish_port(
    current: &mut Option<NodeListEntry>,
    section: Option<PortSection>,
    pending: &mut Option<PendingPort>,
    anchors: &mut HashMap<String, NodeListPort>,
) {
    let Some(port) = pending.take() else {
        return;
    };
    if let Some(anchor) = port.anchor {
        assert!(
            anchors
                .insert(anchor.clone(), port.definition.clone())
                .is_none(),
            "duplicate YAML port anchor {anchor}"
        );
    }
    let entry = current.as_mut().expect("pending port must have a Node");
    match section.expect("pending port must have a section") {
        PortSection::Inputs => entry.inputs.push(port.definition),
        PortSection::Outputs => entry.outputs.push(port.definition),
    }
}

fn finish_entry(
    entries: &mut BTreeMap<String, NodeListEntry>,
    current: &mut Option<NodeListEntry>,
) {
    let Some(entry) = current.take() else {
        return;
    };
    let label = entry.label.clone();
    assert!(
        entries.insert(label.clone(), entry).is_none(),
        "duplicate node_list.yml entry {label}"
    );
}

fn yaml_field(value: &str) -> Option<(&str, &str)> {
    let (key, value) = value.split_once(':')?;
    Some((key.trim(), value.trim().trim_matches(['\'', '"'])))
}

fn apply_port_field(port: &mut NodeListPort, value: &str, line_number: usize) {
    let Some((key, value)) = yaml_field(value) else {
        return;
    };
    let target = match key {
        "name" => &mut port.name,
        "key" => &mut port.key,
        "label" => &mut port.label,
        "type" => &mut port.data_type,
        "side" => &mut port.side,
        "exposure" => &mut port.exposure,
        "multiplicity" => &mut port.multiplicity,
        "property" => &mut port.property,
        "variadic" => {
            port.variadic = match value {
                "true" => true,
                "false" => false,
                other => panic!("node_list.yml:{line_number}: invalid variadic value {other}"),
            };
            return;
        }
        _ => return,
    };
    assert!(
        target.replace(value.to_string()).is_none(),
        "node_list.yml:{line_number}: duplicate port field {key}"
    );
}

fn apply_inline_property_metadata(
    metadata: &mut NodeListPropertyMetadata,
    value: &str,
    line_number: usize,
) {
    let body = value
        .strip_prefix('{')
        .and_then(|value| value.strip_suffix('}'))
        .unwrap_or_else(|| {
            panic!("node_list.yml:{line_number}: property_metadata must be an inline YAML map")
        });
    for field in split_top_level_fields(body, line_number) {
        apply_property_metadata_field(metadata, field.trim(), line_number);
    }
}

fn split_top_level_fields(value: &str, line_number: usize) -> Vec<&str> {
    let mut fields = Vec::new();
    let mut start = 0;
    let mut nesting = 0_u32;
    let mut quote = None;
    for (index, character) in value.char_indices() {
        if let Some(open_quote) = quote {
            if character == open_quote {
                quote = None;
            }
            continue;
        }
        match character {
            '\'' | '"' => quote = Some(character),
            '[' | '{' => nesting += 1,
            ']' | '}' => {
                nesting = nesting.checked_sub(1).unwrap_or_else(|| {
                    panic!("node_list.yml:{line_number}: unbalanced inline property_metadata")
                });
            }
            ',' if nesting == 0 => {
                fields.push(
                    value
                        .get(start..index)
                        .expect("char_indices always yields UTF-8 boundaries"),
                );
                start = index + character.len_utf8();
            }
            _ => {}
        }
    }
    assert!(
        nesting == 0 && quote.is_none(),
        "node_list.yml:{line_number}: unbalanced inline property_metadata"
    );
    fields.push(
        value
            .get(start..)
            .expect("field start always follows a UTF-8 boundary"),
    );
    fields
}

fn apply_property_metadata_field(
    metadata: &mut NodeListPropertyMetadata,
    value: &str,
    line_number: usize,
) {
    let Some((key, value)) = yaml_field(value) else {
        panic!("node_list.yml:{line_number}: invalid property_metadata field {value:?}");
    };
    let target = match key {
        "<<" => return,
        "label" => &mut metadata.label,
        "ui_type" => &mut metadata.ui_type,
        "default" => &mut metadata.default,
        "min" => &mut metadata.min,
        "max" => &mut metadata.max,
        "step" => &mut metadata.step,
        "suffix" => &mut metadata.suffix,
        "min_hard_limit" => &mut metadata.min_hard_limit,
        "max_hard_limit" => &mut metadata.max_hard_limit,
        "options" => &mut metadata.options,
        other => panic!("node_list.yml:{line_number}: unsupported property_metadata field {other}"),
    };
    assert!(
        target.replace(value.to_string()).is_none(),
        "node_list.yml:{line_number}: duplicate property_metadata field {key}"
    );
}

fn humanize_port_name(name: &str) -> String {
    name.split('_')
        .map(|word| {
            let mut characters = word.chars();
            characters.next().map_or_else(String::new, |first| {
                first.to_uppercase().chain(characters).collect()
            })
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn parse_data_type(value: &str, node_label: &str, port_name: &str) -> (PortDataType, bool) {
    let data_type = match value {
        "Any" => PortDataType::Any,
        "List<Any>" | "List<Path>" => PortDataType::List,
        "Image" | "List<Image>" => PortDataType::Image,
        "Shape" => PortDataType::Shape,
        "Audio" => PortDataType::Audio,
        "Spectrum" => PortDataType::Spectrum,
        "Scalar/Vector" | "Numeric" => PortDataType::Numeric,
        "Scalar" | "Number" => PortDataType::Number,
        "Integer" => PortDataType::Integer,
        "Boolean" => PortDataType::Boolean,
        "String" => PortDataType::String,
        "Color" => PortDataType::Color,
        "Path" => PortDataType::Path,
        "Vector2" => PortDataType::Vec2,
        "Vector3" => PortDataType::Vec3,
        "Vector4" => PortDataType::Vec4,
        "Enum" => PortDataType::Enum,
        "Asset" => PortDataType::Asset,
        "Gradient" => PortDataType::Gradient,
        "Curve" => PortDataType::Curve,
        "ParticleSystem" => PortDataType::ParticleSystem,
        "Material" => PortDataType::Material,
        "Geometry3D" => PortDataType::Geometry3D,
        "Object3D" => PortDataType::Object3D,
        "List<Object3D>" => PortDataType::Object3DList,
        "Camera3D" => PortDataType::Camera3D,
        "PointSource" => PortDataType::PointSource,
        "Instance3D" => PortDataType::Instance3D,
        "Effector3D" => PortDataType::Effector3D,
        "EffectorStack" => PortDataType::EffectorStack,
        "Field3D" => PortDataType::Field3D,
        "FieldStack" => PortDataType::FieldStack,
        "MotionBehavior" => PortDataType::MotionBehavior,
        other => panic!("{node_label}.{port_name}: unsupported catalog type {other}"),
    };
    (data_type, value == "List<Image>")
}

fn compare_plugin_property_contracts(
    entry: &NodeListEntry,
    descriptor: &OperationDescriptor,
) -> Result<(), String> {
    let yaml = entry.property_contracts()?;
    let compiled = descriptor
        .properties()
        .iter()
        .map(PropertyContract::from_definition)
        .collect::<Vec<_>>();
    if yaml == compiled {
        Ok(())
    } else {
        Err(format!(
            "{}/{}/{}: ordered property definition drift\nnode_list.yml: {yaml:#?}\ndescriptor: {compiled:#?}",
            descriptor.category(),
            descriptor.component_id(),
            descriptor.operation()
        ))
    }
}

#[test]
fn node_list_and_native_catalog_match_bidirectionally() {
    let entries = parse_node_list();
    assert!(!entries.is_empty(), "node_list.yml must not be empty");

    let mut yaml_native_by_id = BTreeMap::new();
    for entry in entries.values() {
        let status = entry.catalog_status.as_deref().unwrap_or_else(|| {
            panic!(
                "{} is missing mandatory catalog_status (native, plugin-managed, or design-needed)",
                entry.label
            )
        });
        assert!(
            matches!(status, "native" | "plugin-managed" | "design-needed"),
            "{} has unsupported catalog_status {status}",
            entry.label
        );
        if status == "design-needed" {
            assert!(
                entry.catalog_id.is_none()
                    && entry.runtime_status.is_none()
                    && entry.operation_category.is_none()
                    && entry.component_id.is_none()
                    && entry.operation.is_none(),
                "{} is design-only but claims an implemented catalog identity",
                entry.label
            );
            continue;
        }
        if status == "plugin-managed" {
            assert!(
                entry.catalog_id.is_none() && entry.runtime_status.is_none(),
                "{} is plugin-managed but claims native catalog metadata",
                entry.label
            );
            for (field, value) in [
                ("operation_category", &entry.operation_category),
                ("component_id", &entry.component_id),
                ("operation", &entry.operation),
            ] {
                assert!(
                    value.as_deref().is_some_and(|value| !value.is_empty()),
                    "{} is plugin-managed but missing {field}",
                    entry.label
                );
            }
            continue;
        }
        assert!(
            entry.operation_category.is_none()
                && entry.component_id.is_none()
                && entry.operation.is_none(),
            "{} is native but also claims a plugin operation identity",
            entry.label
        );
        let catalog_id = entry
            .catalog_id
            .as_deref()
            .unwrap_or_else(|| panic!("{} is native but missing catalog_id", entry.label));
        assert!(
            entry.runtime_status.is_some(),
            "{} is native but missing runtime_status",
            entry.label
        );
        assert!(
            yaml_native_by_id.insert(catalog_id, entry).is_none(),
            "duplicate native catalog_id {catalog_id} in node_list.yml"
        );
    }

    let mut compiled_by_id = BTreeMap::new();
    let mut qa_ids = HashSet::new();
    for descriptor in native_node_catalog() {
        assert!(
            compiled_by_id
                .insert(descriptor.catalog_id(), descriptor)
                .is_none(),
            "duplicate compiled native catalog_id {}",
            descriptor.catalog_id()
        );
        assert!(
            qa_ids.insert(descriptor.qa_id()),
            "duplicate native QA id {}",
            descriptor.qa_id()
        );
        let mut port_keys = HashSet::new();
        for port in descriptor.ports() {
            assert!(
                port_keys.insert((port.direction, port.key.as_str())),
                "{}.{} duplicates a {:?} port key",
                descriptor.catalog_id(),
                port.key,
                port.direction
            );
        }
    }

    assert_eq!(
        yaml_native_by_id.keys().copied().collect::<Vec<_>>(),
        compiled_by_id.keys().copied().collect::<Vec<_>>(),
        "node_list.yml and the compiled native catalog must contain exactly the same stable IDs"
    );
    for (catalog_id, descriptor) in compiled_by_id {
        let entry = yaml_native_by_id[catalog_id];
        assert_eq!(entry.label, descriptor.label(), "{catalog_id}: label drift");
        assert_eq!(
            entry.category.as_deref(),
            Some(descriptor.category()),
            "{catalog_id}: category drift"
        );
        assert_eq!(
            entry.runtime_status.as_deref(),
            Some(descriptor.runtime_status().key()),
            "{catalog_id}: runtime status drift"
        );
        assert_eq!(
            entry.port_definitions(),
            descriptor.ports(),
            "{catalog_id}: ordered typed port contract drift"
        );
    }
}

#[test]
fn node_list_and_bundled_plugin_operations_match_bidirectionally() {
    let entries = parse_node_list();
    let mut yaml_by_identity = BTreeMap::new();
    for entry in entries
        .values()
        .filter(|entry| entry.catalog_status.as_deref() == Some("plugin-managed"))
    {
        let identity = (
            entry
                .operation_category
                .as_deref()
                .expect("plugin-managed operation category"),
            entry
                .component_id
                .as_deref()
                .expect("plugin-managed component id"),
            entry
                .operation
                .as_deref()
                .expect("plugin-managed operation"),
        );
        assert!(
            yaml_by_identity.insert(identity, entry).is_none(),
            "duplicate plugin-managed identity {identity:?} in node_list.yml"
        );
    }

    let manager = PluginManager::default();
    let descriptors = manager
        .bundled_operation_descriptors()
        .expect("all bundled operation descriptors must resolve");
    let mut bundled_by_identity = BTreeMap::new();
    for descriptor in &descriptors {
        let identity = (
            descriptor.category(),
            descriptor.component_id(),
            descriptor.operation(),
        );
        assert!(
            bundled_by_identity.insert(identity, descriptor).is_none(),
            "duplicate bundled operation identity {identity:?}"
        );
    }

    assert_eq!(
        yaml_by_identity.keys().copied().collect::<Vec<_>>(),
        bundled_by_identity.keys().copied().collect::<Vec<_>>(),
        "node_list.yml plugin-managed entries and bundled descriptors must contain exactly the same operation identities; external plugins are intentionally outside this gate"
    );

    for (identity, descriptor) in bundled_by_identity {
        let entry = yaml_by_identity[&identity];
        assert_eq!(entry.label, descriptor.label(), "{identity:?}: label drift");
        assert_eq!(
            entry.port_definitions(),
            descriptor.declared_ports(),
            "{identity:?}: ordered typed port contract drift"
        );
        compare_plugin_property_contracts(entry, descriptor)
            .unwrap_or_else(|error| panic!("{error}"));

        let resolved = manager
            .operation_descriptor(identity.0, identity.1, identity.2)
            .unwrap_or_else(|error| panic!("{identity:?}: descriptor is unreachable: {error}"));
        assert_eq!(resolved.label(), descriptor.label());
        assert_eq!(resolved.declared_ports(), descriptor.declared_ports());

        let node = manager
            .create_operation_node(identity.0, identity.1, identity.2)
            .unwrap_or_else(|error| panic!("{identity:?}: factory is unreachable: {error}"));
        let NodeContent::PluginOperation(content) = node.content() else {
            panic!("{identity:?}: factory did not create a plugin operation Node");
        };
        assert_eq!(content.category, identity.0);
        assert_eq!(content.component_id, identity.1);
        assert_eq!(content.operation, identity.2);
        assert_eq!(content.declared_ports, descriptor.declared_ports());
        assert_eq!(
            node.properties().iter().count(),
            descriptor.properties().len(),
            "{identity:?}: factory property count drift"
        );
        for definition in descriptor.properties() {
            let property = node.properties().get(definition.name()).unwrap_or_else(|| {
                panic!(
                    "{identity:?}: factory omitted property {}",
                    definition.name()
                )
            });
            assert_eq!(
                property.get_static_value(),
                Some(definition.default_value()),
                "{identity:?}: factory default drift for {}",
                definition.name()
            );
        }
    }
}

#[test]
fn plugin_property_contract_gate_rejects_missing_default_and_range_drift() {
    let entries = parse_node_list();
    let mut entry = entries
        .get("Image Opacity")
        .expect("Image Opacity catalog entry")
        .clone();
    let manager = PluginManager::default();
    let descriptors = manager
        .bundled_operation_descriptors()
        .expect("bundled descriptors");
    let descriptor = descriptors
        .iter()
        .find(|descriptor| descriptor.label() == "Image Opacity")
        .expect("Image Opacity descriptor");

    let property_index = entry
        .inputs
        .iter()
        .position(|port| port.property.as_deref() == Some("opacity"))
        .expect("Image Opacity opacity metadata");
    entry.inputs[property_index]
        .property_metadata
        .as_mut()
        .expect("Image Opacity opacity metadata")
        .default = Some("0.5".to_string());
    assert!(
        compare_plugin_property_contracts(&entry, descriptor)
            .expect_err("a wrong YAML default must fail the catalog gate")
            .contains("property definition drift")
    );

    let metadata = entry.inputs[property_index]
        .property_metadata
        .as_mut()
        .expect("Image Opacity opacity metadata");
    metadata.default = Some("1.0".to_string());
    metadata.max = Some("2.0".to_string());
    assert!(
        compare_plugin_property_contracts(&entry, descriptor)
            .expect_err("a wrong YAML range must fail the catalog gate")
            .contains("property definition drift")
    );

    entry.inputs[property_index].property_metadata = None;
    assert!(
        compare_plugin_property_contracts(&entry, descriptor)
            .expect_err("missing YAML metadata must fail the catalog gate")
            .contains("missing complete property_metadata")
    );
}

#[test]
fn every_design_needed_runtime_has_an_explicit_no_output_diagnostic() {
    for descriptor in native_node_catalog() {
        match descriptor.runtime_status() {
            NativeNodeRuntimeStatus::Implemented => {
                assert!(descriptor.runtime_diagnostic().is_none());
            }
            NativeNodeRuntimeStatus::DesignNeeded => {
                let diagnostic = descriptor
                    .runtime_diagnostic()
                    .expect("design-needed Nodes must explain their runtime boundary");
                assert!(diagnostic.contains("design-needed"));
                assert!(diagnostic.contains("No Output"));
            }
        }
    }
}
