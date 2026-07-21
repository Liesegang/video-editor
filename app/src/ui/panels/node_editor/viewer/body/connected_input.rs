use eframe::egui::{self, Color32};
use library::core::framing::{FrameEvaluator, InputValuePreview};
use library::model::frame::color::Color;
use library::model::project::{PortAddress, PortDataType, PortOwner};
use library::model::property::PropertyValue;
use library::model::Project;
use library::plugin::PluginManager;
use library::LibraryError;
use serde_json::{json, Value};
use uuid::Uuid;

use crate::ui::panels::node_editor::{port_owner_composition, qa_container_key, PORT_ROW_HEIGHT};

pub(super) struct RenderedConnectedInput {
    pub(super) response: egui::Response,
    pub(super) control_kind: &'static str,
    pub(super) value: Value,
    pub(super) metadata: Value,
}

pub(super) struct ResolvedControl<T> {
    pub(super) response: egui::Response,
    pub(super) control_kind: &'static str,
    pub(super) enabled: bool,
    pub(super) value: Value,
    pub(super) components: Vec<T>,
    pub(super) metadata: Option<Value>,
}

pub(super) fn resolve_control<T>(
    authored: Option<(egui::Response, &'static str, Value, Vec<T>)>,
    connected: Option<RenderedConnectedInput>,
    fallback_response: egui::Response,
    fallback_value: Value,
) -> ResolvedControl<T> {
    if let Some((response, control_kind, value, components)) = authored {
        let enabled = response.enabled();
        return ResolvedControl {
            response,
            control_kind,
            enabled,
            value,
            components,
            metadata: None,
        };
    }
    connected.map_or(
        ResolvedControl {
            response: fallback_response,
            control_kind: "missing",
            enabled: false,
            value: fallback_value,
            components: Vec::new(),
            metadata: None,
        },
        |rendered| ResolvedControl {
            response: rendered.response,
            control_kind: rendered.control_kind,
            enabled: false,
            value: rendered.value,
            components: Vec::new(),
            metadata: Some(rendered.metadata),
        },
    )
}

struct Presentation {
    label: String,
    tooltip: String,
    color: Color32,
    control_kind: &'static str,
    value: Value,
    metadata: Value,
}

pub(super) fn render(
    ui: &mut egui::Ui,
    project: &Project,
    plugin_manager: Option<&PluginManager>,
    node_id: Uuid,
    port: &str,
    timeline_time: f64,
) -> RenderedConnectedInput {
    let target = PortAddress::new(PortOwner::Node(node_id), port);
    let result = evaluate(project, plugin_manager, &target, timeline_time);
    let presentation = presentation(project, &target, timeline_time, result);
    let response = ui
        .add_sized(
            [104.0, PORT_ROW_HEIGHT - 2.0],
            egui::Label::new(
                egui::RichText::new(&presentation.label)
                    .small()
                    .monospace()
                    .color(presentation.color),
            )
            .selectable(false)
            .sense(egui::Sense::hover())
            .truncate(),
        )
        .on_hover_text(&presentation.tooltip);
    RenderedConnectedInput {
        response,
        control_kind: presentation.control_kind,
        value: presentation.value,
        metadata: presentation.metadata,
    }
}

fn evaluate(
    project: &Project,
    plugin_manager: Option<&PluginManager>,
    target: &PortAddress,
    timeline_time: f64,
) -> Result<InputValuePreview, LibraryError> {
    let plugin_manager = plugin_manager
        .ok_or_else(|| LibraryError::Runtime("Node input evaluator is unavailable".to_string()))?;
    let composition_id = port_owner_composition(project, target.owner).ok_or_else(|| {
        LibraryError::Project(format!("Input owner {:?} has no Composition", target.owner))
    })?;
    let composition = project
        .get_composition(composition_id)
        .ok_or_else(|| LibraryError::Project(format!("Composition {composition_id} is missing")))?;
    FrameEvaluator::new(
        project,
        composition,
        plugin_manager.get_property_evaluators(),
        plugin_manager,
    )
    .evaluate_input_preview(target, timeline_time)
}

fn presentation(
    project: &Project,
    target: &PortAddress,
    timeline_time: f64,
    result: Result<InputValuePreview, LibraryError>,
) -> Presentation {
    match result {
        Ok(InputValuePreview::Value {
            value,
            source,
            declared_type,
        }) => {
            let label = compact_value(&value);
            let value_json = Value::from(&value);
            Presentation {
                tooltip: format!(
                    "Current input at {timeline_time:.3} s: {}\nSource: {}.{}",
                    full_value(&value),
                    qa_container_key(source.owner),
                    source.port,
                ),
                label,
                color: Color32::from_rgb(205, 220, 235),
                control_kind: "connected_value",
                value: value_json.clone(),
                metadata: json!({
                    "input_status": "value",
                    "read_only": true,
                    "timeline_time": timeline_time,
                    "declared_data_type": data_type_key(declared_type),
                    "resolved_data_type": property_value_type(&value),
                    "resolved_value": value_json,
                    "sources": [source_metadata(&source)],
                    "evaluation": "resolved",
                }),
            }
        }
        Ok(InputValuePreview::NoOutput {
            declared_type,
            source,
        }) => {
            let sources = source.iter().map(source_metadata).collect::<Vec<_>>();
            Presentation {
                label: "No Output".to_string(),
                tooltip: format!(
                    "The connected graph produced No Output at {timeline_time:.3} s. This is distinct from zero, false, and an empty value."
                ),
                color: Color32::from_rgb(190, 155, 95),
                control_kind: "connected_no_output",
                value: Value::Null,
                metadata: json!({
                    "input_status": "no_output",
                    "read_only": true,
                    "timeline_time": timeline_time,
                    "declared_data_type": data_type_key(declared_type),
                    "resolved_data_type": Value::Null,
                    "resolved_value": Value::Null,
                    "sources": sources,
                    "evaluation": "no_output",
                }),
            }
        }
        Ok(InputValuePreview::TypeSummary { data_type, sources }) => {
            let count = sources.len();
            let type_label = data_type_label(data_type);
            Presentation {
                label: if count > 1 {
                    format!("{type_label} ×{count}")
                } else {
                    type_label.to_string()
                },
                tooltip: format!(
                    "{type_label} input from {count} connection{}. Runtime payload evaluation is skipped in the Node Editor.",
                    if count == 1 { "" } else { "s" },
                ),
                color: Color32::from_rgb(145, 180, 210),
                control_kind: "connected_type_summary",
                value: Value::Null,
                metadata: json!({
                    "input_status": "type_summary",
                    "read_only": true,
                    "timeline_time": timeline_time,
                    "declared_data_type": project
                        .port_definition(target, library::model::project::PortDirection::Input)
                        .map(|definition| data_type_key(definition.data_type)),
                    "resolved_data_type": data_type_key(data_type),
                    "resolved_value": Value::Null,
                    "sources": sources.iter().map(source_metadata).collect::<Vec<_>>(),
                    "connection_count": count,
                    "evaluation": "skipped_expensive",
                }),
            }
        }
        Err(error) => {
            let message = error.to_string();
            let sources = project
                .connections
                .iter()
                .filter(|connection| connection.to == *target)
                .map(|connection| source_metadata(&connection.from))
                .collect::<Vec<_>>();
            Presentation {
                label: "Error".to_string(),
                tooltip: format!("Cannot evaluate connected input: {message}"),
                color: Color32::from_rgb(235, 95, 95),
                control_kind: "connected_error",
                value: Value::Null,
                metadata: json!({
                    "input_status": "error",
                    "read_only": true,
                    "timeline_time": timeline_time,
                    "resolved_value": Value::Null,
                    "sources": sources,
                    "evaluation": "error",
                    "error": message,
                }),
            }
        }
    }
}

fn source_metadata(source: &PortAddress) -> Value {
    json!({
        "owner": qa_container_key(source.owner),
        "port": source.port,
    })
}

fn compact_value(value: &PropertyValue) -> String {
    match value {
        PropertyValue::Number(value) => compact_number(value.into_inner()),
        PropertyValue::Integer(value) => value.to_string(),
        PropertyValue::String(value) => format!("\"{value}\""),
        PropertyValue::Boolean(value) => value.to_string(),
        PropertyValue::Vec2(value) => format!(
            "({}, {})",
            compact_number(value.x.into_inner()),
            compact_number(value.y.into_inner())
        ),
        PropertyValue::Vec3(value) => format!(
            "({}, {}, {})",
            compact_number(value.x.into_inner()),
            compact_number(value.y.into_inner()),
            compact_number(value.z.into_inner())
        ),
        PropertyValue::Vec4(value) => format!(
            "({}, {}, {}, {})",
            compact_number(value.x.into_inner()),
            compact_number(value.y.into_inner()),
            compact_number(value.z.into_inner()),
            compact_number(value.w.into_inner())
        ),
        PropertyValue::ColorValue(color) => {
            let [r, g, b, a] = color.rgba();
            format!(
                "({}, {}, {}, {}) @ {}",
                compact_number(r),
                compact_number(g),
                compact_number(b),
                compact_number(a),
                color.color_space()
            )
        }
        PropertyValue::Color(color) => color_hex(color),
        PropertyValue::Path(path) => format!("Path[{}]", path.contours().len()),
        PropertyValue::Array(values) => format!("Array[{}]", values.len()),
        PropertyValue::Map(values) => format!("Map[{}]", values.len()),
        PropertyValue::OpaqueJson(value) => match value {
            Value::Null => "JSON null".to_string(),
            Value::Bool(_) => "JSON boolean".to_string(),
            Value::Number(_) => "JSON number".to_string(),
            Value::String(_) => "JSON string".to_string(),
            Value::Array(values) => format!("JSON Array[{}]", values.len()),
            Value::Object(values) => format!("JSON Object[{}]", values.len()),
        },
    }
}

fn full_value(value: &PropertyValue) -> String {
    match value {
        PropertyValue::String(value) => value.clone(),
        _ => compact_value(value),
    }
}

fn compact_number(value: f64) -> String {
    if !value.is_finite() {
        return value.to_string();
    }
    if value == 0.0 {
        return "0".to_string();
    }
    let magnitude = value.abs();
    if !(0.0001..1_000_000.0).contains(&magnitude) {
        return format!("{value:.3e}");
    }
    let mut formatted = format!("{value:.4}");
    while formatted.ends_with('0') {
        formatted.pop();
    }
    if formatted.ends_with('.') {
        formatted.pop();
    }
    formatted
}

fn color_hex(color: &Color) -> String {
    format!(
        "#{:02X}{:02X}{:02X}{:02X}",
        color.r, color.g, color.b, color.a
    )
}

fn property_value_type(value: &PropertyValue) -> &'static str {
    match value {
        PropertyValue::Number(_) => "number",
        PropertyValue::Integer(_) => "integer",
        PropertyValue::String(_) => "string",
        PropertyValue::Boolean(_) => "boolean",
        PropertyValue::Vec2(_) => "vec2",
        PropertyValue::Vec3(_) => "vec3",
        PropertyValue::Vec4(_) => "vec4",
        PropertyValue::ColorValue(_) => "color",
        PropertyValue::Color(_) => "color",
        PropertyValue::Path(_) => "path",
        PropertyValue::Array(_) => "array",
        PropertyValue::Map(_) => "map",
        PropertyValue::OpaqueJson(_) => "opaque_json",
    }
}

