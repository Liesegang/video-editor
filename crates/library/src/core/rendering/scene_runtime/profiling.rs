//! Opt-in GPU stage timing for real Point rendering tests.

use super::*;
use glow::HasContext;
use std::time::Duration;

const STAGES: [PointProfileStage; 6] = [
    PointProfileStage::HashClearBuild,
    PointProfileStage::BudgetGpu,
    PointProfileStage::Search,
    PointProfileStage::Mutual,
    PointProfileStage::ScanCompact,
    PointProfileStage::Draw,
];
const QUERY_COUNT: usize = STAGES.len() * 2;

#[derive(Clone, Debug)]
pub(crate) struct PointGpuStage {
    pub name: &'static str,
    pub elapsed: Duration,
}

#[derive(Clone, Debug)]
pub(crate) struct PointGpuProfile {
    pub stages: Vec<PointGpuStage>,
    pub budget_readback_wait: Duration,
}

#[derive(Clone, Copy)]
pub(super) enum PointProfileStage {
    HashClearBuild,
    BudgetGpu,
    Search,
    Mutual,
    ScanCompact,
    Draw,
}

impl PointProfileStage {
    fn index(self) -> usize {
        match self {
            Self::HashClearBuild => 0,
            Self::BudgetGpu => 1,
            Self::Search => 2,
            Self::Mutual => 3,
            Self::ScanCompact => 4,
            Self::Draw => 5,
        }
    }

    fn name(self) -> &'static str {
        match self {
            Self::HashClearBuild => "hash_clear_build",
            Self::BudgetGpu => "budget_gpu",
            Self::Search => "search",
            Self::Mutual => "mutual",
            Self::ScanCompact => "scan_compact",
            Self::Draw => "draw",
        }
    }
}

pub(super) struct PointGpuProfiler {
    queries: Vec<glow::Query>,
    query_buffer_supported: bool,
    issued: usize,
    budget_readback_wait: Duration,
    last: Option<PointGpuProfile>,
}

impl PointGpuProfiler {
    fn create(gl: &glow::Context) -> Result<Self, LibraryError> {
        let mut queries = Vec::with_capacity(QUERY_COUNT);
        for _ in 0..QUERY_COUNT {
            // SAFETY: SceneRuntime exclusively owns the current GL context.
            match unsafe { gl.create_query() } {
                Ok(query) => queries.push(query),
                Err(diagnostic) => {
                    // SAFETY: no allocated query escaped this failed constructor.
                    unsafe {
                        for query in queries {
                            gl.delete_query(query);
                        }
                    }
                    return Err(LibraryError::Render(format!(
                        "Point GPU profiler query allocation failed: {diagnostic}"
                    )));
                }
            }
        }
        Ok(Self {
            queries,
            query_buffer_supported: (gl.version().major, gl.version().minor) >= (4, 4)
                || gl
                    .supported_extensions()
                    .contains("GL_ARB_query_buffer_object"),
            issued: 0,
            budget_readback_wait: Duration::ZERO,
            last: None,
        })
    }

    pub(super) fn begin_frame(&mut self) {
        self.issued = 0;
        self.budget_readback_wait = Duration::ZERO;
        self.last = None;
    }

    pub(super) fn clear_report(&mut self) {
        self.last = None;
    }

    pub(super) fn begin_stage(
        &mut self,
        gl: &glow::Context,
        stage: PointProfileStage,
    ) -> Result<(), LibraryError> {
        self.issue(gl, stage.index() * 2)
    }

    pub(super) fn end_stage(
        &mut self,
        gl: &glow::Context,
        stage: PointProfileStage,
    ) -> Result<(), LibraryError> {
        self.issue(gl, stage.index() * 2 + 1)
    }

    pub(super) fn set_budget_readback_wait(&mut self, elapsed: Duration) {
        self.budget_readback_wait = elapsed;
    }

    fn issue(&mut self, gl: &glow::Context, index: usize) -> Result<(), LibraryError> {
        if index != self.issued {
            return Err(LibraryError::Render(
                "Point GPU profiler stages were issued out of order".into(),
            ));
        }
        let query = self.queries.get(index).copied().ok_or_else(|| {
            LibraryError::Render("Point GPU profiler query index exceeded its allocation".into())
        })?;
        // SAFETY: the query is owned by this profiler and no active-query
        // target is mutated by a timestamp command.
        unsafe {
            gl.query_counter(query, glow::TIMESTAMP);
        }
        gl_operation_result(gl, "profiler timestamp")?;
        self.issued += 1;
        Ok(())
    }

