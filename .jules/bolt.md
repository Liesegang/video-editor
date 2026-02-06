## 2024-05-23 - Text Rendering Allocation Removal
**Learning:** `char::to_string()` in hot loops (rendering) causes significant allocation overhead. Using string slices (`&str`) derived from `char_indices` eliminates these allocations.
**Action:** When iterating over characters for rendering or measurement where the API accepts `&str`, always use `char_indices` and slice the original string instead of converting `char` to `String`.
