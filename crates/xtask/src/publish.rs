use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::env;
use std::ffi::{OsStr, OsString};
use std::fs::{self, File};
use std::io::{BufReader, Read};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::thread;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::pe::{PeFile, parse_file};
use crate::{
    PYTHON_DLL, PYTHON_VERSION, PublishOptions, TaskError, TaskResult, X86_64_MACHINE, io_error,
    require_windows,
};

#[derive(Deserialize)]
struct CargoMetadata {
    target_directory: PathBuf,
}

#[derive(Serialize)]
struct BundleManifest {
    schema: u32,
    application: &'static str,
    platform: &'static str,
    architecture: &'static str,
    python: &'static str,
    files: Vec<BundleFile>,
}

#[derive(Serialize)]
struct BundleFile {
    path: String,
    bytes: u64,
    sha256: String,
}

struct FfmpegDistribution {
    executable: PathBuf,
    directories: Vec<PathBuf>,
}

pub(crate) fn run(repository: &Path, options: &PublishOptions) -> TaskResult<()> {
    require_windows("publish")?;
    if !options.skip_build {
        run_release_build(repository)?;
    }
    let target = cargo_target_directory(repository)?;
    let release = target.join("release");
    let requested = options
        .output
        .clone()
        .unwrap_or_else(|| target.join("publish/windows-x86_64/RuViE"));
    let output = prepare_output_path(repository, &release, &requested)?;
    let (staging, backup) = unique_siblings(&output)?;
    fs::create_dir(&staging)
        .map_err(|error| io_error("create publication staging directory", &staging, error))?;
    if let Err(error) = populate_distribution(repository, &release, &staging) {
        return Err(cleanup_after_error(&staging, error));
    }
    commit_staging(&staging, &output, &backup)?;
    println!("[publish] {}", output.display());
    Ok(())
}

fn run_release_build(repository: &Path) -> TaskResult<()> {
    let status = Command::new(cargo_program())
        .args([
            "build",
            "--package",
            "app",
            "--release",
            "--no-default-features",
            "--locked",
        ])
        .current_dir(repository)
        .status()
        .map_err(|error| TaskError::new(format!("cannot execute cargo build: {error}")))?;
    if status.success() {
        Ok(())
    } else {
        Err(TaskError::new(format!("cargo build failed with {status}")))
    }
}

fn cargo_program() -> OsString {
    env::var_os("CARGO").unwrap_or_else(|| OsString::from("cargo"))
}

fn cargo_target_directory(repository: &Path) -> TaskResult<PathBuf> {
    let output = Command::new(cargo_program())
        .args(["metadata", "--format-version", "1", "--no-deps"])
        .current_dir(repository)
        .output()
        .map_err(|error| TaskError::new(format!("cannot execute cargo metadata: {error}")))?;
    if !output.status.success() {
        return Err(TaskError::new(format!(
            "cargo metadata failed with {}: {}",
            output.status,
            String::from_utf8_lossy(&output.stderr).trim()
        )));
    }
    let metadata: CargoMetadata = serde_json::from_slice(&output.stdout)
        .map_err(|error| TaskError::new(format!("cannot decode cargo metadata: {error}")))?;
    Ok(metadata.target_directory)
}

fn prepare_output_path(repository: &Path, release: &Path, requested: &Path) -> TaskResult<PathBuf> {
    let absolute = if requested.is_absolute() {
        requested.to_path_buf()
    } else {
        repository.join(requested)
    };
    let name = absolute
        .file_name()
        .ok_or_else(|| TaskError::new("publication output must name a directory"))?;
    let parent = absolute
        .parent()
        .ok_or_else(|| TaskError::new("publication output has no parent directory"))?;
    fs::create_dir_all(parent)
        .map_err(|error| io_error("create publication parent", parent, error))?;
    let parent = required_directory(parent, "publication parent")?;
    let output = parent.join(name);
    let repository = required_directory(repository, "repository root")?;
    let release = required_directory(release, "Cargo release directory")?;
    if repository.starts_with(&output)
        || release.starts_with(&output)
        || output.starts_with(&release)
    {
        return Err(TaskError::new(format!(
            "unsafe publication output {}; choose a dedicated directory outside target/release",
            output.display()
        )));
    }
    if output.exists() {
        let metadata = fs::symlink_metadata(&output)
            .map_err(|error| io_error("inspect existing publication", &output, error))?;
        if !metadata.is_dir() || metadata.file_type().is_symlink() {
            return Err(TaskError::new(format!(
                "publication output must be a real directory: {}",
                output.display()
            )));
        }
    }
    Ok(output)
}

