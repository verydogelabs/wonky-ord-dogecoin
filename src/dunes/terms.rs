use super::*;

#[derive(Default, Serialize, Deserialize, Debug, PartialEq, Copy, Clone)]
pub struct Terms {
  pub limit: Option<u128>,
  pub cap: Option<u128>,
  pub height: (Option<u64>, Option<u64>),
  pub offset: (Option<u64>, Option<u64>),
}
