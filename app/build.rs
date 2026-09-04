use std::env;
use std::fs;
use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::process::Command;

use sha2::{Digest, Sha256};

const REQUIRED_PYTHON_VERSION: &str = "3.13.14";
const RUNTIME_MARKER: &str = ".ruvie-cpython-runtime";

fn main() -> Result<(), String> {
    println!("cargo:rerun-if-env-changed=PYO3_PYTHON");
    println!("cargo:rerun-if-env-changed=RUVIE_PYTHON_HOME");
    println!("cargo:rerun-if-env-changed=PYO3_ENVIRONMENT_SIGNATURE");

    bundle_assets()
        .map_err(|error| format!("cannot bundle RuViE's application assets: {error}"))?;

    let target = env::var("TARGET").unwrap_or_default();
    if !target.contains("windows") {
        return Ok(());
    }

    bundle_windows_python()
        .map_err(|error| format!("cannot bundle RuViE's Windows CPython runtime: {error}"))?;
    Ok(())
}

fn bundle_windows_python() -> Result<(), String> {
    let configured_home = env::var_os("RUVIE_PYTHON_HOME").ok_or_else(|| {
        "RUVIE_PYTHON_HOME is not configured; run `cargo run -p xtask -- bootstrap` first"
            .to_owned()
    })?;
    let configured_executable = env::var_os("PYO3_PYTHON").ok_or_else(|| {
        "PYO3_PYTHON is not configured; run `cargo run -p xtask -- bootstrap` first".to_owned()
    })?;
    let home = PathBuf::from(configured_home)
        .canonicalize()
        .map_err(|error| format!("configured Python home is unavailable: {error}"))?;
    let python = PathBuf::from(configured_executable)
        .canonicalize()
        .map_err(|error| format!("configured Python executable is unavailable: {error}"))?;

    validate_python(&python, &home)?;

    let profile_directory = cargo_profile_directory()?;
    let bundled_home = profile_directory.join("python");
    let source_build = fs::read_to_string(home.join("BUILD")).unwrap_or_default();
    let runtime_fingerprint =
        windows_runtime_fingerprint(&home).map_err(display_io("fingerprint managed runtime"))?;
    let expected_marker = format!(
        "version={REQUIRED_PYTHON_VERSION}\nbuild={}\nlayout=windows-bundle-v2\nsha256={runtime_fingerprint}\n",
        source_build.trim(),
    );
    let existing_marker = fs::read_to_string(bundled_home.join(RUNTIME_MARKER)).ok();
    let standard_library = bundled_home.join("Lib/encodings/__init__.py");
    let untrusted_site_packages = bundled_home.join("Lib/site-packages");
    if existing_marker.as_deref() != Some(expected_marker.as_str())
        || !standard_library.is_file()
        || untrusted_site_packages.exists()
    {
        if bundled_home.exists() {
            fs::remove_dir_all(&bundled_home).map_err(display_io("remove stale runtime"))?;
        }
        copy_windows_runtime(&home, &bundled_home).map_err(display_io("copy managed runtime"))?;
        fs::write(bundled_home.join(RUNTIME_MARKER), &expected_marker)
            .map_err(display_io("write runtime marker"))?;
    }

    for library in [
        "python313.dll",
        "python3.dll",
        "vcruntime140.dll",
        "vcruntime140_1.dll",
    ] {
        copy_if_changed(&home.join(library), &profile_directory.join(library))
            .map_err(display_io("copy runtime loader library"))?;
    }

    let manifest_source = Path::new(&env::var("CARGO_MANIFEST_DIR").unwrap_or_default())
        .join("../python-runtime/cpython-runtime.json");
    copy_if_changed(
        &manifest_source,
        &profile_directory.join("python-runtime-manifest.json"),
    )
    .map_err(display_io("copy runtime manifest"))?;

    println!("cargo:rerun-if-changed={}", home.join("BUILD").display());
    println!("cargo:rerun-if-changed={}", manifest_source.display());
    Ok(())
}

