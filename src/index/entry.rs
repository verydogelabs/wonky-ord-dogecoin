use crate::dunes::MintError;
use crate::sat::Sat;
use crate::sat_point::SatPoint;

use super::*;

pub(crate) trait Entry: Sized {
  type Value;

  fn load(value: Self::Value) -> Self;

  fn store(self) -> Self::Value;
}

pub(super) type BlockHashValue = [u8; 32];

impl Entry for BlockHash {
  type Value = BlockHashValue;

  fn load(value: Self::Value) -> Self {
    BlockHash::from_inner(value)
  }

  fn store(self) -> Self::Value {
    self.into_inner()
  }
}

pub(crate) type TxidValue = [u8; 32];

impl Entry for Txid {
  type Value = TxidValue;

  fn load(value: Self::Value) -> Self {
    Txid::from_inner(value)
  }

  fn store(self) -> Self::Value {
    self.into_inner()
  }
}

#[derive(Debug, PartialEq, Copy, Clone, Serialize, Deserialize)]
pub(crate) struct DuneEntry {
  pub(crate) block: u64,
  pub(crate) burned: u128,
  pub(crate) divisibility: u8,
  pub(crate) etching: Txid,
  pub(crate) terms: Option<Terms>,
  pub(crate) mints: u128,
  pub(crate) number: u64,
  pub(crate) premine: u128,
  pub(crate) dune: Dune,
  pub(crate) spacers: u32,
  pub(crate) supply: u128,
  pub(crate) symbol: Option<char>,
  pub(crate) timestamp: u64,
  pub(crate) turbo: bool,
}

pub(super) type DuneEntryValue = (
  u64,                     // block
  u128,                    // burned
  u8,                      // divisibility
  (u128, u128),            // etching
  Option<TermsEntryValue>, // terms parameters
  u128,                    // mints
  u64,                     // number
  (u128, u32),             // dune + spacers
  (u128, u128),            // supply + premine
  u32,                     // symbol
  u64,                     // timestamp
  bool,                    // turbo
);

type TermsEntryValue = (
  Option<u128>,               // cap
  Option<u128>,               // limit
  (Option<u64>, Option<u64>), // height
  (Option<u64>, Option<u64>), // offset
);

impl DuneEntry {
  pub(crate) fn spaced_dune(&self) -> SpacedDune {
    SpacedDune {
      dune: self.dune,
      spacers: self.spacers,
    }
  }

  pub fn mintable(&self, height: u64) -> Result<u128, MintError> {
    let Some(terms) = self.terms else {
      return Err(MintError::Unmintable);
    };

    if let Some(start) = self.start() {
      if height < start {
        return Err(MintError::Start(start));
      }
    }

    if let Some(end) = self.end() {
      if height >= end {
        return Err(MintError::End(end));
      }
    }

    if let Some(cap) = terms.cap {
      if self.mints >= cap {
        return Err(MintError::Cap(cap));
      }
    } else {
      if self.mints >= u128::MAX {
        return Err(MintError::Cap(u128::MAX));
      }
    }

    Ok(terms.limit.unwrap_or_default())
  }

  pub fn pile(&self, amount: u128) -> Pile {
    Pile {
      amount,
      divisibility: self.divisibility,
      symbol: self.symbol,
    }
  }

  pub fn start(&self) -> Option<u64> {
    let terms = self.terms?;

    let relative = terms
      .offset
      .0
      .map(|offset| self.block.saturating_add(offset));

    let absolute = terms.height.0;

    relative
      .zip(absolute)
      .map(|(relative, absolute)| relative.max(absolute))
      .or(relative)
      .or(absolute)
  }

  pub fn end(&self) -> Option<u64> {
    let terms = self.terms?;

    let relative = terms
      .offset
      .1
      .map(|offset| self.block.saturating_add(offset));

    let absolute = terms.height.1;

    relative
      .zip(absolute)
      .map(|(relative, absolute)| relative.min(absolute))
      .or(relative)
      .or(absolute)
  }

  pub fn supply(&self) -> u128 {
    self.premine + self.supply + self.burned
  }
}

impl Default for DuneEntry {
  fn default() -> Self {
    Self {
      block: 0,
      burned: 0,
      divisibility: 0,
      etching: Txid::all_zeros(),
      terms: None,
      mints: 0,
      number: 0,
      premine: 0,
      dune: Dune(0),
      spacers: 0,
      supply: 0,
      symbol: None,
      timestamp: 0,
      turbo: false,
    }
  }
}

/*pub(super) type TxidValue = [u8; 32];

impl Entry for Txid {
  type Value = TxidValue;

  fn load(value: Self::Value) -> Self {
    Txid::from_byte_array(value)
  }

  fn store(self) -> Self::Value {
    Txid::to_byte_array(self)
  }
}*/

impl Entry for DuneEntry {
  type Value = DuneEntryValue;
  fn load(
    (
      block,
      burned,
      divisibility,
      etching,
      terms,
      mints,
      number,
      (dune, spacers),
      (supply, premine),
      symbol,
      timestamp,
      turbo,
    ): DuneEntryValue,
  ) -> Self {
    Self {
      block,
      burned,
      divisibility,
      etching: {
        let low = etching.0.to_le_bytes();
        let high = etching.1.to_le_bytes();
        let bytes: Vec<u8> = [low, high].concat();
        Txid::from_slice(bytes.as_slice()).unwrap_or(Txid::all_zeros())
      },
      terms: terms.map(|(cap, limit, height, offset)| Terms {
        cap,
        limit,
        height,
        offset,
      }),
      mints,
      number,
      premine,
      dune: Dune(dune),
      spacers,
      supply,
      symbol: char::from_u32(symbol),
      timestamp,
      turbo,
    }
  }

