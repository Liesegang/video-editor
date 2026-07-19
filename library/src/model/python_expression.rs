//! Python expression evaluation without compile-time `libpython` linkage.
//!
//! The default application keeps one unbuffered Python worker process and
//! exchanges JSON lines with it. This preserves existing Python expressions
//! while allowing the Rust binary to build on machines where Xcode's Python
//! sysconfig points at a removed framework library.

use std::env;
use std::ffi::OsString;
use std::io::{BufRead, BufReader, Write};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::sync::{Mutex, OnceLock};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::error::LibraryError;

const RESPONSE_TIMEOUT: Duration = Duration::from_secs(2);
const WORKER_SCRIPT: &str = r#"
import contextlib
import io
import json
import math
import random
import sys

expression_globals = {
    "__builtins__": __builtins__,
    "math": math,
    "random": random,
}

for request_line in sys.stdin:
    try:
        request = json.loads(request_line)
        with contextlib.redirect_stdout(io.StringIO()), contextlib.redirect_stderr(io.StringIO()):
            result = eval(request["expression"], expression_globals, {"t": request["t"]})
        value = float(result)
        if not math.isfinite(value):
            raise ValueError("expression result must be finite")
        response = {"value": value}
    except Exception as error:
        response = {"error": f"{type(error).__name__}: {error}"}
    sys.stdout.write(json.dumps(response, separators=(",", ":")) + "\n")
    sys.stdout.flush()
"#;

#[derive(Serialize)]
struct Request<'a> {
    expression: &'a str,
    t: f64,
}

#[derive(Deserialize)]
struct Response {
    value: Option<f64>,
    error: Option<String>,
}

#[derive(Debug)]
enum WorkerError {
    Expression(String),
    Transport(String),
}

impl WorkerError {
    fn transport(message: impl Into<String>) -> Self {
        Self::Transport(message.into())
    }

    fn into_library_error(self) -> LibraryError {
        match self {
            Self::Expression(message) => {
                LibraryError::Runtime(format!("Python easing expression failed: {message}"))
            }
            Self::Transport(message) => {
                LibraryError::Runtime(format!("Python easing worker failed: {message}"))
            }
        }
    }
}

struct PythonWorker {
    child: Child,
    stdin: Option<ChildStdin>,
    responses: Receiver<Result<String, String>>,
    reader_thread: Option<JoinHandle<()>>,
    response_timeout: Duration,
}

fn stop_child(child: &mut Child, context: &str) {
    if let Err(error) = child.kill()
        && error.kind() != std::io::ErrorKind::InvalidInput
    {
        log::debug!("Failed to stop Python expression worker {context}: {error}");
    }
    if let Err(error) = child.wait() {
        log::debug!("Failed to reap Python expression worker {context}: {error}");
    }
}

impl PythonWorker {
    fn spawn() -> Result<Self, WorkerError> {
        let configured = env::var_os("VIDEO_EDITOR_PYTHON");
        let candidates = configured
            .into_iter()
            .chain([OsString::from("python3"), OsString::from("python")])
            .collect();
        Self::spawn_with_candidates(candidates, WORKER_SCRIPT, RESPONSE_TIMEOUT)
    }

