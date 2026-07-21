/// Convert an item index between the canonical back-to-front order and the
/// visual front-to-back order. The mapping is its own inverse.
pub(crate) fn reverse_index(index: usize, item_count: usize) -> Option<usize> {
    (index < item_count).then(|| item_count - 1 - index)
}

/// Convert an insertion slot between canonical back-to-front order and
/// visual front-to-back order. A list with `item_count` items has
/// `item_count + 1` slots, so the mapping is its own inverse.
pub(crate) fn reverse_slot(slot: usize, item_count: usize) -> Option<usize> {
    (slot <= item_count).then(|| item_count - slot)
}

/// Translate a canonical insertion slot into the final canonical item index
/// after removing the dragged item. The two slots adjacent to the source are
/// stable no-op destinations.
pub(crate) fn destination_index_after_removal(
    source_index: usize,
    insertion_slot: usize,
    item_count: usize,
) -> Option<usize> {
    if item_count == 0 || source_index >= item_count || insertion_slot > item_count {
        return None;
    }
    Some(if insertion_slot > source_index {
        insertion_slot - 1
    } else {
        insertion_slot
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn canonical_and_visual_indices_are_exact_inverses() {
        assert_eq!(
            (0..3)
                .map(|index| reverse_index(index, 3))
                .collect::<Vec<_>>(),
            vec![Some(2), Some(1), Some(0)]
        );
        for canonical in 0..3 {
            let visual = reverse_index(canonical, 3);
            assert!(visual.is_some());
            if let Some(visual) = visual {
                assert_eq!(reverse_index(visual, 3), Some(canonical));
            }
        }
        assert_eq!(reverse_index(3, 3), None);
    }

    #[test]
    fn canonical_and_visual_slots_include_both_endpoints() {
        assert_eq!(
            (0..=3)
                .map(|slot| reverse_slot(slot, 3))
                .collect::<Vec<_>>(),
            vec![Some(3), Some(2), Some(1), Some(0)]
        );
        for canonical in 0..=3 {
            let visual = reverse_slot(canonical, 3);
            assert!(visual.is_some());
            if let Some(visual) = visual {
                assert_eq!(reverse_slot(visual, 3), Some(canonical));
            }
        }
        assert_eq!(reverse_slot(4, 3), None);
    }

    #[test]
    fn removal_mapping_keeps_adjacent_slots_stable() {
        assert_eq!(destination_index_after_removal(2, 0, 4), Some(0));
        assert_eq!(destination_index_after_removal(0, 3, 4), Some(2));
        assert_eq!(destination_index_after_removal(1, 1, 4), Some(1));
        assert_eq!(destination_index_after_removal(1, 2, 4), Some(1));
        assert_eq!(destination_index_after_removal(4, 0, 4), None);
    }
}
