use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::drc20::script_key::ScriptKey;
use crate::InscriptionId;

use super::*;

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct TokenInfo {
  pub tick: Tick,
  pub inscription_id: InscriptionId,
  pub inscription_number: u64,
  pub supply: u128,
  pub minted: u128,
  pub limit_per_mint: u128,
  pub decimal: u8,
  pub deploy_by: ScriptKey,
  pub deployed_number: u64,
  pub deployed_timestamp: u32,
  pub latest_mint_number: u64,
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct ExtendedTokenInfo {
  pub token_info: Option<TokenInfo>,
  pub holder_info: HoldersInfoForTick,
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct HolderBalanceForTick {
  pub overall_balance: String,
  pub transferable_balance: String,
  pub available_balance: String,
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct HoldersInfoForTick {
  pub holder_to_balance: HashMap<String, HolderBalanceForTick>,
  pub nr_of_holder: usize,
}
