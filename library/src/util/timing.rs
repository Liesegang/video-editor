use std::borrow::Cow;
use std::time::Instant;

use log::{self, Level};

pub struct ScopedTimer {
    label: Option<Cow<'static, str>>,
    level: Level,
    start: Option<Instant>,
}

impl ScopedTimer {
    pub fn with_level(label: impl Into<Cow<'static, str>>, level: Level) -> Self {
        Self {
            label: Some(label.into()),
            level,
            start: Some(Instant::now()),
        }
    }

    pub fn info(label: impl Into<Cow<'static, str>>) -> Self {
        Self::with_level(label, Level::Info)
    }

    pub fn debug(label: impl Into<Cow<'static, str>>) -> Self {
        Self::with_level(label, Level::Debug)
    }

    pub fn debug_lazy<F>(label_gen: F) -> Self
    where
        F: FnOnce() -> String,
    {
        if log::log_enabled!(Level::Debug) {
            Self {
                label: Some(Cow::Owned(label_gen())),
                level: Level::Debug,
                start: Some(Instant::now()),
            }
        } else {
            Self {
                label: None,
                level: Level::Debug,
                start: None,
            }
        }
    }
}

impl Drop for ScopedTimer {
    fn drop(&mut self) {
        if let (Some(label), Some(start)) = (&self.label, self.start) {
            let duration = start.elapsed().as_millis();
            log::log!(self.level, "{} took {} ms", label, duration);
        }
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

pub fn measure_debug_lazy<T, F, L>(label_gen: L, f: F) -> T
where
    F: FnOnce() -> T,
    L: FnOnce() -> String,
{
    if log::log_enabled!(Level::Debug) {
        measure_debug(label_gen(), f)
    } else {
        f()
    }
}
