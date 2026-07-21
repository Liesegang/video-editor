use std::ffi::CString;
use std::num::NonZeroUsize;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, OnceLock};

use lru::LruCache;
use pyo3::exceptions::{PyNameError, PySyntaxError, PyTypeError, PyZeroDivisionError};
use pyo3::prelude::*;
use pyo3::types::{
    PyAnyMethods, PyBool, PyCode, PyCodeInput, PyCodeMethods, PyDict, PyDictMethods, PyFloat,
    PyInt, PyList, PyListMethods, PyModule, PySequence, PySequenceMethods, PyString,
};
use sha2::{Digest, Sha256};

use crate::{
    Diagnostic, DiagnosticKind, EvaluationContext, OutputType, Phase, PythonValue, SourceSpan,
};

const CACHE_CONTRACT: &[u8] = b"ruvie-cpython-3.13-expression-v1";
const DEFAULT_CACHE_CAPACITY: usize = 256;
const EXPRESSION_FILENAME: &str = "<ruvie-expression>";
const MODULE_FILENAME: &str = "<ruvie-script>";
const HELPERS_SOURCE: &str = include_str!("helpers.py");

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum CodeMode {
    Expression,
    Module,
}

#[derive(Clone, Debug)]
pub struct PythonHostConfig {
    pub cache_capacity: usize,
    /// CPython prefix containing `lib/python3.13`. If absent, the runtime
    /// requires `RUVIE_PYTHON_HOME`; it never searches the system PATH.
    pub python_home: Option<PathBuf>,
    pub extra_site_package_paths: Vec<PathBuf>,
}

impl Default for PythonHostConfig {
    fn default() -> Self {
        Self {
            cache_capacity: DEFAULT_CACHE_CAPACITY,
            python_home: None,
            extra_site_package_paths: Vec::new(),
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[doc(hidden)]
pub struct CacheStats {
    pub hits: u64,
    pub compilations: u64,
}

#[derive(Clone)]
pub struct CompiledCode {
    inner: Arc<CompiledCodeInner>,
}

impl std::fmt::Debug for CompiledCode {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("CompiledCode")
            .field("mode", &self.inner.mode)
            .field("source", &self.inner.source)
            .finish_non_exhaustive()
    }
}

struct CompiledCodeInner {
    code: Py<PyCode>,
    mode: CodeMode,
    source: Arc<str>,
}

type CacheKey = [u8; 32];
type CachedCompilation = Result<CompiledCode, Diagnostic>;
type CacheEntry = Arc<OnceLock<CachedCompilation>>;

struct PythonHostInner {
    cache: Mutex<LruCache<CacheKey, CacheEntry>>,
    helper_globals: Py<PyDict>,
    extra_site_package_paths: Vec<PathBuf>,
    cache_hits: AtomicU64,
    compilations: AtomicU64,
}

/// A standard-GIL CPython host. Clones share compiled code and configuration.
#[derive(Clone)]
pub struct PythonHost {
    inner: Arc<PythonHostInner>,
}

static GLOBAL_HOST: OnceLock<Result<PythonHost, Diagnostic>> = OnceLock::new();
static CPYTHON_HOME: OnceLock<Result<PathBuf, Diagnostic>> = OnceLock::new();

pub fn initialize_global(config: PythonHostConfig) -> Result<&'static PythonHost, Diagnostic> {
    GLOBAL_HOST
        .get_or_init(|| PythonHost::new(config))
        .as_ref()
        .map_err(Clone::clone)
}

pub fn global_host() -> Result<&'static PythonHost, Diagnostic> {
    initialize_global(PythonHostConfig::default())
}

impl PythonHost {
    /// Explicitly initializes CPython once, then constructs a host cache.
    pub fn new(config: PythonHostConfig) -> Result<Self, Diagnostic> {
        let python_home = config.python_home.clone().or_else(|| {
            std::env::var_os("RUVIE_PYTHON_HOME")
                .filter(|value| !value.is_empty())
                .map(PathBuf::from)
        });
        let python_home = python_home.ok_or_else(|| {
            Diagnostic::compile(
                DiagnosticKind::InvalidContext,
                "RUVIE_PYTHON_HOME is required; run through scripts/with-managed-python.sh",
                None,
                None,
            )
        })?;
        initialize_cpython(&python_home)?;
        let capacity = NonZeroUsize::new(config.cache_capacity.max(1)).unwrap_or(NonZeroUsize::MIN);
        let helper_globals = Python::attach(build_helper_globals)?;
        let host = Self {
            inner: Arc::new(PythonHostInner {
                cache: Mutex::new(LruCache::new(capacity)),
                helper_globals,
                extra_site_package_paths: config.extra_site_package_paths,
                cache_hits: AtomicU64::new(0),
                compilations: AtomicU64::new(0),
            }),
        };
        Python::attach(verify_interpreter)?;
        Ok(host)
    }

