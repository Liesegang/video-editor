use std::collections::HashMap;
use std::path::Path;

use ordered_float::OrderedFloat;
use sha2::{Digest, Sha256};

use crate::model::authoring::{
    DataRow, DataSourceRef, GeneratedItem, GeneratedItemSpec, GeneratedProvenance,
    ModuleInstanceId, SourceRef, TimelineInterval,
};
use crate::model::project::property::{PropertyValue, Vec2};

#[derive(Clone, PartialEq, Debug)]
pub struct ParsedTable {
    pub stable_key_field: String,
    pub rows: Vec<DataRow>,
    pub fingerprint: String,
}

pub fn parse_table(path: &Path, source: &str) -> Result<(DataSourceRef, ParsedTable), String> {
    let extension = path
        .extension()
        .and_then(|extension| extension.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    let (source_ref, stable_key_field, rows) = match extension.as_str() {
        "csv" => {
            let (stable_key_field, rows) = parse_csv(source)?;
            (
                DataSourceRef::Csv {
                    path: path.to_string_lossy().into_owned(),
                },
                stable_key_field,
                rows,
            )
        }
        "json" => {
            let (stable_key_field, rows) = parse_json(source)?;
            (
                DataSourceRef::Json {
                    path: path.to_string_lossy().into_owned(),
                },
                stable_key_field,
                rows,
            )
        }
        _ => return Err("Data source must be a CSV or JSON file".to_string()),
    };
    Ok((
        source_ref,
        ParsedTable {
            stable_key_field,
            rows,
            fingerprint: format!("{:x}", Sha256::digest(source.as_bytes())),
        },
    ))
}

pub fn generate_text_items(
    generator_id: ModuleInstanceId,
    table: &ParsedTable,
    duration: f64,
    revision: u64,
    data_source_id: crate::model::authoring::DataSourceId,
) -> Result<Vec<GeneratedItem>, String> {
    let interval = TimelineInterval::new(0.0, duration.max(1.0))?;
    table
        .rows
        .iter()
        .enumerate()
        .map(|(index, row)| {
            let label = ["text", "label", "name"]
                .into_iter()
                .find_map(|key| row.values.get(key).and_then(display_value))
                .unwrap_or_else(|| row.stable_key.clone());
            let x = numeric_value(row.values.get("x")).unwrap_or(80.0);
            let y = numeric_value(row.values.get("y")).unwrap_or(80.0 + index as f64 * 72.0);
            let mut authored_values = row.values.clone();
            authored_values.insert(
                "position".to_string(),
                PropertyValue::Vec2(Vec2 {
                    x: OrderedFloat(x),
                    y: OrderedFloat(y),
                }),
            );
            let stable_id = GeneratedItem::stable_id(generator_id, &row.stable_key);
            Ok(GeneratedItem {
                stable_id,
                generator_id,
                generator_version: 1,
                source_key: row.stable_key.clone(),
                generated_spec: GeneratedItemSpec {
                    name: label.clone(),
                    source: SourceRef::Text { text: label },
                    interval,
                    layer: index as i64,
                    authored_values,
                    module_parameters: HashMap::new(),
                },
                provenance: GeneratedProvenance {
                    data_source_id: Some(data_source_id),
                    source_fingerprint: table.fingerprint.clone(),
                    generated_at_revision: revision,
                },
            })
        })
        .collect()
}

fn parse_csv(source: &str) -> Result<(String, Vec<DataRow>), String> {
    let mut reader = csv::ReaderBuilder::new()
        .flexible(false)
        .from_reader(source.as_bytes());
    let headers = reader
        .headers()
        .map_err(|error| format!("Cannot read CSV headers: {error}"))?
        .iter()
        .map(str::to_string)
        .collect::<Vec<_>>();
    if headers.is_empty() {
        return Err("CSV has no columns".to_string());
    }
    let stable_key_field =
        preferred_key(headers.iter().map(String::as_str)).unwrap_or_else(|| headers[0].clone());
    let key_index = headers
        .iter()
        .position(|header| header == &stable_key_field)
        .ok_or_else(|| "Selected CSV stable-key column is missing".to_string())?;
    let mut rows = Vec::new();
    for (index, record) in reader.records().enumerate() {
        let record =
            record.map_err(|error| format!("Cannot read CSV row {}: {error}", index + 2))?;
        let stable_key = record.get(key_index).unwrap_or_default().trim().to_string();
        if stable_key.is_empty() {
            return Err(format!("CSV row {} has an empty stable key", index + 2));
        }
        let values = headers
            .iter()
            .zip(record.iter())
            .map(|(header, value)| (header.clone(), parse_csv_value(value)))
            .collect();
        rows.push(DataRow { stable_key, values });
    }
    Ok((stable_key_field, validate_unique_keys(rows)?))
}

fn parse_json(source: &str) -> Result<(String, Vec<DataRow>), String> {
    let value: serde_json::Value =
        serde_json::from_str(source).map_err(|error| format!("Cannot parse JSON: {error}"))?;
    let rows = match value {
        serde_json::Value::Array(rows) => rows,
        serde_json::Value::Object(mut object) => object
            .remove("rows")
            .and_then(|rows| rows.as_array().cloned())
            .ok_or_else(|| {
                "JSON must be an array of objects or contain a 'rows' array".to_string()
            })?,
        _ => return Err("JSON must be an array of objects".to_string()),
    };
    let first = rows
        .first()
        .and_then(serde_json::Value::as_object)
        .ok_or_else(|| "JSON data has no object rows".to_string())?;
    let stable_key_field = preferred_key(first.keys().map(String::as_str))
        .or_else(|| first.keys().next().cloned())
        .ok_or_else(|| "JSON row has no fields".to_string())?;
    let mut parsed = Vec::new();
    for (index, row) in rows.into_iter().enumerate() {
        let object = row
            .as_object()
            .ok_or_else(|| format!("JSON row {} is not an object", index + 1))?;
        let stable_key = object
            .get(&stable_key_field)
            .and_then(json_key)
            .ok_or_else(|| format!("JSON row {} has no scalar stable key", index + 1))?;
        let values = object
            .iter()
            .map(|(key, value)| (key.clone(), PropertyValue::from(value.clone())))
            .collect();
        parsed.push(DataRow { stable_key, values });
    }
    Ok((stable_key_field, validate_unique_keys(parsed)?))
}

fn validate_unique_keys(rows: Vec<DataRow>) -> Result<Vec<DataRow>, String> {
    let mut keys = std::collections::HashSet::new();
    for row in &rows {
        if !keys.insert(row.stable_key.clone()) {
            return Err(format!("Duplicate stable key '{}'", row.stable_key));
        }
    }
    Ok(rows)
}

fn preferred_key<'a>(fields: impl Iterator<Item = &'a str>) -> Option<String> {
    let fields = fields.collect::<Vec<_>>();
    ["id", "key", "stable_key"]
        .into_iter()
        .find(|candidate| fields.iter().any(|field| field == candidate))
        .map(str::to_string)
}

