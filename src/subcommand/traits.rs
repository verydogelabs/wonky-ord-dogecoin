use super::*;

#[derive(Debug, Parser)]
pub(crate) struct Traits {
  #[clap(help = "Show traits for <SAT>.")]
  sat: Sat,
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
pub struct Output {
  pub number: u128,
  pub decimal: String,
  pub height: u64,
  pub epoch: u64,
  pub offset: u64,
  pub rarity: Rarity,
}

impl Traits {
  pub(crate) fn run(self) -> Result {
    print_json(Output {
      number: self.sat.n(),
      decimal: self.sat.decimal().to_string(),
      height: self.sat.height().0,
      epoch: self.sat.epoch().0,
      offset: self.sat.third(),
      rarity: self.sat.rarity(),
    })?;

    Ok(())
  }
}