fn unique_siblings(output: &Path) -> TaskResult<(PathBuf, PathBuf)> {
    let parent = output
        .parent()
        .ok_or_else(|| TaskError::new("publication output has no parent"))?;
    let name = output
        .file_name()
        .ok_or_else(|| TaskError::new("publication output has no name"))?;
    for attempt in 0_u16..100 {
        let staging = sibling(parent, name, "staging", attempt);
        let backup = sibling(parent, name, "backup", attempt);
        if !staging.exists() && !backup.exists() {
            return Ok((staging, backup));
        }
    }
    Err(TaskError::new(
        "cannot allocate unique publication staging paths",
    ))
}

fn sibling(parent: &Path, name: &OsStr, role: &str, attempt: u16) -> PathBuf {
    let mut generated = OsString::from(".");
    generated.push(name);
    generated.push(format!(".{role}-{}-{attempt}", std::process::id()));
    parent.join(generated)
}

fn populate_distribution(repository: &Path, release: &Path, staging: &Path) -> TaskResult<()> {
    let application = required_file(&release.join("app.exe"), "release app.exe")?;
    let python = required_directory(&release.join("python"), "bundled Python home")?;
    let assets = required_directory(&release.join("assets"), "application assets")?;
    let runtime_manifest = required_file(
        &release.join("python-runtime-manifest.json"),
        "Python runtime manifest",
    )?;
    required_file(
        &python.join("Lib/encodings/__init__.py"),
        "Python standard library",
    )?;

    copy_file(&application, &staging.join("app.exe"))?;
    copy_tree(&python, &staging.join("python"))?;
    copy_tree(&assets, &staging.join("assets"))?;
    copy_file(
        &runtime_manifest,
        &staging.join("python-runtime-manifest.json"),
    )?;
    copy_licenses(repository, staging)?;

    let application_pe = parse_file(&application)?;
    require_x86_64(&application, &application_pe)?;
    let imports: BTreeSet<String> = application_pe.imports.into_iter().collect();
    if !imports.contains(PYTHON_DLL) {
        return Err(TaskError::new(format!(
            "app.exe must import {PYTHON_DLL}; found {}",
            imports.iter().cloned().collect::<Vec<_>>().join(", ")
        )));
    }
    let native_search = app_runtime_search_directories(release)?;
    copy_app_runtime_imports(staging, &imports, &native_search)?;
    let ffmpeg = select_ffmpeg_distribution(release, &imports)?;
    copy_ffmpeg_runtime(staging, &imports, &ffmpeg)?;
    copy_ffmpeg_license(staging, &ffmpeg)?;
    write_bundle_manifest(staging)
}

fn copy_licenses(repository: &Path, staging: &Path) -> TaskResult<()> {
    let licenses = staging.join("licenses");
    fs::create_dir(&licenses)
        .map_err(|error| io_error("create license directory", &licenses, error))?;
    copy_file(
        &required_file(&repository.join("LICENSE"), "project license")?,
        &licenses.join("RuViE-LICENSE"),
    )?;
    copy_file(
        &required_file(
            &repository.join("THIRD_PARTY_NOTICES.md"),
            "third-party notices",
        )?,
        &licenses.join("THIRD_PARTY_NOTICES.md"),
    )
}

fn copy_app_runtime_imports(
    staging: &Path,
    imports: &BTreeSet<String>,
    search: &[PathBuf],
) -> TaskResult<()> {
    for import in imports {
        if is_system_library(import) || is_ffmpeg_library(import) {
            continue;
        }
        let source = find_file(import, search).ok_or_else(|| {
            TaskError::new(format!(
                "non-system app dependency {import} was not found beside app.exe or in the Visual C++ redistributable"
            ))
        })?;
        copy_file(&source, &staging.join(import))?;
    }
    Ok(())
}

fn app_runtime_search_directories(release: &Path) -> TaskResult<Vec<PathBuf>> {
    let mut candidates = vec![release.to_path_buf()];
    if let Some(redist) = env::var_os("VCToolsRedistDir") {
        add_vc_redist_candidates(&mut candidates, &PathBuf::from(redist))?;
    }
    for variable in ["ProgramFiles", "ProgramFiles(x86)"] {
        let Some(program_files) = env::var_os(variable) else {
            continue;
        };
        let visual_studio = PathBuf::from(program_files).join("Microsoft Visual Studio");
        for year in child_directories(&visual_studio)? {
            for edition in child_directories(&year)? {
                let versions = edition.join("VC/Redist/MSVC");
                for version in child_directories(&versions)? {
                    add_vc_redist_candidates(&mut candidates, &version)?;
                }
            }
        }
    }
    canonical_non_system_directories(candidates)
}

