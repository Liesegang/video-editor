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