    pub fn compile_expression(&self, source: &str) -> Result<CompiledCode, Diagnostic> {
        self.compile(source, CodeMode::Expression)
    }

    /// Compiles a statement/module body for future automation and plugin use.
    pub fn compile_module(&self, source: &str) -> Result<CompiledCode, Diagnostic> {
        self.compile(source, CodeMode::Module)
    }

    pub fn evaluate(
        &self,
        source: &str,
        context: &EvaluationContext,
        output_type: OutputType,
    ) -> Result<PythonValue, Diagnostic> {
        let code = self.compile_expression(source)?;
        self.evaluate_compiled(&code, context, output_type)
    }

    pub fn evaluate_compiled(
        &self,
        compiled: &CompiledCode,
        context: &EvaluationContext,
        output_type: OutputType,
    ) -> Result<PythonValue, Diagnostic> {
        if compiled.inner.mode != CodeMode::Expression {
            return Err(Diagnostic::evaluate(
                DiagnosticKind::InvalidContext,
                "module code cannot be evaluated as an Expression",
                None,
            ));
        }
        validate_context_value(context, output_type)?;
        Python::attach(|py| {
            self.with_site_paths(py, || {
                let globals = self.expression_globals(py, context)?;
                let result = compiled
                    .inner
                    .code
                    .bind(py)
                    .run(Some(&globals), Some(&globals))
                    .map_err(|error| {
                        diagnostic_from_pyerr(py, &error, &compiled.inner.source, Phase::Evaluate)
                    })?;
                extract_value(&result, output_type)
            })
        })
    }

    /// Executes trusted Python statements with ordinary builtins and imports.
    /// Host API registration is intentionally a later slice.
    pub fn execute_module(&self, source: &str) -> Result<(), Diagnostic> {
        let compiled = self.compile_module(source)?;
        Python::attach(|py| {
            self.with_site_paths(py, || {
                let globals = self.base_globals(py)?;
                compiled
                    .inner
                    .code
                    .bind(py)
                    .run(Some(&globals), Some(&globals))
                    .map_err(|error| diagnostic_from_pyerr(py, &error, source, Phase::Evaluate))?;
                Ok(())
            })
        })
    }

    #[doc(hidden)]
    pub fn cache_stats(&self) -> CacheStats {
        CacheStats {
            hits: self.inner.cache_hits.load(Ordering::Relaxed),
            compilations: self.inner.compilations.load(Ordering::Relaxed),
        }
    }

    fn compile(&self, source: &str, mode: CodeMode) -> Result<CompiledCode, Diagnostic> {
        if source.trim().is_empty() {
            return Err(Diagnostic::compile(
                DiagnosticKind::Parse,
                "Python source is empty",
                None,
                None,
            ));
        }
        let key = cache_key(source, mode);
        let (entry, cache_hit) = {
            let mut cache = self.cache_guard();
            if let Some(entry) = cache.get(&key).cloned() {
                (entry, true)
            } else {
                let entry = Arc::new(OnceLock::new());
                drop(cache.put(key, Arc::clone(&entry)));
                (entry, false)
            }
        };
        if cache_hit {
            self.inner.cache_hits.fetch_add(1, Ordering::Relaxed);
        }
        entry
            .get_or_init(|| {
                self.inner.compilations.fetch_add(1, Ordering::Relaxed);
                Python::attach(|py| compile_uncached(py, source, mode))
            })
            .clone()
    }

