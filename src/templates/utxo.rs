use super::*;

#[derive(Debug, PartialEq, Clone, Serialize, Deserialize)]
pub(crate) struct Utxo {
    pub(crate) txid: Txid,
    pub(crate) vout: u32,
    pub(crate) script: Script,
    pub(crate) shibes: u64,
    pub(crate) confirmations: Option<u32>,
}