fn data_type_key(data_type: PortDataType) -> &'static str {
    match data_type {
        PortDataType::Any => "any",
        PortDataType::List => "list",
        PortDataType::Image => "image",
        PortDataType::Shape => "shape",
        PortDataType::Audio => "audio",
        PortDataType::Spectrum => "spectrum",
        PortDataType::Numeric => "numeric",
        PortDataType::Number => "number",
        PortDataType::Integer => "integer",
        PortDataType::Boolean => "boolean",
        PortDataType::String => "string",
        PortDataType::Color => "color",
        PortDataType::Path => "path",
        PortDataType::Vec2 => "vec2",
        PortDataType::Vec3 => "vec3",
        PortDataType::Vec4 => "vec4",
        PortDataType::Enum => "enum",
        PortDataType::Asset => "asset",
        PortDataType::Gradient => "gradient",
        PortDataType::Curve => "curve",
        PortDataType::ParticleSystem => "particle_system",
        PortDataType::Material => "material",
        PortDataType::Geometry3D => "geometry_3d",
        PortDataType::Object3D => "object_3d",
        PortDataType::Object3DList => "object_3d_list",
        PortDataType::Camera3D => "camera_3d",
        PortDataType::PointSource => "point_source",
        PortDataType::Instance3D => "instance_3d",
        PortDataType::Effector3D => "effector_3d",
        PortDataType::EffectorStack => "effector_stack",
        PortDataType::Field3D => "field_3d",
        PortDataType::FieldStack => "field_stack",
        PortDataType::MotionBehavior => "motion_behavior",
    }
}

