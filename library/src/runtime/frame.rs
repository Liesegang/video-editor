use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Debug, Default)]
pub struct Region {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

// Implement Hash manually for Region since f64 doesn't implement Hash
impl std::hash::Hash for Region {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        ordered_float::OrderedFloat(self.x).hash(state);
        ordered_float::OrderedFloat(self.y).hash(state);
        ordered_float::OrderedFloat(self.width).hash(state);
        ordered_float::OrderedFloat(self.height).hash(state);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::hash::{Hash, Hasher};

    #[test]
    fn region_default_is_zero() {
        let r = Region::default();
        assert_eq!(r.x, 0.0);
        assert_eq!(r.y, 0.0);
        assert_eq!(r.width, 0.0);
        assert_eq!(r.height, 0.0);
    }

    #[test]
    fn region_clone_and_copy() {
        let r = Region {
            x: 10.0,
            y: 20.0,
            width: 100.0,
            height: 200.0,
        };
        let r2 = r; // Copy
        let r3 = r.clone();
        assert_eq!(r, r2);
        assert_eq!(r, r3);
    }

    #[test]
    fn region_equality() {
        let r1 = Region {
            x: 1.0,
            y: 2.0,
            width: 3.0,
            height: 4.0,
        };
        let r2 = Region {
            x: 1.0,
            y: 2.0,
            width: 3.0,
            height: 4.0,
        };
        let r3 = Region {
            x: 1.0,
            y: 2.0,
            width: 3.0,
            height: 5.0,
        };
        assert_eq!(r1, r2);
        assert_ne!(r1, r3);
    }

    #[test]
    fn region_hash_consistency() {
        let r1 = Region {
            x: 1.0,
            y: 2.0,
            width: 3.0,
            height: 4.0,
        };
        let r2 = Region {
            x: 1.0,
            y: 2.0,
            width: 3.0,
            height: 4.0,
        };
        assert_eq!(compute_hash(&r1), compute_hash(&r2));
    }

    #[test]
    fn region_hash_differs_for_different_values() {
        let r1 = Region {
            x: 0.0,
            y: 0.0,
            width: 100.0,
            height: 100.0,
        };
        let r2 = Region {
            x: 0.0,
            y: 0.0,
            width: 200.0,
            height: 100.0,
        };
        assert_ne!(compute_hash(&r1), compute_hash(&r2));
    }

    #[test]
    fn region_serialization_roundtrip() {
        let r = Region {
            x: 10.5,
            y: 20.5,
            width: 1920.0,
            height: 1080.0,
        };
        let json = serde_json::to_string(&r).unwrap();
        let r2: Region = serde_json::from_str(&json).unwrap();
        assert_eq!(r, r2);
    }

    fn compute_hash<T: Hash>(val: &T) -> u64 {
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        val.hash(&mut hasher);
        hasher.finish()
    }
}
