## 2024-05-22 - Memory vs Code Reality
**Learning:** The memory system suggested that `SkiaRenderer` already used `char_indices` and `&str` slices, but the actual code used `chars` and `to_string` allocations. This indicates that memory might reflect a desired state or a reverted change, or simply be inaccurate.
**Action:** Always verify the code state even if memory suggests an optimization is already present. Use memory as a hint, not truth.

## 2024-05-22 - Optimization Impact
**Learning:** Replacing `char::to_string` with `&str` slicing in a text rendering loop yielded a ~2.1x performance improvement in benchmarks.
**Action:** Look for allocation loops in hot paths (rendering) involving strings.
