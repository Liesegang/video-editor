## 2026-01-30 - Zero-allocation text rendering
**Learning:** `char::to_string()` in rendering loops creates massive heap allocation overhead.
**Action:** Use `text.char_indices()` and `&str` slices (`&text[i..i+len]`) to handle characters without allocation.