fn parse_csv_value(value: &str) -> PropertyValue {
    let trimmed = value.trim();
    if let Ok(value) = trimmed.parse::<i64>() {
        PropertyValue::Integer(value)
    } else if let Ok(value) = trimmed.parse::<f64>() {
        PropertyValue::Number(OrderedFloat(value))
    } else if let Ok(value) = trimmed.parse::<bool>() {
        PropertyValue::Boolean(value)
    } else {
        PropertyValue::String(value.to_string())
    }
}

fn json_key(value: &serde_json::Value) -> Option<String> {
    match value {
        serde_json::Value::String(value) => Some(value.clone()),
        serde_json::Value::Number(value) => Some(value.to_string()),
        serde_json::Value::Bool(value) => Some(value.to_string()),
        _ => None,
    }
}

fn display_value(value: &PropertyValue) -> Option<String> {
    match value {
        PropertyValue::String(value) => Some(value.clone()),
        PropertyValue::Integer(value) => Some(value.to_string()),
        PropertyValue::Number(value) => Some(value.to_string()),
        PropertyValue::Boolean(value) => Some(value.to_string()),
        _ => None,
    }
}

fn numeric_value(value: Option<&PropertyValue>) -> Option<f64> {
    match value {
        Some(PropertyValue::Integer(value)) => Some(*value as f64),
        Some(PropertyValue::Number(value)) => Some(value.into_inner()),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn csv_uses_id_as_stable_key_and_rejects_duplicates() {
        let (_, table) = parse_table(
            Path::new("labels.csv"),
            "name,id,x\nFirst,row-1,15\nSecond,row-2,25\n",
        )
        .expect("valid CSV");
        assert_eq!(table.stable_key_field, "id");
        assert_eq!(table.rows[1].stable_key, "row-2");
        assert!(parse_table(Path::new("labels.csv"), "id\nsame\nsame\n").is_err());
    }

    #[test]
    fn json_rows_preserve_typed_values() {
        let (_, table) = parse_table(
            Path::new("labels.json"),
            r#"{"rows":[{"id":"hero","text":"Hello","x":42.5}]}"#,
        )
        .expect("valid JSON");
        assert_eq!(table.rows[0].stable_key, "hero");
        assert_eq!(
            table.rows[0].values["x"],
            PropertyValue::Number(OrderedFloat(42.5))
        );
    }
}
