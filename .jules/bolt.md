## 2024-05-22 - Text Rendering Allocations
**Learning:** `char::to_string()` in hot loops (rendering) causes significant allocation overhead. Rust's string slicing `&str` combined with `char_indices` allows zero-allocation text processing if APIs support `&str`.
**Action:** Always prefer `&str` slicing over `String` creation for temporary text processing, especially in render loops. Verify API compatibility (e.g., `skia_safe` supports it).
