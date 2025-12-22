use std::borrow::Cow;
use std::time::Instant;

use log::{self, Level};

pub struct ScopedTimer {
    label: Cow<'static, str>,
    level: Level,
    start: Instant,
}

impl ScopedTimer {
    pub fn with_level(label: impl Into<Cow<'static, str>>, level: Level) -> Self {
        Self {
            label: label.into(),
            level,
            start: Instant::now(),
        }
    }

    pub fn info(label: impl Into<Cow<'static, str>>) -> Self {
        Self::with_level(label, Level::Info)
    }

    pub fn debug(label: impl Into<Cow<'static, str>>) -> Self {
        Self::with_level(label, Level::Debug)
    }

    pub fn debug_lazy(label_fn: impl FnOnce() -> String) -> Option<Self> {
        if log::log_enabled!(Level::Debug) {
            Some(Self::with_level(label_fn(), Level::Debug))
        } else {
            None
        }
    }
}

impl Drop for ScopedTimer {
    fn drop(&mut self) {
        let duration = self.start.elapsed().as_millis();
        log::log!(self.level, "{} took {} ms", self.label, duration);
    }
}

pub fn measure<T, F>(label: impl Into<Cow<'static, str>>, level: Level, f: F) -> T
where
    F: FnOnce() -> T,
{
    let _timer = ScopedTimer::with_level(label, level);
    f()
}

pub fn measure_info<T, F>(label: impl Into<Cow<'static, str>>, f: F) -> T
where
    F: FnOnce() -> T,
{
    measure(label, Level::Info, f)
}

pub fn measure_debug<T, F>(label: impl Into<Cow<'static, str>>, f: F) -> T
where
    F: FnOnce() -> T,
{
    measure(label, Level::Debug, f)
}

pub fn measure_debug_lazy<T, L, F>(label_fn: L, f: F) -> T
where
    L: FnOnce() -> String,
    F: FnOnce() -> T,
{
    let _timer = ScopedTimer::debug_lazy(label_fn);
    f()
}
