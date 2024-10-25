use serde::{Deserialize, Serialize};

#[derive(Debug, PartialEq, Clone, Deserialize, Serialize)]
pub struct Deploy {
  #[serde(rename = "tick")]
  pub tick: String,
  #[serde(rename = "max")]
  pub max_supply: String,
  #[serde(rename = "lim")]
  pub mint_limit: Option<String>,
  #[serde(rename = "dec")]
  pub decimals: Option<String>,
}