    fn spawn_with_candidates(
        candidates: Vec<OsString>,
        script: &str,
        response_timeout: Duration,
    ) -> Result<Self, WorkerError> {
        let mut errors = Vec::new();
        for executable in candidates {
            let mut command = Command::new(&executable);
            command
                .arg("-u")
                .arg("-c")
                .arg(script)
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                // Never pipe user-controlled stderr without draining it: a
                // noisy expression worker must not fill a pipe and deadlock.
                .stderr(Stdio::null());
            match command.spawn() {
                Ok(mut child) => {
                    let Some(stdin) = child.stdin.take() else {
                        stop_child(&mut child, "without stdin");
                        return Err(WorkerError::transport(
                            "Python expression worker has no stdin pipe",
                        ));
                    };
                    let Some(stdout) = child.stdout.take() else {
                        stop_child(&mut child, "without stdout");
                        return Err(WorkerError::transport(
                            "Python expression worker has no stdout pipe",
                        ));
                    };
                    let (sender, responses) = mpsc::channel();
                    let reader_thread = thread::Builder::new()
                        .name("python-expression-reader".to_string())
                        .spawn(move || {
                            let mut reader = BufReader::new(stdout);
                            loop {
                                let mut line = String::new();
                                match reader.read_line(&mut line) {
                                    Ok(0) => break,
                                    Ok(_) => {
                                        if sender.send(Ok(line)).is_err() {
                                            break;
                                        }
                                    }
                                    Err(error) => {
                                        drop(sender.send(Err(error.to_string())));
                                        break;
                                    }
                                }
                            }
                        })
                        .map_err(|error| {
                            if let Err(kill_error) = child.kill() {
                                log::debug!(
                                    "Failed to stop worker after reader spawn error: {kill_error}"
                                );
                            }
                            if let Err(wait_error) = child.wait() {
                                log::debug!(
                                    "Failed to reap worker after reader spawn error: {wait_error}"
                                );
                            }
                            WorkerError::transport(format!(
                                "cannot start Python response reader: {error}"
                            ))
                        })?;
                    return Ok(Self {
                        child,
                        stdin: Some(stdin),
                        responses,
                        reader_thread: Some(reader_thread),
                        response_timeout,
                    });
                }
                Err(error) => errors.push(format!("{executable:?}: {error}")),
            }
        }
        Err(WorkerError::transport(format!(
            "cannot start Python expression worker ({})",
            errors.join("; ")
        )))
    }

    fn evaluate(&mut self, expression: &str, t: f64) -> Result<f64, WorkerError> {
        let stdin = self
            .stdin
            .as_mut()
            .ok_or_else(|| WorkerError::transport("Python expression worker stdin is closed"))?;
        serde_json::to_writer(&mut *stdin, &Request { expression, t }).map_err(|error| {
            WorkerError::transport(format!("cannot encode Python expression request: {error}"))
        })?;
        stdin
            .write_all(b"\n")
            .and_then(|()| stdin.flush())
            .map_err(|error| {
                WorkerError::transport(format!("cannot write Python expression request: {error}"))
            })?;

        let line = match self.responses.recv_timeout(self.response_timeout) {
            Ok(Ok(line)) => line,
            Ok(Err(error)) => {
                return Err(WorkerError::transport(format!(
                    "cannot read Python expression response: {error}"
                )));
            }
            Err(RecvTimeoutError::Timeout) => {
                return Err(WorkerError::transport(format!(
                    "expression exceeded the {:?} response timeout",
                    self.response_timeout
                )));
            }
            Err(RecvTimeoutError::Disconnected) => {
                return Err(WorkerError::transport(
                    "Python expression worker exited without a response",
                ));
            }
        };
        let response: Response = serde_json::from_str(&line).map_err(|error| {
            WorkerError::transport(format!("invalid Python expression response: {error}"))
        })?;
        match (response.value, response.error) {
            (Some(value), _) if value.is_finite() => Ok(value),
            (Some(_), _) => Err(WorkerError::Expression(
                "expression result must be finite".to_string(),
            )),
            (None, Some(error)) => Err(WorkerError::Expression(error)),
            (None, None) => Err(WorkerError::transport(
                "Python expression response has no value",
            )),
        }
    }
}

impl Drop for PythonWorker {
    fn drop(&mut self) {
        drop(self.stdin.take());
        stop_child(&mut self.child, "during shutdown");
        if let Some(reader_thread) = self.reader_thread.take()
            && reader_thread.join().is_err()
        {
            log::debug!("Python expression reader thread panicked during shutdown");
        }
    }
}

static WORKER: OnceLock<Mutex<Option<PythonWorker>>> = OnceLock::new();

fn evaluate_with_worker(
    worker: &mut Option<PythonWorker>,
    expression: &str,
    t: f64,
    mut spawn_worker: impl FnMut() -> Result<PythonWorker, WorkerError>,
) -> Result<f64, WorkerError> {
    let mut last_transport_error = None;

    for _ in 0..2 {
        if worker.is_none() {
            *worker = Some(spawn_worker()?);
        }
        let result = worker
            .as_mut()
            .ok_or_else(|| WorkerError::transport("worker was not initialized"))?
            .evaluate(expression, t);
        match result {
            Ok(value) => return Ok(value),
            Err(error @ WorkerError::Expression(_)) => return Err(error),
            Err(WorkerError::Transport(error)) => {
                last_transport_error = Some(error);
                *worker = None;
            }
        }
    }
    Err(WorkerError::transport(last_transport_error.unwrap_or_else(
        || "worker failed without a reason".to_string(),
    )))
}

