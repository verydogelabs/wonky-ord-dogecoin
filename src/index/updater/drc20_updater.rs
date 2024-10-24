use bigdecimal::num_bigint::Sign;

use {
  super::*,
  crate::{Instant, Result},
  bitcoin::Txid,
  std::collections::HashMap,
};

use crate::drc20::errors::Error::LedgerError;
use crate::drc20::operation::{InscriptionOp, Operation};
use crate::drc20::params::{BIGDECIMAL_TEN, MAX_DECIMAL_WIDTH};
use crate::drc20::script_key::ScriptKey;
use crate::drc20::{
  max_script_tick_id_key, max_script_tick_key, min_script_tick_id_key, min_script_tick_key,
  script_tick_id_key, script_tick_key, Balance, BlockContext, DRC20Error, Deploy, DeployEvent,
  Event, InscribeTransferEvent, Message, Mint, MintEvent, Num, Tick, TokenInfo, Transfer,
  TransferEvent, TransferInfo, TransferableLog,
};
use crate::subcommand::Output;

#[derive(Debug, Clone, PartialEq)]
pub struct ExecutionMessage {
  pub(self) txid: Txid,
  pub(self) inscription_id: InscriptionId,
  pub(self) inscription_number: u64,
  pub(self) old_satpoint: SatPoint,
  pub(self) new_satpoint: SatPoint,
  pub(self) from: ScriptKey,
  pub(self) to: Option<ScriptKey>,
  pub(self) op: Operation,
}

pub(super) struct Drc20Updater<'a, 'db, 'tx> {
    drc20_token_info: &'a mut Table<'db, 'tx, &'static str, &'static [u8]>,
    drc20_token_holder: &'a mut MultimapTable<'db, 'tx, &'static str, &'static str>,
    drc20_token_balance: &'a mut Table<'db, 'tx, &'static str, &'static [u8]>,
    drc20_inscribe_transfer: &'a mut Table<'db, 'tx, &'static [u8; 36], &'static [u8]>,
    drc20_transferable_log: &'a mut Table<'db, 'tx, &'static str, &'static [u8]>,
    inscription_id_to_inscription_entry: &'a Table<'db, 'tx, &'static InscriptionIdValue, InscriptionEntryValue>,
    transaction_id_to_transaction: &'a mut Table<'db, 'tx, &'static TxidValue, &'static [u8]>,
}

impl<'a, 'db, 'tx> Drc20Updater<'a, 'db, 'tx> {
    pub(super) fn new(
        drc20_token_info: &'a mut Table<'db, 'tx, &'static str, &'static [u8]>,
        drc20_token_holder: &'a mut MultimapTable<'db, 'tx, &'static str, &'static str>,
        drc20_token_balance: &'a mut Table<'db, 'tx, &'static str, &'static [u8]>,
        drc20_inscribe_transfer: &'a mut Table<'db, 'tx, &'static [u8; 36], &'static [u8]>,
        drc20_transferable_log: &'a mut Table<'db, 'tx, &'static str, &'static [u8]>,
        inscription_id_to_inscription_entry: &'a Table<'db, 'tx, &'static InscriptionIdValue, InscriptionEntryValue>,
        transaction_id_to_transaction: &'a mut Table<'db, 'tx, &'static TxidValue, &'static [u8]>,
    ) -> Result<Self> {
        Ok(Self {
            drc20_token_info,
            drc20_token_holder,
            drc20_token_balance,
            drc20_inscribe_transfer,
            drc20_transferable_log,
            inscription_id_to_inscription_entry,
            transaction_id_to_transaction,
        })
    }

    pub(crate) fn index_block(
        &mut self,
        context: BlockContext,
        block: &BlockData,
        operations: HashMap<Txid, Vec<InscriptionOp>>,
    ) -> Result {
        let start = Instant::now();
        let mut messages_size = 0;
        for (tx, txid) in block.txdata.iter() {
            // skip coinbase transaction.
            if tx
                .input
                .first()
                .map(|tx_in| tx_in.previous_output.is_null())
                .unwrap_or_default()
            {
                continue;
            }

            // index inscription operations.
            if let Some(tx_operations) = operations.get(txid) {
                // Resolve and execute messages.
                let messages = self.resolve_message(tx, tx_operations)?;
                for msg in messages.iter() {
                    self.execute_message(context, msg)?;
                }
                messages_size += messages.len();
            }
        }

        log::info!(
      "DRC20 Updater indexed block {} with {} messages in {} ms",
      context.blockheight,
      messages_size,
      (Instant::now() - start).as_millis(),
    );
        Ok(())
    }

