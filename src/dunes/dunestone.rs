use std::collections::HashMap;
use super::*;

const MAX_SPACERS: u32 = 0b00000111_11111111_11111111_11111111;

#[derive(Default, Serialize, Debug, PartialEq)]
pub struct Dunestone {
  pub edicts: Vec<Edict>,
  pub etching: Option<Etching>,
  pub pointer: Option<u32>,
  pub cenotaph: bool,
}


struct Message {
  cenotaph: bool,
  fields: HashMap<u128, u128>,
  edicts: Vec<Edict>,
}

impl Message {
  fn from_integers(tx: &Transaction, payload: &[u128]) -> Self {
    let mut edicts = Vec::new();
    let mut fields = HashMap::new();
    let mut cenotaph = false;

    for i in (0..payload.len()).step_by(2) {
      let tag = payload[i];

      if Tag::Body == tag {
        let mut id = 0u128;
        for chunk in payload[i + 1..].chunks_exact(3) {
          id = id.saturating_add(chunk[0]);
          if let Some(edict) = Edict::from_integers(tx, id, chunk[1], chunk[2]) {
            edicts.push(edict);
          } else {
            cenotaph = true;
          }
        }
        break;
      }

      let Some(&value) = payload.get(i + 1) else {
        break;
      };

      fields.entry(tag).or_insert(value);
    }

    Self { cenotaph, fields, edicts }
  }
}

impl Dunestone {
  pub fn from_transaction(transaction: &Transaction) -> Option<Self> {
    Self::decipher(transaction).ok().flatten()
  }

  fn decipher(transaction: &Transaction) -> Result<Option<Self>, script::Error> {
    let Some(payload) = Dunestone::payload(transaction)? else {
      return Ok(None);
    };

    let integers = Dunestone::integers(&payload);

    let Message { cenotaph, mut fields, mut edicts } = Message::from_integers(transaction, &integers);

    /* Ignore deadline
    let deadline = Tag::Deadline
        .take(&mut fields)
        .and_then(|deadline| u32::try_from(deadline).ok());*/

    let pointer = Tag::Pointer
        .take(&mut fields)
        .and_then(|default| u32::try_from(default).ok());

    let divisibility = Tag::Divisibility
        .take(&mut fields)
        .and_then(|divisibility| u8::try_from(divisibility).ok())
        .and_then(|divisibility| (divisibility <= MAX_DIVISIBILITY).then_some(divisibility));

    let limit = Tag::Limit
      .take(&mut fields)
      .map(|limit| limit.clamp(0, MAX_LIMIT));

    let dune = Tag::Dune.take(&mut fields).map(Dune);

    let cap = Tag::Cap
        .take(&mut fields)
        .map(|cap| cap);

    let premine = Tag::Premine
        .take(&mut fields)
        .map(|premine| premine);

    if premine.unwrap_or_default() > 0 {
      edicts.push(Edict{
        id: 0,
        amount: premine.unwrap_or_default(),
        output: 1,
      });
    }

    let spacers = Tag::Spacers
        .take(&mut fields)
        .and_then(|spacers| u32::try_from(spacers).ok())
        .and_then(|spacers| (spacers <= MAX_SPACERS).then_some(spacers));

    let symbol = Tag::Symbol
        .take(&mut fields)
        .and_then(|symbol| u32::try_from(symbol).ok())
        .and_then(char::from_u32);

    let height = (
    Tag::HeightStart.take(&mut fields)
        .and_then(|start_height| u64::try_from(start_height).ok()),
    Tag::HeightEnd.take(&mut fields)
        .and_then(|end_height| u64::try_from(end_height).ok())
    );

    let offset = (
      Tag::OffsetStart.take(&mut fields)
          .and_then(|start_offset| u64::try_from(start_offset).ok()),
      Tag::OffsetEnd.take(&mut fields)
          .and_then(|end_offset| u64::try_from(end_offset).ok())
    );

    let mut flags = Tag::Flags.take(&mut fields).unwrap_or_default();

    let etch = Flag::Etching.take(&mut flags);

    let terms = Flag::Terms.take(&mut flags);

    let turbo = Flag::Turbo.take(&mut flags);

    let overflow = (|| {
      let premine = premine.unwrap_or_default();
      let cap = cap.unwrap_or_default();
      let limit = limit.unwrap_or_default();
      premine.checked_add(cap.checked_mul(limit)?)
    })()
        .is_none();

    let etching = if etch {
      Some(Etching {
        divisibility,
        dune,
        spacers,
        symbol,
        terms: terms.then_some(Terms {
          cap,
          height,
          limit,
          offset,
        }),
        premine,
        turbo,
      })
    } else {
      None
    };

    Ok(Some(Self {
      cenotaph: cenotaph || overflow || flags != 0 || fields.keys().any(|tag| tag % 2 == 0),
      pointer,
      edicts,
      etching,
    }))
  }

