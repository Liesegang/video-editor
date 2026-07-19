/// Effectorの適用対象スコープ
#[derive(
    Debug, Default, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize, Hash,
)]
pub enum EffectorTarget {
    #[default]
    Block, // 全体（全文字通してインデックス計算）
    Line,  // 行ごと（行内でインデックスリセット）
    Char,  // 文字ごと（各文字独立、index=0固定）
    Parts, // パーツ/パスごと（将来実装）
}

pub struct EffectorEntry {
    pub effector: Box<dyn super::effectors::Effector>,
    pub target: EffectorTarget,
}

impl EffectorEntry {
    pub fn new(effector: Box<dyn super::effectors::Effector>, target: EffectorTarget) -> Self {
        Self { effector, target }
    }
}
