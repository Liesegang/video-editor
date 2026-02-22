use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, Hash)]
pub struct Color {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub a: u8,
}

impl Default for Color {
    fn default() -> Self {
        Self {
            r: 255,
            g: 255,
            b: 255,
            a: 255,
        }
    }
}

impl Color {
    pub fn black() -> Self {
        Self {
            r: 0,
            g: 0,
            b: 0,
            a: 255,
        }
    }

    pub fn white() -> Self {
        Self {
            r: 255,
            g: 255,
            b: 255,
            a: 255,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::hash::{Hash, Hasher};

    #[test]
    fn default_is_white() {
        assert_eq!(Color::default(), Color::white());
    }

    #[test]
    fn black_values() {
        let c = Color::black();
        assert_eq!((c.r, c.g, c.b, c.a), (0, 0, 0, 255));
    }

    #[test]
    fn white_values() {
        let c = Color::white();
        assert_eq!((c.r, c.g, c.b, c.a), (255, 255, 255, 255));
    }

    #[test]
    fn color_equality() {
        let c1 = Color {
            r: 100,
            g: 200,
            b: 50,
            a: 128,
        };
        let c2 = Color {
            r: 100,
            g: 200,
            b: 50,
            a: 128,
        };
        let c3 = Color {
            r: 100,
            g: 200,
            b: 50,
            a: 127,
        };
        assert_eq!(c1, c2);
        assert_ne!(c1, c3);
    }

    #[test]
    fn color_hash_consistent() {
        let c1 = Color {
            r: 10,
            g: 20,
            b: 30,
            a: 40,
        };
        let c2 = Color {
            r: 10,
            g: 20,
            b: 30,
            a: 40,
        };
        assert_eq!(compute_hash(&c1), compute_hash(&c2));
    }

    #[test]
    fn color_serialization_roundtrip() {
        let c = Color {
            r: 42,
            g: 128,
            b: 200,
            a: 100,
        };
        let json = serde_json::to_string(&c).unwrap();
        let c2: Color = serde_json::from_str(&json).unwrap();
        assert_eq!(c, c2);
    }

    fn compute_hash<T: Hash>(val: &T) -> u64 {
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        val.hash(&mut hasher);
        hasher.finish()
    }
}
