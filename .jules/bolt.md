## 2026-01-23 - Rust String Allocation in Loops
**Learning:** Using `ch.to_string()` inside a tight loop (like text rendering) creates unnecessary heap allocations for every character, which significantly impacts performance (observed ~24% slowdown in text rendering).
**Action:** Use `text.char_indices()` and slice `&text[i..i + ch.len_utf8()]` to get `&str` slices without allocation when working with Skia or other APIs that accept `&str`.