fn data_type_label(data_type: PortDataType) -> &'static str {
    match data_type {
        PortDataType::Any => "Complex",
        PortDataType::List => "List",
        PortDataType::Image => "Image",
        PortDataType::Shape => "Shape",
        PortDataType::Audio => "Audio",
        PortDataType::Spectrum => "Spectrum",
        PortDataType::Path => "Path",
        PortDataType::Numeric => "Numeric",
        PortDataType::Number => "Number",
        PortDataType::Integer => "Integer",
        PortDataType::Boolean => "Boolean",
        PortDataType::String => "String",
        PortDataType::Color => "Color",
        PortDataType::Vec2 => "Vec2",
        PortDataType::Vec3 => "Vec3",
        PortDataType::Vec4 => "Vec4",
        PortDataType::Enum => "Enum",
        PortDataType::Asset => "Asset",
        PortDataType::Gradient => "Gradient",
        PortDataType::Curve => "Curve",
        PortDataType::ParticleSystem => "Particle System",
        PortDataType::Material => "Material",
        PortDataType::Geometry3D => "Geometry 3D",
        PortDataType::Object3D => "Object 3D",
        PortDataType::Object3DList => "Object 3D List",
        PortDataType::Camera3D => "Camera 3D",
        PortDataType::PointSource => "Point Source",
        PortDataType::Instance3D => "Instance 3D",
        PortDataType::Effector3D => "Effector 3D",
        PortDataType::EffectorStack => "Effector Stack",
        PortDataType::Field3D => "Field 3D",
        PortDataType::FieldStack => "Field Stack",
        PortDataType::MotionBehavior => "Motion Behavior",
    }
}