    pub fn resolve_message(
        &mut self,
        tx: &Transaction,
        operations: &[InscriptionOp],
    ) -> Result<Vec<Message>> {
        let mut messages = Vec::new();
        let mut operation_iter = operations.iter().peekable();
        let new_inscriptions: Vec<Inscription> = match Inscription::from_transactions(vec![tx.clone()]) {
            ParsedInscription::None => { vec![] }
            ParsedInscription::Partial => { vec![] }
            ParsedInscription::Complete(inscription) => vec![inscription]
        };

        for input in &tx.input {
            // "operations" is a list of all the operations in the current block, and they are ordered.
            // We just need to find the operation corresponding to the current transaction here.
            while let Some(operation) = operation_iter.peek() {
                if operation.old_satpoint.outpoint != input.previous_output {
                    break;
                }
                let operation = operation_iter.next().unwrap();

                // Parse DRC20 message through inscription operation.
                if let Some(msg) =
                    Message::resolve(&mut self.drc20_inscribe_transfer, &new_inscriptions, operation)?
                {
                    messages.push(msg);
                    continue;
                }
            }
        }
        Ok(messages)
    }

    pub fn execute_message(&mut self, context: BlockContext, msg: &Message) -> Result {
        let exec_msg = self.create_execution_message(msg, context.network)?;
        let _ = match &exec_msg.op {
            Operation::Deploy(deploy) => {
                Self::process_deploy(self, context.clone(), &exec_msg, deploy.clone())
            }
            Operation::Mint(mint) => Self::process_mint(self, context.clone(), &exec_msg.clone(), mint.clone()),
            Operation::InscribeTransfer(transfer) => {
                Self::process_inscribe_transfer(self, context.clone(), &exec_msg.clone(), transfer.clone())
            }
            Operation::Transfer(_) => Self::process_transfer(self, context.clone(), &exec_msg.clone()),
        };
        Ok(())
    }

    pub fn create_execution_message(
        &mut self,
        msg: &Message,
        network: Network,
    ) -> Result<ExecutionMessage> {
        Ok(ExecutionMessage {
            txid: msg.txid,
            inscription_id: msg.inscription_id,
            inscription_number: Self::get_inscription_number_by_id(self, msg.inscription_id)?,
            old_satpoint: msg.old_satpoint,
            new_satpoint: msg
                .new_satpoint
                .ok_or(anyhow!("new satpoint cannot be None"))?,
            from: Self::get_script_key_on_satpoint(self, msg.old_satpoint, network)?,
            to: if msg.sat_in_outputs {
                Some(Self::get_script_key_on_satpoint(self,
                    msg.new_satpoint.unwrap(),
                    network,
                )?)
            } else {
                None
            },
            op: msg.op.clone(),
        })
    }