pub(crate) fn evaluate(expression: &str, t: f64) -> Result<f64, LibraryError> {
    let worker = WORKER.get_or_init(|| Mutex::new(None));
    let mut worker = worker
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    evaluate_with_worker(&mut worker, expression, t, PythonWorker::spawn)
        .map_err(WorkerError::into_library_error)
}

#[cfg(test)]
mod tests {
    use std::ffi::OsString;
    use std::process::{Command, Stdio};
    use std::time::{Duration, Instant};

    use super::{PythonWorker, WORKER_SCRIPT, WorkerError, evaluate_with_worker};

    fn candidates() -> Vec<OsString> {
        vec![OsString::from("python3"), OsString::from("python")]
    }

    #[test]
    fn missing_python_is_an_explicit_transport_error() {
        let result = PythonWorker::spawn_with_candidates(
            vec![OsString::from("/definitely/missing/video-editor-python")],
            "",
            Duration::from_millis(50),
        );
        assert!(matches!(result, Err(WorkerError::Transport(_))));
    }

    #[test]
    fn invalid_json_and_worker_crash_are_explicit_transport_errors() {
        let invalid_script = "import sys\nfor line in sys.stdin:\n print('not-json', flush=True)";
        let mut invalid = PythonWorker::spawn_with_candidates(
            candidates(),
            invalid_script,
            Duration::from_millis(250),
        )
        .unwrap();
        assert!(matches!(
            invalid.evaluate("t", 0.5),
            Err(WorkerError::Transport(_))
        ));

        let mut crashed = PythonWorker::spawn_with_candidates(
            candidates(),
            "import sys\nsys.exit(7)",
            Duration::from_millis(250),
        )
        .unwrap();
        assert!(matches!(
            crashed.evaluate("t", 0.5),
            Err(WorkerError::Transport(_))
        ));
    }

    #[test]
    fn hanging_expression_times_out_and_drop_reaps_the_process() {
        let mut worker = PythonWorker::spawn_with_candidates(
            candidates(),
            "import sys, time\nfor line in sys.stdin:\n time.sleep(60)",
            Duration::from_millis(50),
        )
        .unwrap();
        let pid = worker.child.id();
        let started = Instant::now();
        assert!(matches!(
            worker.evaluate("t", 0.5),
            Err(WorkerError::Transport(_))
        ));
        assert!(started.elapsed() < Duration::from_secs(1));
        drop(worker);

        #[cfg(unix)]
        {
            let status = Command::new("kill")
                .args(["-0", &pid.to_string()])
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status()
                .unwrap();
            assert!(!status.success(), "worker process {pid} was not reaped");
        }
    }

    #[test]
    fn noisy_stderr_cannot_block_a_valid_response() {
        let script = "import sys\nfor line in sys.stdin:\n sys.stderr.write('x' * 1000000)\n sys.stderr.flush()\n print('{\"value\":1.25}', flush=True)";
        let mut worker =
            PythonWorker::spawn_with_candidates(candidates(), script, Duration::from_secs(1))
                .unwrap();
        assert_eq!(worker.evaluate("t", 0.5).unwrap(), 1.25);
    }

    #[test]
    fn expression_errors_keep_the_worker_usable() {
        let mut worker = PythonWorker::spawn_with_candidates(
            candidates(),
            WORKER_SCRIPT,
            Duration::from_millis(250),
        )
        .unwrap();
        assert!(matches!(
            worker.evaluate("missing_name", 0.5),
            Err(WorkerError::Expression(_))
        ));
        assert_eq!(worker.evaluate("t * 2", 0.5).unwrap(), 1.0);
    }

    #[test]
    fn transport_failure_restarts_once_and_retries_the_request() {
        let mut worker = None;
        let mut spawn_count = 0;
        let result = evaluate_with_worker(&mut worker, "t * t", 0.5, || {
            spawn_count += 1;
            let script = if spawn_count == 1 {
                "import sys\nfor line in sys.stdin:\n sys.exit(7)"
            } else {
                WORKER_SCRIPT
            };
            PythonWorker::spawn_with_candidates(candidates(), script, Duration::from_millis(250))
        });

        assert_eq!(result.unwrap(), 0.25);
        assert_eq!(spawn_count, 2);
    }
}
