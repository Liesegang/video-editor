use std::borrow::Cow;
use std::time::Instant;

use log::{self, Level};

pub struct ScopedTimer {
    label: Cow<'static, str>,
    level: Level,
    start: Option<Instant>,
}

impl ScopedTimer {
    pub fn new(label: impl Into<Cow<'static, str>>, level: Level) -> Self {
        if log::log_enabled!(level) {
            Self {
                label: label.into(),
                level,
                start: Some(Instant::now()),
            }
        } else {
            Self {
                label: Cow::Borrowed(""),
                level,
                start: None,
            }
        }
    }

    pub fn with_level(label: impl Into<Cow<'static, str>>, level: Level) -> Self {
        Self::new(label, level)
    }

    pub fn info(label: impl Into<Cow<'static, str>>) -> Self {
        Self::new(label, Level::Info)
    }

    pub fn debug(label: impl Into<Cow<'static, str>>) -> Self {
        Self::new(label, Level::Debug)
    }

    pub fn debug_lazy<F, S>(f: F) -> Self
    where
        F: FnOnce() -> S,
        S: Into<Cow<'static, str>>,
    {
        if log::log_enabled!(Level::Debug) {
            Self {
                label: f().into(),
                level: Level::Debug,
                start: Some(Instant::now()),
            }
        } else {
            Self {
                label: Cow::Borrowed(""),
                level: Level::Debug,
                start: None,
            }
        }
    }

    pub fn info_lazy<F, S>(f: F) -> Self
    where
        F: FnOnce() -> S,
        S: Into<Cow<'static, str>>,
    {
        if log::log_enabled!(Level::Info) {
            Self {
                label: f().into(),
                level: Level::Info,
                start: Some(Instant::now()),
            }
        } else {
            Self {
                label: Cow::Borrowed(""),
                level: Level::Info,
                start: None,
            }
        }
    }
}

impl Drop for ScopedTimer {
    fn drop(&mut self) {
        if let Some(start) = self.start {
            let duration = start.elapsed().as_millis();
            log::log!(self.level, "{} took {} ms", self.label, duration);
        }
    }
}

pub fn measure<T, F>(label: impl Into<Cow<'static, str>>, level: Level, f: F) -> T
where
    F: FnOnce() -> T,
{
    let _timer = ScopedTimer::new(label, level);
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

pub fn measure_debug_lazy<T, F, L, S>(label_fn: L, f: F) -> T
where
    F: FnOnce() -> T,
    L: FnOnce() -> S,
    S: Into<Cow<'static, str>>,
{
    let _timer = ScopedTimer::debug_lazy(label_fn);
    f()
}

    pub fn measure_info_lazy<T, F, L, S>(label_fn: L, f: F) -> T
    where
        F: FnOnce() -> T,
        L: FnOnce() -> S,
        S: Into<Cow<'static, str>>,
    {
        let _timer = ScopedTimer::info_lazy(label_fn);
        f()
    }