  fn process_deploy(
    &mut self,
    context: BlockContext,
    msg: &ExecutionMessage,
    deploy: Deploy,
  ) -> Result<Event, errors::Error<DRC20Error>> {
    // ignore inscribe inscription to coinbase.
    let to_script_key = msg.to.clone().ok_or(DRC20Error::InscribeToCoinbase)?;

    let tick = deploy.tick.parse::<Tick>()?;

    if let Some(stored_tick_info) = Self::get_token_info(self, &tick).map_err(|e| LedgerError(e))? {
      return Err(errors::Error::DRC20Error(DRC20Error::DuplicateTick(
        stored_tick_info.tick.to_string(),
      )));
    }

    let dec = Num::from_str(&deploy.decimals.map_or(MAX_DECIMAL_WIDTH.to_string(), |v| v))?
      .checked_to_u8()?;
    if dec > MAX_DECIMAL_WIDTH {
      return Err(errors::Error::DRC20Error(DRC20Error::DecimalsTooLarge(dec)));
    }
    let base = BIGDECIMAL_TEN.checked_powu(u64::from(dec))?;

    let supply = Num::from_str(&deploy.max_supply)?;

    if supply.sign() == Sign::NoSign || supply > drc20::params::MAXIMUM_SUPPLY.to_owned() {
      return Err(errors::Error::DRC20Error(DRC20Error::InvalidSupply(
        supply.to_string(),
      )));
    }

    let limit = Num::from_str(&deploy.mint_limit.map_or(deploy.max_supply, |v| v))?;

    if limit.sign() == Sign::NoSign || limit > drc20::params::MAXIMUM_SUPPLY.to_owned() {
      return Err(errors::Error::DRC20Error(DRC20Error::MintLimitOutOfRange(
        tick.to_lowercase().to_string(),
        limit.to_string(),
      )));
    }

    let supply = supply.checked_mul(&base)?.checked_to_u128()?;
    let limit = limit.checked_mul(&base)?.checked_to_u128()?;

    let new_info = TokenInfo {
      inscription_id: msg.inscription_id,
      inscription_number: msg.inscription_number,
      tick: tick.clone(),
      supply,
      limit_per_mint: limit,
      decimal: dec,
      minted: 0u128,
      deploy_by: to_script_key.clone(),
      deployed_number: context.blockheight,
      latest_mint_number: context.blockheight,
      deployed_timestamp: context.blocktime,
    };
    Self::insert_token_info(self, &tick, &new_info).map_err(|e| LedgerError(e))?;

    Ok(Event::Deploy(DeployEvent {
      txid: None,
      vout: msg.new_satpoint.outpoint.vout,
      deployed_by: to_script_key,
      supply,
      limit_per_mint: limit,
      decimal: dec,
      tick: new_info.tick,
    }))
  }

  fn process_mint(
    &mut self,
    context: BlockContext,
    msg: &ExecutionMessage,
    mint: Mint,
  ) -> Result<Event, errors::Error<DRC20Error>> {
    // ignore inscribe inscription to coinbase.
    let to_script_key = msg.to.clone().ok_or(DRC20Error::InscribeToCoinbase)?;

    let tick = mint.tick.parse::<Tick>()?;

    let token_info = Self::get_token_info(self, &tick)
      .map_err(|e| LedgerError(e))?
      .ok_or(DRC20Error::TickNotFound(tick.to_string()))?;

    let base = BIGDECIMAL_TEN.checked_powu(u64::from(token_info.decimal))?;

    let mut amt = Num::from_str(&mint.amount)?;

    if amt.scale() > i64::from(token_info.decimal) {
      return Err(errors::Error::DRC20Error(DRC20Error::AmountOverflow(
        amt.to_string(),
      )));
    }

    amt = amt.checked_mul(&base)?;
    if amt.sign() == Sign::NoSign {
      return Err(errors::Error::DRC20Error(DRC20Error::InvalidZeroAmount));
    }
    if amt > Into::<Num>::into(token_info.limit_per_mint) {
      return Err(errors::Error::DRC20Error(DRC20Error::AmountExceedLimit(
        amt.to_string(),
      )));
    }
    let minted = Into::<Num>::into(token_info.minted);
    let supply = Into::<Num>::into(token_info.supply);

    if minted >= supply {
      return Err(errors::Error::DRC20Error(DRC20Error::TickMinted(
        token_info.tick.to_string(),
      )));
    }

    // cut off any excess.
    let mut out_msg = None;
    amt = if amt.checked_add(&minted)? > supply {
      let new = supply.checked_sub(&minted)?;
      out_msg = Some(format!(
        "amt has been cut off to fit the supply! origin: {}, now: {}",
        amt, new
      ));
      new
    } else {
      amt
    };

    // get or initialize user balance.
    let mut balance = Self::get_balance(self, &to_script_key, &tick)
      .map_err(|e| LedgerError(e))?
      .map_or(Balance::new(&tick), |v| v);

    // add amount to available balance.
    balance.overall_balance = Into::<Num>::into(balance.overall_balance)
      .checked_add(&amt)?
      .checked_to_u128()?;

    // store to database.
    Self::update_token_balance(self, &to_script_key, balance).map_err(|e| LedgerError(e))?;
    Self::insert_token_holder(self, &to_script_key, tick.clone()).map_err(|e| LedgerError(e))?;

    // update token minted.
    let minted = minted.checked_add(&amt)?.checked_to_u128()?;
    Self::update_mint_token_info(self, &tick, minted, context.blockheight)
      .map_err(|e| LedgerError(e))?;

    Ok(Event::Mint(MintEvent {
      txid: None,
      to: to_script_key,
      vout: msg.new_satpoint.outpoint.vout,
      tick: token_info.tick,
      amount: amt.checked_to_u128()?,
      msg: out_msg,
    }))
  }

