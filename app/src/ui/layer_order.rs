/// Convert an item index between the canonical back-to-front order and the
/// visual front-to-back order. The mapping is its own inverse.
pub(crate) fn reverse_index(index: usize, item_count: usize) -> Option<usize> {
    (index < item_count).then(|| item_count - 1 - index)
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
}
