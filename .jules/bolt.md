## 2024-05-23 - Lazy Debug Logging
**Learning:** Rust's `format!` macro allocates eagerly. Wrapping debug logs in `if log::log_enabled!(Debug)` or using a lazy evaluation pattern prevents expensive string allocations in hot paths (like rendering loops) when debug logging is off.
**Action:** Use `ScopedTimer::debug_lazy` and `measure_debug_lazy` for performance monitoring instead of eager formatting.
