## 2024-05-23 - Text Rendering Allocations
**Learning:** The `SkiaRenderer` text loop was allocating a new `String` for every character using `ch.to_string()`, which is a significant bottleneck. `skia-safe` methods accept `&str`, so slicing `text` using `char_indices` avoids this entirely.
**Action:** When iterating characters for rendering, always use `char_indices` to get byte ranges and create slices `&str` instead of converting `char` to `String`.
