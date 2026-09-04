mod bootstrap;
mod pe;
mod publish;

use std::env;
use std::ffi::OsString;
use std::fmt;
use std::io;
use std::path::{Path, PathBuf};

const PYTHON_VERSION: &str = "3.13.14";
const PYTHON_DLL: &str = "python313.dll";
const X86_64_MACHINE: u16 = 0x8664;

type TaskResult<T> = Result<T, TaskError>;

#[derive(Debug)]
struct TaskError(String);

impl TaskError {
    fn new(message: impl Into<String>) -> Self {
        Self(message.into())
    }
}

impl fmt::Display for TaskError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl std::error::Error for TaskError {}

#[derive(Debug, PartialEq, Eq)]
enum TaskCommand {
    Bootstrap,
    Publish(PublishOptions),
    Help,
}

#[derive(Debug, Default, PartialEq, Eq)]
struct PublishOptions {
    skip_build: bool,
    output: Option<PathBuf>,
}

fn main() -> TaskResult<()> {
    let command = parse_arguments(env::args_os().skip(1))?;
    let repository = repository_root()?;
    match command {
        TaskCommand::Bootstrap => bootstrap::run(&repository),
        TaskCommand::Publish(options) => publish::run(&repository, &options),
        TaskCommand::Help => {
            print_usage();
            Ok(())
        }
    }
}

fn parse_arguments(arguments: impl IntoIterator<Item = OsString>) -> TaskResult<TaskCommand> {
    let mut arguments = arguments.into_iter();
    let Some(command) = arguments.next() else {
        return Ok(TaskCommand::Help);
    };
    if command == "--help" || command == "-h" || command == "help" {
        return Ok(TaskCommand::Help);
    }
    if command == "bootstrap" {
        if let Some(extra) = arguments.next() {
            return Err(TaskError::new(format!(
                "bootstrap does not accept argument '{}'",
                extra.to_string_lossy()
            )));
        }
        return Ok(TaskCommand::Bootstrap);
    }
    if command != "publish" {
        return Err(TaskError::new(format!(
            "unknown xtask command '{}'; expected bootstrap or publish",
            command.to_string_lossy()
        )));
    }

    let mut options = PublishOptions::default();
    while let Some(argument) = arguments.next() {
        if argument == "--skip-build" {
            if options.skip_build {
                return Err(TaskError::new("--skip-build was specified more than once"));
            }
            options.skip_build = true;
        } else if argument == "--output" {
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
        } else if argument == "--help" || argument == "-h" {
            return Ok(TaskCommand::Help);
        } else {
            return Err(TaskError::new(format!(
                "unknown publish argument '{}'",
                argument.to_string_lossy()
            )));
        }
    }
    Ok(TaskCommand::Publish(options))
}

fn print_usage() {
    println!("cargo xtask bootstrap");
    println!("cargo xtask publish [--skip-build] [--output PATH]");
}

fn repository_root() -> TaskResult<PathBuf> {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .map(Path::to_path_buf)
        .ok_or_else(|| TaskError::new("xtask has no repository parent directory"))
}

fn require_windows(operation: &str) -> TaskResult<()> {
    if cfg!(target_os = "windows") {
        Ok(())
    } else {
        Err(TaskError::new(format!(
            "xtask {operation} is supported only on Windows"
        )))
    }
}

fn io_error(operation: &str, path: &Path, error: io::Error) -> TaskError {
    TaskError::new(format!("cannot {operation} {}: {error}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_publish_options() -> TaskResult<()> {
        let parsed = parse_arguments([
            OsString::from("publish"),
            OsString::from("--skip-build"),
            OsString::from("--output"),
            OsString::from("out/RuViE"),
        ])?;
        assert_eq!(
            parsed,
            TaskCommand::Publish(PublishOptions {
                skip_build: true,
                output: Some(PathBuf::from("out/RuViE")),
            })
        );
        Ok(())
    }

    #[test]
    fn rejects_duplicate_publish_options() {
        let result = parse_arguments([
            OsString::from("publish"),
            OsString::from("--skip-build"),
            OsString::from("--skip-build"),
        ]);
        assert!(result.is_err());
    }
}
