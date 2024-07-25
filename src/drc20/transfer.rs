use super::*;
use serde::{Deserialize, Serialize};
#[derive(Debug, PartialEq, Deserialize, Serialize)]
pub struct TransferInfo {
    pub tick: Tick,
    pub amt: u128,
}

#[derive(Debug, PartialEq, Clone, Deserialize, Serialize)]
pub struct Transfer {
    #[serde(rename = "tick")]
    pub tick: String,
    #[serde(rename = "amt")]
    pub amount: String,
}
