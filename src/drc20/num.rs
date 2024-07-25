use super::{DRC20Error};
use bigdecimal::{
    num_bigint::{BigInt, Sign, ToBigInt},
    BigDecimal, ToPrimitive,
};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::{
    fmt::{Display, Formatter},
    str::FromStr,
};

#[derive(PartialEq, PartialOrd, Debug, Clone)]
pub struct Num(BigDecimal);

impl Num {
    pub fn checked_add(&self, other: &Num) -> Result<Self, DRC20Error> {
        Ok(Self(self.0.clone() + &other.0))
    }

    pub fn checked_sub(&self, other: &Num) -> Result<Self, DRC20Error> {
        if self.0 < other.0 {
            return Err(DRC20Error::Overflow {
                op: String::from("checked_sub"),
                org: self.clone().to_string(),
                other: other.clone().to_string(),
            });
        }

        Ok(Self(self.0.clone() - &other.0))
    }

    pub fn sign(&self) -> Sign {
        self.0.sign()
    }

    pub fn checked_to_u128(&self) -> Result<u128, DRC20Error> {
        if !self.0.is_integer() {
            return Err(DRC20Error::InvalidInteger(self.clone().to_string()));
        }
        self
            .0
            .to_bigint()
            .ok_or(DRC20Error::InternalError(format!(
                "convert {} to bigint failed",
                self.0
            )))?
            .to_u128()
            .ok_or(DRC20Error::Overflow {
                op: String::from("to_u128"),
                org: self.clone().to_string(),
                other: Self(BigDecimal::from(BigInt::from(u128::MAX))).to_string(),
            })
    }
}

impl From<u64> for Num {
    fn from(n: u64) -> Self {
        Self(BigDecimal::from(n))
    }
}

impl From<u128> for Num {
    fn from(n: u128) -> Self {
        Self(BigDecimal::from(BigInt::from(n)))
    }
}

impl FromStr for Num {
    type Err = DRC20Error;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        if s.starts_with('.') || s.ends_with('.') || s.find(['e', 'E', '+', '-']).is_some() {
            return Err(DRC20Error::InvalidNum(s.to_string()));
        }
        let num = BigDecimal::from_str(s).map_err(|_| DRC20Error::InvalidNum(s.to_string()))?;

        Ok(Self(num))
    }
}

impl Display for Num {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}

impl Serialize for Num {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let s = self.to_string();
        serializer.serialize_str(&s)
    }
}

impl<'de> Deserialize<'de> for Num {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        Ok(Self(
            BigDecimal::from_str(&s).map_err(serde::de::Error::custom)?,
        ))
    }
}