  fn process_inscribe_transfer(
    &mut self,
    _context: BlockContext,
    msg: &ExecutionMessage,
    transfer: Transfer,
  ) -> Result<Event, errors::Error<DRC20Error>> {
    // ignore inscribe inscription to coinbase.
    let to_script_key = msg.to.clone().ok_or(DRC20Error::InscribeToCoinbase)?;

    let tick = transfer.tick.parse::<Tick>()?;

    let token_info = Self::get_token_info(self, &tick)
      .map_err(|e| LedgerError(e))?
      .ok_or(DRC20Error::TickNotFound(tick.to_string()))?;

    let base = BIGDECIMAL_TEN.checked_powu(u64::from(token_info.decimal))?;

    let mut amt = Num::from_str(&transfer.amount)?;

    if amt.scale() > i64::from(token_info.decimal) {
      return Err(errors::Error::DRC20Error(DRC20Error::AmountOverflow(
        amt.to_string(),
      )));
    }

    amt = amt.checked_mul(&base)?;
    if amt.sign() == Sign::NoSign || amt > Into::<Num>::into(token_info.supply) {
      return Err(errors::Error::DRC20Error(DRC20Error::AmountOverflow(
        amt.to_string(),
      )));
    }

    let mut balance = Self::get_balance(self, &to_script_key, &tick)
      .map_err(|e| LedgerError(e))?
      .map_or(Balance::new(&tick), |v| v);

    let overall = Into::<Num>::into(balance.overall_balance);
    let transferable = Into::<Num>::into(balance.transferable_balance);
    let available = overall.checked_sub(&transferable)?;
    if available < amt {
      return Err(errors::Error::DRC20Error(DRC20Error::InsufficientBalance(
        available.to_string(),
        amt.to_string(),
      )));
    }

    balance.transferable_balance = transferable.checked_add(&amt)?.checked_to_u128()?;

    let amt = amt.checked_to_u128()?;
    Self::update_token_balance(self, &to_script_key, balance).map_err(|e| LedgerError(e))?;

    let inscription = TransferableLog {
      inscription_id: msg.inscription_id,
      inscription_number: msg.inscription_number,
      amount: amt,
      tick: token_info.tick.clone(),
      owner: to_script_key.clone(),
    };
    Self::insert_transferable(self, &inscription.owner, &tick, inscription.clone())
      .map_err(|e| LedgerError(e))?;

    Self::insert_inscribe_transfer_inscription(
      self,
      msg.inscription_id,
      TransferInfo {
        tick: inscription.tick,
        amt,
      },
    )
    .map_err(|e| LedgerError(e))?;

    Ok(Event::InscribeTransfer(InscribeTransferEvent {
      txid: None,
      to: to_script_key,
      vout: msg.new_satpoint.outpoint.vout,
      tick: token_info.tick.clone(),
      amount: amt,
    }))
  }

