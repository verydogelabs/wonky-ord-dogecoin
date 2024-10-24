use {
  self::{
    dunes::{Dune, DuneId},
    entry::{
      BlockHashValue, DuneEntryValue, DuneIdValue, Entry, InscriptionEntry, InscriptionEntryValue,
      InscriptionIdValue, OutPointMapValue, OutPointValue, SatPointValue, SatRange, TxidValue,
    },
    reorg::*,
    updater::Updater,
  },
  bitcoin::BlockHeader,
  bitcoincore_rpc::{Auth, Client, json::GetBlockHeaderResult},
  chrono::SubsecRound,
  crate::inscription::ParsedInscription,
  crate::wallet::Wallet,
  indicatif::{ProgressBar, ProgressStyle},
  log::log_enabled,
  redb::{
    Database, DatabaseError, MultimapTable, MultimapTableDefinition, ReadableMultimapTable,
    ReadableTable, StorageError, Table, TableDefinition, WriteTransaction,
  },
  std::collections::HashMap,
  std::io::Cursor,
  std::sync::atomic::{self, AtomicBool},
  super::*,
  url::Url,
};

use crate::drc20::{Balance, max_script_tick_key, min_script_tick_key, script_tick_key, Tick, TokenInfo, TransferableLog, min_script_tick_id_key, max_script_tick_id_key};
use crate::drc20::script_key::ScriptKey;
use crate::sat::Sat;
use crate::sat_point::SatPoint;
use crate::templates::BlockHashAndConfirmations;

pub(crate) use self::entry::DuneEntry;

pub(crate) mod entry;
mod reorg;
mod fetcher;
mod rtx;
mod updater;

const SCHEMA_VERSION: u64 = 6;

macro_rules! define_table {
  ($name:ident, $key:ty, $value:ty) => {
    const $name: TableDefinition<$key, $value> = TableDefinition::new(stringify!($name));
  };
}

macro_rules! define_multimap_table {
  ($name:ident, $key:ty, $value:ty) => {
    const $name: MultimapTableDefinition<$key, $value> =
      MultimapTableDefinition::new(stringify!($name));
  };
}

define_table! { HEIGHT_TO_BLOCK_HASH, u32, &BlockHashValue }
define_table! { INSCRIPTION_ID_TO_INSCRIPTION_ENTRY, &InscriptionIdValue, InscriptionEntryValue }
define_table! { INSCRIPTION_ID_TO_DUNE, &InscriptionIdValue, u128 }
define_table! { INSCRIPTION_ID_TO_SATPOINT, &InscriptionIdValue, &SatPointValue }
define_table! { INSCRIPTION_NUMBER_TO_INSCRIPTION_ID, u64, &InscriptionIdValue }
define_table! { OUTPOINT_TO_DUNE_BALANCES, &OutPointValue, &[u8] }
define_table! { INSCRIPTION_ID_TO_TXIDS, &InscriptionIdValue, &[u8] }
define_table! { INSCRIPTION_TXID_TO_TX, &[u8], &[u8] }
define_table! { PARTIAL_TXID_TO_INSCRIPTION_TXIDS, &[u8], &[u8] }
define_table! { OUTPOINT_TO_SAT_RANGES, &OutPointValue, &[u8] }
define_table! { OUTPOINT_TO_VALUE, &OutPointValue, u64}
define_multimap_table! { ADDRESS_TO_OUTPOINT, &[u8], &OutPointValue}
define_table! { DUNE_ID_TO_DUNE_ENTRY, DuneIdValue, DuneEntryValue }
define_table! { DUNE_TO_DUNE_ID, u128, DuneIdValue }
define_table! { SATPOINT_TO_INSCRIPTION_ID, &SatPointValue, &InscriptionIdValue }
define_table! { SAT_TO_INSCRIPTION_ID, u64, &InscriptionIdValue }
define_table! { SAT_TO_SATPOINT, u64, &SatPointValue }
define_table! { STATISTIC_TO_COUNT, u64, u64 }
define_table! { TRANSACTION_ID_TO_DUNE, &TxidValue, u128 }
define_table! { TRANSACTION_ID_TO_TRANSACTION, &TxidValue, &[u8] }
define_table! { WRITE_TRANSACTION_STARTING_BLOCK_COUNT_TO_TIMESTAMP, u32, u128 }
define_table! { DRC20_BALANCES, &str, &[u8] }
define_table! { DRC20_TOKEN, &str, &[u8] }
define_table! { DRC20_INSCRIBE_TRANSFER, &InscriptionIdValue, &[u8] }
define_table! { DRC20_TRANSFERABLELOG, &str, &[u8] }
define_multimap_table! { DRC20_TOKEN_HOLDER, &str, &str}

pub(crate) struct Index {
  auth: Auth,
  client: Client,
  database: Database,
  path: PathBuf,
  first_inscription_height: u32,
  first_dune_height: u32,
  genesis_block_coinbase_transaction: Transaction,
  genesis_block_coinbase_txid: Txid,
  height_limit: Option<u32>,
  index_drc20: bool,
  index_dunes: bool,
  index_sats: bool,
  index_transactions: bool,
  unrecoverably_reorged: AtomicBool,
  rpc_url: String,
  nr_parallel_requests: usize,
  chain: Chain,
}

#[derive(Debug, PartialEq)]
pub(crate) enum List {
  Spent,
  Unspent(Vec<(u64, u64)>),
}

#[derive(Copy, Clone)]
#[repr(u64)]
pub(crate) enum Statistic {
  Commits,
  IndexDrc20,
  IndexDunes,
  IndexSats,
  LostSats,
  OutputsTraversed,
  ReservedDunes,
  Dunes,
  SatRanges,
  Schema,
  IndexTransactions,
}

impl Statistic {
  fn key(self) -> u64 {
    self.into()
  }
}

impl From<Statistic> for u64 {
  fn from(statistic: Statistic) -> Self {
    statistic as u64
  }
}

#[derive(Serialize)]
pub(crate) struct Info {
  pub(crate) blocks_indexed: u32,
  pub(crate) branch_pages: u64,
  pub(crate) fragmented_bytes: u64,
  pub(crate) index_file_size: u64,
  pub(crate) index_path: PathBuf,
  pub(crate) leaf_pages: u64,
  pub(crate) metadata_bytes: u64,
  pub(crate) outputs_traversed: u64,
  pub(crate) page_size: usize,
  pub(crate) sat_ranges: u64,
  pub(crate) stored_bytes: u64,
  pub(crate) transactions: Vec<TransactionInfo>,
  pub(crate) tree_height: u32,
  pub(crate) utxos_indexed: u64,
}

#[derive(Serialize)]
pub(crate) struct TransactionInfo {
  pub(crate) starting_block_count: u32,
  pub(crate) starting_timestamp: u128,
}

trait BitcoinCoreRpcResultExt<T> {
  fn into_option(self) -> Result<Option<T>>;
}

impl<T> BitcoinCoreRpcResultExt<T> for Result<T, bitcoincore_rpc::Error> {
  fn into_option(self) -> Result<Option<T>> {
    match self {
      Ok(ok) => Ok(Some(ok)),
      Err(bitcoincore_rpc::Error::JsonRpc(bitcoincore_rpc::jsonrpc::error::Error::Rpc(
        bitcoincore_rpc::jsonrpc::error::RpcError { code: -8, .. },
      ))) => Ok(None),
      Err(bitcoincore_rpc::Error::JsonRpc(bitcoincore_rpc::jsonrpc::error::Error::Rpc(
        bitcoincore_rpc::jsonrpc::error::RpcError { message, .. },
      )))
        if message.ends_with("not found") =>
      {
        Ok(None)
      }
      Err(err) => Err(err.into()),
    }
  }
}

impl Index {
  pub(crate) fn open(options: &Options) -> Result<Self> {
    let rpc_url = options.rpc_url();
    let nr_parallel_requests = options.nr_parallel_requests();
    let cookie_file = options.cookie_file()?;
    // if cookie_file is emtpy / not set try to parse username:password from RPC URL to create the UserPass auth
    let auth: Auth = if !cookie_file.exists() {
      let url = Url::parse(&rpc_url)?;
      let username = url.username().to_string();
      let password = url.password().map(|x| x.to_string()).unwrap_or_default();

      log::info!(
        "Connecting to Dogecoin Core RPC server at {rpc_url} using credentials from the url"
      );

      Auth::UserPass(username, password)
    } else {
      log::info!(
        "Connecting to Dogecoin Core RPC server at {rpc_url} using credentials from `{}`",
        cookie_file.display()
      );

      Auth::CookieFile(cookie_file)
    };

    let client = Client::new(&rpc_url, auth.clone()).context("failed to connect to RPC URL")?;

    let data_dir = options.data_dir()?;

    if let Err(err) = fs::create_dir_all(&data_dir) {
      bail!("failed to create data dir `{}`: {err}", data_dir.display());
    }

    let path = if let Some(path) = &options.index {
      path.clone()
    } else {
      data_dir.join("index.redb")
    };

    let index_drc20;
    let index_dunes;
    let index_sats;
    let index_transactions;

    let database = match unsafe { Database::builder().open(&path) } {
      Ok(database) => {
        {
          let tx = database.begin_read()?;
          let schema_version = tx
            .open_table(STATISTIC_TO_COUNT)?
            .get(&Statistic::Schema.key())?
            .map(|x| x.value())
            .unwrap_or(0);

          match schema_version.cmp(&SCHEMA_VERSION) {
            cmp::Ordering::Less =>
              bail!(
              "index at `{}` appears to have been built with an older, incompatible version of ord, consider deleting and rebuilding the index: index schema {schema_version}, ord schema {SCHEMA_VERSION}",
              path.display()
            ),
            cmp::Ordering::Greater =>
              bail!(
              "index at `{}` appears to have been built with a newer, incompatible version of ord, consider updating ord: index schema {schema_version}, ord schema {SCHEMA_VERSION}",
              path.display()
            ),
            cmp::Ordering::Equal => {}
          }

          let statistics = tx.open_table(STATISTIC_TO_COUNT)?;

          index_dunes = statistics
            .get(&Statistic::IndexDunes.key())?
            .unwrap()
            .value()
            != 0;
          index_sats = statistics
            .get(&Statistic::IndexSats.key())?
            .unwrap()
            .value()
            != 0;
          index_transactions = statistics
            .get(&Statistic::IndexTransactions.key())?
            .unwrap()
            .value()
            != 0;
          index_drc20 = statistics
            .get(&Statistic::IndexDrc20.key())?
            .unwrap()
            .value()
            != 0;
        }

        database
      }
      Err(DatabaseError::Storage(StorageError::Io(error)))
        if error.kind() == io::ErrorKind::NotFound =>
      {
        let db_cache_size = match options.db_cache_size {
          Some(db_cache_size) => db_cache_size,
          None => {
            let mut sys = System::new();
            sys.refresh_memory();
            usize::try_from(sys.total_memory() / 4)?
          }
        };

        let database = Database::builder()
          .set_cache_size(db_cache_size)
          .create(&path)?;

        let tx = database.begin_write()?;

        #[cfg(test)]
        let tx = {
          let mut tx = tx;
          tx.set_durability(redb::Durability::None);
          tx
        };

        tx.open_table(HEIGHT_TO_BLOCK_HASH)?;
        tx.open_table(INSCRIPTION_ID_TO_INSCRIPTION_ENTRY)?;
        tx.open_table(INSCRIPTION_ID_TO_DUNE)?;
        tx.open_table(INSCRIPTION_ID_TO_SATPOINT)?;
        tx.open_table(INSCRIPTION_NUMBER_TO_INSCRIPTION_ID)?;
        tx.open_table(INSCRIPTION_ID_TO_TXIDS)?;
        tx.open_table(INSCRIPTION_TXID_TO_TX)?;
        tx.open_table(PARTIAL_TXID_TO_INSCRIPTION_TXIDS)?;
        tx.open_table(OUTPOINT_TO_VALUE)?;
        tx.open_multimap_table(ADDRESS_TO_OUTPOINT)?;
        tx.open_table(SATPOINT_TO_INSCRIPTION_ID)?;
        tx.open_table(SAT_TO_INSCRIPTION_ID)?;
        tx.open_table(SAT_TO_SATPOINT)?;
        tx.open_table(WRITE_TRANSACTION_STARTING_BLOCK_COUNT_TO_TIMESTAMP)?;

        {
          let mut outpoint_to_sat_ranges = tx.open_table(OUTPOINT_TO_SAT_RANGES)?;
          let mut statistics = tx.open_table(STATISTIC_TO_COUNT)?;

          if options.index_sats {
            outpoint_to_sat_ranges.insert(&OutPoint::null().store(), [].as_slice())?;
          }

          index_drc20 = options.index_dunes();
          index_dunes = options.index_dunes();
          index_sats = options.index_sats;
          index_transactions = options.index_transactions;

          statistics.insert(&Statistic::IndexDrc20.key(), &u64::from(index_drc20))?;

          statistics.insert(&Statistic::IndexDunes.key(), &u64::from(index_dunes))?;

          statistics.insert(&Statistic::IndexSats.key(), &u64::from(index_sats))?;

          statistics.insert(
            &Statistic::IndexTransactions.key(),
            &u64::from(index_transactions),
          )?;

          statistics.insert(&Statistic::Schema.key(), &SCHEMA_VERSION)?;
        }

        tx.commit()?;

        database
      }
      Err(error) => return Err(error.into()),
    };

    let genesis_block_coinbase_transaction =
      options.chain().genesis_block().coinbase().unwrap().clone();

    Ok(Self {
      genesis_block_coinbase_txid: genesis_block_coinbase_transaction.txid(),
      auth,
      client,
      database,
      path,
      first_inscription_height: options.first_inscription_height(),
      first_dune_height: options.first_dune_height(),
      genesis_block_coinbase_transaction,
      height_limit: options.height_limit,
      index_drc20,
      index_dunes,
      index_sats,
      index_transactions,
      unrecoverably_reorged: AtomicBool::new(false),
      rpc_url,
      nr_parallel_requests,
      chain: options.chain_argument,
    })
  }