    fn base_globals<'py>(&self, py: Python<'py>) -> Result<Bound<'py, PyDict>, Diagnostic> {
        self.inner
            .helper_globals
            .bind(py)
            .copy()
            .map_err(|error| diagnostic_from_pyerr(py, &error, HELPERS_SOURCE, Phase::Evaluate))
    }

    fn expression_globals<'py>(
        &self,
        py: Python<'py>,
        context: &EvaluationContext,
    ) -> Result<Bound<'py, PyDict>, Diagnostic> {
        let globals = self.base_globals(py)?;
        globals
            .set_item("time", context.time())
            .and_then(|()| globals.set_item("t", context.time()))
            .and_then(|()| globals.set_item("fps", context.fps()))
            .and_then(|()| globals.set_item("frame", context.frame()))
            .and_then(|()| globals.set_item("frame_index", context.frame()))
            .and_then(|()| globals.set_item("width", context.width()))
            .and_then(|()| globals.set_item("height", context.height()))
            .map_err(|error| diagnostic_from_pyerr(py, &error, "", Phase::Evaluate))?;
        let vector_type = globals
            .get_item("_RuvieVector")
            .map_err(|error| diagnostic_from_pyerr(py, &error, "", Phase::Evaluate))?
            .ok_or_else(|| {
                Diagnostic::runtime("Python helper vector type is missing", None, None)
            })?;
        let resolution = vector_type
            .call1((vec![context.width() as f64, context.height() as f64],))
            .map_err(|error| diagnostic_from_pyerr(py, &error, "", Phase::Evaluate))?;
        globals
            .set_item("resolution", resolution)
            .map_err(|error| diagnostic_from_pyerr(py, &error, "", Phase::Evaluate))?;
        if let Some(value) = context.value() {
            globals
                .set_item("value", value_to_python(py, value, &globals)?)
                .map_err(|error| diagnostic_from_pyerr(py, &error, "", Phase::Evaluate))?;
        }
        Ok(globals)
    }

    fn with_site_paths<T>(
        &self,
        py: Python<'_>,
        operation: impl FnOnce() -> Result<T, Diagnostic>,
    ) -> Result<T, Diagnostic> {
        if self.inner.extra_site_package_paths.is_empty() {
            return operation();
        }
        let sys = PyModule::import(py, "sys")
            .map_err(|error| diagnostic_from_pyerr(py, &error, "", Phase::Evaluate))?;
        let path_value = sys
            .getattr("path")
            .map_err(|error| diagnostic_from_pyerr(py, &error, "", Phase::Evaluate))?;
        let original_path = path_value
            .cast::<PyList>()
            .map_err(|error| Diagnostic::runtime(error.to_string(), None, None))?;
        let original = original_path
            .call_method0("copy")
            .and_then(|value| value.cast_into::<PyList>().map_err(Into::into))
            .map_err(|error| diagnostic_from_pyerr(py, &error, "", Phase::Evaluate))?;
        let configured = original
            .call_method0("copy")
            .and_then(|value| value.cast_into::<PyList>().map_err(Into::into))
            .map_err(|error| diagnostic_from_pyerr(py, &error, "", Phase::Evaluate))?;
        for extra in self.inner.extra_site_package_paths.iter().rev() {
            let rendered = extra.to_string_lossy();
            let already_present = configured
                .iter()
                .filter_map(|item| item.extract::<String>().ok())
                .any(|item| item == rendered);
            if !already_present {
                configured
                    .insert(0, rendered.as_ref())
                    .map_err(|error| diagnostic_from_pyerr(py, &error, "", Phase::Evaluate))?;
            }
        }
        sys.setattr("path", configured)
            .map_err(|error| diagnostic_from_pyerr(py, &error, "", Phase::Evaluate))?;
        let result = operation();
        let restore = sys
            .setattr("path", original)
            .map_err(|error| diagnostic_from_pyerr(py, &error, "", Phase::Evaluate));
        restore?;
        result
    }

    fn cache_guard(&self) -> MutexGuard<'_, LruCache<CacheKey, CacheEntry>> {
        self.inner
            .cache
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }
}

fn build_helper_globals(py: Python<'_>) -> Result<Py<PyDict>, Diagnostic> {
    let globals = PyDict::new(py);
    py.run(
        &CString::new(HELPERS_SOURCE).map_err(|_| nul_source_diagnostic(Phase::Compile))?,
        Some(&globals),
        Some(&globals),
    )
    .map_err(|error| diagnostic_from_pyerr(py, &error, HELPERS_SOURCE, Phase::Evaluate))?;
    Ok(globals.unbind())
}