#[cfg(test)]
mod tests {
    use ordered_float::OrderedFloat;

    use super::*;
    use library::model::path::{FillRule, PathValue};
    use library::model::property::{ColorSpaceRef, ColorValue, Vec2, Vec3, Vec4};

    #[test]
    fn compact_value_labels_cover_every_editable_scalar_family() {
        assert_eq!(
            compact_value(&PropertyValue::Number(OrderedFloat(1.25))),
            "1.25"
        );
        assert_eq!(compact_value(&PropertyValue::Integer(42)), "42");
        assert_eq!(compact_value(&PropertyValue::Boolean(true)), "true");
        assert_eq!(
            compact_value(&PropertyValue::String("hello".into())),
            "\"hello\""
        );
        assert_eq!(
            compact_value(&PropertyValue::Vec2(Vec2 {
                x: OrderedFloat(1.0),
                y: OrderedFloat(2.0),
            })),
            "(1, 2)"
        );
        assert_eq!(
            compact_value(&PropertyValue::Vec3(Vec3 {
                x: OrderedFloat(1.0),
                y: OrderedFloat(2.0),
                z: OrderedFloat(3.0),
            })),
            "(1, 2, 3)"
        );
        assert_eq!(
            compact_value(&PropertyValue::Vec4(Vec4 {
                x: OrderedFloat(1.0),
                y: OrderedFloat(2.0),
                z: OrderedFloat(3.0),
                w: OrderedFloat(4.0),
            })),
            "(1, 2, 3, 4)"
        );
        assert_eq!(
            compact_value(&PropertyValue::Color(Color {
                r: 0x12,
                g: 0x34,
                b: 0x56,
                a: 0x78,
            })),
            "#12345678"
        );
        assert_eq!(
            compact_value(&PropertyValue::ColorValue(
                ColorValue::new(ColorSpaceRef::linear_srgb(), [1.25, -0.5, 0.0, 1.0]).unwrap()
            )),
            "(1.25, -0.5, 0, 1) @ linear-srgb"
        );
        assert_eq!(
            compact_value(&PropertyValue::Path(PathValue::empty(FillRule::EvenOdd))),
            "Path[0]"
        );
        assert_eq!(
            compact_value(&PropertyValue::OpaqueJson(json!([null, 7]))),
            "JSON Array[2]"
        );
    }