fn add_vc_redist_candidates(candidates: &mut Vec<PathBuf>, root: &Path) -> TaskResult<()> {
    candidates.push(root.to_path_buf());
    for architecture in [root.join("x64"), root.join("onecore/x64")] {
        candidates.push(architecture.clone());
        for runtime in child_directories(&architecture)? {
            if runtime
                .file_name()
                .is_some_and(|name| name.to_string_lossy().starts_with("Microsoft.VC"))
            {
                candidates.push(runtime);
            }
        }
    }
    Ok(())
}

fn child_directories(parent: &Path) -> TaskResult<Vec<PathBuf>> {
    if !parent.is_dir() {
        return Ok(Vec::new());
    }
    let mut directories = Vec::new();
    for entry in read_directory(parent)? {
        let path = entry.path();
        let file_type = entry
            .file_type()
            .map_err(|error| io_error("inspect directory entry", &path, error))?;
        if file_type.is_dir() && !file_type.is_symlink() {
            directories.push(path);
        }
    }
    Ok(directories)
}

fn select_ffmpeg_distribution(
    release: &Path,
    imports: &BTreeSet<String>,
) -> TaskResult<FfmpegDistribution> {
    let required = imports
        .iter()
        .filter(|name| is_ffmpeg_library(name))
        .cloned()
        .collect::<Vec<_>>();
    if required.is_empty() {
        return Err(TaskError::new("app.exe has no FFmpeg library imports"));
    }

    let mut groups = Vec::new();
    if let Some(configured) = env::var_os("RUVIE_FFMPEG_DIR") {
        groups.push(ffmpeg_root_group(PathBuf::from(configured)));
    }
    groups.push(vec![release.to_path_buf()]);
    if let Some(vcpkg) = env::var_os("VCPKG_ROOT") {
        let installed = PathBuf::from(vcpkg).join("installed/x64-windows");
        groups.push(vec![installed.join("bin"), installed.join("tools/ffmpeg")]);
    }
    if let Some(path) = env::var_os("PATH") {
        groups.extend(env::split_paths(&path).map(|directory| vec![directory]));
    }

    let mut examined = Vec::new();
    for group in groups {
        let directories = canonical_non_system_directories(group)?;
        if directories.is_empty() {
            continue;
        }
        examined.extend(directories.iter().cloned());
        let Some(executable) = find_file("ffmpeg.exe", &directories) else {
            continue;
        };
        if required
            .iter()
            .all(|library| find_file(library, &directories).is_some())
        {
            return Ok(FfmpegDistribution {
                executable,
                directories,
            });
        }
    }
    Err(TaskError::new(format!(
        "no single FFmpeg distribution contains ffmpeg.exe and all app imports ({}); set RUVIE_FFMPEG_DIR. Examined: {}",
        required.join(", "),
        examined
            .iter()
            .map(|path| path.display().to_string())
            .collect::<Vec<_>>()
            .join(", ")
    )))
}

fn ffmpeg_root_group(root: PathBuf) -> Vec<PathBuf> {
    vec![root.clone(), root.join("bin"), root.join("tools/ffmpeg")]
}

fn canonical_non_system_directories(
    candidates: impl IntoIterator<Item = PathBuf>,
) -> TaskResult<Vec<PathBuf>> {
    let windows = env::var_os("SystemRoot")
        .map(PathBuf::from)
        .and_then(|path| path.canonicalize().ok());
    let mut seen = BTreeSet::new();
    let mut directories = Vec::new();
    for candidate in candidates {
        if !candidate.is_dir() {
            continue;
        }
        let canonical = candidate
            .canonicalize()
            .map_err(|error| io_error("canonicalize native search directory", &candidate, error))?;
        if windows
            .as_ref()
            .is_some_and(|root| canonical.starts_with(root))
        {
            continue;
        }
        if seen.insert(canonical.to_string_lossy().to_ascii_lowercase()) {
            directories.push(canonical);
        }
    }
    Ok(directories)
}