  pub(crate) fn get_unspent_outputs(&self, _wallet: Wallet) -> Result<BTreeMap<OutPoint, Amount>> {
    let mut utxos = BTreeMap::new();
    utxos.extend(
      self
        .client
        .list_unspent(None, None, None, None, None)?
        .into_iter()
        .map(|utxo| {
          let outpoint = OutPoint::new(utxo.txid, utxo.vout);
          let amount = utxo.amount;

          (outpoint, amount)
        }),
    );

    #[derive(Deserialize)]
    pub(crate) struct JsonOutPoint {
      txid: bitcoin::Txid,
      vout: u32,
    }

    for JsonOutPoint { txid, vout } in self
      .client
      .call::<Vec<JsonOutPoint>>("listlockunspent", &[])?
    {
      utxos.insert(
        OutPoint { txid, vout },
        Amount::from_sat(self.client.get_raw_transaction(&txid)?.output[vout as usize].value),
      );
    }
    let rtx = self.database.begin_read()?;
    let outpoint_to_value = rtx.open_table(OUTPOINT_TO_VALUE)?;
    for outpoint in utxos.keys() {
      if outpoint_to_value.get(&outpoint.store())?.is_none() {
        return Err(anyhow!(
          "output in Dogecoin Core wallet but not in ord index: {outpoint}"
        ));
      }
    }

    Ok(utxos)
  }

  pub(crate) fn get_unspent_output_ranges(
    &self,
    wallet: Wallet,
  ) -> Result<Vec<(OutPoint, Vec<(u64, u64)>)>> {
    self
      .get_unspent_outputs(wallet)?
      .into_keys()
      .map(|outpoint| match self.list(outpoint)? {
        Some(List::Unspent(sat_ranges)) => Ok((outpoint, sat_ranges)),
        Some(List::Spent) => bail!("output {outpoint} in wallet but is spent according to index"),
        None => bail!("index has not seen {outpoint}"),
      })
      .collect()
  }

  pub(crate) fn has_dune_index(&self) -> bool {
    self.index_dunes
  }

  pub(crate) fn has_sat_index(&self) -> bool {
    self.index_sats
  }

  pub(crate) fn info(&self) -> Result<Info> {
    let wtx = self.begin_write()?;

    let stats = wtx.stats()?;

    let info = {
      let statistic_to_count = wtx.open_table(STATISTIC_TO_COUNT)?;
      let sat_ranges = statistic_to_count
        .get(&Statistic::SatRanges.key())?
        .map(|x| x.value())
        .unwrap_or(0);
      let outputs_traversed = statistic_to_count
        .get(&Statistic::OutputsTraversed.key())?
        .map(|x| x.value())
        .unwrap_or(0);
      let transactions: Vec<TransactionInfo> = wtx
        .open_table(WRITE_TRANSACTION_STARTING_BLOCK_COUNT_TO_TIMESTAMP)?
        .range(0..)?
        .map(|result| {
          result.map(
            |(starting_block_count, starting_timestamp)| TransactionInfo {
              starting_block_count: starting_block_count.value(),
              starting_timestamp: starting_timestamp.value(),
            },
          )
        })
        .collect::<Result<Vec<_>, _>>()?;
      Info {
        index_path: self.path.clone(),
        blocks_indexed: wtx
          .open_table(HEIGHT_TO_BLOCK_HASH)?
          .range(0..)?
          .rev()
          .next()
          .map(|result| result.map(|(height, _hash)| height.value() + 1))
          .transpose()?
          .unwrap_or(0),
        branch_pages: stats.branch_pages(),
        fragmented_bytes: stats.fragmented_bytes(),
        index_file_size: fs::metadata(&self.path)?.len(),
        leaf_pages: stats.leaf_pages(),
        metadata_bytes: stats.metadata_bytes(),
        sat_ranges,
        outputs_traversed,
        page_size: stats.page_size(),
        stored_bytes: stats.stored_bytes(),
        transactions,
        tree_height: stats.tree_height(),
        utxos_indexed: wtx.open_table(OUTPOINT_TO_SAT_RANGES)?.len()?,
      }
    };

    Ok(info)
  }

  pub(crate) fn update(&self) -> Result {
    let mut updater = Updater::new(self)?;

    loop {
      match updater.update_index() {
        Ok(ok) => return Ok(ok),
        Err(err) => {
          log::info!("{}", err.to_string());

            match err.downcast_ref() {
              Some(&ReorgError::Recoverable { height, depth }) => {
                Reorg::handle_reorg(self, height, depth)?;

                updater = Updater::new(self)?;
              }
              Some(&ReorgError::Unrecoverable) => {
                self
                  .unrecoverably_reorged
                  .store(true, atomic::Ordering::Relaxed);
                return Err(anyhow!(ReorgError::Unrecoverable));
              }
              _ => return Err(err),
            };
          }
        }
    }
  }

  pub(crate) fn is_unrecoverably_reorged(&self) -> bool {
    self.unrecoverably_reorged.load(atomic::Ordering::Relaxed)
  }

  fn begin_read(&self) -> Result<rtx::Rtx> {
    Ok(rtx::Rtx(self.database.begin_read()?))
  }

  fn begin_write(&self) -> Result<WriteTransaction> {
    if cfg!(test) {
      let mut tx = self.database.begin_write()?;
      tx.set_durability(redb::Durability::None);
      Ok(tx)
    } else {
      Ok(self.database.begin_write()?)
    }
  }

  fn increment_statistic(wtx: &WriteTransaction, statistic: Statistic, n: u64) -> Result {
    let mut statistic_to_count = wtx.open_table(STATISTIC_TO_COUNT)?;
    let value = statistic_to_count
      .get(&(statistic.key()))?
      .map(|x| x.value())
      .unwrap_or(0)
      + n;
    statistic_to_count.insert(&statistic.key(), &value)?;
    Ok(())
  }

  #[cfg(test)]
  pub(crate) fn statistic(&self, statistic: Statistic) -> u64 {
    self
      .database
      .begin_read()
      .unwrap()
      .open_table(STATISTIC_TO_COUNT)
      .unwrap()
      .get(&statistic.key())
      .unwrap()
      .map(|x| x.value())
      .unwrap_or(0)
  }

  pub(crate) fn height(&self) -> Result<Option<Height>> {
    self.begin_read()?.height()
  }

  pub(crate) fn block_count(&self) -> Result<u32> {
    self.begin_read()?.block_count()
  }

  pub(crate) fn block_hash(&self, height: Option<u32>) -> Result<Option<BlockHash>> {
    self.begin_read()?.block_hash(height)
  }

  pub(crate) fn blocks(&self, take: usize) -> Result<Vec<(u32, BlockHash)>> {
    let mut blocks = Vec::new();

    let rtx = self.begin_read()?;

    let block_count = rtx.block_count()?;

    let height_to_block_hash = rtx.0.open_table(HEIGHT_TO_BLOCK_HASH)?;

    for result in height_to_block_hash.range(0..block_count)?.rev().take(take) {
      let (height, block_hash) = match result {
        Ok(value) => value,
        Err(e) => {
          return Err(e.into());
        }
      };

      blocks.push((height.value(), Entry::load(*block_hash.value())));
    }

    Ok(blocks)
  }

  pub(crate) fn rare_sat_satpoints(&self) -> Result<Vec<(Sat, SatPoint)>> {
    let rtx = self.database.begin_read()?;

    let sat_to_satpoint = rtx.open_table(SAT_TO_SATPOINT)?;

    let mut result = Vec::with_capacity(sat_to_satpoint.len()?.try_into().unwrap());

    for range in sat_to_satpoint.range(0..)? {
      let (sat, satpoint) = range?;
      result.push((Sat(sat.value()), Entry::load(*satpoint.value())));
    }

    Ok(result)
  }

  pub(crate) fn rare_sat_satpoint(&self, sat: Sat) -> Result<Option<SatPoint>> {
    Ok(
      self
        .database
        .begin_read()?
        .open_table(SAT_TO_SATPOINT)?
        .get(&sat.n())?
        .map(|satpoint| Entry::load(*satpoint.value())),
    )
  }

  pub(crate) fn get_dune_by_id(&self, id: DuneId) -> Result<Option<Dune>> {
    Ok(
      self
        .database
        .begin_read()?
        .open_table(DUNE_ID_TO_DUNE_ENTRY)?
        .get(&id.store())?
        .map(|entry| DuneEntry::load(entry.value()).dune),
    )
  }

  pub(crate) fn dune(&self, dune: Dune) -> Result<Option<(DuneId, DuneEntry)>> {
    let rtx = self.database.begin_read()?;

    let entry = match rtx.open_table(DUNE_TO_DUNE_ID)?.get(dune.0)? {
      Some(id) => rtx
        .open_table(DUNE_ID_TO_DUNE_ENTRY)?
        .get(id.value())?
        .map(|entry| (DuneId::load(id.value()), DuneEntry::load(entry.value()))),
      None => None,
    };

    Ok(entry)
  }

  pub(crate) fn dunes(&self) -> Result<Vec<(DuneId, DuneEntry)>> {
    let mut entries = Vec::new();

    for result in self
      .database
      .begin_read()?
      .open_table(DUNE_ID_TO_DUNE_ENTRY)?
      .iter()?
    {
      let (id, entry) = result?;
      entries.push((DuneId::load(id.value()), DuneEntry::load(entry.value())));
    }

    Ok(entries)
  }

