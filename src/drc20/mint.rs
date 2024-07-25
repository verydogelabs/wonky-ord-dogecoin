use serde::{Deserialize, Serialize};

#[derive(Debug, PartialEq, Clone, Deserialize, Serialize)]
pub struct Mint {
    #[serde(rename = "tick")]
    pub tick: String,
    #[serde(rename = "amt")]
    pub amount: String,
}
