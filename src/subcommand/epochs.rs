use super::*;

#[derive(Debug, PartialEq, Serialize, Deserialize)]
pub struct Output {
  pub starting_sats: Vec<Sat>,
}

pub(crate) fn run() -> SubcommandResult {
  let mut starting_sats = Vec::new();
  for sat in Epoch::get_starting_sats() {
    starting_sats.push(sat.clone());
  }

  Ok(Box::new(Output { starting_sats }))
}