    #[test]
    fn current_complex_port_families_have_stable_metadata_labels() {
        assert_eq!(data_type_key(PortDataType::List), "list");
        assert_eq!(
            data_type_label(PortDataType::Object3DList),
            "Object 3D List"
        );
        assert_eq!(
            data_type_key(PortDataType::MotionBehavior),
            "motion_behavior"
        );
    }

    #[test]
    fn status_presentations_publish_distinct_qa_contracts() {
        let project = Project::new("presentation");
        let target = PortAddress::new(PortOwner::Node(Uuid::new_v4()), "value");
        let source = PortAddress::new(PortOwner::Node(Uuid::new_v4()), "result");
        let no_output = presentation(
            &project,
            &target,
            1.5,
            Ok(InputValuePreview::NoOutput {
                declared_type: PortDataType::Number,
                source: Some(source.clone()),
            }),
        );
        assert_eq!(no_output.label, "No Output");
        assert_eq!(no_output.metadata["input_status"], "no_output");
        assert_eq!(no_output.metadata["resolved_value"], Value::Null);

        let summary = presentation(
            &project,
            &target,
            1.5,
            Ok(InputValuePreview::TypeSummary {
                data_type: PortDataType::Audio,
                sources: vec![source],
            }),
        );
        assert_eq!(summary.label, "Audio");
        assert_eq!(summary.metadata["input_status"], "type_summary");
        assert_eq!(summary.metadata["evaluation"], "skipped_expensive");

        let error = presentation(
            &project,
            &target,
            1.5,
            Err(LibraryError::Validation("broken wire".to_string())),
        );
        assert_eq!(error.label, "Error");
        assert_eq!(error.metadata["input_status"], "error");
        assert!(error.metadata["error"]
            .as_str()
            .unwrap()
            .contains("broken wire"));
    }

    #[test]
    fn read_only_value_renders_as_a_real_hoverable_non_selectable_label() {
        let context = egui::Context::default();
        let mut rect = egui::Rect::NOTHING;
        let presentation = Presentation {
            label: "(1, 2, 3, 4)".to_string(),
            tooltip: "current value".to_string(),
            color: Color32::WHITE,
            control_kind: "connected_value",
            value: json!({"x": 1.0, "y": 2.0, "z": 3.0, "w": 4.0}),
            metadata: json!({"input_status": "value", "read_only": true}),
        };
        drop(context.run(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(240.0, 80.0),
                )),
                ..Default::default()
            },
            |context| {
                egui::CentralPanel::default().show(context, |ui| {
                    let response = ui.add_sized(
                        [104.0, PORT_ROW_HEIGHT - 2.0],
                        egui::Label::new(&presentation.label)
                            .selectable(false)
                            .sense(egui::Sense::hover()),
                    );
                    rect = response.rect;
                    assert!(!response.dragged());
                });
            },
        ));
        assert!(rect.is_positive());
        assert_eq!(presentation.metadata["read_only"], true);
    }
}