fn initialize_cpython(requested_home: &Path) -> Result<(), Diagnostic> {
    let canonical_home = requested_home.canonicalize().map_err(|error| {
        Diagnostic::compile(
            DiagnosticKind::InvalidContext,
            format!(
                "RUVIE_PYTHON_HOME '{}' is unavailable: {error}",
                requested_home.display()
            ),
            None,
            None,
        )
    })?;
    CPYTHON_HOME
        .get_or_init(|| {
            initialize_cpython_once(canonical_home.clone()).map(|()| canonical_home.clone())
        })
        .as_ref()
        .map(|initialized_home| {
            if initialized_home == &canonical_home {
                Ok(())
            } else {
                Err(Diagnostic::compile(
                    DiagnosticKind::InvalidContext,
                    format!(
                        "CPython is already initialized from '{}', not '{}'",
                        initialized_home.display(),
                        canonical_home.display()
                    ),
                    None,
                    None,
                ))
            }
        })
        .map_err(Clone::clone)?
}

fn initialize_cpython_once(home: PathBuf) -> Result<(), Diagnostic> {
    let home = CString::new(home.to_string_lossy().as_bytes()).map_err(|_| {
        Diagnostic::compile(
            DiagnosticKind::InvalidContext,
            "RUVIE_PYTHON_HOME cannot contain a NUL byte",
            None,
            None,
        )
    })?;
    // SAFETY: initialization is serialized by `CPYTHON_HOME`; no Python API is
    // called before this block, and the config is cleared on every exit path.
    unsafe {
        if pyo3::ffi::Py_IsInitialized() != 0 {
            return Err(Diagnostic::compile(
                DiagnosticKind::InvalidContext,
                "CPython was initialized before the RuViE runtime configured it",
                None,
                None,
            ));
        }
        let mut config = std::mem::MaybeUninit::<pyo3::ffi::PyConfig>::uninit();
        pyo3::ffi::PyConfig_InitIsolatedConfig(config.as_mut_ptr());
        let mut config = config.assume_init();
        config.install_signal_handlers = 0;
        config.parse_argv = 0;
        config.write_bytecode = 0;
        config.site_import = 1;
        let status =
            pyo3::ffi::PyConfig_SetBytesString(&mut config, &mut config.home, home.as_ptr());
        if pyo3::ffi::PyStatus_Exception(status) != 0 {
            let message = py_status_message(status);
            pyo3::ffi::PyConfig_Clear(&mut config);
            return Err(Diagnostic::compile(
                DiagnosticKind::Runtime,
                message,
                None,
                None,
            ));
        }
        let status = pyo3::ffi::Py_InitializeFromConfig(&config);
        pyo3::ffi::PyConfig_Clear(&mut config);
        if pyo3::ffi::PyStatus_Exception(status) != 0 {
            return Err(Diagnostic::compile(
                DiagnosticKind::Runtime,
                py_status_message(status),
                None,
                None,
            ));
        }
        // Release the main-thread GIL so `Python::attach` is safe from any
        // caller thread for the rest of the process lifetime.
        pyo3::ffi::PyEval_SaveThread();
    }
    Ok(())
}

unsafe fn py_status_message(status: pyo3::ffi::PyStatus) -> String {
    if status.err_msg.is_null() {
        return format!(
            "CPython initialization exited with status {}",
            status.exitcode
        );
    }
    // SAFETY: CPython owns `err_msg` for the lifetime of this status value.
    unsafe { std::ffi::CStr::from_ptr(status.err_msg) }
        .to_string_lossy()
        .into_owned()
}

fn verify_interpreter(py: Python<'_>) -> Result<(), Diagnostic> {
    let sys = PyModule::import(py, "sys")
        .map_err(|error| diagnostic_from_pyerr(py, &error, "", Phase::Evaluate))?;
    let implementation = sys
        .getattr("implementation")
        .and_then(|value| value.getattr("name"))
        .and_then(|value| value.extract::<String>())
        .map_err(|error| diagnostic_from_pyerr(py, &error, "", Phase::Evaluate))?;
    let version = sys
        .getattr("version_info")
        .and_then(|value| value.extract::<(u8, u8, u8, String, u8)>())
        .map_err(|error| diagnostic_from_pyerr(py, &error, "", Phase::Evaluate))?;
    if implementation != "cpython" || version.0 != 3 || version.1 != 13 || version.2 != 14 {
        return Err(Diagnostic::compile(
            DiagnosticKind::InvalidContext,
            format!(
                "RuViE requires standard CPython 3.13.14, found {implementation} {}.{}.{}",
                version.0, version.1, version.2
            ),
            None,
            None,
        ));
    }
    let gil_enabled = sys
        .getattr("_is_gil_enabled")
        .and_then(|function| function.call0())
        .and_then(|value| value.extract::<bool>())
        .map_err(|error| diagnostic_from_pyerr(py, &error, "", Phase::Evaluate))?;
    if !gil_enabled {
        return Err(Diagnostic::compile(
            DiagnosticKind::InvalidContext,
            "RuViE requires the standard-GIL CPython build",
            None,
            None,
        ));
    }
    Ok(())
}

