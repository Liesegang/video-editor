use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::model::project::property::PropertyValue;

use super::{
    DataSourceId, GeneratedItemId, ModuleInstanceId, OverrideId, SourceRef, TimelineInterval,
};

#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
#[serde(deny_unknown_fields)]
pub struct DataSource {
    pub id: DataSourceId,
    pub name: String,
    pub source: DataSourceRef,
    pub stable_key_field: String,
    pub cached_rows: Vec<DataRow>,
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum DataSourceRef {
    Csv { path: String },
    Json { path: String },
    EmbeddedTable,
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
#[serde(deny_unknown_fields)]
pub struct DataRow {
    pub stable_key: String,
    pub values: HashMap<String, PropertyValue>,
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
#[serde(deny_unknown_fields)]
pub struct GeneratedItem {
    pub stable_id: GeneratedItemId,
    pub generator_id: ModuleInstanceId,
    pub generator_version: u64,
    pub source_key: String,
    pub generated_spec: GeneratedItemSpec,
    pub provenance: GeneratedProvenance,
}

impl GeneratedItem {
    pub fn stable_id(generator_id: ModuleInstanceId, source_key: &str) -> GeneratedItemId {
        let mut digest = Sha256::new();
        digest.update(generator_id.as_uuid().as_bytes());
        digest.update(source_key.as_bytes());
        let hash = digest.finalize();
        let mut bytes = [0_u8; 16];
        bytes.copy_from_slice(&hash[..16]);
        bytes[6] = (bytes[6] & 0x0f) | 0x50;
        bytes[8] = (bytes[8] & 0x3f) | 0x80;
        GeneratedItemId::from_uuid(uuid::Uuid::from_bytes(bytes))
    }
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
#[serde(deny_unknown_fields)]
pub struct GeneratedItemSpec {
    pub name: String,
    pub source: SourceRef,
    pub interval: TimelineInterval,
    pub layer: i64,
    pub authored_values: HashMap<String, PropertyValue>,
    pub module_parameters: HashMap<String, PropertyValue>,
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug)]
#[serde(deny_unknown_fields)]
pub struct GeneratedProvenance {
    pub data_source_id: Option<DataSourceId>,
    pub source_fingerprint: String,
    pub generated_at_revision: u64,
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
#[serde(deny_unknown_fields)]
pub struct Override {
    pub id: OverrideId,
    pub generated_item_id: GeneratedItemId,
    pub patch: Vec<OverridePatch>,
    pub status: OverrideStatus,
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
#[serde(deny_unknown_fields)]
pub struct OverridePatch {
    pub path: OverridePath,
    pub operator: OverrideOperator,
    pub value: PropertyValue,
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum OverridePath {
    SourceText,
    AuthoredProperty { key: String },
    ModuleParameter { key: String },
}

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
#[serde(rename_all = "snake_case")]
pub enum OverrideOperator {
    Replace,
    Add,
    Multiply,
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum OverrideStatus {
    Active,
    Orphaned,
    Conflict { reason: String },
}