  pub(crate) fn encipher(&self) -> Script {
    let mut payload = Vec::new();

    if let Some(etching) = self.etching {
      let mut flags = 0;
      Flag::Etching.set(&mut flags);

      if etching.terms.is_some() {
        Flag::Etching.set(&mut flags);
      }

      Tag::Flags.encode(flags, &mut payload);

      if let Some(dune) = etching.dune {
        Tag::Dune.encode(dune.0, &mut payload);
      }

      if let Some(divisibility) = etching.divisibility {
        Tag::Divisibility.encode(divisibility.into(), &mut payload);
      }

      if let Some(spacers) = etching.spacers {
        Tag::Spacers.encode(spacers.into(), &mut payload);
      }

      if let Some(symbol) = etching.symbol {
        Tag::Symbol.encode(symbol.into(), &mut payload);
      }

      if let Some(premine) = etching.premine {
        Tag::Premine.encode(premine.into(), &mut payload);
      }

      if let Some(mint) = etching.terms {
        if let Some(limit) = mint.limit {
          Tag::Limit.encode(limit, &mut payload);
        }

        if let Some(term) = mint.height.1 {
          Tag::HeightEnd.encode(term.into(), &mut payload);
        }

        if let Some(cap) = mint.cap {
          Tag::Cap.encode(cap.into(), &mut payload);
        }
      }
    }

    if let Some(default_output) = self.pointer {
      Tag::Pointer.encode(default_output.into(), &mut payload);
    }

    if self.cenotaph {
      Tag::Cenotaph.encode(0, &mut payload);
    }

    if !self.edicts.is_empty() {
      varint::encode_to_vec(Tag::Body.into(), &mut payload);

      let mut edicts = self.edicts.clone();
      edicts.sort_by_key(|edict| edict.id);

      let mut id = 0;
      for edict in edicts {
        varint::encode_to_vec(edict.id - id, &mut payload);
        varint::encode_to_vec(edict.amount, &mut payload);
        varint::encode_to_vec(edict.output, &mut payload);
        id = edict.id;
      }
    }

    let mut builder = script::Builder::new()
        .push_opcode(opcodes::all::OP_RETURN)
        .push_slice(b"D");

    for chunk in payload.chunks(bitcoin::blockdata::constants::MAX_SCRIPT_ELEMENT_SIZE) {
      let push= chunk.try_into().unwrap();
      builder = builder.push_slice(push);
    }

    builder.into_script()
  }

  fn payload(transaction: &Transaction) -> Result<Option<Vec<u8>>, script::Error> {
    for output in &transaction.output {
      let mut instructions = output.script_pubkey.instructions();

      if instructions.next().transpose()? != Some(Instruction::Op(opcodes::all::OP_RETURN)) {
        continue;
      }

      if instructions.next().transpose()? != Some(Instruction::PushBytes(b"D".as_ref().into())) {
        continue;
      }

      let mut payload = Vec::new();

      for result in instructions {
        if let Instruction::PushBytes(push) = result? {
          payload.extend_from_slice(push.as_ref().into());
        }
      }

      return Ok(Some(payload));
    }

    Ok(None)
  }

  fn integers(payload: &[u8]) -> Vec<u128> {
    let mut integers = Vec::new();
    let mut i = 0;

    while i < payload.len() {
      let (integer, length) = varint::decode(&payload[i..]);
      integers.push(integer);
      i += length;
    }

    integers
  }
}

#[cfg(test)]
mod tests {
  use bitcoin::PackedLockTime;
  use {
    super::*,
    bitcoin::{locktime, Script, TxOut},
  };

  #[test]
  fn from_transaction_returns_none_if_decipher_returns_error() {
    assert_eq!(
      Dunestone::from_transaction(&Transaction {
        input: Vec::new(),
        output: vec![TxOut {
          script_pubkey: Script::from_bytes(vec![opcodes::all::OP_PUSHBYTES_4.to_u8()]),
          value: 0,
        }],
        lock_time: PackedLockTime::ZERO,
        version: 0,
      }),
      None
    );
  }

  #[test]
  fn deciphering_transaction_with_no_outputs_returns_none() {
    assert_eq!(
      Dunestone::decipher(&Transaction {
        input: Vec::new(),
        output: Vec::new(),
        lock_time: PackedLockTime::ZERO,
        version: 0,
      }),
      Ok(None)
    );
  }

