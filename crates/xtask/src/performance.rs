use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::Value;

use super::{TaskError, TaskResult, io_error};

const DEFAULT_WARMUP: u32 = 2;
const DEFAULT_SAMPLES: u32 = 5;

#[derive(Debug, PartialEq, Eq)]
pub(super) struct PerformanceOptions {
    output: Option<PathBuf>,
    warmup: u32,
    samples: u32,
    gpu_preview: bool,
}

impl PerformanceOptions {
    pub(super) fn parse(arguments: impl IntoIterator<Item = OsString>) -> TaskResult<Self> {
        let mut arguments = arguments.into_iter();
        let mut options = Self {
            output: None,
            warmup: DEFAULT_WARMUP,
            samples: DEFAULT_SAMPLES,
            gpu_preview: false,
        };
        let mut warmup_seen = false;
        let mut samples_seen = false;
        while let Some(argument) = arguments.next() {
            if argument == "--output" {
                if options.output.is_some() {
                    return Err(TaskError::new("--output was specified more than once"));
                }
                let value = arguments
                    .next()
                    .ok_or_else(|| TaskError::new("--output requires a path"))?;
                if value.is_empty() {
                    return Err(TaskError::new("--output path cannot be empty"));
                }
                options.output = Some(PathBuf::from(value));
            } else if argument == "--warmup" {
                if warmup_seen {
                    return Err(TaskError::new("--warmup was specified more than once"));
                }
                warmup_seen = true;
                options.warmup = parse_count(&mut arguments, "--warmup", true)?;
            } else if argument == "--samples" {
                if samples_seen {
                    return Err(TaskError::new("--samples was specified more than once"));
                }
                samples_seen = true;
                options.samples = parse_count(&mut arguments, "--samples", false)?;
            } else if argument == "--gpu-preview" {
                if options.gpu_preview {
                    return Err(TaskError::new("--gpu-preview was specified more than once"));
                }
                options.gpu_preview = true;
            } else {
                return Err(TaskError::new(format!(
                    "unknown performance-baseline argument '{}'",
                    argument.to_string_lossy()
                )));
            }
        }
        Ok(options)
    }
}

fn parse_count(
    arguments: &mut impl Iterator<Item = OsString>,
    option: &str,
    allow_zero: bool,
) -> TaskResult<u32> {
    let value = arguments
        .next()
        .ok_or_else(|| TaskError::new(format!("{option} requires a count")))?;
    let count = value.to_string_lossy().parse::<u32>().map_err(|_| {
        TaskError::new(format!(
            "{option} requires an unsigned integer, not '{}'",
            value.to_string_lossy()
        ))
    })?;
    if count == 0 && !allow_zero {
        return Err(TaskError::new(format!(
            "{option} must be greater than zero"
        )));
    }
    Ok(count)
}

pub(super) fn run(repository: &Path, options: &PerformanceOptions) -> TaskResult<()> {
    let output = options
        .output
        .clone()
        .unwrap_or_else(|| PathBuf::from("target/performance/production-baseline.json"));
    let output = if output.is_absolute() {
        output
    } else {
        repository.join(output)
    };
    let parent = output
        .parent()
        .ok_or_else(|| TaskError::new("performance output path has no parent directory"))?;
    fs::create_dir_all(parent).map_err(|error| io_error("create directory", parent, error))?;

    let cargo = std::env::var_os("CARGO").unwrap_or_else(|| OsString::from("cargo"));
    let mut command = Command::new(cargo);
    command
        .current_dir(repository)
        .args([
            "bench",
            "--locked",
            "--package",
            "library",
            "--bench",
            "production_baseline",
            "--",
            "--output",
        ])
        .arg(&output)
        .args([
            "--warmup",
            &options.warmup.to_string(),
            "--samples",
            &options.samples.to_string(),
            "--repository-root",
        ])
        .arg(repository);
    if options.gpu_preview {
        command.arg("--gpu-preview");
    }
    let status = command
        .status()
        .map_err(|error| TaskError::new(format!("cannot start performance benchmark: {error}")))?;
    if !status.success() {
        return Err(TaskError::new(format!(
            "performance benchmark exited with {status}"
        )));
    }

    let source = fs::read_to_string(&output)
        .map_err(|error| io_error("read performance report", &output, error))?;
    let report: Value = serde_json::from_str(&source).map_err(|error| {
        TaskError::new(format!(
            "performance report '{}' is not JSON: {error}",
            output.display()
        ))
    })?;
    validate_report(&report, options.samples)?;
    println!("performance baseline: {}", output.display());
    Ok(())
}

