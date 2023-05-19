use super::*;

#[derive(Debug, PartialEq, Serialize, Deserialize)]
pub struct Output {
  pub starting_sats: Vec<Sat>,
}

pub(crate) fn run() -> Result {
  let mut starting_sats = Vec::new();
  for sat in Epoch::get_starting_sats() {
    starting_sats.push(sat.clone());
  }

  print_json(Output { starting_sats })?;

  Ok(())
}
