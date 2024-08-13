use super::*;
use crate::{InscriptionId, SatPoint};
use serde::{Deserialize, Serialize};
use crate::drc20::script_key::ScriptKey;

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub enum OperationType {
    Deploy,
    Mint,
    InscribeTransfer,
    Transfer,
}
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct Receipt {
    pub inscription_id: InscriptionId,
    pub inscription_number: i64,
    pub old_satpoint: SatPoint,
    pub new_satpoint: SatPoint,
    pub op: OperationType,
    pub from: ScriptKey,
    pub to: ScriptKey,
    pub result: Result<Event, DRC20Error>,
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub enum Event {
    Deploy(DeployEvent),
    Mint(MintEvent),
    InscribeTransfer(InscripbeTransferEvent),
    Transfer(TransferEvent),
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct DeployEvent {
    pub supply: u128,
    pub limit_per_mint: u128,
    pub tick: Tick,
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct MintEvent {
    pub tick: Tick,
    pub amount: u128,
    pub msg: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct InscripbeTransferEvent {
    pub tick: Tick,
    pub amount: u128,
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct TransferEvent {
    pub tick: Tick,
    pub amount: u128,
    pub msg: Option<String>,
}
