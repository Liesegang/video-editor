## 2024-05-23 - Text Rendering Optimization
**Learning:** Switching from `char.to_string()` to `&str` slices (via `text.char_indices()`) in text rendering loops significantly improved performance (1.7x speedup in benchmark).
**Action:** Always prefer string slices over allocating new Strings when iterating characters for rendering or measurement if the API supports `AsRef<str>`.
