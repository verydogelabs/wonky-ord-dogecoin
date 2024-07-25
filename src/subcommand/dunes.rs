use super::*;

#[derive(Debug, PartialEq, Serialize, Deserialize)]
pub struct Output {
  pub dunes: BTreeMap<Dune, DuneInfo>,
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
pub struct DuneInfo {
  pub block: u64,
  pub burned: u128,
  pub divisibility: u8,
  pub etching: Txid,
  pub height: u64,
  pub id: DuneId,
  pub index: u32,
  pub terms: Option<Terms>,
  pub mints: u128,
  pub number: u64,
  pub premine: u128,
  pub dune: Dune,
  pub spacers: u32,
  pub supply: u128,
  pub symbol: Option<char>,
  pub timestamp: DateTime<Utc>,
  pub turbo: bool,
  pub tx: u32,
}

pub(crate) fn run(options: Options) -> SubcommandResult {
  let index = Index::open(&options)?;

  ensure!(
    index.has_dune_index(),
    "`ord dunes` requires index created with `--index-dunes` flag",
  );

  index.update()?;

  Ok(Box::new(Output {
    dunes: index
      .dunes()?
      .into_iter()
      .map(
        |(
          id,
          entry @ DuneEntry {
            block,
            burned,
            divisibility,
            etching,
            terms,
            mints,
            number,
            premine,
            dune,
            spacers,
            supply,
            symbol,
            timestamp,
            turbo,
          },
        )| {
          (
            dune,
            DuneInfo {
              block,
              burned,
              divisibility,
              etching,
              height: id.height,
              id,
              index: id.index,
              terms,
              mints,
              number,
              premine,
              timestamp: crate::timestamp(timestamp),
              dune,
              spacers,
              supply,
              symbol,
              turbo,
              tx: id.index,
            },
          )
        },
      )
      .collect::<BTreeMap<Dune, DuneInfo>>(),
  }))
}
