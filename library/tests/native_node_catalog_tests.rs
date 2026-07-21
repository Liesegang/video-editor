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
use library::model::{NativeNodeRuntimeStatus, native_node_catalog};

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
            apply_port_field(&mut port.definition, trimmed, line_number);
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
        "List<Any>" => PortDataType::List,
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

#[test]
fn node_list_and_native_catalog_match_bidirectionally() {
    let entries = parse_node_list();
    assert!(!entries.is_empty(), "node_list.yml must not be empty");

    let mut yaml_native_by_id = BTreeMap::new();
    for entry in entries.values() {
        let status = entry.catalog_status.as_deref().unwrap_or_else(|| {
            panic!(
                "{} is missing mandatory catalog_status (native or design-needed)",
                entry.label
            )
        });
        assert!(
            matches!(status, "native" | "design-needed"),
            "{} has unsupported catalog_status {status}",
            entry.label
        );
        if status == "design-needed" {
            assert!(
                entry.catalog_id.is_none() && entry.runtime_status.is_none(),
                "{} is design-only but claims native catalog metadata",
                entry.label
            );
            continue;
        }
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