  #[test]
  fn deciphering_transaction_with_non_op_return_output_returns_none() {
    assert_eq!(
      Dunestone::decipher(&Transaction {
        input: Vec::new(),
        output: vec![TxOut {
          script_pubkey: script::Builder::new().push_slice(&*[]).into_script(),
          value: 0
        }],
        lock_time: PackedLockTime::ZERO,
        version: 0,
      }),
      Ok(None)
    );
  }

  #[test]
  fn deciphering_transaction_with_bare_op_return_returns_none() {
    assert_eq!(
      Dunestone::decipher(&Transaction {
        input: Vec::new(),
        output: vec![TxOut {
          script_pubkey: script::Builder::new()
            .push_opcode(opcodes::all::OP_RETURN)
            .into_script(),
          value: 0
        }],
        lock_time: PackedLockTime::ZERO,
        version: 0,
      }),
      Ok(None)
    );
  }

  #[test]
  fn deciphering_transaction_with_non_matching_op_return_returns_none() {
    assert_eq!(
      Dunestone::decipher(&Transaction {
        input: Vec::new(),
        output: vec![TxOut {
          script_pubkey: script::Builder::new()
            .push_opcode(opcodes::all::OP_RETURN)
            .push_slice(b"FOOO")
            .into_script(),
          value: 0
        }],
        lock_time: PackedLockTime::ZERO,
        version: 0,
      }),
      Ok(None)
    );
  }

  #[test]
  fn deciphering_valid_dunestone_with_invalid_script_returns_script_error() {
    let result = Dunestone::decipher(&Transaction {
      input: Vec::new(),
      output: vec![TxOut {
        script_pubkey: Script::from_bytes(vec![opcodes::all::OP_PUSHBYTES_4.to_u8()]),
        value: 0,
      }],
      lock_time: PackedLockTime::ZERO,
      version: 0,
    });

    match result {
      Ok(_) => panic!("expected error"),
      Err(Error::Script(_)) => {}
      Err(err) => panic!("unexpected error: {err}"),
    }
  }

  #[test]
  fn deciphering_valid_dunestone_with_invalid_script_postfix_returns_script_error() {
    let mut script_pubkey = script::Builder::new()
      .push_opcode(opcodes::all::OP_RETURN)
      .push_slice(b"D")
      .into_script()
      .into_bytes();

    script_pubkey.push(opcodes::all::OP_PUSHBYTES_4.to_u8());

    let result = Dunestone::decipher(&Transaction {
      input: Vec::new(),
      output: vec![TxOut {
        script_pubkey: Script::from_bytes(script_pubkey),
        value: 0,
      }],
      lock_time: PackedLockTime::ZERO,
      version: 0,
    });

    match result {
      Ok(_) => panic!("expected error"),
      Err(Error::Script(_)) => {}
      Err(err) => panic!("unexpected error: {err}"),
    }
  }

  #[test]
  fn deciphering_dunestone_with_invalid_varint_returns_varint_error() {
    let result = Dunestone::decipher(&Transaction {
      input: Vec::new(),
      output: vec![TxOut {
        script_pubkey: script::Builder::new()
          .push_opcode(opcodes::all::OP_RETURN)
          .push_slice(b"D")
          .push_slice(&*[128])
          .into_script(),
        value: 0,
      }],
      lock_time: PackedLockTime::ZERO,
      version: 0,
    });

    match result {
      Ok(_) => panic!("expected error"),
      Err(Error::Varint) => {}
      Err(err) => panic!("unexpected error: {err}"),
    }
  }

  #[test]
  fn non_push_opcodes_in_dunestone_are_ignored() {
    assert_eq!(
      Dunestone::decipher(&Transaction {
        input: Vec::new(),
        output: vec![TxOut {
          script_pubkey: script::Builder::new()
            .push_opcode(opcodes::all::OP_RETURN)
            .push_slice(b"D")
            .push_slice([0, 1])
            .push_opcode(opcodes::all::OP_VERIFY)
            .push_slice([2, 3])
            .into_script(),
          value: 0,
        }],
        lock_time: locktime::absolute::LockTime::ZERO,
        version: 0,
      })
      .unwrap()
      .unwrap(),
      Dunestone {
        edicts: vec![Edict {
          id: 1,
          amount: 2,
          output: 3,
        }],
        ..Default::default()
      },
    );
  }