  pub(crate) fn get_dune_balance(&self, outpoint: OutPoint, id: DuneId) -> Result<u128> {
    if self.block_count()? >= self.first_dune_height && self.index_dunes {
      let rtx = self.database.begin_read()?;

      let outpoint_to_balances = rtx.open_table(OUTPOINT_TO_DUNE_BALANCES)?;

      let Some(balances) = outpoint_to_balances.get(&outpoint.store())? else {
        return Ok(0);
      };

      let balances_buffer = balances.value();

      let mut i = 0;
      while i < balances_buffer.len() {
        let (balance_id, length) = dunes::varint::decode(&balances_buffer[i..]);
        i += length;
        let (amount, length) = dunes::varint::decode(&balances_buffer[i..]);
        i += length;

        if DuneId::try_from(balance_id).unwrap() == id {
          return Ok(amount);
        }
      }
    }
    Ok(0)
  }

  pub(crate) fn get_dune_balances_for_outpoint(
    &self,
    outpoint: OutPoint,
  ) -> Result<Vec<(SpacedDune, Pile)>> {
    if self.block_count()? >= self.first_dune_height && self.index_dunes {
      let rtx = &self.database.begin_read()?;

      let outpoint_to_balances = rtx.open_table(OUTPOINT_TO_DUNE_BALANCES)?;

      let id_to_dune_entries = rtx.open_table(DUNE_ID_TO_DUNE_ENTRY)?;

      let Some(balances) = outpoint_to_balances.get(&outpoint.store())? else {
        return Ok(Vec::new());
      };

      let balances_buffer = balances.value();

      let mut balances = Vec::new();
      let mut i = 0;
      while i < balances_buffer.len() {
        let (id, length) = dunes::varint::decode(&balances_buffer[i..]);
        i += length;
        let (amount, length) = dunes::varint::decode(&balances_buffer[i..]);
        i += length;

        let id = DuneId::try_from(id).unwrap();

        let entry = DuneEntry::load(id_to_dune_entries.get(id.store())?.unwrap().value());

        balances.push((
          entry.spaced_dune(),
          Pile {
            amount,
            divisibility: entry.divisibility,
            symbol: entry.symbol,
          },
        ));
      }
      Ok(balances)
    } else {
      Ok(Vec::new())
    }
  }

  pub(crate) fn get_dunic_outputs(&self, outpoints: &[OutPoint]) -> Result<BTreeSet<OutPoint>> {
    if self.block_count()? >= self.first_dune_height && self.index_dunes {
      let rtx = self.database.begin_read()?;

      let outpoint_to_balances = rtx.open_table(OUTPOINT_TO_DUNE_BALANCES)?;

      let mut dunic = BTreeSet::new();

      for outpoint in outpoints {
        if outpoint_to_balances.get(&outpoint.store())?.is_some() {
          dunic.insert(*outpoint);
        }
      }

      Ok(dunic)
    } else {
      Ok(BTreeSet::new())
    }
  }

  pub(crate) fn get_dune_balance_map(
    &self,
  ) -> Result<BTreeMap<SpacedDune, BTreeMap<OutPoint, u128>>> {
    let outpoint_balances = self.get_dune_balances()?;

    let rtx = self.database.begin_read()?;

    let dune_id_to_dune_entry = rtx.open_table(DUNE_ID_TO_DUNE_ENTRY)?;

    let mut dune_balances: BTreeMap<SpacedDune, BTreeMap<OutPoint, u128>> = BTreeMap::new();

    for (outpoint, balances) in outpoint_balances {
      for (dune_id, amount) in balances {
        let spaced_dune = DuneEntry::load(
          dune_id_to_dune_entry
            .get(&dune_id.store())?
            .unwrap()
            .value(),
        )
        .spaced_dune();

        *dune_balances
          .entry(spaced_dune)
          .or_default()
          .entry(outpoint)
          .or_default() += amount;
      }
    }

    Ok(dune_balances)
  }

  pub(crate) fn get_dune_balances(&self) -> Result<Vec<(OutPoint, Vec<(DuneId, u128)>)>> {
    let mut result = Vec::new();

    if self.block_count()? >= self.first_dune_height && self.index_dunes {
      for entry in self
        .database
        .begin_read()?
        .open_table(OUTPOINT_TO_DUNE_BALANCES)?
        .iter()?
      {
        let (outpoint, balances_buffer) = entry?;
        let outpoint = OutPoint::load(*outpoint.value());
        let balances_buffer = balances_buffer.value();

        let mut balances = Vec::new();
        let mut i = 0;
        while i < balances_buffer.len() {
          let (id, length) = dunes::varint::decode(&balances_buffer[i..]);
          i += length;
          let (balance, length) = dunes::varint::decode(&balances_buffer[i..]);
          i += length;
          balances.push((DuneId::try_from(id)?, balance));
        }

        result.push((outpoint, balances));
      }
    }
    Ok(result)
  }

  pub(crate) fn get_account_outputs(&self, address: String) -> Result<Vec<OutPoint>> {
    let mut result: Vec<OutPoint> = Vec::new();

    self
      .database
      .begin_read()?
      .open_multimap_table(ADDRESS_TO_OUTPOINT)?
      .get(address.as_bytes())?
      .for_each(|res| {
        if let Ok(item) = res {
          result.push(OutPoint::load(*item.value()));
        } else {
          println!("Error: {:?}", res.err().unwrap());
        }
      });

    Ok(result)
  }

  pub(crate) fn block_header(&self, hash: BlockHash) -> Result<Option<BlockHeader>> {
    self.client.get_block_header(&hash).into_option()
  }

  pub(crate) fn block_header_info(&self, hash: BlockHash) -> Result<Option<GetBlockHeaderResult>> {
    self.client.get_block_header_info(&hash).into_option()
  }

  pub(crate) fn get_block_by_height(&self, height: u32) -> Result<Option<Block>> {
    let tx = self.database.begin_read()?;

    let indexed = tx.open_table(HEIGHT_TO_BLOCK_HASH)?.get(&height)?.is_some();

    if !indexed {
      return Ok(None);
    }

    Ok(
      self
        .client
        .get_block_hash(height.into())
        .into_option()?
        .map(|hash| self.client.get_block(&hash))
        .transpose()?,
    )
  }

  pub(crate) fn get_block_by_hash(&self, hash: BlockHash) -> Result<Option<Block>> {
    let tx = self.database.begin_read()?;

    // check if the given hash exists as a value in the database
    let indexed =
      tx.open_table(HEIGHT_TO_BLOCK_HASH)?
        .range(0..)?
        .rev()
        .any(|result| match result {
          Ok((_, block_hash)) => block_hash.value() == hash.as_inner(),
          Err(_) => false,
        });

    if !indexed {
      return Ok(None);
    }

    self.client.get_block(&hash).into_option()
  }

  pub(crate) fn get_drc20_balances(&self, script_key: &ScriptKey) -> Result<Vec<Balance>> {
    if self.block_count().unwrap() >= self.first_inscription_height {
      let rtx = self.database.begin_read()?;

      let drc20_token_balance = rtx.open_table(DRC20_BALANCES)?;

      return Ok(
        drc20_token_balance
          .range(
            min_script_tick_key(script_key).as_str()..max_script_tick_key(script_key).as_str(),
          )?
          .flat_map(|result| {
            result.map(|(_, data)| bincode::deserialize::<Balance>(data.value()).unwrap())
          })
          .collect(),
      );
    } else {
      return Ok(vec![]);
    }
  }

  pub(crate) fn get_drc20_balance(
    &self,
    script_key: &ScriptKey,
    tick: &Tick,
  ) -> Result<Option<Balance>> {
    if self.block_count().unwrap() >= self.first_inscription_height {
      let rtx = self.database.begin_read()?;

      let drc20_token_balance = rtx.open_table(DRC20_BALANCES)?;

      return Ok(
        drc20_token_balance
          .get(script_tick_key(script_key, tick).as_str())?
          .map(|v| bincode::deserialize::<Balance>(v.value()).unwrap()),
      );
    } else {
      return Ok(None);
    }
  }

  pub(crate) fn get_drc20_token_info(&self, tick: &Tick) -> Result<Option<TokenInfo>> {
    if self.block_count().unwrap() >= self.first_inscription_height {
      let rtx = self.database.begin_read()?;

      let drc20_token_info = rtx.open_table(DRC20_TOKEN)?;
      return Ok(
        drc20_token_info
          .get(tick.to_lowercase().hex().as_str())?
          .map(|v| bincode::deserialize::<TokenInfo>(v.value()).unwrap()),
      );
    } else {
      return Ok(None);
    }
  }

  pub(crate) fn get_drc20_tokens_info(&self) -> Result<Vec<TokenInfo>> {
    if self.block_count().unwrap() >= self.first_inscription_height {
      let rtx = self.database.begin_read()?;

      let drc20_token_info = rtx.open_table(DRC20_TOKEN)?;
      return Ok(
        drc20_token_info
          .range::<&str>(..)?
          .flat_map(|result| {
            result.map(|(_, data)| bincode::deserialize::<TokenInfo>(data.value()).unwrap())
          })
          .collect(),
      );
    } else {
      return Ok(vec![]);
    }
  }

  pub(crate) fn get_drc20_token_holder(&self, tick: &Tick) -> Result<Vec<ScriptKey>> {
    if self.block_count().unwrap() >= self.first_inscription_height {
      let rtx = self.database.begin_read()?;

      let drc20_token_holder = rtx.open_multimap_table(DRC20_TOKEN_HOLDER)?;

      return Ok(
        drc20_token_holder
          .get(tick.to_lowercase().hex().as_str())?
          .flat_map(|result| {
            result.into_iter().filter_map(|(scriptKey)| {
              ScriptKey::from_str(scriptKey.value(), self.chain.network())
            })
          })
          .collect(),
      );
    } else {
      return Ok(vec![]);
    }
  }

  pub(crate) fn get_drc20_transferable_by_range(
    &self,
    script: &ScriptKey,
  ) -> Result<Vec<TransferableLog>, redb::Error> {
    let rtx = self.database.begin_read()?;
    let drc20_transferable_log = rtx.open_table(DRC20_TRANSFERABLELOG)?;
    let result = Ok(
      drc20_transferable_log
        .range(min_script_tick_key(script).as_str()..max_script_tick_key(script).as_str())?
        .flat_map(|result| {
          result.map(|(_, v)| rmp_serde::from_slice::<TransferableLog>(v.value()).unwrap())
        })
        .collect(),
    );

    result
  }

  pub(crate) fn get_drc20_transferable_by_tick(
    &self,
    script: &ScriptKey,
    tick: &Tick,
  ) -> Result<Vec<TransferableLog>, redb::Error> {
    let rtx = self.database.begin_read()?;
    let drc20_transferable_log = rtx.open_table(DRC20_TRANSFERABLELOG)?;
    let result = Ok(
      drc20_transferable_log
        .range(
          min_script_tick_id_key(script, tick).as_str()
            ..max_script_tick_id_key(script, tick).as_str(),
        )?
        .flat_map(|result| {
          result.map(|(_, v)| rmp_serde::from_slice::<TransferableLog>(v.value()).unwrap())
        })
        .collect(),
    );

    result
  }

