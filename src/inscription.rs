use {
  super::*,
  bitcoin::{
    blockdata::{opcodes, script},
    Script,
  },
  std::str,
};

const PROTOCOL_ID: &[u8] = b"ord";

#[derive(Debug, PartialEq, Clone)]
pub(crate) struct Inscription {
  body: Option<Vec<u8>>,
  content_type: Option<Vec<u8>>,
}

#[derive(Debug, PartialEq)]
pub(crate) enum ParsedInscription {
  None,
  Partial,
  Complete(Inscription),
}

impl Inscription {
  #[cfg(test)]
  pub(crate) fn new(content_type: Option<Vec<u8>>, body: Option<Vec<u8>>) -> Self {
    Self { content_type, body }
  }

  pub(crate) fn from_transactions(txs: Vec<Transaction>) -> ParsedInscription {
    let mut sig_scripts = Vec::with_capacity(txs.len());
    for i in 0..txs.len() {
      if txs[i].input.is_empty() {
        return ParsedInscription::None;
      }
      sig_scripts.push(txs[i].input[0].script_sig.clone());
    }
    InscriptionParser::parse(sig_scripts)
  }

  pub(crate) fn from_file(chain: Chain, path: impl AsRef<Path>) -> Result<Self, Error> {
    let path = path.as_ref();

    let body = fs::read(path).with_context(|| format!("io error reading {}", path.display()))?;

    if let Some(limit) = chain.inscription_content_size_limit() {
      let len = body.len();
      if len > limit {
        bail!("content size of {len} bytes exceeds {limit} byte limit for {chain} inscriptions");
      }
    }

    let content_type = Media::content_type_for_path(path)?;

    Ok(Self {
      body: Some(body),
      content_type: Some(content_type.into()),
    })
  }

  fn append_reveal_script_to_builder(&self, mut builder: script::Builder) -> script::Builder {
    builder = builder
      .push_opcode(opcodes::OP_FALSE)
      .push_opcode(opcodes::all::OP_IF)
      .push_slice(PROTOCOL_ID);

    if let Some(content_type) = &self.content_type {
      builder = builder.push_slice(&[1]).push_slice(content_type);
    }

    if let Some(body) = &self.body {
      builder = builder.push_slice(&[]);
      for chunk in body.chunks(520) {
        builder = builder.push_slice(chunk);
      }
    }

    builder.push_opcode(opcodes::all::OP_ENDIF)
  }

  pub(crate) fn append_reveal_script(&self, builder: script::Builder) -> Script {
    self.append_reveal_script_to_builder(builder).into_script()
  }

  pub(crate) fn media(&self) -> Media {
    if self.body.is_none() {
      return Media::Unknown;
    }

    let Some(content_type) = self.content_type() else {
      return Media::Unknown;
    };

    content_type.parse().unwrap_or(Media::Unknown)
  }

  pub(crate) fn body(&self) -> Option<&[u8]> {
    Some(self.body.as_ref()?)
  }

  pub(crate) fn into_body(self) -> Option<Vec<u8>> {
    self.body
  }

  pub(crate) fn content_length(&self) -> Option<usize> {
    Some(self.body()?.len())
  }

  pub(crate) fn content_type(&self) -> Option<&str> {
    str::from_utf8(self.content_type.as_ref()?).ok()
  }

  #[cfg(test)]
  pub(crate) fn to_witness(&self) -> Witness {
    let builder = script::Builder::new();

    let script = self.append_reveal_script(builder);

    let mut witness = Witness::new();

    witness.push(script);
    witness.push([]);

    witness
  }
}

struct InscriptionParser {}

