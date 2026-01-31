## 2024-05-22 - Skia Text Rendering Allocations
**Learning:** `skia-safe` supports `&str` slices for `measure_str` and `draw_str`, allowing zero-allocation text rendering loops. Previous implementation allocated a new `String` for every character, causing significant overhead in tight loops.
**Action:** Always prefer `text.char_indices()` and slicing over `ch.to_string()` when iterating characters for rendering APIs that support `AsRef<str>`.