  #[test]
  fn deciphering_empty_dunestone_is_successful() {
    assert_eq!(
      Dunestone::decipher(&Transaction {
        input: Vec::new(),
        output: vec![TxOut {
          script_pubkey: script::Builder::new()
            .push_opcode(opcodes::all::OP_RETURN)
            .push_slice(b"D")
            .into_script(),
          value: 0
        }],
        lock_time: PackedLockTime::ZERO,
        version: 0,
      }),
      Ok(Some(Dunestone::default()))
    );
  }

  fn payload(integers: &[u128]) -> Vec<u8> {
    let mut payload = Vec::new();

    for integer in integers {
      payload.extend(varint::encode(*integer));
    }

    payload
  }

  #[test]
  fn error_in_input_aborts_search_for_dunestone() {
    let payload = payload(&[0, 1, 2, 3]);

    let payload = payload.as_slice().try_into().unwrap();

    let result = Dunestone::decipher(&Transaction {
      input: Vec::new(),
      output: vec![
        TxOut {
          script_pubkey: script::Builder::new()
            .push_opcode(opcodes::all::OP_RETURN)
            .push_slice(b"D")
            .push_slice(&*[128])
            .into_script(),
          value: 0,
        },
        TxOut {
          script_pubkey: script::Builder::new()
            .push_opcode(opcodes::all::OP_RETURN)
            .push_slice(b"D")
            .push_slice(payload)
            .into_script(),
          value: 0,
        },
      ],
      lock_time: PackedLockTime::ZERO,
      version: 0,
    });

    match result {
      Ok(_) => panic!("expected error"),
      Err(Error::Varint) => {}
      Err(err) => panic!("unexpected error: {err}"),
    }
  }

  #[test]
  fn deciphering_non_empty_dunestone_is_successful() {
    let payload = payload(&[0, 1, 2, 3]);

    let payload: &PushBytes = payload.as_slice().try_into().unwrap();

    assert_eq!(
      Dunestone::decipher(&Transaction {
        input: Vec::new(),
        output: vec![TxOut {
          script_pubkey: script::Builder::new()
              .push_opcode(opcodes::all::OP_RETURN)
              .push_slice(b"D")
              .push_slice(payload)
              .into_script(),
          value: 0
        }],
        lock_time: locktime::absolute::LockTime::ZERO,
        version: 0,
      }),
      Ok(Some(Dunestone {
        edicts: vec![Edict {
          id: 1,
          amount: 2,
          output: 3,
        }],
        ..Default::default()
      }))
    );
  }

  #[test]
  fn decipher_etching() {
    let payload = payload(&[2, 4, 0, 1, 2, 3]);

    let payload: &PushBytes = payload.as_slice().try_into().unwrap();

    assert_eq!(
      Dunestone::decipher(&Transaction {
        input: Vec::new(),
        output: vec![TxOut {
          script_pubkey: script::Builder::new()
              .push_opcode(opcodes::all::OP_RETURN)
              .push_slice(b"D")
              .push_slice(payload)
              .into_script(),
          value: 0
        }],
        lock_time: locktime::absolute::LockTime::ZERO,
        version: 0,
      }),
      Ok(Some(Dunestone {
        edicts: vec![Edict {
          id: 1,
          amount: 2,
          output: 3,
        }],
        etching: Some(Etching {
          dune: Dune(4),
          ..Default::default()
        }),
        ..Default::default()
      }))
    );
  }

  #[test]
  fn duplicate_tags_are_ignored() {
    let payload = payload(&[2, 4, 2, 5, 0, 1, 2, 3]);

    let payload: &PushBytes = payload.as_slice().try_into().unwrap();

    assert_eq!(
      Dunestone::decipher(&Transaction {
        input: Vec::new(),
        output: vec![TxOut {
          script_pubkey: script::Builder::new()
              .push_opcode(opcodes::all::OP_RETURN)
              .push_slice(b"D")
              .push_slice(payload)
              .into_script(),
          value: 0
        }],
        lock_time: locktime::absolute::LockTime::ZERO,
        version: 0,
      }),
      Ok(Some(Dunestone {
        edicts: vec![Edict {
          id: 1,
          amount: 2,
          output: 3,
        }],
        etching: Some(Etching {
          dune: Dune(4),
          ..Default::default()
        }),
        ..Default::default()
      }))
    );
  }

  #[test]
  fn unrecognized_odd_tag_is_ignored() {
    let payload = payload(&[127, 100, 0, 1, 2, 3]);

    let payload: &PushBytes = payload.as_slice().try_into().unwrap();

    assert_eq!(
      Dunestone::decipher(&Transaction {
        input: Vec::new(),
        output: vec![TxOut {
          script_pubkey: script::Builder::new()
              .push_opcode(opcodes::all::OP_RETURN)
              .push_slice(b"D")
              .push_slice(payload)
              .into_script(),
          value: 0
        }],
        lock_time: locktime::absolute::LockTime::ZERO,
        version: 0,
      }),
      Ok(Some(Dunestone {
        edicts: vec![Edict {
          id: 1,
          amount: 2,
          output: 3,
        }],
        ..Default::default()
      }))
    );
  }