fn compile_uncached(py: Python<'_>, source: &str, mode: CodeMode) -> CachedCompilation {
    let source_c = CString::new(source).map_err(|_| nul_source_diagnostic(Phase::Compile))?;
    let filename = match mode {
        CodeMode::Expression => EXPRESSION_FILENAME,
        CodeMode::Module => MODULE_FILENAME,
    };
    let filename_c = CString::new(filename).map_err(|_| nul_source_diagnostic(Phase::Compile))?;
    let input = match mode {
        CodeMode::Expression => PyCodeInput::Eval,
        CodeMode::Module => PyCodeInput::File,
    };
    let code = PyCode::compile(py, &source_c, &filename_c, input)
        .map_err(|error| diagnostic_from_pyerr(py, &error, source, Phase::Compile))?;
    Ok(CompiledCode {
        inner: Arc::new(CompiledCodeInner {
            code: code.unbind(),
            mode,
            source: Arc::from(source),
        }),
    })
}

fn cache_key(source: &str, mode: CodeMode) -> CacheKey {
    let mut hasher = Sha256::new();
    hasher.update(CACHE_CONTRACT);
    hasher.update([mode as u8]);
    hasher.update(source.as_bytes());
    hasher.finalize().into()
}

fn value_to_python<'py>(
    py: Python<'py>,
    value: &PythonValue,
    globals: &Bound<'py, PyDict>,
) -> Result<Bound<'py, PyAny>, Diagnostic> {
    match value {
        PythonValue::Number(value) => Ok(PyFloat::new(py, *value).into_any()),
        PythonValue::Integer(value) => Ok(PyInt::new(py, *value).into_any()),
        PythonValue::Vec2(value) => helper_vector(py, globals, value),
        PythonValue::Vec3(value) => helper_vector(py, globals, value),
        PythonValue::Vec4(value) | PythonValue::Color(value) => helper_vector(py, globals, value),
        PythonValue::Bool(value) => Ok(PyBool::new(py, *value).to_owned().into_any()),
        PythonValue::String(value) => Ok(PyString::new(py, value).into_any()),
    }
}

fn helper_vector<'py, const N: usize>(
    py: Python<'py>,
    globals: &Bound<'py, PyDict>,
    components: &[f64; N],
) -> Result<Bound<'py, PyAny>, Diagnostic> {
    let vector_type = globals
        .get_item("_RuvieVector")
        .map_err(|error| diagnostic_from_pyerr(py, &error, "", Phase::Evaluate))?
        .ok_or_else(|| Diagnostic::runtime("Python helper vector type is missing", None, None))?;
    vector_type
        .call1((components.to_vec(),))
        .map_err(|error| diagnostic_from_pyerr(py, &error, "", Phase::Evaluate))
}

fn extract_value(value: &Bound<'_, PyAny>, output: OutputType) -> Result<PythonValue, Diagnostic> {
    match output {
        OutputType::Number => {
            if value.is_instance_of::<pyo3::types::PyBool>() {
                return Err(type_mismatch("Number", value));
            }
            let number = value
                .extract::<f64>()
                .map_err(|_| type_mismatch("Number", value))?;
            finite(number).map(PythonValue::Number)
        }
        OutputType::Integer => {
            if value.is_instance_of::<pyo3::types::PyBool>() {
                return Err(type_mismatch("Integer", value));
            }
            value
                .extract::<i64>()
                .map(PythonValue::Integer)
                .map_err(|_| type_mismatch("Integer", value))
        }
        OutputType::Vec2 => extract_vector::<2>(value).map(PythonValue::Vec2),
        OutputType::Vec3 => extract_vector::<3>(value).map(PythonValue::Vec3),
        OutputType::Vec4 => extract_vector::<4>(value).map(PythonValue::Vec4),
        OutputType::Color => {
            let channels = extract_vector::<4>(value)?;
            if channels
                .iter()
                .any(|channel| !(0.0..=1.0).contains(channel))
            {
                return Err(Diagnostic::evaluate(
                    DiagnosticKind::TypeMismatch,
                    "Color channels must be in 0.0..=1.0",
                    None,
                ));
            }
            Ok(PythonValue::Color(channels))
        }
        OutputType::Bool => value
            .extract::<bool>()
            .map(PythonValue::Bool)
            .map_err(|_| type_mismatch("Bool", value)),
        OutputType::String => value
            .extract::<String>()
            .map(PythonValue::String)
            .map_err(|_| type_mismatch("String", value)),
    }
}

