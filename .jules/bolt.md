## 2024-05-22 - [Text Rendering Allocations]
**Learning:** `skia_safe` text rendering methods like `measure_str` and `draw_str` accept `impl AsRef<str>`, allowing `&str` slices. Using `char::to_string()` inside drawing loops creates unnecessary heap allocations per character.
**Action:** When iterating over text for individual character rendering, use `text.char_indices()` to get `&str` slices directly instead of converting `char` to `String`.