  #[test]
  fn tag_with_no_value_is_ignored() {
    let payload = payload(&[2, 4, 2]);

    let payload: &PushBytes = payload.as_slice().try_into().unwrap();

    assert_eq!(
      Dunestone::decipher(&Transaction {
        input: Vec::new(),
        output: vec![TxOut {
          script_pubkey: script::Builder::new()
              .push_opcode(opcodes::all::OP_RETURN)
              .push_slice(b"D")
              .push_slice(payload)
              .into_script(),
          value: 0
        }],
        lock_time: locktime::absolute::LockTime::ZERO,
        version: 0,
      }),
      Ok(Some(Dunestone {
        etching: Some(Etching {
          dune: Dune(4),
          ..Default::default()
        }),
        ..Default::default()
      }))
    );
  }

  #[test]
  fn additional_integers_in_body_are_ignored() {
    let payload = payload(&[2, 4, 0, 1, 2, 3, 4, 5]);

    let payload: &PushBytes = payload.as_slice().try_into().unwrap();

    assert_eq!(
      Dunestone::decipher(&Transaction {
        input: Vec::new(),
        output: vec![TxOut {
          script_pubkey: script::Builder::new()
              .push_opcode(opcodes::all::OP_RETURN)
              .push_slice(b"D")
              .push_slice(payload)
              .into_script(),
          value: 0
        }],
        lock_time: locktime::absolute::LockTime::ZERO,
        version: 0,
      }),
      Ok(Some(Dunestone {
        edicts: vec![Edict {
          id: 1,
          amount: 2,
          output: 3,
        }],
        etching: Some(Etching {
          dune: Dune(4),
          ..Default::default()
        }),
        ..Default::default()
      }))
    );
  }

  #[test]
  fn decipher_etching_with_divisibility() {
    let payload = payload(&[2, 4, 1, 5, 0, 1, 2, 3]);

    let payload: &PushBytes = payload.as_slice().try_into().unwrap();

    assert_eq!(
      Dunestone::decipher(&Transaction {
        input: Vec::new(),
        output: vec![TxOut {
          script_pubkey: script::Builder::new()
              .push_opcode(opcodes::all::OP_RETURN)
              .push_slice(b"D")
              .push_slice(payload)
              .into_script(),
          value: 0
        }],
        lock_time: locktime::absolute::LockTime::ZERO,
        version: 0,
      }),
      Ok(Some(Dunestone {
        edicts: vec![Edict {
          id: 1,
          amount: 2,
          output: 3,
        }],
        etching: Some(Etching {
          dune: Dune(4),
          divisibility: 5,
          ..Default::default()
        }),
        ..Default::default()
      }))
    );
  }

  #[test]
  fn divisibility_above_max_is_ignored() {
    let payload = payload(&[2, 4, 1, (MAX_DIVISIBILITY + 1).into(), 0, 1, 2, 3]);

    let payload: &PushBytes = payload.as_slice().try_into().unwrap();

    assert_eq!(
      Dunestone::decipher(&Transaction {
        input: Vec::new(),
        output: vec![TxOut {
          script_pubkey: script::Builder::new()
              .push_opcode(opcodes::all::OP_RETURN)
              .push_slice(b"D")
              .push_slice(payload)
              .into_script(),
          value: 0
        }],
        lock_time: locktime::absolute::LockTime::ZERO,
        version: 0,
      }),
      Ok(Some(Dunestone {
        edicts: vec![Edict {
          id: 1,
          amount: 2,
          output: 3,
        }],
        etching: Some(Etching {
          dune: Dune(4),
          ..Default::default()
        }),
        ..Default::default()
      }))
    );
  }

  #[test]
  fn symbol_above_max_is_ignored() {
    let payload = payload(&[2, 4, 3, u128::from(u32::from(char::MAX) + 1), 0, 1, 2, 3]);

    let payload: &PushBytes = payload.as_slice().try_into().unwrap();

    assert_eq!(
      Dunestone::decipher(&Transaction {
        input: Vec::new(),
        output: vec![TxOut {
          script_pubkey: script::Builder::new()
              .push_opcode(opcodes::all::OP_RETURN)
              .push_slice(b"D")
              .push_slice(payload)
              .into_script(),
          value: 0
        }],
        lock_time: locktime::absolute::LockTime::ZERO,
        version: 0,
      }),
      Ok(Some(Dunestone {
        edicts: vec![Edict {
          id: 1,
          amount: 2,
          output: 3,
        }],
        etching: Some(Etching {
          dune: Dune(4),
          ..Default::default()
        }),
        ..Default::default()
      }))
    );
  }

