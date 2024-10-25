use std::{fmt::Formatter, str::FromStr};

use bitcoin::Network;
use serde::{de, Deserialize, Deserializer, Serialize, Serializer};

use crate::drc20::script_key::ScriptKey;
use crate::inscription_id::InscriptionId;

use super::*;

pub const TICK_BYTE_COUNT: usize = 4;

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Tick([u8; TICK_BYTE_COUNT]);

impl FromStr for Tick {
  type Err = DRC20Error;

  fn from_str(s: &str) -> Result<Self, Self::Err> {
    let bytes = s.as_bytes();

    if bytes.len() != TICK_BYTE_COUNT {
      return Err(DRC20Error::InvalidTickLen(s.to_string()));
    }

    Ok(Self(bytes.try_into().unwrap()))
  }
}

impl Tick {
  pub fn as_str(&self) -> &str {
    // NOTE: Tick comes from &str by from_str,
    // so it could be calling unwrap when convert to str
    std::str::from_utf8(self.0.as_slice()).unwrap()
  }

  pub fn to_lowercase(&self) -> LowerTick {
    LowerTick::new(&self.as_str().to_lowercase())
  }
}

impl Serialize for Tick {
  fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
  where
    S: Serializer,
  {
    self.as_str().serialize(serializer)
  }
}

impl<'de> Deserialize<'de> for Tick {
  fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
  where
    D: Deserializer<'de>,
  {
    Self::from_str(&String::deserialize(deserializer)?)
      .map_err(|e| de::Error::custom(format!("deserialize tick error: {}", e)))
  }
}

impl Display for Tick {
  fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
    write!(f, "{}", self.as_str())
  }
}

#[derive(Debug, Clone, PartialEq)]
pub struct LowerTick(Box<[u8]>);

impl LowerTick {
  fn new(str: &str) -> Self {
    LowerTick(str.as_bytes().to_vec().into_boxed_slice())
  }

  pub fn as_str(&self) -> &str {
    std::str::from_utf8(&self.0).unwrap()
  }

  pub fn hex(&self) -> String {
    let mut data = [0u8; TICK_BYTE_COUNT * 4];
    data[..self.0.len()].copy_from_slice(&self.0);
    hex::encode(data)
  }

  pub fn min_hex() -> String {
    hex::encode([0u8; TICK_BYTE_COUNT * 4])
  }

  pub fn max_hex() -> String {
    hex::encode([0xffu8; TICK_BYTE_COUNT * 4])
  }
}

impl Display for LowerTick {
  fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
    write!(f, "{}", self.as_str())
  }
}

pub fn script_tick_id_key(
  script: &ScriptKey,
  tick: &Tick,
  inscription_id: &InscriptionId,
) -> String {
  format!(
    "{}_{}_{}",
    script,
    tick.to_lowercase().hex(),
    inscription_id
  )
}

pub fn min_script_tick_id_key(script: &ScriptKey, tick: &Tick) -> String {
  script_tick_key(script, tick)
}

pub fn max_script_tick_id_key(script: &ScriptKey, tick: &Tick) -> String {
  // because hex format of `InscriptionId` will be 0~f, so `g` is greater than `InscriptionId.to_string()` in bytes order
  format!("{}_{}_g", script, tick.to_lowercase().hex())
}

pub fn script_tick_key(script: &ScriptKey, tick: &Tick) -> String {
  format!("{}_{}", script, tick.to_lowercase().hex())
}

pub fn min_script_tick_key(script: &ScriptKey) -> String {
  format!("{}_{}", script, LowerTick::min_hex())
}

pub fn max_script_tick_key(script: &ScriptKey) -> String {
  format!("{}_{}", script, LowerTick::max_hex())
}

pub fn deserialize_script_tick_key(
  serialized: &str,
  network: Network,
) -> Option<(ScriptKey, Tick)> {
  // Split the string by '_'
  let parts: Vec<&str> = serialized.splitn(2, '_').collect();

  // Ensure there are exactly two parts
  if parts.len() != 2 {
    return None;
  }

  // Attempt to parse `ScriptKey` from the first part
  let script = ScriptKey::from_str(parts[0], network);

  if script.is_none() {
    return None;
  }

  // Attempt to parse `Tick` from the second part
  let tick_hex = parts[1];
  let lower_tick = hex::decode(tick_hex).ok()?;

  if lower_tick.len() != TICK_BYTE_COUNT * 4 {
    return None;
  }

  let tick_bytes = &lower_tick[..TICK_BYTE_COUNT];
  let tick = Tick(tick_bytes.try_into().ok()?);

  // Return the deserialized `(ScriptKey, Tick)` tuple
  Some((script.unwrap(), tick))
}
