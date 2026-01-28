## 2024-05-22 - Skia Text Rendering Allocations
**Learning:** `char::to_string()` in inner loops (like text rendering) creates significant overhead due to heap allocation. Using `&str` slices via `text.char_indices()` avoids this.
**Action:** When iterating over characters for rendering or measurement in Rust, always prefer `text[i..i+len]` slices over `ch.to_string()` if the API supports `AsRef<str>`.