  fn store(self) -> Self::Value {
    (
      self.block,
      self.burned,
      self.divisibility,
      {
        let bytes_vec = self.etching.to_vec();
        let bytes: [u8; 32] = match bytes_vec.len() {
          32 => {
            let mut array = [0; 32];
            array.copy_from_slice(&bytes_vec);
            array
          }
          _ => panic!("Vector length is not 32"),
        };
        (
          u128::from_le_bytes([
            bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5], bytes[6], bytes[7],
            bytes[8], bytes[9], bytes[10], bytes[11], bytes[12], bytes[13], bytes[14], bytes[15],
          ]),
          u128::from_le_bytes([
            bytes[16], bytes[17], bytes[18], bytes[19], bytes[20], bytes[21], bytes[22], bytes[23],
            bytes[24], bytes[25], bytes[26], bytes[27], bytes[28], bytes[29], bytes[30], bytes[31],
          ]),
        )
      },
      self.terms.map(
        |Terms {
           cap,
           limit,
           height,
           offset,
         }| (cap, limit, height, offset),
      ),
      self.mints,
      self.number,
      (self.dune.0, self.spacers),
      (self.supply, self.premine),
      self.symbol.map(u32::from).unwrap_or(u32::MAX),
      self.timestamp,
      self.turbo,
    )
  }
}

pub(super) type DuneIdValue = (u64, u32);

impl Entry for DuneId {
  type Value = DuneIdValue;

  fn load((height, index): Self::Value) -> Self {
    Self { height, index }
  }

  fn store(self) -> Self::Value {
    (self.height, self.index)
  }
}

pub(super) type DuneAddressBalance = (u128, u128);

pub(crate) struct InscriptionEntry {
  pub(crate) fee: u64,
  pub(crate) height: u32,
  pub(crate) inscription_number: u64,
  pub(crate) sat: Option<Sat>,
  pub(crate) sequence_number: u64,
  pub(crate) timestamp: u32,
}

pub(crate) type InscriptionEntryValue = (
  u64,         // fee
  u32,         // height
  u64,         // inscription number
  Option<u64>, // sat
  u64,         // sequence number
  u32,         // timestamp
);

impl Entry for InscriptionEntry {
  type Value = InscriptionEntryValue;

  fn load(
    (fee, height, inscription_number, sat, sequence_number, timestamp): InscriptionEntryValue,
  ) -> Self {
    Self {
      fee,
      height,
      inscription_number,
      sat: sat.map(Sat),
      sequence_number,
      timestamp,
    }
  }

  fn store(self) -> Self::Value {
    (
      self.fee,
      self.height,
      self.inscription_number,
      self.sat.map(Sat::n),
      self.sequence_number,
      self.timestamp,
    )
  }
}

pub type InscriptionIdValue = [u8; 36];

impl Entry for InscriptionId {
  type Value = InscriptionIdValue;

  fn load(value: Self::Value) -> Self {
    let (txid, index) = value.split_at(32);
    Self {
      txid: Txid::from_inner(txid.try_into().unwrap()),
      index: u32::from_be_bytes(index.try_into().unwrap()),
    }
  }

  fn store(self) -> Self::Value {
    let mut value = [0; 36];
    let (txid, index) = value.split_at_mut(32);
    txid.copy_from_slice(self.txid.as_inner());
    index.copy_from_slice(&self.index.to_be_bytes());
    value
  }
}

pub(crate) struct OutPointMap {
  pub(crate) value: u64,
  pub(crate) address: [u8; 34],
}

pub(crate) type OutPointMapValue = (u64, [u8; 34]);

impl Entry for OutPointMap {
  type Value = OutPointMapValue;

  fn load(value: Self::Value) -> Self {
    Self {
      value: value.0,
      address: value.1,
    }
  }

  fn store(self) -> Self::Value {
    (self.value, self.address)
  }
}

pub type OutPointValue = [u8; 36];

impl Entry for OutPoint {
  type Value = OutPointValue;

  fn load(value: Self::Value) -> Self {
    Decodable::consensus_decode(&mut io::Cursor::new(value)).unwrap()
  }

  fn store(self) -> Self::Value {
    let mut value = [0; 36];
    self.consensus_encode(&mut value.as_mut_slice()).unwrap();
    value
  }
}

pub(super) type SatPointValue = [u8; 44];

impl Entry for SatPoint {
  type Value = SatPointValue;

  fn load(value: Self::Value) -> Self {
    Decodable::consensus_decode(&mut io::Cursor::new(value)).unwrap()
  }

  fn store(self) -> Self::Value {
    let mut value = [0; 44];
    self.consensus_encode(&mut value.as_mut_slice()).unwrap();
    value
  }
}

pub(super) type SatRange = (u64, u64);

impl Entry for SatRange {
  type Value = [u8; 11];

  fn load([b0, b1, b2, b3, b4, b5, b6, b7, b8, b9, b10]: Self::Value) -> Self {
    let raw_base = u64::from_le_bytes([b0, b1, b2, b3, b4, b5, b6, 0]);

    // 51 bit base
    let base = raw_base & ((1 << 51) - 1);

    let raw_delta = u64::from_le_bytes([b6, b7, b8, b9, b10, 0, 0, 0]);

    // 33 bit delta
    let delta = raw_delta >> 3;

    (base, base + delta)
  }

  fn store(self) -> Self::Value {
    let base = self.0;
    let delta = self.1 - self.0;
    let n = u128::from(base) | u128::from(delta) << 51;
    n.to_le_bytes()[0..11].try_into().unwrap()
  }
}
