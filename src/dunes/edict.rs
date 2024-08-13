use super::*;

#[derive(Default, Serialize, Debug, PartialEq, Copy, Clone)]
pub struct Edict {
  pub id: u128,
  pub amount: u128,
  pub output: u128,
}

impl Edict {
  pub(crate) fn from_integers(
    tx: &Transaction,
    id: u128,
    amount: u128,
    output: u128,
  ) -> Option<Self> {
    let dune_id = DuneId::try_from(id).ok()?;

    if dune_id.height == 0 && dune_id.index > 0 {
      return None;
    }

    if output > u128::try_from(tx.output.len()).ok()? {
      return None;
    }

    Some(Self { id, amount, output })
  }
}
