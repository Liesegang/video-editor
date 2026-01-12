## 2024-05-22 - Skia Text Rendering Optimization
**Learning:** `skia-safe`'s `measure_str` and `draw_str` accept `&str`. Converting `char` to `String` inside a rendering loop is a significant performance anti-pattern in Rust, causing unnecessary heap allocations.
**Action:** Always prefer `text.char_indices()` and slicing `&text[i..i+len]` over `ch.to_string()` when working with individual characters in a string, especially in hot loops. Pre-allocate vectors using `Vec::with_capacity` when the size is known or can be estimated (e.g. using byte length as an upper bound for char count).
