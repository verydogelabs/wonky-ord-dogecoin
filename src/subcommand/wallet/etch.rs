use bitcoin::PackedLockTime;
use super::*;

#[derive(Debug, Parser)]
pub(crate) struct Etch {
  #[clap(long, help = "Set divisibility to <DIVISIBILITY>.")]
  divisibility: u8,
  #[clap(long, help = "Etch with fee rate of <FEE_RATE> sats/vB.")]
  fee_rate: FeeRate,
  #[clap(long, help = "Etch dune <DUNE>. May contain `.` or `•`as spacers.")]
  dune: SpacedDune,
  #[clap(long, help = "Set supply to <SUPPLY>.")]
  supply: Decimal,
  #[clap(long, help = "Set currency symbol to <SYMBOL>.")]
  symbol: char,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct Output {
  pub transaction: Txid,
}

impl Etch {
  pub(crate) fn run(self, options: Options) -> SubcommandResult {
    let index = Index::open(&options)?;

    ensure!(
      index.has_dune_index(),
      "`ord wallet etch` requires index created with `--index-dunes` flag",
    );

    index.update()?;

    let SpacedDune { dune, spacers } = self.dune;

    let client = options.dogecoin_rpc_client_for_wallet_command(false)?;

    let count = client.get_block_count()?;

    ensure!(
      index.dune(dune)?.is_none(),
      "dune `{}` has already been etched",
      dune,
    );

    let minimum_at_height =
        Dune::minimum_at_height(options.chain(), Height(u32::try_from(count).unwrap() + 1));

    ensure!(
      dune >= minimum_at_height,
      "dune is less than minimum for next block: {} < {minimum_at_height}",
      dune,
    );

    ensure!(!dune.is_reserved(), "dune `{}` is reserved", dune);

    ensure!(
      self.divisibility <= crate::dunes::MAX_DIVISIBILITY,
      "<DIVISIBILITY> must be equal to or less than 38"
    );

    let destination = get_change_address(&client)?;

    let dunestone = Dunestone {
      etching: Some(Etching {
        divisibility: Some(self.divisibility),
        terms: None,
        premine: None,
        dune: Some(dune),
        spacers: Some(spacers),
        symbol: Some(self.symbol),
        turbo: false,
      }),
      edicts: vec![Edict {
        amount: self.supply.to_amount(self.divisibility)?,
        id: 0,
        output: 1,
      }],
      pointer: None,
      cenotaph: false,
    };

    let script_pubkey = dunestone.encipher();

    ensure!(
      script_pubkey.len() <= 82,
      "dunestone greater than maximum OP_RETURN size: {} > 82",
      script_pubkey.len()
    );

    let unfunded_transaction = Transaction {
      version: 1,
      lock_time: PackedLockTime::ZERO,
      input: Vec::new(),
      output: vec![
        TxOut {
          script_pubkey,
          value: 0,
        },
        TxOut {
          script_pubkey: destination.script_pubkey(),
          value: TARGET_POSTAGE.to_sat(),
        },
      ],
    };

    let inscriptions = index
      .get_inscriptions(None)?
      .keys()
      .map(|satpoint| satpoint.outpoint)
      .collect::<Vec<OutPoint>>();

    if !client.lock_unspent(&inscriptions)? {
      bail!("failed to lock UTXOs");
    }

    let unsigned_transaction = fund_raw_transaction(&client, self.fee_rate, &unfunded_transaction)?;

    let signed_transaction = client
        .sign_raw_transaction_with_wallet(&unsigned_transaction, None, None)?
        .hex;

    let transaction = client.send_raw_transaction(&signed_transaction)?;

    Ok(Box::new(Output { transaction }))
  }
}