  pub(crate) fn get_drc20_transferable_by_id(
    &self,
    script_key: &ScriptKey,
    inscription_ids: &[InscriptionId],
  ) -> Result<HashMap<InscriptionId, Option<TransferableLog>>, redb::Error> {
    if self.block_count().unwrap() >= self.first_inscription_height {
      let rtx = self.database.begin_read()?;

      let drc20_transferable_log = rtx.open_table(DRC20_TRANSFERABLELOG)?;

      let transferable_log_vec: Vec<TransferableLog> = drc20_transferable_log
        .range(min_script_tick_key(script_key).as_str()..max_script_tick_key(script_key).as_str())?
        .flat_map(|result| {
          result.map(|(_, v)| rmp_serde::from_slice::<TransferableLog>(v.value()).unwrap())
        })
        .collect();

      Ok(
        inscription_ids
          .iter()
          .map(|id| {
            let transferable_log = transferable_log_vec
              .iter()
              .find(|log| log.inscription_id == *id)
              .cloned();
            (*id, transferable_log)
          })
          .collect(),
      )
    } else {
      return Ok(inscription_ids.iter().map(|id| (*id, None)).collect());
    }
  }

  pub(crate) fn get_etching(&self, txid: Txid) -> Result<Option<SpacedDune>> {
    if self.block_count().unwrap() >= self.first_dune_height {
      let rtx = self.database.begin_read()?;

      let transaction_id_to_dune = rtx.open_table(TRANSACTION_ID_TO_DUNE)?;
      let Some(dune) = transaction_id_to_dune.get(&txid.store())? else {
        return Ok(None);
      };

      let dune_to_dune_id = rtx.open_table(DUNE_TO_DUNE_ID)?;
      let id = dune_to_dune_id.get(dune.value())?.unwrap();

      let dune_id_to_dune_entry = rtx.open_table(DUNE_ID_TO_DUNE_ENTRY)?;
      let entry = dune_id_to_dune_entry.get(&id.value())?.unwrap();

      Ok(Some(DuneEntry::load(entry.value()).spaced_dune()))
    } else {
      Ok(None)
    }
  }

  pub(crate) fn get_inscription_id_by_sat(&self, sat: Sat) -> Result<Option<InscriptionId>> {
    Ok(
      self
        .database
        .begin_read()?
        .open_table(SAT_TO_INSCRIPTION_ID)?
        .get(&sat.n())?
        .map(|inscription_id| Entry::load(*inscription_id.value())),
    )
  }

  pub(crate) fn get_dune_by_inscription_id(
    &self,
    inscription_id: InscriptionId,
  ) -> Result<Option<SpacedDune>> {
    let rtx = self.database.begin_read()?;
    let Some(dune) = rtx
      .open_table(INSCRIPTION_ID_TO_DUNE)?
      .get(&inscription_id.store())?
      .map(|entry| Dune(entry.value()))
    else {
      return Ok(None);
    };
    let dune_to_dune_id = rtx.open_table(DUNE_TO_DUNE_ID)?;
    let id = dune_to_dune_id.get(dune.0)?.unwrap();

    let dune_id_to_dune_entry = rtx.open_table(DUNE_ID_TO_DUNE_ENTRY)?;
    let entry = dune_id_to_dune_entry.get(&id.value())?.unwrap();

    Ok(Some(DuneEntry::load(entry.value()).spaced_dune()))
  }

  pub(crate) fn get_inscription_id_by_inscription_number(
    &self,
    n: u64,
  ) -> Result<Option<InscriptionId>> {
    Ok(
      self
        .database
        .begin_read()?
        .open_table(INSCRIPTION_NUMBER_TO_INSCRIPTION_ID)?
        .get(&n)?
        .map(|id| Entry::load(*id.value())),
    )
  }

  pub(crate) fn get_inscription_satpoint_by_id(
    &self,
    inscription_id: InscriptionId,
  ) -> Result<Option<SatPoint>> {
    Ok(
      self
        .database
        .begin_read()?
        .open_table(INSCRIPTION_ID_TO_SATPOINT)?
        .get(&inscription_id.store())?
        .map(|satpoint| Entry::load(*satpoint.value())),
    )
  }

  pub(crate) fn get_inscription_by_id(
    &self,
    inscription_id: InscriptionId,
  ) -> Result<Option<Inscription>> {
    if self
      .database
      .begin_read()?
      .open_table(INSCRIPTION_ID_TO_SATPOINT)?
      .get(&inscription_id.store())?
      .is_none()
    {
      return Ok(None);
    }

    let reader = self.database.begin_read()?;

    let table = reader.open_table(INSCRIPTION_ID_TO_TXIDS)?;
    let txids_result = table.get(&inscription_id.store())?;

    match txids_result {
      Some(txids) => {
        let mut txs = vec![];

        let txids = txids.value();

        for i in 0..txids.len() / 32 {
          let txid_buf = &txids[i * 32..i * 32 + 32];
          let table = reader.open_table(INSCRIPTION_TXID_TO_TX)?;
          let tx_result = table.get(txid_buf)?;

          match tx_result {
            Some(tx_result) => {
              let tx_buf = tx_result.value().to_vec();
              let mut cursor = Cursor::new(tx_buf);
              let tx = bitcoin::Transaction::consensus_decode(&mut cursor)?;
              txs.push(tx);
            }
            None => return Ok(None),
          }
        }

        let parsed_inscription = Inscription::from_transactions(txs);

        match parsed_inscription {
          ParsedInscription::None => return Ok(None),
          ParsedInscription::Partial => return Ok(None),
          ParsedInscription::Complete(inscription) => Ok(Some(inscription)),
        }
      }

      None => return Ok(None),
    }
  }

  pub(crate) fn inscription_exists(&self, inscription_id: InscriptionId) -> Result<bool> {
    Ok(
      self
        .database
        .begin_read()?
        .open_table(INSCRIPTION_ID_TO_SATPOINT)?
        .get(&inscription_id.store())?
        .is_some(),
    )
  }

  pub(crate) fn inscription_count(&self, txid: Txid) -> Result<u32> {
    let start_id = InscriptionId { index: 0, txid };

    let end_id = InscriptionId {
      index: u32::MAX,
      txid,
    };

    Ok(
      self
        .database
        .begin_read()?
        .open_table(INSCRIPTION_ID_TO_SATPOINT)?
        .range::<&InscriptionIdValue>(&start_id.store()..&end_id.store())?
        .count()
        .try_into()?,
    )
  }

  pub(crate) fn get_inscriptions_on_output(
    &self,
    outpoint: OutPoint,
  ) -> Result<Vec<InscriptionId>> {
    Self::inscriptions_on_output(
      &self
        .database
        .begin_read()?
        .open_table(SATPOINT_TO_INSCRIPTION_ID)?,
      outpoint,
    )?
    .into_iter()
    .map(|result| {
      result
        .map(|(_satpoint, inscription_id)| inscription_id)
        .map_err(|e| e.into())
    })
    .collect()
  }

  pub(crate) fn get_transaction(&self, txid: Txid) -> Result<Option<Transaction>> {
    if txid == self.genesis_block_coinbase_txid {
      return Ok(Some(self.genesis_block_coinbase_transaction.clone()));
    }

    if self.index_transactions {
      if let Some(transaction) = self
        .database
        .begin_read()?
        .open_table(TRANSACTION_ID_TO_TRANSACTION)?
        .get(&txid.store())?
      {
        return Ok(Some(consensus::encode::deserialize(transaction.value())?));
      }
    }

    if let Ok(tx) = self.client.get_raw_transaction(&txid) {
      Ok(Some(tx))
    } else {
      Ok(None)
    }
  }

  pub(crate) fn get_network(&self) -> Result<Network> {
    Ok(self.chain.network())
  }

  pub(crate) fn get_transaction_blockhash(
    &self,
    txid: Txid,
  ) -> Result<Option<BlockHashAndConfirmations>> {
    if let Ok(result) = self.client.get_raw_transaction_info(&txid) {
      Ok(Some(BlockHashAndConfirmations {
        hash: result.blockhash,
        confirmations: result.confirmations,
      }))
    } else {
      Ok(None)
    }
  }

  pub(crate) fn is_transaction_in_active_chain(&self, txid: Txid) -> Result<bool> {
    Ok(
      self
        .client
        .get_raw_transaction_info(&txid)
        .into_option()?
        .and_then(|info| info.in_active_chain)
        .unwrap_or(false),
    )
  }

  pub(crate) fn find(&self, sat: Sat) -> Result<Option<SatPoint>> {
    let rtx = self.begin_read()?;

    if rtx.block_count()? <= Sat(sat.0).height().n() {
      return Ok(None);
    }

    let outpoint_to_sat_ranges = rtx.0.open_table(OUTPOINT_TO_SAT_RANGES)?;

    for result in outpoint_to_sat_ranges.range::<&[u8; 36]>(&[0; 36]..)? {
      let (key, value) = match result {
        Ok(pair) => pair,
        Err(err) => {
          return Err(err.into());
        }
      };

      let mut offset = 0;
      for chunk in value.value().chunks_exact(24) {
        let (start, end) = SatRange::load(chunk.try_into().unwrap());
        if start <= sat.0 && sat.0 < end {
          return Ok(Some(SatPoint {
            outpoint: Entry::load(*key.value()),
            offset: offset + u64::try_from(sat.0 - start).unwrap(),
          }));
        }
        offset += u64::try_from(end - start).unwrap();
      }
    }

    Ok(None)
  }

  fn list_inner(&self, outpoint: OutPointValue) -> Result<Option<Vec<u8>>> {
    Ok(
      self
        .database
        .begin_read()?
        .open_table(OUTPOINT_TO_SAT_RANGES)?
        .get(&outpoint)?
        .map(|outpoint| outpoint.value().to_vec()),
    )
  }

  pub(crate) fn list(&self, outpoint: OutPoint) -> Result<Option<List>> {
    if !self.index_sats {
      return Ok(None);
    }

    let array = outpoint.store();

    let sat_ranges = self.list_inner(array)?;

    match sat_ranges {
      Some(sat_ranges) => Ok(Some(List::Unspent(
        sat_ranges
          .chunks_exact(24)
          .map(|chunk| SatRange::load(chunk.try_into().unwrap()))
          .collect(),
      ))),
      None => {
        if self.is_transaction_in_active_chain(outpoint.txid)? {
          Ok(Some(List::Spent))
        } else {
          Ok(None)
        }
      }
    }
  }

  pub(crate) fn blocktime(&self, height: Height) -> Result<Blocktime> {
    let height = height.n();

    match self.get_block_by_height(height)? {
      Some(block) => Ok(Blocktime::confirmed(block.header.time)),
      None => {
        let tx = self.database.begin_read()?;

        let current = tx
          .open_table(HEIGHT_TO_BLOCK_HASH)?
          .range(0..)?
          .rev()
          .next()
          .map(|result| match result {
            Ok((height, _hash)) => Some(height.value()),
            Err(_) => None,
          })
          .flatten()
          .unwrap_or(0);

        let expected_blocks = height.checked_sub(current).with_context(|| {
          format!("current {current} height is greater than sat height {height}")
        })?;

        Ok(Blocktime::Expected(
          Utc::now()
            .round_subsecs(0)
            .checked_add_signed(chrono::Duration::seconds(
              10 * 60 * i64::try_from(expected_blocks)?,
            ))
            .ok_or_else(|| anyhow!("block timestamp out of range"))?,
        ))
      }
    }
  }

  pub(crate) fn get_inscriptions(
    &self,
    n: Option<usize>,
  ) -> Result<BTreeMap<SatPoint, InscriptionId>> {
    self
      .database
      .begin_read()?
      .open_table(SATPOINT_TO_INSCRIPTION_ID)?
      .range::<&[u8; 44]>(&[0; 44]..)?
      .map(|result| {
        result
          .map(|(satpoint, id)| (Entry::load(*satpoint.value()), Entry::load(*id.value())))
          .map_err(|e| e.into())
      })
      .take(n.unwrap_or(usize::MAX))
      .collect()
  }