fn copy_ffmpeg_runtime(
    staging: &Path,
    app_imports: &BTreeSet<String>,
    distribution: &FfmpegDistribution,
) -> TaskResult<()> {
    let executable = &distribution.executable;
    let search = &distribution.directories;
    let mut pending = VecDeque::new();
    let mut sources = BTreeMap::new();
    add_native(
        "ffmpeg.exe",
        executable,
        staging,
        &mut sources,
        &mut pending,
    )?;
    for import in app_imports.iter().filter(|name| is_ffmpeg_library(name)) {
        let source = find_file(import, search).ok_or_else(|| {
            TaskError::new(format!(
                "app.exe imports {import}, but it was not found in any FFmpeg source"
            ))
        })?;
        add_native(import, &source, staging, &mut sources, &mut pending)?;
    }
    while let Some((name, source)) = pending.pop_front() {
        let pe = parse_file(&source)?;
        require_x86_64(&source, &pe)?;
        for import in pe.imports {
            if is_system_library(&import) || sources.contains_key(&import) {
                continue;
            }
            let bundled = staging.join(&import);
            let dependency = if bundled.is_file() {
                bundled
            } else {
                find_file(&import, search).ok_or_else(|| {
                    TaskError::new(format!(
                        "{name} imports non-system DLL {import}, but its closure cannot resolve it"
                    ))
                })?
            };
            add_native(&import, &dependency, staging, &mut sources, &mut pending)?;
        }
    }
    println!("[publish] FFmpeg: {}", executable.display());
    Ok(())
}

fn add_native(
    name: &str,
    source: &Path,
    staging: &Path,
    sources: &mut BTreeMap<String, PathBuf>,
    pending: &mut VecDeque<(String, PathBuf)>,
) -> TaskResult<()> {
    let normalized = name.to_ascii_lowercase();
    if let Some(previous) = sources.get(&normalized) {
        if hash_file(previous)? != hash_file(source)? {
            return Err(TaskError::new(format!(
                "native DLL collision for {name}: {} and {}",
                previous.display(),
                source.display()
            )));
        }
        return Ok(());
    }
    copy_file(source, &staging.join(&normalized))?;
    sources.insert(normalized.clone(), source.to_path_buf());
    pending.push_back((normalized, source.to_path_buf()));
    Ok(())
}

fn copy_ffmpeg_license(staging: &Path, distribution: &FfmpegDistribution) -> TaskResult<()> {
    let names = ["LICENSE", "LICENSE.txt", "COPYING.LGPLv2.1", "copyright"];
    let mut roots = Vec::new();
    for directory in &distribution.directories {
        roots.push(directory.clone());
        if let Some(parent) = directory.parent() {
            roots.push(parent.to_path_buf());
            roots.push(parent.join("share/ffmpeg"));
        }
    }
    for root in roots {
        for name in names {
            let candidate = root.join(name);
            if candidate.is_file() {
                copy_file(&candidate, &staging.join("licenses/FFmpeg-LICENSE"))?;
                return Ok(());
            }
        }
    }
    Err(TaskError::new(
        "FFmpeg license was not found next to the selected distribution",
    ))
}

fn find_file(name: &str, directories: &[PathBuf]) -> Option<PathBuf> {
    directories
        .iter()
        .map(|directory| directory.join(name))
        .find(|candidate| candidate.is_file())
}

fn is_ffmpeg_library(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    Path::new(&lower)
        .extension()
        .is_some_and(|extension| extension.eq_ignore_ascii_case("dll"))
        && [
            "avcodec-",
            "avdevice-",
            "avfilter-",
            "avformat-",
            "avutil-",
            "postproc-",
            "swresample-",
            "swscale-",
        ]
        .iter()
        .any(|prefix| lower.starts_with(prefix))
}