fn bundle_assets() -> Result<(), String> {
    let manifest_directory = PathBuf::from(
        env::var_os("CARGO_MANIFEST_DIR")
            .ok_or_else(|| "Cargo did not provide CARGO_MANIFEST_DIR".to_owned())?,
    );
    let source = manifest_directory.join("../assets");
    let destination = cargo_profile_directory()?.join("assets");
    if destination.exists() {
        fs::remove_dir_all(&destination).map_err(display_io("remove stale application assets"))?;
    }
    copy_directory(&source, &destination).map_err(display_io("copy application assets"))?;
    println!("cargo:rerun-if-changed={}", source.display());
    Ok(())
}

fn cargo_profile_directory() -> Result<PathBuf, String> {
    let out_directory = PathBuf::from(
        env::var_os("OUT_DIR").ok_or_else(|| "Cargo did not provide OUT_DIR".to_owned())?,
    );
    out_directory
        .ancestors()
        .nth(3)
        .map(Path::to_path_buf)
        .ok_or_else(|| {
            format!(
                "cannot derive Cargo profile directory from {}",
                out_directory.display()
            )
        })
}

fn validate_python(python: &Path, home: &Path) -> Result<(), String> {
    let output = Command::new(python)
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
    let stdout = String::from_utf8(output.stdout)
        .map_err(|error| format!("runtime probe returned non-UTF-8 output: {error}"))?;
    let mut lines = stdout.lines();
    let version = lines.next().unwrap_or_default();
    let reported_home = PathBuf::from(lines.next().unwrap_or_default());
    let standard_gil = lines.next().unwrap_or_default();
    if version != REQUIRED_PYTHON_VERSION {
        return Err(format!(
            "RuViE requires CPython {REQUIRED_PYTHON_VERSION}, but PyO3 is configured for {version}"
        ));
    }
    let reported_home = reported_home
        .canonicalize()
        .map_err(|error| format!("reported Python home is unavailable: {error}"))?;
    if reported_home != home {
        return Err(format!(
            "PYO3_PYTHON resolves to '{}', but RUVIE_PYTHON_HOME resolves to '{}'",
            reported_home.display(),
            home.display()
        ));
    }
    if standard_gil != "True" {
        return Err("RuViE requires a standard-GIL CPython build".to_owned());
    }
    Ok(())
}

fn copy_directory(source: &Path, destination: &Path) -> io::Result<()> {
    fs::create_dir_all(destination)?;
    for entry in fs::read_dir(source)? {
        let entry = entry?;
        let source_path = entry.path();
        let destination_path = destination.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_directory(&source_path, &destination_path)?;
        } else {
            fs::copy(source_path, destination_path)?;
        }
    }
    Ok(())
}

fn copy_windows_runtime(source: &Path, destination: &Path) -> io::Result<()> {
    fs::create_dir_all(destination)?;
    for directory in ["DLLs", "Lib", "tcl"] {
        let source_directory = source.join(directory);
        if source_directory.is_dir() {
            copy_python_directory(&source_directory, &destination.join(directory), directory)?;
        }
    }
    for file in [
        "BUILD",
        "LICENSE.txt",
        "python313.dll",
        "python3.dll",
        "vcruntime140.dll",
        "vcruntime140_1.dll",
    ] {
        let source_file = source.join(file);
        if source_file.is_file() {
            fs::copy(source_file, destination.join(file))?;
        }
    }
    Ok(())
}