  #[test]
  fn decipher_etching_with_symbol() {
    let payload = payload(&[2, 4, 3, 'a'.into(), 0, 1, 2, 3]);

    let payload: &PushBytes = payload.as_slice().try_into().unwrap();

    assert_eq!(
      Dunestone::decipher(&Transaction {
        input: Vec::new(),
        output: vec![TxOut {
          script_pubkey: script::Builder::new()
              .push_opcode(opcodes::all::OP_RETURN)
              .push_slice(b"D")
              .push_slice(payload)
              .into_script(),
          value: 0
        }],
        lock_time: locktime::absolute::LockTime::ZERO,
        version: 0,
      }),
      Ok(Some(Dunestone {
        edicts: vec![Edict {
          id: 1,
          amount: 2,
          output: 3,
        }],
        etching: Some(Etching {
          dune: Dune(4),
          symbol: Some('a'),
          ..Default::default()
        }),
        ..Default::default()
      }))
    );
  }

  #[test]
  fn decipher_etching_with_divisibility_and_symbol() {
    let payload = payload(&[2, 4, 1, 1, 3, 'a'.into(), 0, 1, 2, 3]);

    let payload: &PushBytes = payload.as_slice().try_into().unwrap();

    assert_eq!(
      Dunestone::decipher(&Transaction {
        input: Vec::new(),
        output: vec![TxOut {
          script_pubkey: script::Builder::new()
              .push_opcode(opcodes::all::OP_RETURN)
              .push_slice(b"D")
              .push_slice(payload)
              .into_script(),
          value: 0
        }],
        lock_time: locktime::absolute::LockTime::ZERO,
        version: 0,
      }),
      Ok(Some(Dunestone {
        edicts: vec![Edict {
          id: 1,
          amount: 2,
          output: 3,
        }],
        etching: Some(Etching {
          dune: Dune(4),
          divisibility: 1,
          symbol: Some('a'),
          ..Default::default()
        }),
        ..Default::default()
      }))
    );
  }

  #[test]
  fn tag_values_are_not_parsed_as_tags() {
    let payload = payload(&[2, 4, 1, 0, 0, 1, 2, 3]);

    let payload: &PushBytes = payload.as_slice().try_into().unwrap();

    assert_eq!(
      Dunestone::decipher(&Transaction {
        input: Vec::new(),
        output: vec![TxOut {
          script_pubkey: script::Builder::new()
              .push_opcode(opcodes::all::OP_RETURN)
              .push_slice(b"D")
              .push_slice(payload)
              .into_script(),
          value: 0
        }],
        lock_time: locktime::absolute::LockTime::ZERO,
        version: 0,
      }),
      Ok(Some(Dunestone {
        edicts: vec![Edict {
          id: 1,
          amount: 2,
          output: 3,
        }],
        etching: Some(Etching {
          dune: Dune(4),
          ..Default::default()
        }),
        ..Default::default()
      }))
    );
  }

  #[test]
  fn dunestone_may_contain_multiple_edicts() {
    let payload = payload(&[0, 1, 2, 3, 3, 5, 6]);

    let payload: &PushBytes = payload.as_slice().try_into().unwrap();

    assert_eq!(
      Dunestone::decipher(&Transaction {
        input: Vec::new(),
        output: vec![TxOut {
          script_pubkey: script::Builder::new()
              .push_opcode(opcodes::all::OP_RETURN)
              .push_slice(b"D")
              .push_slice(payload)
              .into_script(),
          value: 0
        }],
        lock_time: locktime::absolute::LockTime::ZERO,
        version: 0,
      }),
      Ok(Some(Dunestone {
        edicts: vec![
          Edict {
            id: 1,
            amount: 2,
            output: 3,
          },
          Edict {
            id: 4,
            amount: 5,
            output: 6,
          },
        ],
        ..Default::default()
      }))
    );
  }

