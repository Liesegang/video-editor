## 2026-02-03 - Text Rendering Allocation
**Learning:** `char::to_string()` inside tight rendering loops causes significant performance overhead due to heap allocations.
**Action:** Use `text.char_indices()` and `&str` slices to avoid allocation when interacting with APIs that accept `impl AsRef<str>`.
