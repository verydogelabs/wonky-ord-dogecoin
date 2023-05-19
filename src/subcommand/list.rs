use super::*;

#[derive(Debug, Parser)]
pub(crate) struct List {
  #[clap(help = "List sats in <OUTPOINT>.")]
  outpoint: OutPoint,
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
pub struct Output {
  pub output: OutPoint,
  pub start: u128,
  pub size: u64,
  pub rarity: Rarity,
}

impl List {
  pub(crate) fn run(self, options: Options) -> Result {
    let index = Index::open(&options)?;

    index.update()?;

    match index.list(self.outpoint)? {
      Some(crate::index::List::Unspent(ranges)) => {
        let mut outputs = Vec::new();
        for (output, start, size, rarity) in list(self.outpoint, ranges) {
          outputs.push(Output {
            output,
            start,
            size,
            rarity,
          });
        }

        print_json(outputs)?;

        Ok(())
      }
      Some(crate::index::List::Spent) => Err(anyhow!("output spent.")),
      None => Err(anyhow!("output not found")),
    }
  }
}

fn list(outpoint: OutPoint, ranges: Vec<(u128, u128)>) -> Vec<(OutPoint, u128, u64, Rarity)> {
  ranges
    .into_iter()
    .map(|(start, end)| {
      let size = u64::try_from(end - start).unwrap();
      let rarity = Sat(start).rarity();

      (outpoint, start, size, rarity)
    })
    .collect()
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  #[ignore]
  fn list_ranges() {
    let outpoint =
      OutPoint::from_str("1a91e3dace36e2be3bf030a65679fe821aa1d6ef92e7c9902eb318182c355691:5")
        .unwrap();
    let ranges = vec![
      (50 * COIN_VALUE as u128, 55 * COIN_VALUE as u128),
      (10 as u128, 100 as u128),
      (1050000000000000 as u128, 1150000000000000 as u128),
    ];
    assert_eq!(
      list(outpoint, ranges),
      vec![
        (
          OutPoint::from_str("1a91e3dace36e2be3bf030a65679fe821aa1d6ef92e7c9902eb318182c355691:5")
            .unwrap(),
          50 * COIN_VALUE as u128,
          5 * COIN_VALUE,
          Rarity::Uncommon,
        ),
        (
          OutPoint::from_str("1a91e3dace36e2be3bf030a65679fe821aa1d6ef92e7c9902eb318182c355691:5")
            .unwrap(),
          10,
          90,
          Rarity::Common,
        ),
        (
          OutPoint::from_str("1a91e3dace36e2be3bf030a65679fe821aa1d6ef92e7c9902eb318182c355691:5")
            .unwrap(),
          1050000000000000,
          100000000000000,
          Rarity::Epic,
        )
      ]
    )
  }
}
