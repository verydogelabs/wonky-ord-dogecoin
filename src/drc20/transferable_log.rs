use super::*;
use crate::InscriptionId;
use serde::{Deserialize, Serialize};
use crate::drc20::script_key::ScriptKey;

#[derive(Debug, PartialEq, Clone, Serialize, Deserialize)]
pub struct TransferableLog {
    pub inscription_id: InscriptionId,
    pub inscription_number: u64,
    pub amount: u128,
    pub tick: Tick,
    pub owner: ScriptKey,
}
