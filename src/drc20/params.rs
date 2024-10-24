use super::num::Num;
use once_cell::sync::Lazy;

pub const PROTOCOL_LITERAL: &str = "drc-20";
pub const MAX_DECIMAL_WIDTH: u8 = 18;
pub static BIGDECIMAL_TEN: Lazy<Num> = Lazy::new(|| Num::from(10u64));
pub static MAXIMUM_SUPPLY: Lazy<Num> = Lazy::new(|| Num::from(u64::MAX));

#[allow(dead_code)]
pub const fn default_decimals() -> u8 {
  MAX_DECIMAL_WIDTH
}