fn validate_report(report: &Value, expected_samples: u32) -> TaskResult<()> {
    if report.get("schema_version").and_then(Value::as_u64) != Some(1) {
        return Err(TaskError::new(
            "performance report has no supported schema_version",
        ));
    }
    for pointer in [
        "/environment/os/name",
        "/environment/os/architecture",
        "/build/profile",
        "/build/git_commit",
        "/fixture/sha256",
        "/run/warmup_iterations",
        "/run/sample_count",
    ] {
        if report.pointer(pointer).is_none() {
            return Err(TaskError::new(format!(
                "performance report is missing {pointer}"
            )));
        }
    }
    if report.pointer("/run/sample_count").and_then(Value::as_u64)
        != Some(u64::from(expected_samples))
    {
        return Err(TaskError::new(
            "performance report sample count does not match the request",
        ));
    }
    let metrics = report
        .get("metrics")
        .and_then(Value::as_array)
        .ok_or_else(|| TaskError::new("performance report metrics must be an array"))?;
    if metrics.is_empty() {
        return Err(TaskError::new("performance report contains no metrics"));
    }
    for metric in metrics {
        let status = metric.get("status").and_then(Value::as_str);
        match status {
            Some("measured")
                if metric.get("summary").is_some()
                    && metric
                        .get("samples_ns_per_operation")
                        .and_then(Value::as_array)
                        .is_some_and(|samples| samples.len() == expected_samples as usize) => {}
            Some("unavailable")
                if metric.get("value").is_some_and(Value::is_null)
                    && metric.get("reason").and_then(Value::as_str).is_some() => {}
            _ => {
                return Err(TaskError::new(
                    "performance metric is neither measured nor explicitly unavailable",
                ));
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_performance_options() -> TaskResult<()> {
        let options = PerformanceOptions::parse([
            OsString::from("--output"),
            OsString::from("result.json"),
            OsString::from("--warmup"),
            OsString::from("0"),
            OsString::from("--samples"),
            OsString::from("3"),
        ])?;
        assert_eq!(
            options,
            PerformanceOptions {
                output: Some(PathBuf::from("result.json")),
                warmup: 0,
                samples: 3,
                gpu_preview: false,
            }
        );
        Ok(())
    }

    #[test]
    fn rejects_zero_samples() {
        assert!(
            PerformanceOptions::parse([OsString::from("--samples"), OsString::from("0")]).is_err()
        );
    }

    #[test]
    fn gpu_preview_is_opt_in_and_cannot_be_repeated() {
        assert!(!PerformanceOptions::parse([]).unwrap().gpu_preview);
        assert!(
            PerformanceOptions::parse([OsString::from("--gpu-preview")])
                .unwrap()
                .gpu_preview
        );
        assert!(
            PerformanceOptions::parse([
                OsString::from("--gpu-preview"),
                OsString::from("--gpu-preview"),
            ])
            .is_err()
        );
    }

    #[test]
    fn validates_measured_and_unavailable_metrics() -> TaskResult<()> {
        let report = serde_json::json!({
            "schema_version": 1,
            "environment": {"os": {"name": "test", "architecture": "x86_64"}},
            "build": {"profile": "bench", "git_commit": "abc"},
            "fixture": {"sha256": "def"},
            "run": {"warmup_iterations": 1, "sample_count": 2},
            "metrics": [
                {
                    "status": "measured",
                    "samples_ns_per_operation": [1, 2],
                    "summary": {"median_ns": 1}
                },
                {"status": "unavailable", "value": null, "reason": "not probed"}
            ]
        });
        validate_report(&report, 2)
    }
}
