use std::env;
use std::path::Path;
use std::process::Command;

use serde::Serialize;

use crate::BenchResult;

#[derive(Debug, Serialize)]
pub struct EnvironmentInfo {
    pub os: OperatingSystemInfo,
    pub cpu: HardwareProbe,
    pub gpu: HardwareProbe,
    pub graphics_driver: HardwareProbe,
    pub logical_parallelism: HardwareProbe,
}

#[derive(Debug, Serialize)]
pub struct OperatingSystemInfo {
    pub name: &'static str,
    pub architecture: &'static str,
    pub version: HardwareProbe,
}

#[derive(Debug, Serialize)]
pub struct HardwareProbe {
    pub value: Option<String>,
    pub reason: Option<String>,
}

impl HardwareProbe {
    fn available(value: impl Into<String>) -> Self {
        Self {
            value: Some(value.into()),
            reason: None,
        }
    }

    fn unavailable(reason: impl Into<String>) -> Self {
        Self {
            value: None,
            reason: Some(reason.into()),
        }
    }
}

impl EnvironmentInfo {
    pub fn probe() -> Self {
        Self {
            os: OperatingSystemInfo {
                name: env::consts::OS,
                architecture: env::consts::ARCH,
                version: os_version(),
            },
            cpu: cpu_model(),
            gpu: HardwareProbe::unavailable(
                "the portable baseline selects the deterministic CPU Skia backend and does not claim a GPU device",
            ),
            graphics_driver: HardwareProbe::unavailable(
                "no portable standard-library graphics-driver query exists; the measured renderer is CPU Skia",
            ),
            logical_parallelism: std::thread::available_parallelism().map_or_else(
                |error| HardwareProbe::unavailable(format!("query failed: {error}")),
                |count| HardwareProbe::available(count.get().to_string()),
            ),
        }
    }
}

fn cpu_model() -> HardwareProbe {
    if let Some(identifier) = env::var_os("PROCESSOR_IDENTIFIER") {
        let identifier = identifier.to_string_lossy().trim().to_string();
        if !identifier.is_empty() {
            return HardwareProbe::available(identifier);
        }
    }
    #[cfg(target_os = "linux")]
    if let Ok(source) = std::fs::read_to_string("/proc/cpuinfo")
        && let Some(model) = source.lines().find_map(|line| {
            line.strip_prefix("model name")
                .and_then(|rest| rest.split_once(':'))
                .map(|(_, value)| value.trim())
        })
    {
        return HardwareProbe::available(model);
    }
    #[cfg(target_os = "macos")]
    if let Some(model) = command_output("sysctl", &["-n", "machdep.cpu.brand_string"])
        && !model.is_empty()
    {
        return HardwareProbe::available(model);
    }
    HardwareProbe::unavailable("CPU model was not exposed by this operating system")
}

fn os_version() -> HardwareProbe {
    let result = if cfg!(target_os = "windows") {
        command_output("cmd", &["/C", "ver"])
    } else if cfg!(target_os = "macos") {
        command_output("sw_vers", &["-productVersion"])
    } else {
        command_output("uname", &["-sr"])
    };
    result.map_or_else(
        || HardwareProbe::unavailable("operating-system version command was unavailable"),
        HardwareProbe::available,
    )
}

fn command_output(program: &str, arguments: &[&str]) -> Option<String> {
    let output = Command::new(program).args(arguments).output().ok()?;
    if !output.status.success() {
        return None;
    }
    let value = String::from_utf8_lossy(&output.stdout).trim().to_string();
    (!value.is_empty()).then_some(value)
}

#[derive(Debug, Serialize)]
pub struct BuildInfo {
    pub profile: &'static str,
    pub debug_assertions: bool,
    pub git_commit: String,
    pub git_dirty: bool,
    pub rustc: HardwareProbe,
}

impl BuildInfo {
    pub fn probe(repository_root: &Path) -> BenchResult<Self> {
        let git_commit =
            required_command_output(repository_root, "git", &["rev-parse", "--verify", "HEAD"])?;
        let status = required_command_output(
            repository_root,
            "git",
            &["status", "--porcelain=v1", "--untracked-files=normal"],
        )?;
        let rustc = command_output("rustc", &["--version"]).map_or_else(
            || HardwareProbe::unavailable("rustc --version was unavailable"),
            HardwareProbe::available,
        );
        Ok(Self {
            profile: "bench",
            debug_assertions: cfg!(debug_assertions),
            git_commit,
            git_dirty: !status.is_empty(),
            rustc,
        })
    }
}

fn required_command_output(
    directory: &Path,
    program: &str,
    arguments: &[&str],
) -> BenchResult<String> {
    let output = Command::new(program)
        .current_dir(directory)
        .args(arguments)
        .output()?;
    if !output.status.success() {
        return Err(format!("{program} {} failed", arguments.join(" ")).into());
    }
    Ok(String::from_utf8(output.stdout)?.trim().to_string())
}
