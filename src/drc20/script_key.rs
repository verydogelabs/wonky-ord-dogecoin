use std::fmt::{Display, Formatter};
use std::str::FromStr;

use bitcoin::{Address, Network, Script, ScriptHash};
use serde::{Deserialize, Serialize};

#[derive(Debug, PartialEq, Eq, Hash, Clone, Serialize, Deserialize)]
pub enum ScriptKey {
  Address(Address),
  ScriptHash(ScriptHash),
}

impl ScriptKey {
  pub fn from_str(script: &str, network: Network) -> Option<Self> {
    match Address::from_str(script) {
      Ok(address) => Some(ScriptKey::from_address(address, network)),
      Err(_) => None,
    }
  }
  pub fn from_address(address: Address, network: Network) -> Self {
    ScriptKey::Address(Address {
      payload: address.payload,
      network,
    })
  }
  pub fn from_script(script: &Script, network: Network) -> Self {
    match Address::from_script(script, network) {
      Ok(address) => ScriptKey::Address(address),
      Err(_) => ScriptKey::ScriptHash(script.script_hash()),
    }
  }
}

impl Display for ScriptKey {
  fn fmt(&self, f: &mut Formatter) -> std::fmt::Result {
    write!(
      f,
      "{}",
      match self {
        ScriptKey::Address(address) => address.clone().to_string(),
        ScriptKey::ScriptHash(script_hash) => script_hash.to_string(),
      }
    )
  }
}
