use std::ffi::OsString;
use std::path::{Path, PathBuf};

use crate::{Diagnostic, DiagnosticKind};

const RUNTIME_DIRECTORY: &str = "python";

pub(crate) fn resolve_python_home(explicit: Option<PathBuf>) -> Result<PathBuf, Diagnostic> {
    let executable = std::env::current_exe().ok();
    let environment = std::env::var_os("RUVIE_PYTHON_HOME").filter(|value| !value.is_empty());
    resolve_python_home_from(explicit, executable.as_deref(), environment)
}

fn resolve_python_home_from(
    explicit: Option<PathBuf>,
    executable: Option<&Path>,
    environment: Option<OsString>,
) -> Result<PathBuf, Diagnostic> {
    if let Some(home) = explicit {
        return validate_home(home, "configured CPython runtime");
    }

    if let Some(executable_directory) = executable.and_then(Path::parent) {
        let bundled = executable_directory.join(RUNTIME_DIRECTORY);
        if bundled.exists() {
            return validate_home(bundled, "bundled CPython runtime");
        }
    }

    if let Some(home) = environment {
        return validate_home(PathBuf::from(home), "RUVIE_PYTHON_HOME");
    }

    let guidance = if cfg!(windows) {
        "Bundled CPython 3.13.14 was not found beside the application. Run `cargo run -p xtask -- bootstrap` once and rebuild, or use a published RuViE directory."
    } else {
        "Bundled CPython 3.13.14 was not found beside the application. Run through scripts/with-managed-python.sh or use a packaged RuViE application."
    };
    Err(invalid_context(guidance))
}

fn validate_home(home: PathBuf, source: &str) -> Result<PathBuf, Diagnostic> {
    if !home.is_dir() {
        return Err(invalid_context(format!(
            "{source} '{}' is not a directory",
            home.display()
        )));
    }
    if !contains_standard_library(&home) {
        return Err(invalid_context(format!(
            "{source} '{}' does not contain the CPython 3.13 standard library",
            home.display()
        )));
    }
    Ok(home)
}

fn contains_standard_library(home: &Path) -> bool {
    // uv's Windows distribution uses `Lib`; Unix distributions use
    // `lib/python3.13`. Recognizing both layouts keeps package validation
    // independent of the machine performing the build.
    home.join("Lib/encodings/__init__.py").is_file()
        || home.join("lib/python3.13/encodings/__init__.py").is_file()
}

fn invalid_context(message: impl Into<String>) -> Diagnostic {
    Diagnostic::compile(DiagnosticKind::InvalidContext, message.into(), None, None)
}

#[cfg(test)]
mod tests {
    use std::fs;

    use tempfile::tempdir;

    use super::*;

    fn runtime_at(root: &Path, windows_layout: bool) -> Result<PathBuf, std::io::Error> {
        let home = root.join("python");
        let encodings = if windows_layout {
            home.join("Lib/encodings")
        } else {
            home.join("lib/python3.13/encodings")
        };
        fs::create_dir_all(&encodings)?;
        fs::write(encodings.join("__init__.py"), "")?;
        Ok(home)
    }

    #[test]
    fn bundled_runtime_precedes_environment_override() -> Result<(), Box<dyn std::error::Error>> {
        let temporary = tempdir()?;
        let executable_directory = temporary.path().join("app");
        fs::create_dir_all(&executable_directory)?;
        let bundled = runtime_at(&executable_directory, true)?;
        let environment = runtime_at(&temporary.path().join("environment"), false)?;

        assert_eq!(
            resolve_python_home_from(
                None,
                Some(&executable_directory.join("app.exe")),
                Some(environment.into_os_string()),
            )?,
            bundled
        );
        Ok(())
    }

    #[test]
    fn explicit_runtime_precedes_bundle() -> Result<(), Box<dyn std::error::Error>> {
        let temporary = tempdir()?;
        let executable_directory = temporary.path().join("app");
        fs::create_dir_all(&executable_directory)?;
        drop(runtime_at(&executable_directory, true)?);
        let explicit = runtime_at(&temporary.path().join("explicit"), false)?;

        assert_eq!(
            resolve_python_home_from(
                Some(explicit.clone()),
                Some(&executable_directory.join("app.exe")),
                None,
            )?,
            explicit
        );
        Ok(())
    }

    #[test]
    fn rejects_a_runtime_without_standard_library() -> Result<(), Box<dyn std::error::Error>> {
        let temporary = tempdir()?;
        let invalid = temporary.path().join("empty");
        fs::create_dir_all(&invalid)?;

        let error = resolve_python_home_from(Some(invalid), None, None)
            .expect_err("an empty runtime must be rejected");
        assert!(error.message.contains("standard library"));
        Ok(())
    }
}