  #[test]
  fn id_deltas_saturate_to_max() {
    let payload = payload(&[0, 1, 2, 3, u128::max_value(), 5, 6]);

    let payload: &PushBytes = payload.as_slice().try_into().unwrap();

    assert_eq!(
      Dunestone::decipher(&Transaction {
        input: Vec::new(),
        output: vec![TxOut {
          script_pubkey: script::Builder::new()
              .push_opcode(opcodes::all::OP_RETURN)
              .push_slice(b"D")
              .push_slice(payload)
              .into_script(),
          value: 0
        }],
        lock_time: locktime::absolute::LockTime::ZERO,
        version: 0,
      }),
      Ok(Some(Dunestone {
        edicts: vec![
          Edict {
            id: 1,
            amount: 2,
            output: 3,
          },
          Edict {
            id: u128::max_value(),
            amount: 5,
            output: 6,
          },
        ],
        ..Default::default()
      }))
    );
  }

  #[test]
  fn payload_pushes_are_concatenated() {
    assert_eq!(
      Dunestone::decipher(&Transaction {
        input: Vec::new(),
        output: vec![TxOut {
          script_pubkey: script::Builder::new()
              .push_opcode(opcodes::all::OP_RETURN)
              .push_slice(b"D")
              .push_slice::<&PushBytes>(varint::encode(2).as_slice().try_into().unwrap())
              .push_slice::<&PushBytes>(varint::encode(4).as_slice().try_into().unwrap())
              .push_slice::<&PushBytes>(varint::encode(1).as_slice().try_into().unwrap())
              .push_slice::<&PushBytes>(varint::encode(5).as_slice().try_into().unwrap())
              .push_slice::<&PushBytes>(varint::encode(0).as_slice().try_into().unwrap())
              .push_slice::<&PushBytes>(varint::encode(1).as_slice().try_into().unwrap())
              .push_slice::<&PushBytes>(varint::encode(2).as_slice().try_into().unwrap())
              .push_slice::<&PushBytes>(varint::encode(3).as_slice().try_into().unwrap())
              .into_script(),
          value: 0
        }],
        lock_time: locktime::absolute::LockTime::ZERO,
        version: 0,
      }),
      Ok(Some(Dunestone {
        edicts: vec![Edict {
          id: 1,
          amount: 2,
          output: 3,
        }],
        etching: Some(Etching {
          dune: Dune(4),
          divisibility: 5,
          ..Default::default()
        }),
        ..Default::default()
      }))
    );
  }

  #[test]
  fn dunestone_may_be_in_second_output() {
    let payload = payload(&[0, 1, 2, 3]);

    let payload: &PushBytes = payload.as_slice().try_into().unwrap();

    assert_eq!(
      Dunestone::decipher(&Transaction {
        input: Vec::new(),
        output: vec![
          TxOut {
            script_pubkey: ScriptBuf::new(),
            value: 0,
          },
          TxOut {
            script_pubkey: script::Builder::new()
                .push_opcode(opcodes::all::OP_RETURN)
                .push_slice(b"D")
                .push_slice(payload)
                .into_script(),
            value: 0
          }
        ],
        lock_time: locktime::absolute::LockTime::ZERO,
        version: 0,
      }),
      Ok(Some(Dunestone {
        edicts: vec![Edict {
          id: 1,
          amount: 2,
          output: 3,
        }],
        ..Default::default()
      }))
    );
  }

  #[test]
  fn dunestone_may_be_after_non_matching_op_return() {
    let payload = payload(&[0, 1, 2, 3]);

    let payload: &PushBytes = payload.as_slice().try_into().unwrap();

    assert_eq!(
      Dunestone::decipher(&Transaction {
        input: Vec::new(),
        output: vec![
          TxOut {
            script_pubkey: script::Builder::new()
                .push_opcode(opcodes::all::OP_RETURN)
                .push_slice(b"FOO")
                .into_script(),
            value: 0,
          },
          TxOut {
            script_pubkey: script::Builder::new()
                .push_opcode(opcodes::all::OP_RETURN)
                .push_slice(b"D")
                .push_slice(payload)
                .into_script(),
            value: 0
          }
        ],
        lock_time: locktime::absolute::LockTime::ZERO,
        version: 0,
      }),
      Ok(Some(Dunestone {
        edicts: vec![Edict {
          id: 1,
          amount: 2,
          output: 3,
        }],
        ..Default::default()
      }))
    );
  }

