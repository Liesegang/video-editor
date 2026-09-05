#[path = "production_baseline/fixtures.rs"]
mod fixtures;
#[path = "production_baseline/gpu_preview.rs"]
mod gpu_preview;
#[path = "production_baseline/measurements.rs"]
mod measurements;
#[path = "production_baseline/report.rs"]
mod report;
#[path = "production_baseline/system.rs"]
mod system;

use std::env;
use std::fs;
use std::path::PathBuf;

use report::{BaselineReport, RunConfiguration};

type BenchResult<T> = Result<T, Box<dyn std::error::Error + Send + Sync>>;

#[derive(Debug)]
struct Arguments {
    output: PathBuf,
    repository_root: PathBuf,
    warmup: u32,
    samples: u32,
    gpu_preview: bool,
}

fn main() -> BenchResult<()> {
    env_logger::init();
    let raw_arguments = env::args_os().skip(1).collect::<Vec<_>>();
    // `cargo test --all-targets` executes harness-free benchmark binaries with
    // no arguments. Keep that repository gate fast while still exercising the
    // report contract; only `cargo bench` receives the explicit workload.
    if raw_arguments.is_empty() {
        report::contract_self_check()?;
        println!("production baseline contract self-test passed");
        return Ok(());
    }
    let arguments = parse_arguments(raw_arguments)?;
    let fixture_set = fixtures::FixtureSet::build(&arguments.repository_root)?;
    let mut metrics = measurements::run(
        &fixture_set,
        RunConfiguration {
            warmup_iterations: arguments.warmup,
            sample_count: arguments.samples,
        },
    )?;
    let gpu_driver = if arguments.gpu_preview {
        let measured = gpu_preview::run(
            &fixture_set,
            RunConfiguration {
                warmup_iterations: arguments.warmup,
                sample_count: arguments.samples,
            },
        )?;
        metrics.extend(measured.metrics);
        Some(measured.driver)
    } else {
        metrics.push(report::unavailable(
            "gpu_preview_frame",
            "preview",
            "Render one Preview frame on the active graphics device",
            "RenderService<SkiaRenderer> GPU backend",
            "GPU measurement requires the opt-in --gpu-preview flag; the default workload selects CPU Skia",
        ));
        None
    };
    let mut report = BaselineReport::new(
        &arguments.repository_root,
        &fixture_set,
        arguments.warmup,
        arguments.samples,
        metrics,
    )?;
    if let Some(driver) = gpu_driver {
        report.environment.gpu = system::HardwareProbe::available(driver.renderer);
        report.environment.graphics_driver = system::HardwareProbe::available(driver.version);
    }
    let source = serde_json::to_string_pretty(&report)?;
    let parent = arguments
        .output
        .parent()
        .ok_or("performance output path has no parent directory")?;
    fs::create_dir_all(parent)?;
    fs::write(&arguments.output, format!("{source}\n"))?;
    println!("wrote {}", arguments.output.display());
    Ok(())
}

fn parse_arguments(
    arguments: impl IntoIterator<Item = std::ffi::OsString>,
) -> BenchResult<Arguments> {
    let mut arguments = arguments.into_iter();
    let mut output = None;
    let mut repository_root = None;
    let mut warmup = None;
    let mut samples = None;
    let mut cargo_bench_flag = false;
    let mut gpu_preview = false;
    while let Some(argument) = arguments.next() {
        let target = if argument == "--gpu-preview" {
            if gpu_preview {
                return Err("--gpu-preview was repeated".into());
            }
            gpu_preview = true;
            continue;
        } else if argument == "--bench" {
            if cargo_bench_flag {
                return Err("Cargo --bench marker was repeated".into());
            }
            cargo_bench_flag = true;
            continue;
        } else if argument == "--output" {
            &mut output
        } else if argument == "--repository-root" {
            &mut repository_root
        } else if argument == "--warmup" {
            let value = required_value(&mut arguments, "--warmup")?;
            warmup = Some(parse_count(&value, "--warmup", true)?);
            continue;
        } else if argument == "--samples" {
            let value = required_value(&mut arguments, "--samples")?;
            samples = Some(parse_count(&value, "--samples", false)?);
            continue;
        } else {
            return Err(format!(
                "unknown production_baseline argument '{}'",
                argument.to_string_lossy()
            )
            .into());
        };
        if target.is_some() {
            return Err(format!("argument '{}' was repeated", argument.to_string_lossy()).into());
        }
        *target = Some(PathBuf::from(required_value(
            &mut arguments,
            &argument.to_string_lossy(),
        )?));
    }
    Ok(Arguments {
        output: output.ok_or("--output is required")?,
        repository_root: repository_root.ok_or("--repository-root is required")?,
        warmup: warmup.ok_or("--warmup is required")?,
        samples: samples.ok_or("--samples is required")?,
        gpu_preview,
    })
}

fn required_value(
    arguments: &mut impl Iterator<Item = std::ffi::OsString>,
    option: &str,
) -> BenchResult<std::ffi::OsString> {
    arguments
        .next()
        .ok_or_else(|| format!("{option} requires a value").into())
}

fn parse_count(value: &std::ffi::OsStr, option: &str, allow_zero: bool) -> BenchResult<u32> {
    let count = value
        .to_string_lossy()
        .parse::<u32>()
        .map_err(|_| format!("{option} requires an unsigned integer"))?;
    if count == 0 && !allow_zero {
        return Err(format!("{option} must be greater than zero").into());
    }
    Ok(count)
}