  pub(crate) fn get_homepage_inscriptions(&self) -> Result<Vec<InscriptionId>> {
    self
      .database
      .begin_read()?
      .open_table(INSCRIPTION_NUMBER_TO_INSCRIPTION_ID)?
      .iter()?
      .rev()
      .take(8)
      .map(|result| {
        result
          .map(|(_number, id)| Entry::load(*id.value()))
          .map_err(|e| e.into())
      })
      .collect()
  }

  pub(crate) fn get_latest_inscriptions_with_prev_and_next(
    &self,
    n: usize,
    from: Option<u64>,
  ) -> Result<(Vec<InscriptionId>, Option<u64>, Option<u64>)> {
    let rtx = self.database.begin_read()?;

    let inscription_number_to_inscription_id =
      rtx.open_table(INSCRIPTION_NUMBER_TO_INSCRIPTION_ID)?;

    let latest = match inscription_number_to_inscription_id.iter()?.rev().next() {
      Some(result) => match result {
        Ok((number, _id)) => number.value(),
        Err(err) => return Err(err.into()),
      },
      None => return Ok(Default::default()),
    };

    let from = from.unwrap_or(latest);

    let prev = if let Some(prev) = from.checked_sub(n.try_into()?) {
      inscription_number_to_inscription_id
        .get(&prev)?
        .map(|_| prev)
    } else {
      None
    };

    let next = if from < latest {
      Some(
        from
          .checked_add(n.try_into()?)
          .unwrap_or(latest)
          .min(latest),
      )
    } else {
      None
    };

    let inscriptions = inscription_number_to_inscription_id
      .range(..=from)?
      .rev()
      .take(n)
      .map(|result| {
        result
          .map(|(_number, id)| Entry::load(*id.value()))
          .map_err(|e| e.into())
      })
      .collect::<Result<Vec<InscriptionId>>>()?;

    Ok((inscriptions, prev, next))
  }

  pub(crate) fn get_feed_inscriptions(&self, n: usize) -> Result<Vec<(u64, InscriptionId)>> {
    self
      .database
      .begin_read()?
      .open_table(INSCRIPTION_NUMBER_TO_INSCRIPTION_ID)?
      .iter()?
      .rev()
      .take(n)
      .map(|result| {
        result
          .map(|(number, id)| (number.value(), Entry::load(*id.value())))
          .map_err(|e| e.into())
      })
      .collect()
  }

  pub(crate) fn get_inscription_entry(
    &self,
    inscription_id: InscriptionId,
  ) -> Result<Option<InscriptionEntry>> {
    Ok(
      self
        .database
        .begin_read()?
        .open_table(INSCRIPTION_ID_TO_INSCRIPTION_ENTRY)?
        .get(&inscription_id.store())?
        .map(|value| InscriptionEntry::load(value.value())),
    )
  }

  #[cfg(test)]
  fn assert_inscription_location(
    &self,
    inscription_id: InscriptionId,
    satpoint: SatPoint,
    sat: u128,
  ) {
    let rtx = self.database.begin_read().unwrap();

    let satpoint_to_inscription_id = rtx.open_table(SATPOINT_TO_INSCRIPTION_ID).unwrap();

    let inscription_id_to_satpoint = rtx.open_table(INSCRIPTION_ID_TO_SATPOINT).unwrap();

    assert_eq!(
      satpoint_to_inscription_id.len().unwrap(),
      inscription_id_to_satpoint.len().unwrap(),
    );

    assert_eq!(
      SatPoint::load(
        *inscription_id_to_satpoint
          .get(&inscription_id.store())
          .unwrap()
          .unwrap()
          .value()
      ),
      satpoint,
    );

    assert_eq!(
      InscriptionId::load(
        *satpoint_to_inscription_id
          .get(&satpoint.store())
          .unwrap()
          .unwrap()
          .value()
      ),
      inscription_id,
    );

    if self.has_sat_index().unwrap() {
      assert_eq!(
        InscriptionId::load(
          *rtx
            .open_table(SAT_TO_INSCRIPTION_ID)
            .unwrap()
            .get(&sat)
            .unwrap()
            .unwrap()
            .value()
        ),
        inscription_id,
      );

      assert_eq!(
        SatPoint::load(
          *rtx
            .open_table(SAT_TO_SATPOINT)
            .unwrap()
            .get(&sat)
            .unwrap()
            .unwrap()
            .value()
        ),
        satpoint,
      );
    }
  }

  #[cfg(test)]
  fn assert_non_existence_of_inscription(&self, inscription_id: InscriptionId) {
    let rtx = self.database.begin_read().unwrap();

    let inscription_id_to_satpoint = rtx.open_table(INSCRIPTION_ID_TO_SATPOINT).unwrap();
    assert!(inscription_id_to_satpoint
      .get(&inscription_id.store())
      .unwrap()
      .is_none());

    let inscription_id_to_entry = rtx.open_table(INSCRIPTION_ID_TO_INSCRIPTION_ENTRY).unwrap();
    assert!(inscription_id_to_entry
      .get(&inscription_id.store())
      .unwrap()
      .is_none());

    for range in rtx
      .open_table(INSCRIPTION_NUMBER_TO_INSCRIPTION_ID)
      .unwrap()
      .iter()
      .into_iter()
    {
      for entry in range.into_iter() {
        let (_number, id) = entry.unwrap();
        assert!(InscriptionId::load(*id.value()) != inscription_id);
      }
    }

    for range in rtx
      .open_table(SATPOINT_TO_INSCRIPTION_ID)
      .unwrap()
      .iter()
      .into_iter()
    {
      for entry in range.into_iter() {
        let (_satpoint, ids) = entry.unwrap();
        assert!(!ids
          .into_iter()
          .any(|id| InscriptionId::load(*id.unwrap().value()) == inscription_id))
      }
    }

    if self.has_sat_index().unwrap() {
      for range in rtx
        .open_table(SAT_TO_INSCRIPTION_ID)
        .unwrap()
        .iter()
        .into_iter()
      {
        for entry in range.into_iter() {
          let (_sat, ids) = entry.unwrap();
          assert!(!ids
            .into_iter()
            .any(|id| InscriptionId::load(*id.unwrap().value()) == inscription_id))
        }
      }
    }
  }

  fn inscriptions_on_output<'a: 'tx, 'tx>(
    satpoint_to_id: &'a impl ReadableTable<&'static SatPointValue, &'static InscriptionIdValue>,
    outpoint: OutPoint,
  ) -> Result<impl Iterator<Item = Result<(SatPoint, InscriptionId), StorageError>> + 'tx> {
    let start = SatPoint {
      outpoint,
      offset: 0,
    }
    .store();

    let end = SatPoint {
      outpoint,
      offset: u64::MAX,
    }
    .store();

    Ok(
      satpoint_to_id
        .range::<&[u8; 44]>(&start..=&end)?
        .map(|result| {
          result.map(|(satpoint, id)| (Entry::load(*satpoint.value()), Entry::load(*id.value())))
        }),
    )
  }
}

#[cfg(test)]
mod tests {
  use {
    bitcoin::secp256k1::rand::{self, RngCore},
    crate::index::testing::Context,
    super::*,
  };

  #[test]
  fn height_limit() {
    {
      let context = Context::builder().args(["--height-limit", "0"]).build();
      context.mine_blocks(1);
      assert_eq!(context.index.height().unwrap(), None);
      assert_eq!(context.index.block_count().unwrap(), 0);
    }

    {
      let context = Context::builder().args(["--height-limit", "1"]).build();
      context.mine_blocks(1);
      assert_eq!(context.index.height().unwrap(), Some(Height(0)));
      assert_eq!(context.index.block_count().unwrap(), 1);
    }

    {
      let context = Context::builder().args(["--height-limit", "2"]).build();
      context.mine_blocks(2);
      assert_eq!(context.index.height().unwrap(), Some(Height(1)));
      assert_eq!(context.index.block_count().unwrap(), 2);
    }
  }

  #[test]
  fn inscriptions_below_first_inscription_height_are_skipped() {
    let inscription = inscription("text/plain;charset=utf-8", "hello");
    let template = TransactionTemplate {
      inputs: &[(1, 0, 0)],
      witness: inscription.to_witness(),
      ..Default::default()
    };

    {
      let context = Context::builder().build();
      context.mine_blocks(1);
      let txid = context.rpc_server.broadcast_tx(template.clone());
      let inscription_id = InscriptionId::from(txid);
      context.mine_blocks(1);

      assert_eq!(
        context.index.get_inscription_by_id(inscription_id).unwrap(),
        Some(inscription)
      );

      assert_eq!(
        context
          .index
          .get_inscription_satpoint_by_id(inscription_id)
          .unwrap(),
        Some(SatPoint {
          outpoint: OutPoint { txid, vout: 0 },
          offset: 0,
        })
      );
    }

    {
      let context = Context::builder()
        .arg("--first-inscription-height=3")
        .build();
      context.mine_blocks(1);
      let txid = context.rpc_server.broadcast_tx(template);
      let inscription_id = InscriptionId::from(txid);
      context.mine_blocks(1);

      assert_eq!(
        context
          .index
          .get_inscription_satpoint_by_id(inscription_id)
          .unwrap(),
        None,
      );
    }
  }

  #[test]
  #[ignore]
  fn list_first_coinbase_transaction() {
    let context = Context::builder().arg("--index-sats").build();
    assert_eq!(
      context
        .index
        .list(
          "1a91e3dace36e2be3bf030a65679fe821aa1d6ef92e7c9902eb318182c355691:0"
            .parse()
            .unwrap()
        )
        .unwrap()
        .unwrap(),
      List::Unspent(vec![(0, 50 * COIN_VALUE as u128)])
    )
  }

  #[test]
  #[ignore]
  fn list_second_coinbase_transaction() {
    let context = Context::builder().arg("--index-sats").build();
    let txid = context.mine_blocks(1)[0].txdata[0].txid();
    assert_eq!(
      context.index.list(OutPoint::new(txid, 0)).unwrap().unwrap(),
      List::Unspent(vec![(50 * COIN_VALUE as u128, 100 * COIN_VALUE as u128)])
    )
  }

  #[test]
  #[ignore]
  fn list_split_ranges_are_tracked_correctly() {
    let context = Context::builder().arg("--index-sats").build();

    context.mine_blocks(1);
    let split_coinbase_output = TransactionTemplate {
      inputs: &[(1, 0, 0)],
      outputs: 2,
      fee: 0,
      ..Default::default()
    };
    let txid = context.rpc_server.broadcast_tx(split_coinbase_output);

    context.mine_blocks(1);

    assert_eq!(
      context.index.list(OutPoint::new(txid, 0)).unwrap().unwrap(),
      List::Unspent(vec![(50 * COIN_VALUE as u128, 75 * COIN_VALUE as u128)])
    );

    assert_eq!(
      context.index.list(OutPoint::new(txid, 1)).unwrap().unwrap(),
      List::Unspent(vec![(75 * COIN_VALUE as u128, 100 * COIN_VALUE as u128)])
    );
  }

  #[test]
  #[ignore]
  fn list_merge_ranges_are_tracked_correctly() {
    let context = Context::builder().arg("--index-sats").build();

    context.mine_blocks(2);
    let merge_coinbase_outputs = TransactionTemplate {
      inputs: &[(1, 0, 0), (2, 0, 0)],
      fee: 0,
      ..Default::default()
    };

    let txid = context.rpc_server.broadcast_tx(merge_coinbase_outputs);
    context.mine_blocks(1);

    assert_eq!(
      context.index.list(OutPoint::new(txid, 0)).unwrap().unwrap(),
      List::Unspent(vec![
        (50 * COIN_VALUE as u128, 100 * COIN_VALUE as u128),
        (100 * COIN_VALUE as u128, 150 * COIN_VALUE as u128)
      ]),
    );
  }