impl InscriptionParser {
  fn parse(sig_scripts: Vec<Script>) -> ParsedInscription {
    let sig_script = &sig_scripts[0];

    let mut push_datas_vec = match Self::decode_push_datas(sig_script) {
      Some(push_datas) => push_datas,
      None => return ParsedInscription::None,
    };

    let mut push_datas = push_datas_vec.as_slice();

    // read protocol

    if push_datas.len() < 3 {
      return ParsedInscription::None;
    }

    let protocol = &push_datas[0];

    if protocol != PROTOCOL_ID {
      return ParsedInscription::None;
    }

    // read npieces

    let mut npieces = match Self::push_data_to_number(&push_datas[1]) {
      Some(n) => n,
      None => return ParsedInscription::None,
    };

    if npieces == 0 {
      return ParsedInscription::None;
    }

    // read content type

    let content_type = push_datas[2].clone();

    push_datas = &push_datas[3..];

    // read body

    let mut body = vec![];

    let mut sig_scripts = sig_scripts.as_slice();

    // loop over transactions
    loop {
      // loop over chunks
      loop {
        if npieces == 0 {
          let inscription = Inscription {
            content_type: Some(content_type),
            body: Some(body),
          };

          return ParsedInscription::Complete(inscription);
        }

        if push_datas.len() < 2 {
          break;
        }

        let next = match Self::push_data_to_number(&push_datas[0]) {
          Some(n) => n,
          None => break,
        };

        if next != npieces - 1 {
          break;
        }

        body.append(&mut push_datas[1].clone());

        push_datas = &push_datas[2..];
        npieces -= 1;
      }

      if sig_scripts.len() <= 1 {
        return ParsedInscription::Partial;
      }

      sig_scripts = &sig_scripts[1..];

      push_datas_vec = match Self::decode_push_datas(&sig_scripts[0]) {
        Some(push_datas) => push_datas,
        None => return ParsedInscription::None,
      };

      if push_datas_vec.len() < 2 {
        return ParsedInscription::None;
      }

      let next = match Self::push_data_to_number(&push_datas_vec[0]) {
        Some(n) => n,
        None => return ParsedInscription::None,
      };

      if next != npieces - 1 {
        return ParsedInscription::None;
      }

      push_datas = push_datas_vec.as_slice();
    }
  }

  fn decode_push_datas(script: &Script) -> Option<Vec<Vec<u8>>> {
    let mut bytes = script.as_bytes();
    let mut push_datas = vec![];

    while !bytes.is_empty() {
      // op_0
      if bytes[0] == 0 {
        push_datas.push(vec![]);
        bytes = &bytes[1..];
        continue;
      }

      // op_1 - op_16
      if bytes[0] >= 81 && bytes[0] <= 96 {
        push_datas.push(vec![bytes[0] - 80]);
        bytes = &bytes[1..];
        continue;
      }

      // op_push 1-75
      if bytes[0] >= 1 && bytes[0] <= 75 {
        let len = bytes[0] as usize;
        if bytes.len() < 1 + len {
          return None;
        }
        push_datas.push(bytes[1..1 + len].to_vec());
        bytes = &bytes[1 + len..];
        continue;
      }

      // op_pushdata1
      if bytes[0] == 76 {
        if bytes.len() < 2 {
          return None;
        }
        let len = bytes[1] as usize;
        if bytes.len() < 2 + len {
          return None;
        }
        push_datas.push(bytes[2..2 + len].to_vec());
        bytes = &bytes[2 + len..];
        continue;
      }

      // op_pushdata2
      if bytes[0] == 77 {
        if bytes.len() < 3 {
          return None;
        }
        let len = ((bytes[1] as usize) << 8) + ((bytes[0] as usize) << 0);
        if bytes.len() < 3 + len {
          return None;
        }
        push_datas.push(bytes[3..3 + len].to_vec());
        bytes = &bytes[3 + len..];
        continue;
      }

      // op_pushdata4
      if bytes[0] == 78 {
        if bytes.len() < 5 {
          return None;
        }
        let len = ((bytes[3] as usize) << 24)
          + ((bytes[2] as usize) << 16)
          + ((bytes[1] as usize) << 8)
          + ((bytes[0] as usize) << 0);
        if bytes.len() < 5 + len {
          return None;
        }
        push_datas.push(bytes[5..5 + len].to_vec());
        bytes = &bytes[5 + len..];
        continue;
      }

      return None;
    }

    Some(push_datas)
  }

