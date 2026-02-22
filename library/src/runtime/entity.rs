use crate::runtime::draw_type::DrawStyle;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Hash, Debug)]
pub struct StyleConfig {
    pub id: Uuid,
    pub style: DrawStyle,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime::color::Color;
    use std::hash::{Hash, Hasher};

    #[test]
    fn style_config_with_fill() {
        let sc = StyleConfig {
            id: Uuid::new_v4(),
            style: DrawStyle::Fill {
                color: Color::white(),
                offset: 0.0,
            },
        };
        assert!(matches!(sc.style, DrawStyle::Fill { .. }));
    }

    #[test]
    fn style_config_with_stroke() {
        let sc = StyleConfig {
            id: Uuid::new_v4(),
            style: DrawStyle::Stroke {
                color: Color::black(),
                width: 2.0,
                offset: 0.0,
                cap: crate::runtime::draw_type::CapType::Round,
                join: crate::runtime::draw_type::JoinType::Round,
                miter: 4.0,
                dash_array: vec![],
                dash_offset: 0.0,
            },
        };
        assert!(matches!(sc.style, DrawStyle::Stroke { .. }));
    }

    #[test]
    fn style_config_equality() {
        let id = Uuid::new_v4();
        let s1 = StyleConfig {
            id,
            style: DrawStyle::Fill {
                color: Color::white(),
                offset: 0.0,
            },
        };
        let s2 = StyleConfig {
            id,
            style: DrawStyle::Fill {
                color: Color::white(),
                offset: 0.0,
            },
        };
        assert_eq!(s1, s2);
    }

    #[test]
    fn style_config_inequality_different_id() {
        let s1 = StyleConfig {
            id: Uuid::new_v4(),
            style: DrawStyle::default(),
        };
        let s2 = StyleConfig {
            id: Uuid::new_v4(),
            style: DrawStyle::default(),
        };
        assert_ne!(s1, s2);
    }

    #[test]
    fn style_config_hash_consistent() {
        let id = Uuid::new_v4();
        let s1 = StyleConfig {
            id,
            style: DrawStyle::default(),
        };
        let s2 = StyleConfig {
            id,
            style: DrawStyle::default(),
        };
        assert_eq!(compute_hash(&s1), compute_hash(&s2));
    }

    #[test]
    fn style_config_serialization_roundtrip() {
        let sc = StyleConfig {
            id: Uuid::new_v4(),
            style: DrawStyle::Fill {
                color: Color {
                    r: 128,
                    g: 64,
                    b: 32,
                    a: 200,
                },
                offset: 5.0,
            },
        };
        let json = serde_json::to_string(&sc).unwrap();
        let sc2: StyleConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(sc, sc2);
    }

    fn compute_hash<T: Hash>(val: &T) -> u64 {
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        val.hash(&mut hasher);
        hasher.finish()
    }
}