fn copy_python_directory(source: &Path, destination: &Path, relative: &str) -> io::Result<()> {
    fs::create_dir_all(destination)?;
    for entry in fs::read_dir(source)? {
        let entry = entry?;
        let name = entry.file_name();
        let name_text = name.to_string_lossy();
        let child_relative = format!("{relative}/{}", name_text.replace('\\', "/"));
        if excluded_python_path(&child_relative, entry.file_type()?.is_dir()) {
            continue;
        }
        let source_path = entry.path();
        let destination_path = destination.join(name);
        if entry.file_type()?.is_dir() {
            copy_python_directory(&source_path, &destination_path, &child_relative)?;
        } else {
            fs::copy(source_path, destination_path)?;
        }
    }
    Ok(())
}

fn excluded_python_path(relative: &str, is_directory: bool) -> bool {
    let normalized = relative.replace('\\', "/");
    (is_directory
        && (normalized == "Lib/site-packages"
            || normalized.ends_with("/__pycache__")
            || normalized == "Lib/test"))
        || (!is_directory
            && Path::new(&normalized)
                .extension()
                .is_some_and(|extension| extension.eq_ignore_ascii_case("pyc")))
}

fn windows_runtime_fingerprint(home: &Path) -> io::Result<String> {
    let mut entries = Vec::new();
    for directory in ["DLLs", "Lib", "tcl"] {
        let source = home.join(directory);
        if source.is_dir() {
            collect_runtime_files(&source, directory, &mut entries)?;
        }
    }
    for file in [
        "BUILD",
        "LICENSE.txt",
        "python313.dll",
        "python3.dll",
        "vcruntime140.dll",
        "vcruntime140_1.dll",
    ] {
        let source = home.join(file);
        if source.is_file() {
            entries.push((file.to_owned(), source));
        }
    }
    entries.sort_by(|left, right| left.0.cmp(&right.0));

    let mut digest = Sha256::new();
    for (relative, source) in entries {
        digest.update(relative.as_bytes());
        digest.update([0]);
        let mut file = fs::File::open(source)?;
        let mut buffer = [0_u8; 8 * 1024];
        loop {
            let count = file.read(&mut buffer)?;
            if count == 0 {
                break;
            }
            digest.update(&buffer[..count]);
        }
    }
    Ok(format!("{:x}", digest.finalize()))
}

fn collect_runtime_files(
    source: &Path,
    relative: &str,
    entries: &mut Vec<(String, PathBuf)>,
) -> io::Result<()> {
    for entry in fs::read_dir(source)? {
        let entry = entry?;
        let name = entry.file_name();
        let child_relative = format!("{relative}/{}", name.to_string_lossy().replace('\\', "/"));
        let file_type = entry.file_type()?;
        if excluded_python_path(&child_relative, file_type.is_dir()) {
            continue;
        }
        if file_type.is_dir() {
            collect_runtime_files(&entry.path(), &child_relative, entries)?;
        } else if file_type.is_file() {
            entries.push((child_relative, entry.path()));
        }
    }
    Ok(())
}

fn copy_if_changed(source: &Path, destination: &Path) -> io::Result<()> {
    if !same_file_contents(source, destination)? {
        fs::copy(source, destination)?;
    }
    Ok(())
}

fn same_file_contents(left: &Path, right: &Path) -> io::Result<bool> {
    let left_metadata = fs::metadata(left)?;
    let Ok(right_metadata) = fs::metadata(right) else {
        return Ok(false);
    };
    if left_metadata.len() != right_metadata.len() {
        return Ok(false);
    }
    let mut left_file = fs::File::open(left)?;
    let mut right_file = fs::File::open(right)?;
    let mut left_buffer = [0_u8; 8 * 1024];
    let mut right_buffer = [0_u8; 8 * 1024];
    loop {
        let left_count = left_file.read(&mut left_buffer)?;
        let right_count = right_file.read(&mut right_buffer)?;
        if left_count != right_count || left_buffer[..left_count] != right_buffer[..right_count] {
            return Ok(false);
        }
        if left_count == 0 {
            return Ok(true);
        }
    }
}

fn display_io(operation: &'static str) -> impl FnOnce(io::Error) -> String {
    move |error| format!("failed to {operation}: {error}")
}