fn is_system_library(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    if ["msvcp", "vcruntime", "concrt", "vccorlib"]
        .iter()
        .any(|prefix| lower.starts_with(prefix))
    {
        return false;
    }
    if lower.starts_with("api-ms-win-") || lower.starts_with("ext-ms-win-") {
        return true;
    }
    let known = [
        "advapi32.dll",
        "bcrypt.dll",
        "bcryptprimitives.dll",
        "cfgmgr32.dll",
        "combase.dll",
        "comctl32.dll",
        "comdlg32.dll",
        "crypt32.dll",
        "d2d1.dll",
        "d3d11.dll",
        "dbghelp.dll",
        "dwrite.dll",
        "dwmapi.dll",
        "dxgi.dll",
        "dxva2.dll",
        "gdi32.dll",
        "imm32.dll",
        "kernel32.dll",
        "mf.dll",
        "mfplat.dll",
        "mfuuid.dll",
        "msvcrt.dll",
        "ntdll.dll",
        "ole32.dll",
        "oleaut32.dll",
        "opengl32.dll",
        "powrprof.dll",
        "propsys.dll",
        "secur32.dll",
        "setupapi.dll",
        "shell32.dll",
        "shlwapi.dll",
        "user32.dll",
        "userenv.dll",
        "uxtheme.dll",
        "uiautomationcore.dll",
        "version.dll",
        "winmm.dll",
        "wintrust.dll",
        "ws2_32.dll",
    ]
    .contains(&lower.as_str());
    known || windows_system_library_exists(&lower)
}

fn windows_system_library_exists(name: &str) -> bool {
    let Some(root) = env::var_os("SystemRoot").map(PathBuf::from) else {
        return false;
    };
    [root.join("System32"), root.join("SysWOW64")]
        .iter()
        .any(|directory| directory.join(name).is_file())
}

fn write_bundle_manifest(staging: &Path) -> TaskResult<()> {
    let mut files = Vec::new();
    collect_manifest_files(staging, staging, &mut files)?;
    files.sort_by(|left, right| left.path.cmp(&right.path));
    let manifest = BundleManifest {
        schema: 1,
        application: "RuViE",
        platform: "windows",
        architecture: "x86_64",
        python: PYTHON_VERSION,
        files,
    };
    let encoded = serde_json::to_vec_pretty(&manifest)
        .map_err(|error| TaskError::new(format!("cannot encode bundle manifest: {error}")))?;
    let path = staging.join("bundle-manifest.json");
    fs::write(&path, encoded).map_err(|error| io_error("write bundle manifest", &path, error))
}

fn collect_manifest_files(
    root: &Path,
    directory: &Path,
    files: &mut Vec<BundleFile>,
) -> TaskResult<()> {
    for entry in read_directory(directory)? {
        let path = entry.path();
        let file_type = entry
            .file_type()
            .map_err(|error| io_error("inspect bundle entry", &path, error))?;
        if file_type.is_symlink() {
            return Err(TaskError::new(format!(
                "bundle contains a symbolic link: {}",
                path.display()
            )));
        }
        if file_type.is_dir() {
            collect_manifest_files(root, &path, files)?;
        } else if file_type.is_file() {
            let relative = path.strip_prefix(root).map_err(|error| {
                TaskError::new(format!("cannot make bundle path relative: {error}"))
            })?;
            let relative = relative.to_str().ok_or_else(|| {
                TaskError::new(format!(
                    "bundle path is not valid Unicode: {}",
                    path.display()
                ))
            })?;
            files.push(BundleFile {
                path: relative.replace('\\', "/"),
                bytes: entry
                    .metadata()
                    .map_err(|error| io_error("inspect bundle file", &path, error))?
                    .len(),
                sha256: hash_file(&path)?,
            });
        }
    }
    Ok(())
}

fn copy_tree(source: &Path, destination: &Path) -> TaskResult<()> {
    fs::create_dir(destination)
        .map_err(|error| io_error("create bundle directory", destination, error))?;
    for entry in read_directory(source)? {
        let source_path = entry.path();
        let destination_path = destination.join(entry.file_name());
        let file_type = entry
            .file_type()
            .map_err(|error| io_error("inspect source entry", &source_path, error))?;
        if file_type.is_symlink() {
            return Err(TaskError::new(format!(
                "refusing to package symbolic link {}",
                source_path.display()
            )));
        }
        if file_type.is_dir() {
            copy_tree(&source_path, &destination_path)?;
        } else if file_type.is_file() {
            copy_file(&source_path, &destination_path)?;
        }
    }
    Ok(())
}

fn read_directory(directory: &Path) -> TaskResult<Vec<fs::DirEntry>> {
    let iterator =
        fs::read_dir(directory).map_err(|error| io_error("read directory", directory, error))?;
    let mut entries = iterator
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| io_error("read directory entry", directory, error))?;
    entries.sort_by_key(|entry| entry.file_name().to_string_lossy().to_ascii_lowercase());
    Ok(entries)
}