  #[test]
  fn dunestone_size() {
    #[track_caller]
    fn case(edicts: Vec<Edict>, etching: Option<Etching>, size: usize) {
      assert_eq!(
        Dunestone {
          edicts,
          etching,
          ..Default::default()
        }
            .encipher()
            .len()
            - 1
            - b"D".len(),
        size
      );
    }

    case(Vec::new(), None, 1);

    case(
      Vec::new(),
      Some(Etching {
        dune: Dune(0),
        ..Default::default()
      }),
      4,
    );

    case(
      Vec::new(),
      Some(Etching {
        divisibility: MAX_DIVISIBILITY,
        dune: Dune(0),
        ..Default::default()
      }),
      6,
    );

    case(
      Vec::new(),
      Some(Etching {
        divisibility: MAX_DIVISIBILITY,
        dune: Dune(0),
        symbol: Some('$'),
        ..Default::default()
      }),
      8,
    );

    case(
      Vec::new(),
      Some(Etching {
        dune: Dune(u128::max_value()),
        ..Default::default()
      }),
      22,
    );

    case(
      vec![Edict {
        amount: 0,
        id: DuneId {
          height: 0,
          index: 0,
        }
            .into(),
        output: 0,
      }],
      Some(Etching {
        divisibility: MAX_DIVISIBILITY,
        dune: Dune(u128::max_value()),
        ..Default::default()
      }),
      28,
    );

    case(
      vec![Edict {
        amount: u128::max_value(),
        id: DuneId {
          height: 0,
          index: 0,
        }
            .into(),
        output: 0,
      }],
      Some(Etching {
        divisibility: MAX_DIVISIBILITY,
        dune: Dune(u128::max_value()),
        ..Default::default()
      }),
      46,
    );

    case(
      vec![Edict {
        amount: 0,
        id: DuneId {
          height: 1_000_000,
          index: u16::max_value(),
        }
            .into(),
        output: 0,
      }],
      None,
      11,
    );

    case(
      vec![Edict {
        amount: 0,
        id: CLAIM_BIT,
        output: 0,
      }],
      None,
      12,
    );

    case(
      vec![Edict {
        amount: u128::max_value(),
        id: DuneId {
          height: 1_000_000,
          index: u16::max_value(),
        }
            .into(),
        output: 0,
      }],
      None,
      29,
    );

    case(
      vec![
        Edict {
          amount: u128::max_value(),
          id: DuneId {
            height: 1_000_000,
            index: u16::max_value(),
          }
              .into(),
          output: 0,
        },
        Edict {
          amount: u128::max_value(),
          id: DuneId {
            height: 1_000_000,
            index: u16::max_value(),
          }
              .into(),
          output: 0,
        },
      ],
      None,
      50,
    );

    case(
      vec![
        Edict {
          amount: u128::max_value(),
          id: DuneId {
            height: 1_000_000,
            index: u16::max_value(),
          }
              .into(),
          output: 0,
        },
        Edict {
          amount: u128::max_value(),
          id: DuneId {
            height: 1_000_000,
            index: u16::max_value(),
          }
              .into(),
          output: 0,
        },
        Edict {
          amount: u128::max_value(),
          id: DuneId {
            height: 1_000_000,
            index: u16::max_value(),
          }
              .into(),
          output: 0,
        },
      ],
      None,
      71,
    );

    case(
      vec![
        Edict {
          amount: u64::max_value().into(),
          id: DuneId {
            height: 1_000_000,
            index: u16::max_value(),
          }
              .into(),
          output: 0,
        };
        4
      ],
      None,
      56,
    );

    case(
      vec![
        Edict {
          amount: u64::max_value().into(),
          id: DuneId {
            height: 1_000_000,
            index: u16::max_value(),
          }
              .into(),
          output: 0,
        };
        5
      ],
      None,
      68,
    );

    case(
      vec![
        Edict {
          amount: u64::max_value().into(),
          id: DuneId {
            height: 0,
            index: u16::max_value(),
          }
              .into(),
          output: 0,
        };
        5
      ],
      None,
      65,
    );

    case(
      vec![
        Edict {
          amount: 1_000_000_000_000_000_000,
          id: DuneId {
            height: 1_000_000,
            index: u16::max_value(),
          }
              .into(),
          output: 0,
        };
        5
      ],
      None,
      63,
    );
  }

  #[test]
  fn etching_with_term_greater_than_maximum_is_ignored() {
    let payload = payload(&[2, 4, 6, u128::from(u64::max_value()) + 1]);

    let payload = payload.as_slice().try_into().unwrap();

    assert_eq!(
      Dunestone::decipher(&Transaction {
        input: Vec::new(),
        output: vec![TxOut {
          script_pubkey: script::Builder::new()
              .push_opcode(opcodes::all::OP_RETURN)
              .push_slice(b"D")
              .push_slice(payload)
              .into_script(),
          value: 0
        }],
        lock_time: locktime::absolute::LockTime::ZERO,
        version: 0,
      }),
      Ok(Some(Dunestone {
        etching: Some(Etching {
          dune: Dune(4),
          ..Default::default()
        }),
        ..Default::default()
      }))
    );
  }
}
