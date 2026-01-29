## 2025-05-22 - Unnecessary String Allocations in Render Loops
**Learning:** Iterating over `text.chars()` and calling `.to_string()` for each character inside a rendering loop (e.g., for individual character effects) creates significant allocator pressure and overhead.
**Action:** Use `text.char_indices()` and slice the original string (`&text[i..i+len]`) to pass `&str` slices to Skia/rendering functions. This avoids all heap allocations per character.
