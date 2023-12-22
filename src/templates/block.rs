use super::*;
use std::collections::HashMap;

impl Serialize for Height {
  fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
  {
    // Call as_u64 to get a u64 representation of the height
    let height_as_u64 = self.0;
    serializer.serialize_u64(height_as_u64)
  }
}

#[derive(Serialize)]
pub struct BlockJson {
  hash: BlockHash,
  height: u64,
  txids: String,
  inputs_per_tx: HashMap<Txid, String>,
  outputs_per_tx: HashMap<Txid, String>,
  output_values_per_tx: HashMap<Txid, String>,
  output_addresses_per_tx: HashMap<Txid, String>,
  output_scripts_per_tx: HashMap<Txid, String>,
  inscriptions_per_tx: HashMap<Txid, (InscriptionId, Option<String>, Option<Vec<u8>>)>,
}

impl BlockJson {
  pub fn new(
    block: Block,
    height: u64,
    txids: String,
    inputs_per_tx: HashMap<Txid, String>,
    outputs_per_tx: HashMap<Txid, String>,
    output_values_per_tx: HashMap<Txid, String>,
    inscriptions_per_tx: HashMap<Txid, (InscriptionId, Option<String>, Option<Vec<u8>>)>,
    output_addresses_per_tx: HashMap<Txid, String>,
    output_scripts_per_tx: HashMap<Txid, String>,
  ) -> Self {
    let mut target = block.header.target().to_be_bytes();
    target.reverse();
    Self {
      hash: block.header.block_hash(),
      height: height.into(),
      txids,
      inputs_per_tx,
      outputs_per_tx,
      output_values_per_tx,
      inscriptions_per_tx,
      output_addresses_per_tx,
      output_scripts_per_tx
    }
  }
}

#[derive(Boilerplate)]
pub(crate) struct BlockHtml {
  hash: BlockHash,
  target: BlockHash,
  best_height: Height,
  block: Block,
  height: Height,
  inputs_per_tx: HashMap<Txid, String>,
  outputs_per_tx: HashMap<Txid, String>,
  output_values_per_tx: HashMap<Txid, String>,
  output_addresses_per_tx: HashMap<Txid, String>,
  inscriptions_per_tx: HashMap<Txid, (InscriptionId, Option<String>, Option<Vec<u8>>)>,
}

impl BlockHtml {
  pub(crate) fn new(
    block: Block,
    height: Height,
    best_height: Height,
    inputs_per_tx: HashMap<Txid, String>,
    outputs_per_tx: HashMap<Txid, String>,
    output_values_per_tx: HashMap<Txid, String>,
    inscriptions_per_tx: HashMap<Txid, (InscriptionId, Option<String>, Option<Vec<u8>>)>,
    output_addresses_per_tx: HashMap<Txid, String>,
  ) -> Self {
    let mut target = block.header.target().to_be_bytes();
    target.reverse();

    Self {
      hash: block.header.block_hash(),
      target: BlockHash::from_inner(target),
      block,
      height,
      best_height,
      inputs_per_tx,
      outputs_per_tx,
      output_values_per_tx,
      inscriptions_per_tx,
      output_addresses_per_tx,
    }
  }
}

impl PageContent for BlockHtml {
  fn title(&self) -> String {
    format!("Block {}", self.height)
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn html() {
    assert_regex_match!(
      BlockHtml::new(Chain::Mainnet.genesis_block(), Height(0), Height(0)),
      "
        <h1>Block 0</h1>
        <dl>
          <dt>hash</dt><dd class=monospace>[[:xdigit:]]{64}</dd>
          <dt>target</dt><dd class=monospace>[[:xdigit:]]{64}</dd>
          <dt>timestamp</dt><dd><time>2013-12-06 10:25:40 UTC</time></dd>
          <dt>size</dt><dd>224</dd>
          <dt>weight</dt><dd>896</dd>
        </dl>
        .*
        prev
        next
        .*
        <h2>1 Transaction</h2>
        <ul class=monospace>
          <li><a href=/tx/[[:xdigit:]]{64}>[[:xdigit:]]{64}</a></li>
        </ul>
      "
      .unindent()
    );
  }

  #[test]
  fn next_active_when_not_last() {
    assert_regex_match!(
      BlockHtml::new(Chain::Mainnet.genesis_block(), Height(0), Height(1)),
      r"<h1>Block 0</h1>.*prev\s*<a class=next href=/block/1>next</a>.*"
    );
  }

  #[test]
  fn prev_active_when_not_first() {
    assert_regex_match!(
      BlockHtml::new(Chain::Mainnet.genesis_block(), Height(1), Height(1)),
      r"<h1>Block 1</h1>.*<a class=prev href=/block/0>prev</a>\s*next.*",
    );
  }
}