fn copy_file(source: &Path, destination: &Path) -> TaskResult<()> {
    if destination.exists() {
        if hash_file(source)? == hash_file(destination)? {
            return Ok(());
        }
        return Err(TaskError::new(format!(
            "refusing to overwrite bundle collision at {}",
            destination.display()
        )));
    }
    fs::copy(source, destination)
        .map(|_| ())
        .map_err(|error| io_error("copy bundle file", source, error))
}

fn hash_file(path: &Path) -> TaskResult<String> {
    let file = File::open(path).map_err(|error| io_error("open file for hashing", path, error))?;
    let mut reader = BufReader::new(file);
    let mut digest = Sha256::new();
    let mut buffer = [0_u8; 8 * 1024];
    loop {
        let count = reader
            .read(&mut buffer)
            .map_err(|error| io_error("hash file", path, error))?;
        if count == 0 {
            break;
        }
        digest.update(
            buffer
                .get(..count)
                .ok_or_else(|| TaskError::new("hash buffer count exceeded its capacity"))?,
        );
    }
    Ok(format!("{:x}", digest.finalize()))
}

fn commit_staging(staging: &Path, output: &Path, backup: &Path) -> TaskResult<()> {
    if output.exists() {
        fs::rename(output, backup)
            .map_err(|error| io_error("move previous publication to backup", output, error))?;
        if let Err(error) = fs::rename(staging, output) {
            let restore = fs::rename(backup, output);
            return match restore {
                Ok(()) => Err(io_error("activate staged publication", staging, error)),
                Err(restore_error) => Err(TaskError::new(format!(
                    "cannot activate {} ({error}) and cannot restore {} ({restore_error})",
                    staging.display(),
                    backup.display()
                ))),
            };
        }
        remove_previous_publication(backup);
    } else {
        fs::rename(staging, output)
            .map_err(|error| io_error("activate staged publication", staging, error))?;
    }
    Ok(())
}

fn remove_previous_publication(backup: &Path) {
    let mut last_error = None;
    for attempt in 0_u64..8 {
        match fs::remove_dir_all(backup) {
            Ok(()) => return,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return,
            Err(error) => last_error = Some(error),
        }
        thread::sleep(Duration::from_millis(50 * (attempt + 1)));
    }
    if let Some(error) = last_error {
        eprintln!(
            "[publish] warning: the new publication is active, but the old backup {} could not be removed: {error}",
            backup.display()
        );
    }
}

fn cleanup_after_error(staging: &Path, original: TaskError) -> TaskError {
    match fs::remove_dir_all(staging) {
        Ok(()) => original,
        Err(cleanup) => TaskError::new(format!(
            "{original}; additionally failed to remove staging directory {}: {cleanup}",
            staging.display()
        )),
    }
}

fn required_file(path: &Path, description: &str) -> TaskResult<PathBuf> {
    if !path.is_file() {
        return Err(TaskError::new(format!(
            "{description} is missing: {}",
            path.display()
        )));
    }
    path.canonicalize()
        .map_err(|error| io_error("canonicalize required file", path, error))
}

fn required_directory(path: &Path, description: &str) -> TaskResult<PathBuf> {
    if !path.is_dir() {
        return Err(TaskError::new(format!(
            "{description} is missing: {}",
            path.display()
        )));
    }
    path.canonicalize()
        .map_err(|error| io_error("canonicalize required directory", path, error))
}

fn require_x86_64(path: &Path, pe: &PeFile) -> TaskResult<()> {
    if pe.machine == X86_64_MACHINE {
        Ok(())
    } else {
        Err(TaskError::new(format!(
            "{} has PE machine {:#x}; expected x86_64",
            path.display(),
            pe.machine
        )))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognizes_ffmpeg_imports_without_fixed_abi_numbers() {
        assert!(is_ffmpeg_library("avcodec-61.dll"));
        assert!(is_ffmpeg_library("AVCODEC-63.DLL"));
        assert!(is_ffmpeg_library("swresample-7.dll"));
        assert!(!is_ffmpeg_library("ffmpeg.exe"));
        assert!(!is_ffmpeg_library("kernel32.dll"));
    }

    #[test]
    fn separates_windows_components_from_redistributable_runtimes() {
        assert!(is_system_library("DWrite.dll"));
        assert!(is_system_library("UIAutomationCore.dll"));
        assert!(is_system_library("msvcrt.dll"));
        assert!(!is_system_library("MSVCP140.dll"));
        assert!(!is_system_library("VCRUNTIME140.dll"));
    }
}