  fn process_transfer(
    &mut self,
    _context: BlockContext,
    msg: &ExecutionMessage,
  ) -> Result<Event, errors::Error<DRC20Error>> {
    let mut transferable = Self::get_transferable_by_id(self, &msg.from, &msg.inscription_id)
      .map_err(|e| LedgerError(e))?
      .ok_or(DRC20Error::TransferableNotFound(msg.inscription_id))?;
    let amt = Into::<Num>::into(transferable.amount);

    if transferable.owner != msg.from {
      return Err(errors::Error::DRC20Error(
        DRC20Error::TransferableOwnerNotMatch(msg.inscription_id),
      ));
    }

    let tick = transferable.tick;

    let token_info = Self::get_token_info(self, &tick)
      .map_err(|e| LedgerError(e))?
      .ok_or(DRC20Error::TickNotFound(tick.to_string()))?;

    // update from key balance.
    let mut from_balance = Self::get_balance(self, &msg.from, &tick)
      .map_err(|e| LedgerError(e))?
      .map_or(Balance::new(&tick), |v| v);

    let from_overall = Into::<Num>::into(from_balance.overall_balance);
    let from_transferable = Into::<Num>::into(from_balance.transferable_balance);

    let from_overall = from_overall.checked_sub(&amt)?.checked_to_u128()?;
    let from_transferable = from_transferable.checked_sub(&amt)?.checked_to_u128()?;

    from_balance.overall_balance = from_overall;
    from_balance.transferable_balance = from_transferable;

    Self::update_token_balance(self, &msg.from, from_balance).map_err(|e| LedgerError(e))?;

    // redirect receiver to sender if transfer to coinbase.
    let mut out_msg = None;

    let to_script_key = if msg.to.clone().is_none() {
      out_msg =
        Some("redirect receiver to sender, reason: transfer inscription to coinbase".to_string());
      msg.from.clone()
    } else {
      msg.to.clone().unwrap()
    };

    // update to key balance.
    let mut to_balance = Self::get_balance(self, &to_script_key, &tick)
      .map_err(|e| LedgerError(e))?
      .map_or(Balance::new(&tick), |v| v);

    let to_overall = Into::<Num>::into(to_balance.overall_balance);
    to_balance.overall_balance = to_overall.checked_add(&amt)?.checked_to_u128()?;

    Self::update_token_balance(self, &to_script_key, to_balance).map_err(|e| LedgerError(e))?;

    Self::insert_token_holder(self, &to_script_key, tick.clone()).map_err(|e| LedgerError(e))?;

    if from_overall == 0 && msg.from != to_script_key {
      Self::remove_token_holder(self, &msg.from, tick.clone()).map_err(|e| LedgerError(e))?;
    }

    Self::remove_transferable(self, &msg.from, &tick, msg.inscription_id)
      .map_err(|e| LedgerError(e))?;

    Self::remove_inscribe_transfer_inscription(self, msg.inscription_id)
      .map_err(|e| LedgerError(e))?;

    Ok(Event::Transfer(TransferEvent {
      txid: None,
      from: msg.clone().from,
      to: to_script_key,
      vout: msg.new_satpoint.outpoint.vout,
      tick: token_info.tick,
      amount: amt.checked_to_u128()?,
    }))
  }

    fn insert_transferable(
        &mut self,
        script: &ScriptKey,
        tick: &Tick,
        inscription: TransferableLog,
    ) -> Result<(), redb::Error> {
        self.drc20_transferable_log.insert(
            script_tick_id_key(script, tick, &inscription.inscription_id).as_str(),
            rmp_serde::to_vec(&inscription).unwrap().as_slice(),
        )?;
        Ok(())
    }

    fn remove_transferable(
        &mut self,
        script: &ScriptKey,
        tick: &Tick,
        inscription_id: InscriptionId,
    ) -> Result<(), redb::Error> {
        self
            .drc20_transferable_log
            .remove(script_tick_id_key(script, tick, &inscription_id).as_str())?;
        Ok(())
    }

    fn get_transferable(
        &self,
        script: &ScriptKey
    ) -> Result<Vec<TransferableLog>, redb::Error> {
        Ok(
            self.drc20_transferable_log
                .range(min_script_tick_key(script).as_str()..max_script_tick_key(script).as_str())?
                .flat_map(|result| {
                    result.map(|(_, v)| rmp_serde::from_slice::<TransferableLog>(v.value()).unwrap())
                })
                .collect(),
        )
    }

    fn get_transferable_by_tick(
        &self,
        script: &ScriptKey,
        tick: &Tick,
    ) -> Result<Vec<TransferableLog>, redb::Error> {
        Ok(
            self.drc20_transferable_log
                .range(
                    min_script_tick_id_key(script, tick).as_str()
                        ..max_script_tick_id_key(script, tick).as_str(),
                )?
                .flat_map(|result| {
                    result.map(|(_, v)| rmp_serde::from_slice::<TransferableLog>(v.value()).unwrap())
                })
                .collect(),
        )
    }

    fn get_transferable_by_id(
        &self,
        script: &ScriptKey,
        inscription_id: &InscriptionId,
    ) -> Result<Option<TransferableLog>, redb::Error> {
        Ok(
            Self::get_transferable(self, script)?
                .iter()
                .find(|log| log.inscription_id == *inscription_id)
                .cloned(),
        )
    }

    fn insert_inscribe_transfer_inscription(
        &mut self,
        inscription_id: InscriptionId,
        transfer_info: TransferInfo,
    ) -> Result<(), redb::Error> {
        self.drc20_inscribe_transfer.insert(
            &inscription_id.store(),
            rmp_serde::to_vec(&transfer_info).unwrap().as_slice(),
        )?;
        Ok(())
    }

