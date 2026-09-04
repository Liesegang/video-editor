use std::hint::black_box;
use std::path::Path;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use serde::Serialize;

use crate::BenchResult;
use crate::fixtures::{FixtureMetadata, FixtureSet};
use crate::system::{BuildInfo, EnvironmentInfo};

#[derive(Clone, Copy, Debug)]
pub struct RunConfiguration {
    pub warmup_iterations: u32,
    pub sample_count: u32,
}

#[derive(Debug, Serialize)]
pub struct BaselineReport {
    pub schema_version: u32,
    pub generated_at_unix_ms: u128,
    pub environment: EnvironmentInfo,
    pub build: BuildInfo,
    pub fixture: FixtureMetadata,
    pub run: RunMetadata,
    pub metrics: Vec<MetricResult>,
}

#[derive(Debug, Serialize)]
pub struct RunMetadata {
    pub warmup_iterations: u32,
    pub sample_count: u32,
    pub timer: &'static str,
}

impl BaselineReport {
    pub fn new(
        repository_root: &Path,
        fixtures: &FixtureSet,
        warmup_iterations: u32,
        sample_count: u32,
        metrics: Vec<MetricResult>,
    ) -> BenchResult<Self> {
        let generated_at_unix_ms = SystemTime::now().duration_since(UNIX_EPOCH)?.as_millis();
        Ok(Self {
            schema_version: 1,
            generated_at_unix_ms,
            environment: EnvironmentInfo::probe(),
            build: BuildInfo::probe(repository_root)?,
            fixture: fixtures.metadata().clone(),
            run: RunMetadata {
                warmup_iterations,
                sample_count,
                timer: "std::time::Instant",
            },
            metrics,
        })
    }
}

#[derive(Debug, Serialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum MetricResult {
    Measured {
        name: String,
        category: String,
        description: String,
        production_path: String,
        fixture: String,
        unit: &'static str,
        operations_per_sample: u32,
        samples_ns_per_operation: Vec<u64>,
        summary: MeasurementSummary,
    },
    Unavailable {
        name: String,
        category: String,
        description: String,
        production_path: String,
        value: Option<u64>,
        reason: String,
    },
}

#[derive(Debug, Serialize)]
pub struct MeasurementSummary {
    pub minimum_ns: u64,
    pub median_ns: f64,
    pub mean_ns: f64,
    pub p95_ns: u64,
    pub maximum_ns: u64,
}

impl MeasurementSummary {
    fn from_samples(samples: &[u64]) -> BenchResult<Self> {
        if samples.is_empty() {
            return Err("cannot summarize an empty performance sample".into());
        }
        let mut sorted = samples.to_vec();
        sorted.sort_unstable();
        let middle = sorted.len() / 2;
        let median_ns = if sorted.len().is_multiple_of(2) {
            (sorted[middle - 1] as f64 + sorted[middle] as f64) / 2.0
        } else {
            sorted[middle] as f64
        };
        let mean_ns = sorted.iter().map(|value| *value as f64).sum::<f64>() / sorted.len() as f64;
        let p95_index = (sorted.len() * 95).div_ceil(100).saturating_sub(1);
        Ok(Self {
            minimum_ns: sorted[0],
            median_ns,
            mean_ns,
            p95_ns: sorted[p95_index],
            maximum_ns: sorted[sorted.len() - 1],
        })
    }
}

pub fn contract_self_check() -> BenchResult<()> {
    let summary = MeasurementSummary::from_samples(&[3, 1, 2])?;
    if summary.minimum_ns != 1
        || summary.median_ns != 2.0
        || summary.mean_ns != 2.0
        || summary.p95_ns != 3
        || summary.maximum_ns != 3
    {
        return Err("performance summary contract self-test failed".into());
    }
    let unavailable = unavailable("probe", "self_test", "probe", "none", "not measured");
    let json = serde_json::to_value(unavailable)?;
    if !json.get("value").is_some_and(serde_json::Value::is_null)
        || json.get("reason").and_then(serde_json::Value::as_str) != Some("not measured")
    {
        return Err("unavailable metric must serialize as null plus a reason".into());
    }
    Ok(())
}

pub struct MetricDefinition<'a> {
    pub name: &'a str,
    pub category: &'a str,
    pub description: &'a str,
    pub production_path: &'a str,
    pub fixture: &'a str,
    pub operations_per_sample: u32,
}

pub fn measure(
    definition: MetricDefinition<'_>,
    configuration: RunConfiguration,
    mut operation: impl FnMut() -> BenchResult<()>,
) -> BenchResult<MetricResult> {
    if definition.operations_per_sample == 0 {
        return Err("performance metric operations_per_sample must be non-zero".into());
    }
    for _ in 0..configuration.warmup_iterations {
        black_box(operation()?);
    }
    let mut samples = Vec::with_capacity(configuration.sample_count as usize);
    for _ in 0..configuration.sample_count {
        let started = Instant::now();
        black_box(operation()?);
        let elapsed = started.elapsed().as_nanos();
        let per_operation = elapsed / u128::from(definition.operations_per_sample);
        samples.push(
            u64::try_from(per_operation)
                .map_err(|_| "performance duration exceeds the JSON u64 range")?,
        );
    }
    let summary = MeasurementSummary::from_samples(&samples)?;
    Ok(MetricResult::Measured {
        name: definition.name.to_string(),
        category: definition.category.to_string(),
        description: definition.description.to_string(),
        production_path: definition.production_path.to_string(),
        fixture: definition.fixture.to_string(),
        unit: "nanoseconds_per_operation",
        operations_per_sample: definition.operations_per_sample,
        samples_ns_per_operation: samples,
        summary,
    })
}

pub fn unavailable(
    name: &str,
    category: &str,
    description: &str,
    production_path: &str,
    reason: impl Into<String>,
) -> MetricResult {
    MetricResult::Unavailable {
        name: name.to_string(),
        category: category.to_string(),
        description: description.to_string(),
        production_path: production_path.to_string(),
        value: None,
        reason: reason.into(),
    }
}