  #[test]
  #[ignore]
  fn list_fee_paying_transaction_range() {
    let context = Context::builder().arg("--index-sats").build();

    context.mine_blocks(1);
    let fee_paying_tx = TransactionTemplate {
      inputs: &[(1, 0, 0)],
      outputs: 2,
      fee: 10,
      ..Default::default()
    };
    let txid = context.rpc_server.broadcast_tx(fee_paying_tx);
    let coinbase_txid = context.mine_blocks(1)[0].txdata[0].txid();

    assert_eq!(
      context.index.list(OutPoint::new(txid, 0)).unwrap().unwrap(),
      List::Unspent(vec![(50 * COIN_VALUE as u128, 7499999995)]),
    );

    assert_eq!(
      context.index.list(OutPoint::new(txid, 1)).unwrap().unwrap(),
      List::Unspent(vec![(7499999995, 9999999990)]),
    );

    assert_eq!(
      context
        .index
        .list(OutPoint::new(coinbase_txid, 0))
        .unwrap()
        .unwrap(),
      List::Unspent(vec![(10000000000, 15000000000), (9999999990, 10000000000)])
    );
  }

  #[test]
  #[ignore]
  fn list_two_fee_paying_transaction_range() {
    let context = Context::builder().arg("--index-sats").build();

    context.mine_blocks(2);
    let first_fee_paying_tx = TransactionTemplate {
      inputs: &[(1, 0, 0)],
      fee: 10,
      ..Default::default()
    };
    let second_fee_paying_tx = TransactionTemplate {
      inputs: &[(2, 0, 0)],
      fee: 10,
      ..Default::default()
    };
    context.rpc_server.broadcast_tx(first_fee_paying_tx);
    context.rpc_server.broadcast_tx(second_fee_paying_tx);

    let coinbase_txid = context.mine_blocks(1)[0].txdata[0].txid();

    assert_eq!(
      context
        .index
        .list(OutPoint::new(coinbase_txid, 0))
        .unwrap()
        .unwrap(),
      List::Unspent(vec![
        (15000000000, 20000000000),
        (9999999990, 10000000000),
        (14999999990, 15000000000)
      ])
    );
  }

  #[test]
  #[ignore]
  fn list_null_output() {
    let context = Context::builder().arg("--index-sats").build();

    context.mine_blocks(1);
    let no_value_output = TransactionTemplate {
      inputs: &[(1, 0, 0)],
      fee: 50 * COIN_VALUE,
      ..Default::default()
    };
    let txid = context.rpc_server.broadcast_tx(no_value_output);
    context.mine_blocks(1);

    assert_eq!(
      context.index.list(OutPoint::new(txid, 0)).unwrap().unwrap(),
      List::Unspent(Vec::new())
    );
  }

  #[test]
  #[ignore]
  fn list_null_input() {
    let context = Context::builder().arg("--index-sats").build();

    context.mine_blocks(1);
    let no_value_output = TransactionTemplate {
      inputs: &[(1, 0, 0)],
      fee: 50 * COIN_VALUE,
      ..Default::default()
    };
    context.rpc_server.broadcast_tx(no_value_output);
    context.mine_blocks(1);

    let no_value_input = TransactionTemplate {
      inputs: &[(2, 1, 0)],
      fee: 0,
      ..Default::default()
    };
    let txid = context.rpc_server.broadcast_tx(no_value_input);
    context.mine_blocks(1);

    assert_eq!(
      context.index.list(OutPoint::new(txid, 0)).unwrap().unwrap(),
      List::Unspent(Vec::new())
    );
  }

  #[test]
  fn list_spent_output() {
    let context = Context::builder().arg("--index-sats").build();
    context.mine_blocks(1);
    context.rpc_server.broadcast_tx(TransactionTemplate {
      inputs: &[(1, 0, 0)],
      fee: 0,
      ..Default::default()
    });
    context.mine_blocks(1);
    let txid = context.rpc_server.tx(1, 0).txid();
    assert_eq!(
      context.index.list(OutPoint::new(txid, 0)).unwrap().unwrap(),
      List::Spent,
    );
  }

  #[test]
  fn list_unknown_output() {
    let context = Context::builder().arg("--index-sats").build();

    assert_eq!(
      context
        .index
        .list(
          "0000000000000000000000000000000000000000000000000000000000000000:0"
            .parse()
            .unwrap()
        )
        .unwrap(),
      None
    );
  }

  #[test]
  #[ignore]
  fn find_first_sat() {
    let context = Context::builder().arg("--index-sats").build();
    assert_eq!(
      context.index.find(0).unwrap().unwrap(),
      SatPoint {
        outpoint: "1a91e3dace36e2be3bf030a65679fe821aa1d6ef92e7c9902eb318182c355691:0"
          .parse()
          .unwrap(),
        offset: 0,
      }
    )
  }

  #[test]
  #[ignore]
  fn find_second_sat() {
    let context = Context::builder().arg("--index-sats").build();
    assert_eq!(
      context.index.find(1).unwrap().unwrap(),
      SatPoint {
        outpoint: "1a91e3dace36e2be3bf030a65679fe821aa1d6ef92e7c9902eb318182c355691:0"
          .parse()
          .unwrap(),
        offset: 1,
      }
    )
  }

  #[test]
  #[ignore]
  fn find_first_sat_of_second_block() {
    let context = Context::builder().arg("--index-sats").build();
    context.mine_blocks(1);
    assert_eq!(
      context
        .index
        .find(50 * COIN_VALUE as u128)
        .unwrap()
        .unwrap(),
      SatPoint {
        outpoint: "30f2f037629c6a21c1f40ed39b9bd6278df39762d68d07f49582b23bcb23386a:0"
          .parse()
          .unwrap(),
        offset: 0,
      }
    )
  }

  #[test]
  #[ignore]
  fn find_unmined_sat() {
    let context = Context::builder().arg("--index-sats").build();
    assert_eq!(context.index.find(50 * COIN_VALUE as u128).unwrap(), None);
  }

  #[test]
  #[ignore]
  fn find_first_sat_spent_in_second_block() {
    let context = Context::builder().arg("--index-sats").build();
    context.mine_blocks(1);
    let spend_txid = context.rpc_server.broadcast_tx(TransactionTemplate {
      inputs: &[(1, 0, 0)],
      fee: 0,
      ..Default::default()
    });
    context.mine_blocks(1);
    assert_eq!(
      context
        .index
        .find(50 * COIN_VALUE as u128)
        .unwrap()
        .unwrap(),
      SatPoint {
        outpoint: OutPoint::new(spend_txid, 0),
        offset: 0,
      }
    )
  }

  #[test]
  #[ignore]
  fn inscriptions_are_tracked_correctly() {
    for context in Context::configurations() {
      context.mine_blocks(1);

      let txid = context.rpc_server.broadcast_tx(TransactionTemplate {
        inputs: &[(1, 0, 0)],
        witness: inscription("text/plain", "hello").to_witness(),
        ..Default::default()
      });
      let inscription_id = InscriptionId::from(txid);

      context.mine_blocks(1);

      context.index.assert_inscription_location(
        inscription_id,
        SatPoint {
          outpoint: OutPoint { txid, vout: 0 },
          offset: 0,
        },
        50 * COIN_VALUE as u128,
      );
    }
  }

  #[test]
  #[ignore]
  fn unaligned_inscriptions_are_tracked_correctly() {
    for context in Context::configurations() {
      context.mine_blocks(1);

      let txid = context.rpc_server.broadcast_tx(TransactionTemplate {
        inputs: &[(1, 0, 0)],
        witness: inscription("text/plain", "hello").to_witness(),
        ..Default::default()
      });
      let inscription_id = InscriptionId::from(txid);

      context.mine_blocks(1);

      context.index.assert_inscription_location(
        inscription_id,
        SatPoint {
          outpoint: OutPoint { txid, vout: 0 },
          offset: 0,
        },
        50 * COIN_VALUE as u128,
      );

      let send_txid = context.rpc_server.broadcast_tx(TransactionTemplate {
        inputs: &[(2, 0, 0), (2, 1, 0)],
        ..Default::default()
      });

      context.mine_blocks(1);

      context.index.assert_inscription_location(
        inscription_id,
        SatPoint {
          outpoint: OutPoint {
            txid: send_txid,
            vout: 0,
          },
          offset: 50 * COIN_VALUE,
        },
        50 * COIN_VALUE as u128,
      );
    }
  }

  #[test]
  #[ignore]
  fn merged_inscriptions_are_tracked_correctly() {
    for context in Context::configurations() {
      context.mine_blocks(2);

      let first_txid = context.rpc_server.broadcast_tx(TransactionTemplate {
        inputs: &[(1, 0, 0)],
        witness: inscription("text/plain", "hello").to_witness(),
        ..Default::default()
      });

      let first_inscription_id = InscriptionId::from(first_txid);

      let second_txid = context.rpc_server.broadcast_tx(TransactionTemplate {
        inputs: &[(2, 0, 0)],
        witness: inscription("text/png", [1; 100]).to_witness(),
        ..Default::default()
      });
      let second_inscription_id = InscriptionId::from(second_txid);

      context.mine_blocks(1);

      let merged_txid = context.rpc_server.broadcast_tx(TransactionTemplate {
        inputs: &[(3, 1, 0), (3, 2, 0)],
        ..Default::default()
      });

      context.mine_blocks(1);

      context.index.assert_inscription_location(
        first_inscription_id,
        SatPoint {
          outpoint: OutPoint {
            txid: merged_txid,
            vout: 0,
          },
          offset: 0,
        },
        50 * COIN_VALUE as u128,
      );

      context.index.assert_inscription_location(
        second_inscription_id,
        SatPoint {
          outpoint: OutPoint {
            txid: merged_txid,
            vout: 0,
          },
          offset: 50 * COIN_VALUE,
        },
        100 * COIN_VALUE as u128,
      );
    }
  }

  #[test]
  #[ignore]
  fn inscriptions_that_are_sent_to_second_output_are_are_tracked_correctly() {
    for context in Context::configurations() {
      context.mine_blocks(1);

      let txid = context.rpc_server.broadcast_tx(TransactionTemplate {
        inputs: &[(1, 0, 0)],
        witness: inscription("text/plain", "hello").to_witness(),
        ..Default::default()
      });
      let inscription_id = InscriptionId::from(txid);

      context.mine_blocks(1);

      context.index.assert_inscription_location(
        inscription_id,
        SatPoint {
          outpoint: OutPoint { txid, vout: 0 },
          offset: 0,
        },
        50 * COIN_VALUE as u128,
      );

      let send_txid = context.rpc_server.broadcast_tx(TransactionTemplate {
        inputs: &[(2, 0, 0), (2, 1, 0)],
        outputs: 2,
        ..Default::default()
      });

      context.mine_blocks(1);

      context.index.assert_inscription_location(
        inscription_id,
        SatPoint {
          outpoint: OutPoint {
            txid: send_txid,
            vout: 1,
          },
          offset: 0,
        },
        50 * COIN_VALUE as u128,
      );
    }
  }

