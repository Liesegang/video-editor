## 2024-05-23 - Text Rendering Allocation Optimization
**Learning:** In SkiaRenderer's ensemble text rendering, converting each `char` to `String` (using `.to_string()`) inside hot loops caused significant overhead (approx 10% of rendering time for long text).
**Action:** Use `text.char_indices()` to iterate and slice the original string `&text[i..i+len]` to get `&str` references, avoiding heap allocations completely in the measurement and drawing loops.