    fn finish(&mut self, gl: &glow::Context) -> Result<(), LibraryError> {
        if self.issued != QUERY_COUNT {
            let issued = self.issued;
            self.abort(gl);
            return Err(LibraryError::Render(format!(
                "Point GPU profiler recorded {} of {QUERY_COUNT} timestamps",
                issued
            )));
        }
        let timestamps = match self.read_issued(gl) {
            Ok(timestamps) => timestamps,
            Err(error) => {
                self.issued = 0;
                self.last = None;
                return Err(error);
            }
        };
        let stages = STAGES
            .iter()
            .enumerate()
            .map(|(index, stage)| PointGpuStage {
                name: stage.name(),
                elapsed: Duration::from_nanos(
                    timestamps[index * 2 + 1].saturating_sub(timestamps[index * 2]),
                ),
            })
            .collect();
        self.issued = 0;
        self.last = Some(PointGpuProfile {
            stages,
            budget_readback_wait: self.budget_readback_wait,
        });
        Ok(())
    }

    fn abort(&mut self, gl: &glow::Context) {
        if let Err(error) = self.read_issued(gl) {
            log::debug!("Point profiling cleanup could not read issued timestamps: {error}");
        }
        self.issued = 0;
        self.last = None;
    }

    fn read_issued(&self, gl: &glow::Context) -> Result<Vec<u64>, LibraryError> {
        if self.issued == 0 {
            return Ok(Vec::new());
        }
        let mut timestamps = Vec::with_capacity(self.issued);
        // GL 4.4 interprets the query-result pointer as a byte offset whenever
        // a query buffer is bound. Profiling runs after SceneRuntime restored
        // foreign state, so temporarily unbind and restore that buffer.
        let previous_query_buffer = if self.query_buffer_supported {
            // SAFETY: the current context is exclusively held and this only
            // reads then changes one binding which is restored below.
            unsafe {
                let previous = gl.get_parameter_buffer(glow::QUERY_BUFFER_BINDING);
                gl.bind_buffer(glow::QUERY_BUFFER, None);
                previous
            }
        } else {
            None
        };
        for &query in self.queries.iter().take(self.issued) {
            let mut timestamp = 0_u64;
            // SAFETY: `timestamp` remains live and aligned for the duration of
            // the synchronous query-result write. Query-result retrieval waits
            // for the corresponding timestamp without changing active queries.
            unsafe {
                gl.get_query_parameter_u64_with_offset(
                    query,
                    glow::QUERY_RESULT,
                    (&mut timestamp as *mut u64).addr(),
                );
            }
            timestamps.push(timestamp);
        }
        if self.query_buffer_supported {
            // SAFETY: restores the binding captured above on the same context.
            unsafe {
                gl.bind_buffer(glow::QUERY_BUFFER, previous_query_buffer);
            }
        }
        gl_operation_result(gl, "profiler result readback")?;
        Ok(timestamps)
    }

    pub(super) fn destroy(self, gl: &glow::Context) {
        // SAFETY: the profiler uniquely owns these query names.
        unsafe {
            for query in self.queries {
                gl.delete_query(query);
            }
        }
    }
}

impl SceneRuntime {
    pub(crate) fn set_point_profiling_for_test(
        &mut self,
        enabled: bool,
    ) -> Result<(), LibraryError> {
        if enabled {
            if self.point_profiler.is_none() {
                self.point_profiler = Some(PointGpuProfiler::create(&self.gl)?);
            }
        } else if let Some(profiler) = self.point_profiler.take() {
            let mut profiler = profiler;
            profiler.abort(&self.gl);
            profiler.destroy(&self.gl);
        }
        Ok(())
    }

    pub(crate) fn point_profile_for_test(&self) -> Option<&PointGpuProfile> {
        self.point_profiler
            .as_ref()
            .and_then(|profiler| profiler.last.as_ref())
    }

    pub(super) fn finish_point_profile(
        &mut self,
        result: Result<SceneTexture, LibraryError>,
    ) -> Result<SceneTexture, LibraryError> {
        let Some(profiler) = self.point_profiler.as_mut() else {
            return result;
        };
        match result {
            Ok(texture) => {
                profiler.finish(&self.gl)?;
                Ok(texture)
            }
            Err(error) => {
                profiler.abort(&self.gl);
                Err(error)
            }
        }
    }
}