  #[test]
  fn two_input_fee_spent_inscriptions_are_tracked_correctly() {
    for context in Context::configurations() {
      context.mine_blocks(2);

      let txid = context.rpc_server.broadcast_tx(TransactionTemplate {
        inputs: &[(1, 0, 0)],
        witness: inscription("text/plain", "hello").to_witness(),
        ..Default::default()
      });
      let inscription_id = InscriptionId::from(txid);

      context.mine_blocks(1);

      context.rpc_server.broadcast_tx(TransactionTemplate {
        inputs: &[(2, 0, 0), (3, 1, 0)],
        fee: 50 * COIN_VALUE,
        ..Default::default()
      });

      let coinbase_tx = context.mine_blocks(1)[0].txdata[0].txid();

      context.index.assert_inscription_location(
        inscription_id,
        SatPoint {
          outpoint: OutPoint {
            txid: coinbase_tx,
            vout: 0,
          },
          offset: 50 * COIN_VALUE,
        },
        50 * COIN_VALUE,
      );
    }
  }

  #[test]
  #[ignore]
  fn missing_inputs_are_fetched_from_dogecoin_core() {
    for args in [
      ["--first-inscription-height", "2"].as_slice(),
      ["--first-inscription-height", "2", "--index-sats"].as_slice(),
    ] {
      let context = Context::builder().args(args).build();
      context.mine_blocks(1);

      let txid = context.rpc_server.broadcast_tx(TransactionTemplate {
        inputs: &[(1, 0, 0)],
        witness: inscription("text/plain", "hello").to_witness(),
        ..Default::default()
      });
      let inscription_id = InscriptionId::from(txid);

      context.mine_blocks(1);

      context.index.assert_inscription_location(
        inscription_id,
        SatPoint {
          outpoint: OutPoint { txid, vout: 0 },
          offset: 0,
        },
        50 * COIN_VALUE as u128,
      );

      let send_txid = context.rpc_server.broadcast_tx(TransactionTemplate {
        inputs: &[(2, 0, 0), (2, 1, 0)],
        ..Default::default()
      });

      context.mine_blocks(1);

      context.index.assert_inscription_location(
        inscription_id,
        SatPoint {
          outpoint: OutPoint {
            txid: send_txid,
            vout: 0,
          },
          offset: 50 * COIN_VALUE,
        },
        50 * COIN_VALUE as u128,
      );
    }
  }

  #[test]
  #[ignore]
  fn one_input_fee_spent_inscriptions_are_tracked_correctly() {
    for context in Context::configurations() {
      context.mine_blocks(1);

      let txid = context.rpc_server.broadcast_tx(TransactionTemplate {
        inputs: &[(1, 0, 0)],
        witness: inscription("text/plain", "hello").to_witness(),
        ..Default::default()
      });
      let inscription_id = InscriptionId::from(txid);

      context.mine_blocks(1);

      context.rpc_server.broadcast_tx(TransactionTemplate {
        inputs: &[(2, 1, 0)],
        fee: 50 * COIN_VALUE,
        ..Default::default()
      });

      let coinbase_tx = context.mine_blocks(1)[0].txdata[0].txid();

      context.index.assert_inscription_location(
        inscription_id,
        SatPoint {
          outpoint: OutPoint {
            txid: coinbase_tx,
            vout: 0,
          },
          offset: 50 * COIN_VALUE,
        },
        50 * COIN_VALUE as u128,
      );
    }
  }

  #[test]
  #[ignore]
  fn inscription_can_be_fee_spent_in_first_transaction() {
    for context in Context::configurations() {
      context.mine_blocks(1);

      let txid = context.rpc_server.broadcast_tx(TransactionTemplate {
        inputs: &[(1, 0, 0)],
        fee: 50 * COIN_VALUE,
        witness: inscription("text/plain", "hello").to_witness(),
        ..Default::default()
      });
      let inscription_id = InscriptionId::from(txid);

      let coinbase_tx = context.mine_blocks(1)[0].txdata[0].txid();

      context.index.assert_inscription_location(
        inscription_id,
        SatPoint {
          outpoint: OutPoint {
            txid: coinbase_tx,
            vout: 0,
          },
          offset: 50 * COIN_VALUE,
        },
        50 * COIN_VALUE as u128,
      );
    }
  }

  #[test]
  #[ignore]
  fn lost_inscriptions() {
    for context in Context::configurations() {
      context.mine_blocks(1);

      let txid = context.rpc_server.broadcast_tx(TransactionTemplate {
        inputs: &[(1, 0, 0)],
        fee: 50 * COIN_VALUE,
        witness: inscription("text/plain", "hello").to_witness(),
        ..Default::default()
      });
      let inscription_id = InscriptionId::from(txid);

      context.mine_blocks_with_subsidy(1, 0);

      context.index.assert_inscription_location(
        inscription_id,
        SatPoint {
          outpoint: OutPoint::null(),
          offset: 0,
        },
        50 * COIN_VALUE as u128,
      );
    }
  }

  #[test]
  #[ignore]
  fn multiple_inscriptions_can_be_lost() {
    for context in Context::configurations() {
      context.mine_blocks(1);

      let first_txid = context.rpc_server.broadcast_tx(TransactionTemplate {
        inputs: &[(1, 0, 0)],
        fee: 50 * COIN_VALUE,
        witness: inscription("text/plain", "hello").to_witness(),
        ..Default::default()
      });
      let first_inscription_id = InscriptionId::from(first_txid);

      context.mine_blocks_with_subsidy(1, 0);
      context.mine_blocks(1);

      let second_txid = context.rpc_server.broadcast_tx(TransactionTemplate {
        inputs: &[(3, 0, 0)],
        fee: 50 * COIN_VALUE,
        witness: inscription("text/plain", "hello").to_witness(),
        ..Default::default()
      });
      let second_inscription_id = InscriptionId::from(second_txid);

      context.mine_blocks_with_subsidy(1, 0);

      context.index.assert_inscription_location(
        first_inscription_id,
        SatPoint {
          outpoint: OutPoint::null(),
          offset: 0,
        },
        50 * COIN_VALUE as u128,
      );

      context.index.assert_inscription_location(
        second_inscription_id,
        SatPoint {
          outpoint: OutPoint::null(),
          offset: 50 * COIN_VALUE,
        },
        150 * COIN_VALUE as u128,
      );
    }
  }

  #[test]
  #[ignore]
  fn lost_sats_are_tracked_correctly() {
    let context = Context::builder().arg("--index-sats").build();
    assert_eq!(context.index.statistic(Statistic::LostSats), 0);

    context.mine_blocks(1);
    assert_eq!(context.index.statistic(Statistic::LostSats), 0);

    context.mine_blocks_with_subsidy(1, 0);
    assert_eq!(
      context.index.statistic(Statistic::LostSats),
      50 * COIN_VALUE
    );

    context.mine_blocks_with_subsidy(1, 0);
    assert_eq!(
      context.index.statistic(Statistic::LostSats),
      100 * COIN_VALUE
    );

    context.mine_blocks(1);
    assert_eq!(
      context.index.statistic(Statistic::LostSats),
      100 * COIN_VALUE
    );
  }

  #[test]
  #[ignore]
  fn lost_sat_ranges_are_tracked_correctly() {
    let context = Context::builder().arg("--index-sats").build();

    let null_ranges = || match context.index.list(OutPoint::null()).unwrap().unwrap() {
      List::Unspent(ranges) => ranges,
      _ => panic!(),
    };

    assert!(null_ranges().is_empty());

    context.mine_blocks(1);

    assert!(null_ranges().is_empty());

    context.mine_blocks_with_subsidy(1, 0);

    assert_eq!(
      null_ranges(),
      [(100 * COIN_VALUE as u128, 150 * COIN_VALUE as u128)]
    );

    context.mine_blocks_with_subsidy(1, 0);

    assert_eq!(
      null_ranges(),
      [
        (100 * COIN_VALUE as u128, 150 * COIN_VALUE as u128),
        (150 * COIN_VALUE as u128, 200 * COIN_VALUE as u128)
      ]
    );

    context.mine_blocks(1);

    assert_eq!(
      null_ranges(),
      [
        (100 * COIN_VALUE as u128, 150 * COIN_VALUE as u128),
        (150 * COIN_VALUE as u128, 200 * COIN_VALUE as u128)
      ]
    );

    context.mine_blocks_with_subsidy(1, 0);

    assert_eq!(
      null_ranges(),
      [
        (100 * COIN_VALUE as u128, 150 * COIN_VALUE as u128),
        (150 * COIN_VALUE as u128, 200 * COIN_VALUE as u128),
        (250 * COIN_VALUE as u128, 300 * COIN_VALUE as u128)
      ]
    );
  }

  #[test]
  #[ignore]
  fn lost_inscriptions_get_lost_satpoints() {
    for context in Context::configurations() {
      context.mine_blocks_with_subsidy(1, 0);
      context.mine_blocks(1);

      let txid = context.rpc_server.broadcast_tx(TransactionTemplate {
        inputs: &[(2, 0, 0)],
        outputs: 2,
        witness: inscription("text/plain", "hello").to_witness(),
        ..Default::default()
      });
      let inscription_id = InscriptionId::from(txid);
      context.mine_blocks(1);

      context.rpc_server.broadcast_tx(TransactionTemplate {
        inputs: &[(3, 1, 1), (3, 1, 0)],
        fee: 50 * COIN_VALUE,
        ..Default::default()
      });
      context.mine_blocks_with_subsidy(1, 0);

      context.index.assert_inscription_location(
        inscription_id,
        SatPoint {
          outpoint: OutPoint::null(),
          offset: 75 * COIN_VALUE,
        },
        100 * COIN_VALUE as u128,
      );
    }
  }

  #[test]
  #[ignore]
  fn inscription_skips_zero_value_first_output_of_inscribe_transaction() {
    for context in Context::configurations() {
      context.mine_blocks(1);

      let txid = context.rpc_server.broadcast_tx(TransactionTemplate {
        inputs: &[(1, 0, 0)],
        outputs: 2,
        witness: inscription("text/plain", "hello").to_witness(),
        output_values: &[0, 50 * COIN_VALUE],
        ..Default::default()
      });
      let inscription_id = InscriptionId::from(txid);
      context.mine_blocks(1);

      context.index.assert_inscription_location(
        inscription_id,
        SatPoint {
          outpoint: OutPoint { txid, vout: 1 },
          offset: 0,
        },
        50 * COIN_VALUE as u128,
      );
    }
  }

  #[test]
  #[ignore]
  fn inscription_can_be_lost_in_first_transaction() {
    for context in Context::configurations() {
      context.mine_blocks(1);

      let txid = context.rpc_server.broadcast_tx(TransactionTemplate {
        inputs: &[(1, 0, 0)],
        fee: 50 * COIN_VALUE,
        witness: inscription("text/plain", "hello").to_witness(),
        ..Default::default()
      });
      let inscription_id = InscriptionId::from(txid);
      context.mine_blocks_with_subsidy(1, 0);

      context.index.assert_inscription_location(
        inscription_id,
        SatPoint {
          outpoint: OutPoint::null(),
          offset: 0,
        },
        50 * COIN_VALUE as u128,
      );
    }
  }

  #[test]
  #[ignore]
  fn lost_rare_sats_are_tracked() {
    let context = Context::builder().arg("--index-sats").build();
    context.mine_blocks_with_subsidy(1, 0);
    context.mine_blocks_with_subsidy(1, 0);

    assert_eq!(
      context
        .index
        .rare_sat_satpoint(Sat(50 * COIN_VALUE as u128))
        .unwrap()
        .unwrap(),
      SatPoint {
        outpoint: OutPoint::null(),
        offset: 0,
      },
    );

    assert_eq!(
      context
        .index
        .rare_sat_satpoint(Sat(100 * COIN_VALUE as u128))
        .unwrap()
        .unwrap(),
      SatPoint {
        outpoint: OutPoint::null(),
        offset: 50 * COIN_VALUE,
      },
    );
  }