  fn push_data_to_number(data: &[u8]) -> Option<u64> {
    if data.len() == 0 {
      return Some(0);
    }

    if data.len() > 8 {
      return None;
    }

    let mut n: u64 = 0;
    let mut m: u64 = 0;

    for i in 0..data.len() {
      n += (data[i] as u64) << m;
      m += 8;
    }

    return Some(n);
  }
}

#[cfg(test)]
mod tests {
  use bitcoin::hashes::hex::FromHex;

  use super::*;

  #[test]
  fn empty() {
    assert_eq!(
      InscriptionParser::parse(vec![Script::new()]),
      ParsedInscription::None
    );
  }

  #[test]
  fn no_inscription() {
    assert_eq!(
      InscriptionParser::parse(vec![Script::from_hex("483045022100a942753a4e036f59648469cb6ac19b33b1e423ff5ceaf93007001b54df46ca1f022025f6554a58b6fde5ff24b5e2556acc57d1d2108c0de2a14096e7ddae9c9fb96d0121034523d20080d1abe75a9fbed07b83e695db2f30e2cd89b80b154a0ed70badfc90").unwrap()]),
      ParsedInscription::None
    );
  }

  #[test]
  fn valid() {
    let mut script: Vec<&[u8]> = Vec::new();
    script.push(&[3]);
    script.push(b"ord");
    script.push(&[81]);
    script.push(&[24]);
    script.push(b"text/plain;charset=utf-8");
    script.push(&[0]);
    script.push(&[4]);
    script.push(b"woof");
    assert_eq!(
      InscriptionParser::parse(vec![Script::from(script.concat())]),
      ParsedInscription::Complete(inscription("text/plain;charset=utf-8", "woof"))
    );
  }

  #[test]
  fn valid_empty_fields() {
    let mut script: Vec<&[u8]> = Vec::new();
    script.push(&[3]);
    script.push(b"ord");
    script.push(&[81]);
    script.push(&[0]);
    script.push(&[0]);
    script.push(&[0]);
    assert_eq!(
      InscriptionParser::parse(vec![Script::from(script.concat())]),
      ParsedInscription::Complete(inscription("", ""))
    );
  }

  #[test]
  fn valid_multipart() {
    let mut script: Vec<&[u8]> = Vec::new();
    script.push(&[3]);
    script.push(b"ord");
    script.push(&[82]);
    script.push(&[24]);
    script.push(b"text/plain;charset=utf-8");
    script.push(&[81]);
    script.push(&[4]);
    script.push(b"woof");
    script.push(&[0]);
    script.push(&[5]);
    script.push(b" woof");
    assert_eq!(
      InscriptionParser::parse(vec![Script::from(script.concat())]),
      ParsedInscription::Complete(inscription("text/plain;charset=utf-8", "woof woof"))
    );
  }

  #[test]
  fn valid_multitx() {
    let mut script1: Vec<&[u8]> = Vec::new();
    let mut script2: Vec<&[u8]> = Vec::new();
    script1.push(&[3]);
    script1.push(b"ord");
    script1.push(&[82]);
    script1.push(&[24]);
    script1.push(b"text/plain;charset=utf-8");
    script1.push(&[81]);
    script1.push(&[4]);
    script1.push(b"woof");
    script2.push(&[0]);
    script2.push(&[5]);
    script2.push(b" woof");
    assert_eq!(
      InscriptionParser::parse(vec![
        Script::from(script1.concat()),
        Script::from(script2.concat())
      ]),
      ParsedInscription::Complete(inscription("text/plain;charset=utf-8", "woof woof"))
    );
  }