fn extract_vector<const N: usize>(value: &Bound<'_, PyAny>) -> Result<[f64; N], Diagnostic> {
    let sequence = value
        .cast::<PySequence>()
        .map_err(|_| type_mismatch(&format!("Vec{N}"), value))?;
    if sequence
        .len()
        .map_err(|_| type_mismatch(&format!("Vec{N}"), value))?
        != N
    {
        return Err(type_mismatch(&format!("Vec{N}"), value));
    }
    let mut result = [0.0; N];
    for (index, component) in result.iter_mut().enumerate() {
        let item = sequence
            .get_item(index)
            .map_err(|_| type_mismatch(&format!("Vec{N}"), value))?;
        if item.is_instance_of::<pyo3::types::PyBool>() {
            return Err(type_mismatch(&format!("Vec{N}"), value));
        }
        *component = finite(
            item.extract::<f64>()
                .map_err(|_| type_mismatch(&format!("Vec{N}"), value))?,
        )?;
    }
    Ok(result)
}

fn finite(value: f64) -> Result<f64, Diagnostic> {
    if value.is_finite() {
        Ok(value)
    } else {
        Err(Diagnostic::evaluate(
            DiagnosticKind::NonFinite,
            "Python result must be finite",
            None,
        ))
    }
}

fn validate_context_value(
    context: &EvaluationContext,
    output: OutputType,
) -> Result<(), Diagnostic> {
    if let Some(value) = context.value()
        && value.output_type() != output
    {
        return Err(Diagnostic::evaluate(
            DiagnosticKind::InvalidContext,
            "authored fallback type does not match requested output type",
            None,
        ));
    }
    Ok(())
}

fn type_mismatch(expected: &str, value: &Bound<'_, PyAny>) -> Diagnostic {
    let actual = value.get_type().name().map_or_else(
        |_| "unknown".into(),
        |name| name.to_string_lossy().into_owned(),
    );
    Diagnostic::evaluate(
        DiagnosticKind::TypeMismatch,
        format!("expected {expected}, got Python {actual}"),
        None,
    )
}

fn diagnostic_from_pyerr(py: Python<'_>, error: &PyErr, source: &str, phase: Phase) -> Diagnostic {
    let is_syntax = error.is_instance_of::<PySyntaxError>(py);
    let kind = if is_syntax {
        DiagnosticKind::Parse
    } else if error.is_instance_of::<PyZeroDivisionError>(py) {
        DiagnosticKind::DivisionByZero
    } else if error.is_instance_of::<PyNameError>(py) {
        DiagnosticKind::UnknownName
    } else if error.is_instance_of::<PyTypeError>(py) {
        DiagnosticKind::TypeMismatch
    } else {
        DiagnosticKind::Runtime
    };
    let span = if is_syntax {
        syntax_error_span(py, error, source)
    } else {
        traceback_span(py, error, source)
    };
    let traceback = format_traceback(py, error);
    let message = error.to_string();
    match phase {
        Phase::Compile => Diagnostic::compile(kind, message, traceback, span),
        Phase::Evaluate => {
            let mut diagnostic = Diagnostic::runtime(message, traceback, span);
            diagnostic.kind = kind;
            diagnostic
        }
    }
}

