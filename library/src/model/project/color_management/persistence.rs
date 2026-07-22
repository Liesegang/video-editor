//! Lossless persistence for repairable `color_management` values.
//!
//! This layer accepts raw JSON first. Structurally malformed data stays in the
//! Project and is classified for diagnostics instead of aborting Project load.

use serde::{Deserialize, Deserializer, Serialize, Serializer};
use serde_json::Value;
use uuid::Uuid;

use super::{ColorManagementConfig, ColorManagementStructureIssue, RequestedColorManagementConfig};

impl Serialize for RequestedColorManagementConfig {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        match self {
            Self::Config(config) => config.serialize(serializer),
            Self::Malformed { raw, .. } => raw.serialize(serializer),
        }
    }
}

impl<'de> Deserialize<'de> for RequestedColorManagementConfig {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let raw = Value::deserialize(deserializer)?;
        match serde_json::from_value::<ColorManagementConfig>(raw.clone()) {
            Ok(config) => Ok(Self::Config(config)),
            Err(error) => {
                let mut structure_issues = classify_structure(&raw);
                if structure_issues.is_empty() {
                    structure_issues.push(ColorManagementStructureIssue::InvalidValue {
                        path: "color_management".to_string(),
                        detail: error.to_string(),
                    });
                }
                Ok(Self::Malformed {
                    raw,
                    structure_issues,
                })
            }
        }
    }
}

fn classify_structure(raw: &Value) -> Vec<ColorManagementStructureIssue> {
    let mut issues = Vec::new();
    let Some(object) = expect_object(raw, "color_management", &mut issues) else {
        return issues;
    };
    classify_unknown_fields(
        object,
        "color_management",
        &["config", "working_space", "preview", "export"],
        &mut issues,
    );

    if let Some(config) = object.get("config") {
        classify_config_identity(config, &mut issues);
    }
    classify_optional_string(
        object.get("working_space"),
        "color_management.working_space",
        &mut issues,
    );
    classify_nested_object(
        object.get("preview"),
        "color_management.preview",
        &["display"],
        &["view"],
        &mut issues,
    );
    classify_nested_object(
        object.get("export"),
        "color_management.export",
        &["output_space"],
        &[],
        &mut issues,
    );
    issues
}

fn classify_config_identity(value: &Value, issues: &mut Vec<ColorManagementStructureIssue>) {
    let path = "color_management.config";
    let Some(object) = expect_object(value, path, issues) else {
        return;
    };
    let Some(kind_value) = object.get("kind") else {
        issues.push(ColorManagementStructureIssue::MissingField {
            path: format!("{path}.kind"),
        });
        return;
    };
    let Some(kind) = expect_string(kind_value, &format!("{path}.kind"), issues) else {
        return;
    };
    match kind {
        "bundled" => {
            classify_unknown_fields(object, path, &["kind", "id"], issues);
            classify_required_strings(object, path, &["id"], issues);
        }
        "ocio_builtin" => {
            classify_unknown_fields(object, path, &["kind", "uri", "ocio_version"], issues);
            classify_required_strings(object, path, &["uri", "ocio_version"], issues);
        }
        "project_asset" => {
            classify_unknown_fields(
                object,
                path,
                &["kind", "asset_id", "sha256", "ocio_version"],
                issues,
            );
            classify_required_strings(
                object,
                path,
                &["asset_id", "sha256", "ocio_version"],
                issues,
            );
            if let Some(Value::String(asset_id)) = object.get("asset_id")
                && Uuid::parse_str(asset_id).is_err()
            {
                issues.push(ColorManagementStructureIssue::InvalidValue {
                    path: format!("{path}.asset_id"),
                    detail: "expected UUID".to_string(),
                });
            }
        }
        unknown => issues.push(ColorManagementStructureIssue::UnknownConfigKind {
            kind: unknown.to_string(),
        }),
    }
}

fn classify_nested_object(
    value: Option<&Value>,
    path: &str,
    string_fields: &[&str],
    nullable_string_fields: &[&str],
    issues: &mut Vec<ColorManagementStructureIssue>,
) {
    let Some(value) = value else {
        return;
    };
    let Some(object) = expect_object(value, path, issues) else {
        return;
    };
    for field in object.keys() {
        if !string_fields.contains(&field.as_str())
            && !nullable_string_fields.contains(&field.as_str())
        {
            issues.push(ColorManagementStructureIssue::UnknownField {
                path: format!("{path}.{field}"),
            });
        }
    }
    for field in string_fields {
        classify_optional_string(object.get(*field), &format!("{path}.{field}"), issues);
    }
    for field in nullable_string_fields {
        if let Some(value) = object.get(*field)
            && !value.is_null()
        {
            let _ = expect_string(value, &format!("{path}.{field}"), issues);
        }
    }
}

fn classify_unknown_fields(
    object: &serde_json::Map<String, Value>,
    path: &str,
    allowed_fields: &[&str],
    issues: &mut Vec<ColorManagementStructureIssue>,
) {
    for field in object.keys() {
        if !allowed_fields.contains(&field.as_str()) {
            issues.push(ColorManagementStructureIssue::UnknownField {
                path: format!("{path}.{field}"),
            });
        }
    }
}

fn classify_required_strings(
    object: &serde_json::Map<String, Value>,
    path: &str,
    fields: &[&str],
    issues: &mut Vec<ColorManagementStructureIssue>,
) {
    for field in fields {
        let field_path = format!("{path}.{field}");
        match object.get(*field) {
            Some(value) => {
                let _ = expect_string(value, &field_path, issues);
            }
            None => issues.push(ColorManagementStructureIssue::MissingField { path: field_path }),
        }
    }
}

fn classify_optional_string(
    value: Option<&Value>,
    path: &str,
    issues: &mut Vec<ColorManagementStructureIssue>,
) {
    if let Some(value) = value {
        let _ = expect_string(value, path, issues);
    }
}

fn expect_object<'a>(
    value: &'a Value,
    path: &str,
    issues: &mut Vec<ColorManagementStructureIssue>,
) -> Option<&'a serde_json::Map<String, Value>> {
    match value {
        Value::Object(object) => Some(object),
        Value::Null => {
            issues.push(ColorManagementStructureIssue::Null {
                path: path.to_string(),
            });
            None
        }
        other => {
            issues.push(ColorManagementStructureIssue::WrongType {
                path: path.to_string(),
                expected: "object".to_string(),
                actual: json_type(other).to_string(),
            });
            None
        }
    }
}

fn expect_string<'a>(
    value: &'a Value,
    path: &str,
    issues: &mut Vec<ColorManagementStructureIssue>,
) -> Option<&'a str> {
    match value {
        Value::String(value) => Some(value),
        Value::Null => {
            issues.push(ColorManagementStructureIssue::Null {
                path: path.to_string(),
            });
            None
        }
        other => {
            issues.push(ColorManagementStructureIssue::WrongType {
                path: path.to_string(),
                expected: "string".to_string(),
                actual: json_type(other).to_string(),
            });
            None
        }
    }
}

fn json_type(value: &Value) -> &'static str {
    match value {
        Value::Null => "null",
        Value::Bool(_) => "boolean",
        Value::Number(_) => "number",
        Value::String(_) => "string",
        Value::Array(_) => "array",
        Value::Object(_) => "object",
    }
}
