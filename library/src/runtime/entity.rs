use crate::runtime::draw_type::DrawStyle;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Hash, Debug)]
pub struct StyleConfig {
    pub id: Uuid,
    pub style: DrawStyle,
}
