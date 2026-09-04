use std::env;
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::Command;

use sha2::{Digest, Sha256};

const REQUIRED_PYTHON_VERSION: &str = "3.13.14";

fn main() -> Result<(), String> {
    println!("cargo:rerun-if-env-changed=PYO3_PYTHON");
    println!("cargo:rerun-if-env-changed=RUVIE_PYTHON_HOME");
    println!("cargo:rerun-if-env-changed=PYO3_ENVIRONMENT_SIGNATURE");

    let target = env::var("TARGET").unwrap_or_default();
    if !target.contains("windows") {
        return Ok(());
    }
    prepare_windows_loader()
        .map_err(|error| format!("cannot prepare RuViE's Windows CPython loader: {error}"))?;
    Ok(())
}

fn prepare_windows_loader() -> Result<(), String> {
    let home = required_path("RUVIE_PYTHON_HOME")?;
    let python = required_path("PYO3_PYTHON")?;
    let output = Command::new(&python)
        .args([
            "-c",
            "import platform,sys; print(platform.python_version()); print(sys.base_prefix); print(sys._is_gil_enabled())",
        ])
        .output()
        .map_err(|error| format!("cannot execute {}: {error}", python.display()))?;
    if !output.status.success() {
        return Err(format!(
            "{} rejected the runtime probe: {}",
            python.display(),
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut lines = stdout.lines();
    let version = lines.next().unwrap_or_default();
    let reported_home = PathBuf::from(lines.next().unwrap_or_default());
    let standard_gil = lines.next().unwrap_or_default();
    if version != REQUIRED_PYTHON_VERSION || standard_gil != "True" {
        return Err(format!(
            "RuViE requires standard-GIL CPython {REQUIRED_PYTHON_VERSION}; found {version}"
        ));
    }
    if reported_home.canonicalize().map_err(display_error)? != home {
        return Err("PYO3_PYTHON and RUVIE_PYTHON_HOME identify different runtimes".to_owned());
    }

    let out_directory = PathBuf::from(
        env::var_os("OUT_DIR").ok_or_else(|| "Cargo did not provide OUT_DIR".to_owned())?,
    );
    let profile_directory = out_directory.ancestors().nth(3).ok_or_else(|| {
        format!(
            "cannot derive Cargo profile directory from {}",
            out_directory.display()
        )
    })?;
    for destination in [
        profile_directory.to_path_buf(),
        profile_directory.join("deps"),
    ] {
        fs::create_dir_all(&destination).map_err(display_error)?;
        for library in [
            "python313.dll",
            "python3.dll",
            "vcruntime140.dll",
            "vcruntime140_1.dll",
        ] {
            let source = home.join(library);
            let destination = destination.join(library);
            copy_if_changed(&source, &destination)?;
        }
    }
    println!("cargo:rerun-if-changed={}", home.join("BUILD").display());
    Ok(())
}

fn required_path(variable: &str) -> Result<PathBuf, String> {
    PathBuf::from(env::var_os(variable).ok_or_else(|| {
        format!("{variable} is not configured; run `cargo run -p xtask -- bootstrap` first")
    })?)
    .canonicalize()
    .map_err(display_error)
}

fn copy_if_changed(source: &Path, destination: &Path) -> Result<(), String> {
    if !same_file_contents(source, destination)? {
        fs::copy(source, destination).map_err(display_error)?;
    }
    Ok(())
}

fn same_file_contents(left: &Path, right: &Path) -> Result<bool, String> {
    let left_metadata = fs::metadata(left).map_err(display_error)?;
    let Ok(right_metadata) = fs::metadata(right) else {
        return Ok(false);
    };
    if left_metadata.len() != right_metadata.len() {
        return Ok(false);
    }
    let mut left_file = fs::File::open(left).map_err(display_error)?;
    let mut right_file = fs::File::open(right).map_err(display_error)?;
    let mut left_hash = Sha256::new();
    let mut right_hash = Sha256::new();
    let mut buffer = [0_u8; 8 * 1024];
    loop {
        let count = left_file.read(&mut buffer).map_err(display_error)?;
        if count == 0 {
            break;
        }
        left_hash.update(&buffer[..count]);
    }
    loop {
        let count = right_file.read(&mut buffer).map_err(display_error)?;
        if count == 0 {
            break;
        }
        right_hash.update(&buffer[..count]);
    }
    Ok(left_hash.finalize() == right_hash.finalize())
}

fn display_error(error: std::io::Error) -> String {
    error.to_string()
}