fn syntax_error_span(py: Python<'_>, error: &PyErr, source: &str) -> Option<SourceSpan> {
    let value = error.value(py);
    let line = value.getattr("lineno").ok()?.extract::<usize>().ok()?;
    let column = value.getattr("offset").ok()?.extract::<usize>().ok()?;
    let end_line = value
        .getattr("end_lineno")
        .ok()
        .and_then(|item| item.extract::<usize>().ok())
        .unwrap_or(line);
    let end_column = value
        .getattr("end_offset")
        .ok()
        .and_then(|item| item.extract::<usize>().ok())
        .unwrap_or(column.saturating_add(1));
    span_from_line_columns(
        source,
        line,
        column.saturating_sub(1),
        end_line,
        end_column.saturating_sub(1),
    )
}

fn traceback_span(py: Python<'_>, error: &PyErr, source: &str) -> Option<SourceSpan> {
    let traceback = error.traceback(py)?;
    let module = PyModule::import(py, "traceback").ok()?;
    let frames_value = module
        .getattr("extract_tb")
        .ok()?
        .call1((traceback,))
        .ok()?;
    let frames = frames_value.cast::<PyList>().ok()?;
    let frame = frames.get_item(frames.len().checked_sub(1)?).ok()?;
    let filename = frame.getattr("filename").ok()?.extract::<String>().ok()?;
    if filename != EXPRESSION_FILENAME && filename != MODULE_FILENAME {
        return None;
    }
    let line = frame.getattr("lineno").ok()?.extract::<usize>().ok()?;
    let end_line = frame
        .getattr("end_lineno")
        .ok()
        .and_then(|item| item.extract::<usize>().ok())
        .unwrap_or(line);
    let column = frame
        .getattr("colno")
        .ok()
        .and_then(|item| item.extract::<usize>().ok())
        .unwrap_or(0);
    let end_column = frame
        .getattr("end_colno")
        .ok()
        .and_then(|item| item.extract::<usize>().ok())
        .unwrap_or(column.saturating_add(1));
    span_from_utf8_columns(source, line, column, end_line, end_column)
}

fn span_from_line_columns(
    source: &str,
    line: usize,
    column: usize,
    end_line: usize,
    end_column: usize,
) -> Option<SourceSpan> {
    let start = line_column_to_byte(source, line, column)?;
    let end = line_column_to_byte(source, end_line, end_column).unwrap_or(start);
    Some(SourceSpan {
        start,
        end: end.max(start),
    })
}

fn span_from_utf8_columns(
    source: &str,
    line: usize,
    column: usize,
    end_line: usize,
    end_column: usize,
) -> Option<SourceSpan> {
    let start = line_utf8_column_to_byte(source, line, column)?;
    let end = line_utf8_column_to_byte(source, end_line, end_column).unwrap_or(start);
    Some(SourceSpan {
        start,
        end: end.max(start),
    })
}

fn line_column_to_byte(source: &str, line: usize, column: usize) -> Option<usize> {
    let mut offset = 0;
    let text = source.split_inclusive('\n').nth(line.checked_sub(1)?)?;
    for previous in source.split_inclusive('\n').take(line.saturating_sub(1)) {
        offset += previous.len();
    }
    let content = text.strip_suffix('\n').unwrap_or(text);
    let column_byte = content
        .char_indices()
        .nth(column)
        .map_or(content.len(), |(index, _)| index);
    Some(offset + column_byte)
}

fn line_utf8_column_to_byte(source: &str, line: usize, column: usize) -> Option<usize> {
    let mut offset = 0;
    let text = source.split_inclusive('\n').nth(line.checked_sub(1)?)?;
    for previous in source.split_inclusive('\n').take(line.saturating_sub(1)) {
        offset += previous.len();
    }
    let content = text.strip_suffix('\n').unwrap_or(text);
    let mut column = column.min(content.len());
    while column > 0 && !content.is_char_boundary(column) {
        column -= 1;
    }
    Some(offset + column)
}

fn format_traceback(py: Python<'_>, error: &PyErr) -> Option<String> {
    let module = PyModule::import(py, "traceback").ok()?;
    let formatted = module
        .getattr("format_exception")
        .ok()?
        .call1((error.get_type(py), error.value(py), error.traceback(py)))
        .ok()?
        .extract::<Vec<String>>()
        .ok()?;
    Some(formatted.concat())
}

fn nul_source_diagnostic(phase: Phase) -> Diagnostic {
    let message = "Python source cannot contain a NUL byte";
    match phase {
        Phase::Compile => Diagnostic::compile(DiagnosticKind::Parse, message, None, None),
        Phase::Evaluate => Diagnostic::runtime(message, None, None),
    }
}