    fn remove_inscribe_transfer_inscription(
        &mut self,
        inscription_id: InscriptionId,
    ) -> Result<(), redb::Error> {
        self.drc20_inscribe_transfer
            .remove(&inscription_id.store())?;
        Ok(())
    }

    fn update_token_balance(
        &mut self,
        script_key: &ScriptKey,
        new_balance: Balance,
    ) -> Result<(), redb::Error> {
        self.drc20_token_balance.insert(
            script_tick_key(script_key, &new_balance.tick).as_str(),
            bincode::serialize(&new_balance).unwrap().as_slice(),
        )?;
        Ok(())
    }

    fn get_balance(
        &self,
        script_key: &ScriptKey,
        tick: &Tick,
    ) -> Result<Option<Balance>, redb::Error> {
        Ok(
            self.drc20_token_balance
                .get(script_tick_key(script_key, tick).as_str())?
                .map(|v| bincode::deserialize::<Balance>(v.value()).unwrap()),
        )
    }

    fn insert_token_info(&mut self,
        tick: &Tick,
        new_info: &TokenInfo
    ) -> Result<(), redb::Error> {
        self.drc20_token_info.insert(
            tick.to_lowercase().hex().as_str(),
            bincode::serialize(new_info).unwrap().as_slice(),
        )?;
        Ok(())
    }

    fn update_mint_token_info(
        &mut self,
        tick: &Tick,
        minted_amt: u128,
        minted_block_number: u64,
    ) -> Result<(), redb::Error> {
        let mut info = Self::get_token_info(self, tick)?
            .unwrap_or_else(|| panic!("token {} not exist", tick.as_str()));

        info.minted = minted_amt;
        info.latest_mint_number = minted_block_number;

        self.drc20_token_info.insert(
            tick.to_lowercase().hex().as_str(),
            bincode::serialize(&info).unwrap().as_slice(),
        )?;
        Ok(())
    }

    pub(super) fn get_token_info(
        &self,
        tick: &Tick
    ) -> Result <Option<TokenInfo>, redb::Error> {
        Ok(
            self.drc20_token_info
                .get(tick.to_lowercase().hex().as_str())?
                .map(|v| bincode::deserialize::<TokenInfo>(v.value()).unwrap()),
        )
    }

    pub(super) fn get_script_key_on_satpoint(
        &self,
        satpoint: SatPoint,
        network: Network,
    ) -> Result<ScriptKey> {
        if let Some(transaction) = self.transaction_id_to_transaction
            .get(&satpoint.outpoint.txid.store())? {
            let tx: Transaction = consensus::encode::deserialize(transaction.value())?;
            let pub_key = tx.output[satpoint.outpoint.vout as usize].script_pubkey.clone();
            Ok(ScriptKey::from_script(&pub_key, network))
        } else {
            Err(anyhow!(
                "failed to get tx out! error: outpoint {} not found",
                satpoint.outpoint
            ))
        }
    }

    fn get_inscription_number_by_id(&mut self, inscription_id: InscriptionId) -> Result<u64> {
        Self::get_number_by_inscription_id(self, inscription_id)
            .map_err(|e| anyhow!("failed to get inscription number from state! error: {e}"))?
            .ok_or(anyhow!(
        "failed to get inscription number! error: inscription id {} not found",
        inscription_id
      ))
    }

    pub fn get_number_by_inscription_id(
        &self,
        inscription_id: InscriptionId,
    ) -> Result<Option<u64>, redb::Error> {
        let mut key = [0; 36];
        let (txid, index) = key.split_at_mut(32);
        txid.copy_from_slice(inscription_id.txid.as_ref());
        index.copy_from_slice(&inscription_id.index.to_be_bytes());
        Ok(
            self.inscription_id_to_inscription_entry
                .get(&key)?
                .map(|value| value.value().2),
        )
    }

    fn remove_token_holder(&mut self, script_key: &ScriptKey, tick: Tick) -> std::result::Result<(), redb::Error> {
        self.drc20_token_holder.remove(
            tick.to_lowercase().hex().as_str(),
            script_key.to_string().as_str(),
        )?;
        Ok(())
    }

    fn insert_token_holder(&mut self, script_key: &ScriptKey, tick: Tick) -> Result<(), redb::Error> {
        self.drc20_token_holder.insert(
            tick.to_lowercase().hex().as_str(),
            script_key.to_string().as_str(),
        )?;
        Ok(())
    }
}
