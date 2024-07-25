use anyhow::{anyhow, Result};
use bitcoin::{Network, Txid};
use redb::{ReadableTable, Table};
use crate::drc20::operation::{Action, deserialize_drc20_operation, InscriptionOp, Operation};
use crate::drc20::transfer::Transfer;
use crate::drc20::TransferInfo;
use crate::inscription::Inscription;
use crate::inscription_id::InscriptionId;
use crate::sat_point::SatPoint;

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub struct BlockContext {
    pub network: Network,
    pub blockheight: u64,
    pub blocktime: u32,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Message {
    pub txid: Txid,
    pub inscription_id: InscriptionId,
    pub old_satpoint: SatPoint,
    // `new_satpoint` may be none when the transaction is not yet confirmed and the sat has not been bound to the current outputs.
    pub new_satpoint: Option<SatPoint>,
    pub op: Operation,
    pub sat_in_outputs: bool,
}

impl Message {
    pub(crate) fn resolve<'a, 'db, 'tx>(
        drc20_inscribe_transfer: &'a mut Table<'db, 'tx, &'static [u8; 36], &'static [u8]>,
        new_inscriptions: &[Inscription],
        op: &InscriptionOp,
    ) -> Result<Option<Message>> {
        let sat_in_outputs = op
            .new_satpoint
            .map(|satpoint| satpoint.outpoint.txid == op.txid)
            .unwrap_or(false);

        let drc20_operation = match op.action {
            Action::New {
                inscription: _,
            } if sat_in_outputs => {
                match deserialize_drc20_operation(
                    new_inscriptions
                        .get(usize::try_from(op.inscription_id.index).unwrap())
                        .unwrap(),
                    &op.action,
                ) {
                    Ok(drc20_operation) => drc20_operation,
                    _ => return Ok(None),
                }
            }
            // Transfered inscription operation.
            // Attempt to retrieve the `InscribeTransfer` Inscription information from the data store of DRC20.
            Action::Transfer => match get_inscribe_transfer_inscription(drc20_inscribe_transfer, op.inscription_id) {
                // Ignore non-first transfer operations.
                Ok(Some(transfer_info)) if op.inscription_id.txid == op.old_satpoint.outpoint.txid => {
                    Operation::Transfer(Transfer {
                        tick: transfer_info.tick.as_str().to_string(),
                        amount: transfer_info.amt.to_string(),
                    })
                }
                Err(e) => {
                    return Err(anyhow!(
                        "failed to get inscribe transfer inscription for {}! error: {e}",
                        op.inscription_id,
                    ))
                }
                _ => return Ok(None),
            },
            _ => return Ok(None),
        };
        Ok(Some(Self {
            txid: op.txid,
            inscription_id: op.inscription_id,
            old_satpoint: op.old_satpoint,
            new_satpoint: op.new_satpoint,
            op: drc20_operation,
            sat_in_outputs,
        }))
    }
}

fn get_inscribe_transfer_inscription<'a, 'db, 'tx>(
    drc20_inscribe_transfer: &'a mut Table<'db, 'tx, &'static [u8; 36], &'static [u8]>,
    inscription_id: InscriptionId,
) -> Result<Option<TransferInfo>, redb::Error> {
    let mut value = [0; 36];
    let (txid, index) = value.split_at_mut(32);
    txid.copy_from_slice(inscription_id.txid.as_ref());
    index.copy_from_slice(&inscription_id.index.to_be_bytes());
    Ok(
        drc20_inscribe_transfer
            .get(&value)?
            .map(|v| bincode::deserialize::<TransferInfo>(v.value()).unwrap()),
    )
}