  #[test]
  fn valid_multitx_long() {
    let mut expected = String::new();
    let mut script_parts = vec![];

    let mut script: Vec<Vec<u8>> = Vec::new();
    script.push(vec![3]);
    script.push(b"ord".to_vec());
    const LEN: usize = 100000;
    push_number(&mut script, LEN as u64);
    script.push(vec![24]);
    script.push(b"text/plain;charset=utf-8".to_vec());

    let mut i = 0;
    while i < LEN {
      let text = format!("{}", i % 10);
      expected += text.as_str();
      push_number(&mut script, (LEN - i - 1) as u64);
      script.push(vec![1]);
      script.push(text.as_bytes().to_vec());
      i += 1;

      let text = format!("{}", i % 10);
      expected += text.as_str();
      push_number(&mut script, (LEN - i - 1) as u64);
      script.push(vec![1]);
      script.push(text.as_bytes().to_vec());
      i += 1;

      script_parts.push(script);
      script = Vec::new();
    }

    let mut scripts = vec![];
    script_parts
      .iter()
      .for_each(|script| scripts.push(Script::from(script.concat())));

    assert_eq!(
      InscriptionParser::parse(scripts),
      ParsedInscription::Complete(inscription("text/plain;charset=utf-8", expected))
    );
  }

  #[test]
  fn valid_multitx_extradata() {
    let mut script1: Vec<&[u8]> = Vec::new();
    let mut script2: Vec<&[u8]> = Vec::new();
    script1.push(&[3]);
    script1.push(b"ord");
    script1.push(&[82]);
    script1.push(&[24]);
    script1.push(b"text/plain;charset=utf-8");
    script1.push(&[81]);
    script1.push(&[4]);
    script1.push(b"woof");
    script1.push(&[82]);
    script1.push(&[4]);
    script1.push(b"bark");
    script2.push(&[0]);
    script2.push(&[5]);
    script2.push(b" woof");
    assert_eq!(
      InscriptionParser::parse(vec![
        Script::from(script1.concat()),
        Script::from(script2.concat())
      ]),
      ParsedInscription::Complete(inscription("text/plain;charset=utf-8", "woof woof"))
    );
  }

  #[test]
  fn invalid_multitx_missingdata() {
    let mut script1: Vec<&[u8]> = Vec::new();
    let mut script2: Vec<&[u8]> = Vec::new();
    script1.push(&[3]);
    script1.push(b"ord");
    script1.push(&[82]);
    script1.push(&[24]);
    script1.push(b"text/plain;charset=utf-8");
    script1.push(&[81]);
    script1.push(&[4]);
    script1.push(b"woof");
    script2.push(&[0]);
    assert_eq!(
      InscriptionParser::parse(vec![
        Script::from(script1.concat()),
        Script::from(script2.concat())
      ]),
      ParsedInscription::None
    );
  }

  #[test]
  fn invalid_multitx_wrongcountdown() {
    let mut script1: Vec<&[u8]> = Vec::new();
    let mut script2: Vec<&[u8]> = Vec::new();
    script1.push(&[3]);
    script1.push(b"ord");
    script1.push(&[82]);
    script1.push(&[24]);
    script1.push(b"text/plain;charset=utf-8");
    script1.push(&[81]);
    script1.push(&[4]);
    script1.push(b"woof");
    script2.push(&[81]);
    script2.push(&[5]);
    script2.push(b" woof");
    assert_eq!(
      InscriptionParser::parse(vec![
        Script::from(script1.concat()),
        Script::from(script2.concat())
      ]),
      ParsedInscription::None
    );
  }

  fn push_number(script: &mut Vec<Vec<u8>>, num: u64) {
    if num == 0 {
      script.push(vec![0]);
      return;
    }

    if num <= 16 {
      script.push(vec![(80 + num) as u8]);
      return;
    }

    if num <= 0x7f {
      script.push(vec![1]);
      script.push(vec![num as u8]);
      return;
    }

    if num <= 0x7fff {
      script.push(vec![2]);
      script.push(vec![(num % 256) as u8, (num / 256) as u8]);
      return;
    }

    if num <= 0x7fffff {
      script.push(vec![3]);
      script.push(vec![
        (num % 256) as u8,
        ((num / 256) % 256) as u8,
        (num / 256 / 256) as u8,
      ]);
      return;
    }

    panic!();
  }

