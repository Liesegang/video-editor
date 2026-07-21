/// Smart Backplate（自動背景）の対象レベル
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize, Hash)]
pub enum BackplateTarget {
    Char,  // 文字ごと
    Line,  // 行ごと
    Block, // 全体
    Parts, // パス/パーツごと（文字をグリフパスに分解）
}

/// How an authored background Shape is fitted into each target rectangle.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize, Hash)]
pub enum BackplateFit {
    Stretch,
    Contain,
    Cover,
}

/// Frozen paint-time geometry used only by ABI-v1 runtime Decorators.
/// Built-in and ABI-v2 Backplates consume an authored background Shape and do
/// not carry appearance in their config.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize, Hash)]
pub enum BackplateShape {
    Rect,
    RoundedRect,
    Circle,
}
use skia_safe::{Canvas, Paint, Rect};

/// Legacy extension point for non-graph decorators. Built-in Backplate no
/// longer implements this paint-time interface; it produces Shape geometry.
pub trait Decorator: Send + Sync {
    fn draw(&self, canvas: &Canvas, bounds: Rect, paint: &Paint);
}