  #[test]
  fn old_schema_gives_correct_error() {
    let tempdir = {
      let context = Context::builder().build();

      let wtx = context.index.database.begin_write().unwrap();

      wtx
        .open_table(STATISTIC_TO_COUNT)
        .unwrap()
        .insert(&Statistic::Schema.key(), &0)
        .unwrap();

      wtx.commit().unwrap();

      context.tempdir
    };

    let path = tempdir.path().to_owned();

    let delimiter = if cfg!(windows) { '\\' } else { '/' };

    assert_eq!(
      Context::builder().tempdir(tempdir).try_build().err().unwrap().to_string(),
      format!("index at `{}{delimiter}regtest{delimiter}index.redb` appears to have been built with an older, incompatible version of ord, consider deleting and rebuilding the index: index schema 0, ord schema {SCHEMA_VERSION}", path.display()));
  }

  #[test]
  fn new_schema_gives_correct_error() {
    let tempdir = {
      let context = Context::builder().build();

      let wtx = context.index.database.begin_write().unwrap();

      wtx
        .open_table(STATISTIC_TO_COUNT)
        .unwrap()
        .insert(&Statistic::Schema.key(), &u64::MAX)
        .unwrap();

      wtx.commit().unwrap();

      context.tempdir
    };

    let path = tempdir.path().to_owned();

    let delimiter = if cfg!(windows) { '\\' } else { '/' };

    assert_eq!(
      Context::builder().tempdir(tempdir).try_build().err().unwrap().to_string(),
      format!("index at `{}{delimiter}regtest{delimiter}index.redb` appears to have been built with a newer, incompatible version of ord, consider updating ord: index schema {}, ord schema {SCHEMA_VERSION}", path.display(), u64::MAX));
  }

  #[test]
  fn inscriptions_on_output() {
    for context in Context::configurations() {
      context.mine_blocks(1);

      let txid = context.rpc_server.broadcast_tx(TransactionTemplate {
        inputs: &[(1, 0, 0)],
        witness: inscription("text/plain", "hello").to_witness(),
        ..Default::default()
      });

      let inscription_id = InscriptionId::from(txid);

      assert_eq!(
        context
          .index
          .get_inscriptions_on_output(OutPoint { txid, vout: 0 })
          .unwrap(),
        []
      );

      context.mine_blocks(1);

      assert_eq!(
        context
          .index
          .get_inscriptions_on_output(OutPoint { txid, vout: 0 })
          .unwrap(),
        [inscription_id]
      );

      let send_id = context.rpc_server.broadcast_tx(TransactionTemplate {
        inputs: &[(2, 1, 0)],
        ..Default::default()
      });

      context.mine_blocks(1);

      assert_eq!(
        context
          .index
          .get_inscriptions_on_output(OutPoint { txid, vout: 0 })
          .unwrap(),
        []
      );

      assert_eq!(
        context
          .index
          .get_inscriptions_on_output(OutPoint {
            txid: send_id,
            vout: 0,
          })
          .unwrap(),
        [inscription_id]
      );
    }
  }

  #[test]
  #[ignore]
  fn inscriptions_on_same_sat_after_the_first_are_ignored() {
    for context in Context::configurations() {
      context.mine_blocks(1);

      let first = context.rpc_server.broadcast_tx(TransactionTemplate {
        inputs: &[(1, 0, 0)],
        witness: inscription("text/plain", "hello").to_witness(),
        ..Default::default()
      });

      context.mine_blocks(1);

      let inscription_id = InscriptionId::from(first);

      assert_eq!(
        context
          .index
          .get_inscriptions_on_output(OutPoint {
            txid: first,
            vout: 0
          })
          .unwrap(),
        [inscription_id]
      );

      context.index.assert_inscription_location(
        inscription_id,
        SatPoint {
          outpoint: OutPoint {
            txid: first,
            vout: 0,
          },
          offset: 0,
        },
        50 * COIN_VALUE as u128,
      );

      let second = context.rpc_server.broadcast_tx(TransactionTemplate {
        inputs: &[(2, 1, 0)],
        witness: inscription("text/plain", "hello").to_witness(),
        ..Default::default()
      });

      context.mine_blocks(1);

      context.index.assert_inscription_location(
        inscription_id,
        SatPoint {
          outpoint: OutPoint {
            txid: second,
            vout: 0,
          },
          offset: 0,
        },
        50 * COIN_VALUE as u128,
      );

      assert!(context
        .index
        .get_inscription_entry(second.into())
        .unwrap()
        .is_none());

      assert!(context
        .index
        .get_inscription_by_id(second.into())
        .unwrap()
        .is_none());
    }
  }

  #[test]
  fn get_latest_inscriptions_with_no_prev_and_next() {
    for context in Context::configurations() {
      context.mine_blocks(1);

      let txid = context.rpc_server.broadcast_tx(TransactionTemplate {
        inputs: &[(1, 0, 0)],
        witness: inscription("text/plain", "hello").to_witness(),
        ..Default::default()
      });
      let inscription_id = InscriptionId::from(txid);

      context.mine_blocks(1);

      let (inscriptions, prev, next) = context
        .index
        .get_latest_inscriptions_with_prev_and_next(100, None)
        .unwrap();
      assert_eq!(inscriptions, &[inscription_id]);
      assert_eq!(prev, None);
      assert_eq!(next, None);
    }
  }

  #[test]
  fn get_latest_inscriptions_with_prev_and_next() {
    for context in Context::configurations() {
      context.mine_blocks(1);

      let mut ids = Vec::new();

      for i in 0..103 {
        let txid = context.rpc_server.broadcast_tx(TransactionTemplate {
          inputs: &[(i + 1, 0, 0)],
          witness: inscription("text/plain", "hello").to_witness(),
          ..Default::default()
        });
        ids.push(InscriptionId::from(txid));
        context.mine_blocks(1);
      }

      ids.reverse();

      let (inscriptions, prev, next) = context
        .index
        .get_latest_inscriptions_with_prev_and_next(100, None)
        .unwrap();
      assert_eq!(inscriptions, &ids[..100]);
      assert_eq!(prev, Some(2));
      assert_eq!(next, None);

      let (inscriptions, prev, next) = context
        .index
        .get_latest_inscriptions_with_prev_and_next(100, Some(101))
        .unwrap();
      assert_eq!(inscriptions, &ids[1..101]);
      assert_eq!(prev, Some(1));
      assert_eq!(next, Some(102));

      let (inscriptions, prev, next) = context
        .index
        .get_latest_inscriptions_with_prev_and_next(100, Some(0))
        .unwrap();
      assert_eq!(inscriptions, &ids[102..103]);
      assert_eq!(prev, None);
      assert_eq!(next, Some(100));
    }
  }

  #[test]
  fn unsynced_index_fails() {
    for context in Context::configurations() {
      let mut entropy = [0; 16];
      rand::thread_rng().fill_bytes(&mut entropy);
      let mnemonic = Mnemonic::from_entropy(&entropy).unwrap();
      crate::subcommand::wallet::initialize_wallet(&context.options, mnemonic.to_seed("")).unwrap();
      context.rpc_server.mine_blocks(1);
      assert_regex_match!(
        context
          .index
          .get_unspent_outputs(Wallet::load(&context.options).unwrap())
          .unwrap_err()
          .to_string(),
        r"output in Dogecoin Core wallet but not in ord index: [[:xdigit:]]{64}:\d+"
      );
    }
  }

  #[test]
  fn recover_from_reorg() {
    for context in Context::configurations() {
      context.mine_blocks(1);

      let txid = context.rpc_server.broadcast_tx(TransactionTemplate {
        inputs: &[(1, 0, 0)],
        witness: inscription("text/plain;charset=utf-8", "hello").to_witness(),
        ..Default::default()
      });
      let first_id = InscriptionId { txid, index: 0 };
      let first_location = SatPoint {
        outpoint: OutPoint { txid, vout: 0 },
        offset: 0,
      };

      context.mine_blocks(6);

      context
          .index
          .assert_inscription_location(first_id, first_location, Some(50 * COIN_VALUE));

      let txid = context.rpc_server.broadcast_tx(TransactionTemplate {
        inputs: &[(2, 0, 0)],
        witness: inscription("text/plain;charset=utf-8", "hello").to_witness(),
        ..Default::default()
      });
      let second_id = InscriptionId { txid, index: 0 };
      let second_location = SatPoint {
        outpoint: OutPoint { txid, vout: 0 },
        offset: 0,
      };

      context.mine_blocks(1);

      context
          .index
          .assert_inscription_location(second_id, second_location, Some(100 * COIN_VALUE));

      context.rpc_server.invalidate_tip();
      context.mine_blocks(2);

      context
          .index
          .assert_inscription_location(first_id, first_location, Some(50 * COIN_VALUE));

      context.index.assert_non_existence_of_inscription(second_id);
    }
  }

  #[test]
  fn recover_from_3_block_deep_and_consecutive_reorg() {
    for context in Context::configurations() {
      context.mine_blocks(1);

      let txid = context.rpc_server.broadcast_tx(TransactionTemplate {
        inputs: &[(1, 0, 0)],
        witness: inscription("text/plain;charset=utf-8", "hello").to_witness(),
        ..Default::default()
      });
      let first_id = InscriptionId { txid, index: 0 };
      let first_location = SatPoint {
        outpoint: OutPoint { txid, vout: 0 },
        offset: 0,
      };

      context.mine_blocks(10);

      let txid = context.rpc_server.broadcast_tx(TransactionTemplate {
        inputs: &[(2, 0, 0)],
        witness: inscription("text/plain;charset=utf-8", "hello").to_witness(),
        ..Default::default()
      });
      let second_id = InscriptionId { txid, index: 0 };
      let second_location = SatPoint {
        outpoint: OutPoint { txid, vout: 0 },
        offset: 0,
      };

      context.mine_blocks(1);

      context
          .index
          .assert_inscription_location(second_id, second_location, Some(100 * COIN_VALUE));

      context.rpc_server.invalidate_tip();
      context.rpc_server.invalidate_tip();
      context.rpc_server.invalidate_tip();

      context.mine_blocks(4);

      context.index.assert_non_existence_of_inscription(second_id);

      context.rpc_server.invalidate_tip();

      context.mine_blocks(2);

      context
          .index
          .assert_inscription_location(first_id, first_location, Some(50 * COIN_VALUE));
    }
  }

  #[test]
  fn recover_from_very_unlikely_7_block_deep_reorg() {
    for context in Context::configurations() {
      context.mine_blocks(1);

      let txid = context.rpc_server.broadcast_tx(TransactionTemplate {
        inputs: &[(1, 0, 0)],
        witness: inscription("text/plain;charset=utf-8", "hello").to_witness(),
        ..Default::default()
      });

      context.mine_blocks(11);

      let first_id = InscriptionId { txid, index: 0 };
      let first_location = SatPoint {
        outpoint: OutPoint { txid, vout: 0 },
        offset: 0,
      };

      let txid = context.rpc_server.broadcast_tx(TransactionTemplate {
        inputs: &[(2, 0, 0)],
        witness: inscription("text/plain;charset=utf-8", "hello").to_witness(),
        ..Default::default()
      });

      let second_id = InscriptionId { txid, index: 0 };
      let second_location = SatPoint {
        outpoint: OutPoint { txid, vout: 0 },
        offset: 0,
      };

      context.mine_blocks(7);

      context
          .index
          .assert_inscription_location(second_id, second_location, Some(100 * COIN_VALUE));

      for _ in 0..7 {
        context.rpc_server.invalidate_tip();
      }

      context.mine_blocks(9);

      context.index.assert_non_existence_of_inscription(second_id);

      context
          .index
          .assert_inscription_location(first_id, first_location, Some(50 * COIN_VALUE));
    }
  }
}
