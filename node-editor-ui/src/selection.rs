//! Selection policies that work with any copyable host item identifier.

use egui::{Pos2, Rect};

/// Returns the last (therefore visually topmost) hit containing `position`.
///
/// The host controls z-order by descriptor order. Multiple visual subregions
/// may deliberately carry the same logical ID.
pub fn topmost_hit<Id: Copy>(hits: &[(Id, Rect)], position: Pos2) -> Option<Id> {
    hits.iter()
        .rev()
        .find_map(|(id, rect)| rect.contains(position).then_some(*id))
}

/// Applies a logical click to an ordered selection and its primary item.
///
/// With `additive`, clicking an existing item toggles it off. Without it, an
/// existing item becomes primary without disturbing the rest of the selection.
pub fn after_click<Id: Copy + Eq>(
    current: &[Id],
    primary: Option<Id>,
    clicked: Id,
    additive: bool,
) -> (Vec<Id>, Option<Id>) {
    if additive {
        if current.contains(&clicked) {
            let targets = current
                .iter()
                .copied()
                .filter(|target| *target != clicked)
                .collect::<Vec<_>>();
            let primary = primary
                .filter(|target| targets.contains(target))
                .or_else(|| targets.last().copied());
            return (targets, primary);
        }

        let mut targets = current.to_vec();
        targets.push(clicked);
        return (targets, Some(clicked));
    }

    if current.contains(&clicked) {
        let mut targets = current.to_vec();
        targets.retain(|target| *target != clicked);
        targets.push(clicked);
        return (targets, Some(clicked));
    }

    (vec![clicked], Some(clicked))
}

/// Applies the ordered, possibly duplicated results of a marquee hit test.
pub fn after_marquee<Id: Copy + Eq>(
    current: &[Id],
    marquee_hits: &[Id],
    additive: bool,
) -> (Vec<Id>, Option<Id>) {
    let mut unique = Vec::new();
    for target in marquee_hits.iter().copied() {
        if !unique.contains(&target) {
            unique.push(target);
        }
    }

    if additive {
        let mut targets = current.to_vec();
        for target in unique {
            if !targets.contains(&target) {
                targets.push(target);
            }
        }
        let primary = targets.last().copied();
        (targets, primary)
    } else {
        let primary = unique.last().copied();
        (unique, primary)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn topmost_hit_uses_descriptor_order_without_domain_knowledge() {
        let rect = Rect::from_min_max(Pos2::ZERO, Pos2::new(20.0, 20.0));
        let hits = [(1_u8, rect), (2_u8, rect)];

        assert_eq!(topmost_hit(&hits, Pos2::new(10.0, 10.0)), Some(2));
        assert_eq!(topmost_hit(&hits, Pos2::new(30.0, 10.0)), None);
    }

    #[test]
    fn additive_click_adds_then_toggles_a_logical_item() {
        let (selected, primary) = after_click(&[1_u8], Some(1), 2, true);
        assert_eq!(selected, [1, 2]);
        assert_eq!(primary, Some(2));

        let (selected, primary) = after_click(&selected, primary, 2, true);
        assert_eq!(selected, [1]);
        assert_eq!(primary, Some(1));
    }

    #[test]
    fn marquee_deduplicates_visual_subregions() {
        let (selected, primary) = after_marquee(&[], &[3_u8, 3, 4, 3], false);

        assert_eq!(selected, [3, 4]);
        assert_eq!(primary, Some(4));
    }
}
