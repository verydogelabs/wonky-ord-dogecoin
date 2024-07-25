use super::*;
use serde::{de, Deserialize, Deserializer, Serialize, Serializer};
use std::{fmt::Formatter, str::FromStr};
use crate::drc20::script_key::ScriptKey;

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

pub fn script_tick_key(script: &ScriptKey, tick: &Tick) -> String {
    format!("{}_{}", script, tick.to_lowercase().hex())
}

pub fn min_script_tick_key(script: &ScriptKey) -> String {
    format!("{}_{}", script, LowerTick::min_hex())
}

pub fn max_script_tick_key(script: &ScriptKey) -> String {
    format!("{}_{}", script, LowerTick::max_hex())
}