  #[test]
  fn valid_long() {
    let mut expected = String::new();
    let mut script: Vec<Vec<u8>> = Vec::new();
    script.push(vec![3]);
    script.push(b"ord".to_vec());
    const LEN: usize = 100000;
    push_number(&mut script, LEN as u64);
    script.push(vec![24]);
    script.push(b"text/plain;charset=utf-8".to_vec());
    for i in 0..LEN {
      let text = format!("{}", i % 10);
      expected += text.as_str();
      push_number(&mut script, (LEN - i - 1) as u64);
      script.push(vec![1]);
      script.push(text.as_bytes().to_vec());
    }
    assert_eq!(
      InscriptionParser::parse(vec![Script::from(script.concat())]),
      ParsedInscription::Complete(inscription("text/plain;charset=utf-8", expected))
    );
  }

  #[test]
  fn duplicate_field() {
    let mut script: Vec<&[u8]> = Vec::new();
    script.push(&[3]);
    script.push(b"ord");
    script.push(&[81]);
    script.push(&[24]);
    script.push(b"text/plain;charset=utf-8");
    script.push(&[81]);
    script.push(&[24]);
    script.push(b"text/plain;charset=utf-8");
    script.push(&[0]);
    script.push(&[4]);
    script.push(b"woof");
    assert_eq!(
      InscriptionParser::parse(vec![Script::from(script.concat())]),
      ParsedInscription::Partial,
    );
  }

  #[test]
  fn invalid_tag() {
    let mut script: Vec<&[u8]> = Vec::new();
    script.push(&[3]);
    script.push(b"ord");
    script.push(&[81]);
    script.push(&[24]);
    script.push(b"text/plain;charset=utf-8");
    script.push(&[82]);
    script.push(&[4]);
    script.push(b"woof");
    assert_eq!(
      InscriptionParser::parse(vec![Script::from(script.concat())]),
      ParsedInscription::Partial,
    );
  }

  #[test]
  fn no_content() {
    let mut script: Vec<&[u8]> = Vec::new();
    script.push(&[3]);
    script.push(b"ord");
    script.push(&[81]);
    script.push(&[24]);
    script.push(b"text/plain;charset=utf-8");
    assert_eq!(
      InscriptionParser::parse(vec![Script::from(script.concat())]),
      ParsedInscription::Partial,
    );
  }

  #[test]
  fn no_content_type() {
    let mut script: Vec<&[u8]> = Vec::new();
    script.push(&[3]);
    script.push(b"ord");
    script.push(&[0]);
    script.push(&[4]);
    script.push(b"woof");
    assert_eq!(
      InscriptionParser::parse(vec![Script::from(script.concat())]),
      ParsedInscription::None,
    );
  }

  #[test]
  fn valid_with_extra_data() {
    let mut script: Vec<&[u8]> = Vec::new();
    script.push(&[3]);
    script.push(b"ord");
    script.push(&[81]);
    script.push(&[24]);
    script.push(b"text/plain;charset=utf-8");
    script.push(&[0]);
    script.push(&[4]);
    script.push(b"woof");
    script.push(&[9]);
    script.push(b"woof woof");
    script.push(&[14]);
    script.push(b"woof woof woof");
    assert_eq!(
      InscriptionParser::parse(vec![Script::from(script.concat())]),
      ParsedInscription::Complete(inscription("text/plain;charset=utf-8", "woof"))
    );
  }

  #[test]
  fn prefix_data() {
    let mut script: Vec<&[u8]> = Vec::new();
    script.push(&[4]);
    script.push(b"woof");
    script.push(&[3]);
    script.push(b"ord");
    script.push(&[81]);
    script.push(&[24]);
    script.push(b"text/plain;charset=utf-8");
    script.push(&[0]);
    script.push(&[4]);
    script.push(b"woof");
    assert_eq!(
      InscriptionParser::parse(vec![Script::from(script.concat())]),
      ParsedInscription::None,
    );
  }

  #[test]
  fn wrong_protocol() {
    let mut script: Vec<&[u8]> = Vec::new();
    script.push(&[3]);
    script.push(b"dog");
    script.push(&[81]);
    script.push(&[24]);
    script.push(b"text/plain;charset=utf-8");
    script.push(&[0]);
    script.push(&[4]);
    script.push(b"woof");
    assert_eq!(
      InscriptionParser::parse(vec![Script::from(script.concat())]),
      ParsedInscription::None
    );
  }

