use ordered_float::OrderedFloat;
use serde::{Deserialize, Serialize};
use std::hash::{Hash, Hasher};
#[derive(Serialize, Deserialize, Debug, Clone)] // Removed PartialEq, Eq (manual impl below)
pub struct Transform {
    #[serde(default)]
    pub position: Position,
    #[serde(default)]
    pub scale: Scale,
    #[serde(default)]
    pub anchor: Position,
    #[serde(default)]
    pub rotation: f64,
    #[serde(default = "default_opacity")]
    pub opacity: f64,
}

fn default_opacity() -> f64 {
    1.0
}

impl Hash for Transform {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.position.hash(state);
        self.scale.hash(state);
        self.anchor.hash(state);
        OrderedFloat(self.rotation).hash(state);
        OrderedFloat(self.opacity).hash(state);
    }
}

impl Default for Transform {
    fn default() -> Self {
        Self {
            position: Default::default(),
            scale: Default::default(),
            anchor: Default::default(),
            rotation: 0.0,
            opacity: 1.0,
        }
    }
}

#[derive(Serialize, Deserialize, Debug, Clone)] // Removed PartialEq, Eq
pub struct Position {
    pub x: f64,
    pub y: f64,
}

impl Hash for Position {
    fn hash<H: Hasher>(&self, state: &mut H) {
        OrderedFloat(self.x).hash(state);
        OrderedFloat(self.y).hash(state);
    }
}

impl Default for Position {
    fn default() -> Self {
        Self { x: 0.0, y: 0.0 }
    }
}

#[derive(Serialize, Deserialize, Debug, Clone)] // Removed PartialEq, Eq
pub struct Scale {
    pub x: f64,
    pub y: f64,
}

impl Hash for Scale {
    fn hash<H: Hasher>(&self, state: &mut H) {
        OrderedFloat(self.x).hash(state);
        OrderedFloat(self.y).hash(state);
    }
}

impl Default for Scale {
    fn default() -> Self {
        Self { x: 1.0, y: 1.0 }
    }
}

impl PartialEq for Transform {
    fn eq(&self, other: &Self) -> bool {
        self.position == other.position
            && self.scale == other.scale
            && OrderedFloat(self.rotation) == OrderedFloat(other.rotation)
            && self.anchor == other.anchor
            && OrderedFloat(self.opacity) == OrderedFloat(other.opacity)
    }
}
impl Eq for Transform {}

impl PartialEq for Position {
    fn eq(&self, other: &Self) -> bool {
        OrderedFloat(self.x) == OrderedFloat(other.x)
            && OrderedFloat(self.y) == OrderedFloat(other.y)
    }
}
impl Eq for Position {}

impl PartialEq for Scale {
    fn eq(&self, other: &Self) -> bool {
        OrderedFloat(self.x) == OrderedFloat(other.x)
            && OrderedFloat(self.y) == OrderedFloat(other.y)
    }
}
impl Eq for Scale {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn transform_default() {
        let t = Transform::default();
        assert_eq!(t.position, Position::default());
        assert_eq!(t.scale, Scale::default());
        assert_eq!(t.rotation, 0.0);
        assert_eq!(t.opacity, 1.0);
    }

    #[test]
    fn position_default_is_origin() {
        let p = Position::default();
        assert_eq!(p.x, 0.0);
        assert_eq!(p.y, 0.0);
    }

    #[test]
    fn scale_default_is_one() {
        let s = Scale::default();
        assert_eq!(s.x, 1.0);
        assert_eq!(s.y, 1.0);
    }

    #[test]
    fn transform_equality() {
        let t1 = Transform {
            position: Position { x: 10.0, y: 20.0 },
            scale: Scale { x: 2.0, y: 2.0 },
            anchor: Position { x: 5.0, y: 5.0 },
            rotation: 45.0,
            opacity: 0.5,
        };
        let t2 = Transform {
            position: Position { x: 10.0, y: 20.0 },
            scale: Scale { x: 2.0, y: 2.0 },
            anchor: Position { x: 5.0, y: 5.0 },
            rotation: 45.0,
            opacity: 0.5,
        };
        assert_eq!(t1, t2);
    }

    #[test]
    fn transform_inequality_rotation() {
        let t1 = Transform {
            rotation: 0.0,
            ..Default::default()
        };
        let t2 = Transform {
            rotation: 90.0,
            ..Default::default()
        };
        assert_ne!(t1, t2);
    }

    #[test]
    fn transform_serialization_roundtrip() {
        let t = Transform {
            position: Position { x: 100.0, y: 200.0 },
            scale: Scale { x: 1.5, y: 0.5 },
            anchor: Position { x: 50.0, y: 50.0 },
            rotation: 180.0,
            opacity: 0.75,
        };
        let json = serde_json::to_string(&t).unwrap();
        let t2: Transform = serde_json::from_str(&json).unwrap();
        assert_eq!(t, t2);
    }

    #[test]
    fn transform_hash_consistent() {
        let t1 = Transform::default();
        let t2 = Transform::default();
        let mut h1 = std::collections::hash_map::DefaultHasher::new();
        let mut h2 = std::collections::hash_map::DefaultHasher::new();
        t1.hash(&mut h1);
        t2.hash(&mut h2);
        assert_eq!(h1.finish(), h2.finish());
    }

    #[test]
    fn position_equality() {
        let p1 = Position { x: 1.0, y: 2.0 };
        let p2 = Position { x: 1.0, y: 2.0 };
        let p3 = Position { x: 1.0, y: 3.0 };
        assert_eq!(p1, p2);
        assert_ne!(p1, p3);
    }

    #[test]
    fn scale_equality() {
        let s1 = Scale { x: 2.0, y: 3.0 };
        let s2 = Scale { x: 2.0, y: 3.0 };
        let s3 = Scale { x: 2.0, y: 4.0 };
        assert_eq!(s1, s2);
        assert_ne!(s1, s3);
    }
}
