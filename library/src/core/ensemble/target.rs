/// Effectorの適用対象スコープ
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum EffectorTarget {
    Block, // 全体（全文字通してインデックス計算）
    Line,  // 行ごと（行内でインデックスリセット）
    Char,  // 文字ごと（各文字独立、index=0固定）
    Parts, // パーツ/パスごと（将来実装）
}

impl Default for EffectorTarget {
    fn default() -> Self {
        EffectorTarget::Block
    }
}

/// Effectorとそのターゲットスコープのペア
pub struct EffectorEntry {
    pub effector: Box<dyn super::effectors::Effector>,
    pub target: EffectorTarget,
}

impl EffectorEntry {
    pub fn new(effector: Box<dyn super::effectors::Effector>, target: EffectorTarget) -> Self {
        Self { effector, target }
    }
}