  #[test]
  fn incomplete_multipart() {
    let mut script: Vec<&[u8]> = Vec::new();
    script.push(&[3]);
    script.push(b"ord");
    script.push(&[82]);
    script.push(&[24]);
    script.push(b"text/plain;charset=utf-8");
    script.push(&[81]);
    script.push(&[4]);
    script.push(b"woof");
    assert_eq!(
      InscriptionParser::parse(vec![Script::from(script.concat())]),
      ParsedInscription::Partial
    );
  }

  #[test]
  fn bad_npieces() {
    let mut script: Vec<&[u8]> = Vec::new();
    script.push(&[3]);
    script.push(b"ord");
    script.push(&[82]);
    script.push(&[24]);
    script.push(b"text/plain;charset=utf-8");
    script.push(&[83]);
    script.push(&[4]);
    script.push(b"woof");
    script.push(&[0]);
    script.push(&[4]);
    script.push(b"woof");
    assert_eq!(
      InscriptionParser::parse(vec![Script::from(script.concat())]),
      ParsedInscription::Partial
    );
  }

  #[test]
  fn extract_from_transaction() {
    let mut script: Vec<&[u8]> = Vec::new();
    script.push(&[3]);
    script.push(b"ord");
    script.push(&[81]);
    script.push(&[24]);
    script.push(b"text/plain;charset=utf-8");
    script.push(&[0]);
    script.push(&[4]);
    script.push(b"woof");

    let tx = Transaction {
      version: 0,
      lock_time: bitcoin::PackedLockTime(0),
      input: vec![TxIn {
        previous_output: OutPoint::null(),
        script_sig: Script::from(script.concat()),
        sequence: Sequence(0),
        witness: Witness::new(),
      }],
      output: Vec::new(),
    };

    assert_eq!(
      Inscription::from_transactions(vec![tx]),
      ParsedInscription::Complete(inscription("text/plain;charset=utf-8", "woof")),
    );
  }

  #[test]
  fn do_not_extract_from_second_input() {
    let mut script: Vec<&[u8]> = Vec::new();
    script.push(&[3]);
    script.push(b"ord");
    script.push(&[81]);
    script.push(&[24]);
    script.push(b"text/plain;charset=utf-8");
    script.push(&[0]);
    script.push(&[4]);
    script.push(b"woof");

    let tx = Transaction {
      version: 0,
      lock_time: bitcoin::PackedLockTime(0),
      input: vec![
        TxIn {
          previous_output: OutPoint::null(),
          script_sig: Script::new(),
          sequence: Sequence(0),
          witness: Witness::new(),
        },
        TxIn {
          previous_output: OutPoint::null(),
          script_sig: Script::from(script.concat()),
          sequence: Sequence(0),
          witness: Witness::new(),
        },
      ],
      output: Vec::new(),
    };

    assert_eq!(
      Inscription::from_transactions(vec![tx]),
      ParsedInscription::None
    );
  }

  /*
  #[test]
  fn reveal_script_chunks_data() {
    assert_eq!(
      inscription("foo", [])
        .append_reveal_script(script::Builder::new())
        .instructions()
        .count(),
      7
    );

    assert_eq!(
      inscription("foo", [0; 1])
        .append_reveal_script(script::Builder::new())
        .instructions()
        .count(),
      8
    );

    assert_eq!(
      inscription("foo", [0; 520])
        .append_reveal_script(script::Builder::new())
        .instructions()
        .count(),
      8
    );

    assert_eq!(
      inscription("foo", [0; 521])
        .append_reveal_script(script::Builder::new())
        .instructions()
        .count(),
      9
    );

    assert_eq!(
      inscription("foo", [0; 1040])
        .append_reveal_script(script::Builder::new())
        .instructions()
        .count(),
      9
    );

    assert_eq!(
      inscription("foo", [0; 1041])
        .append_reveal_script(script::Builder::new())
        .instructions()
        .count(),
      10
    );
  }
  */
}
