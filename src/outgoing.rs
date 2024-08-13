use crate::sat_point::SatPoint;
use super::*;

#[derive(Debug, PartialEq, Clone)]
pub(crate) enum Outgoing {
  Amount(Amount),
  InscriptionId(InscriptionId),
  SatPoint(SatPoint),
  Dune { decimal: Decimal, dune: SpacedDune },
}


impl FromStr for Outgoing {
  type Err = Error;

  fn from_str(s: &str) -> Result<Self, Self::Err> {
    lazy_static! {
      static ref SATPOINT: Regex = Regex::new(r"^[[:xdigit:]]{64}:\d+:\d+$").unwrap();
      static ref INSCRIPTION_ID: Regex = Regex::new(r"^[[:xdigit:]]{64}i\d+$").unwrap();
      static ref AMOUNT: Regex = Regex::new(
        r"(?x)
        ^
        (
          \d+
          |
          \.\d+
          |
          \d+\.\d+
        )
        \ *
        (bit|btc|cbtc|mbtc|msat|nbtc|pbtc|sat|satoshi|ubtc)
        (s)?
        $
        "
      )
      .unwrap();
      static ref DUNE: Regex = Regex::new(
        r"(?x)
        ^
        (
          \d+
          |
          \.\d+
          |
          \d+\.\d+
        )
        \ *
        (
          [A-Z•.]+
        )
        $
        "
      )
      .unwrap();
    }

    Ok(if SATPOINT.is_match(s) {
      Self::SatPoint(s.parse()?)
    } else if INSCRIPTION_ID.is_match(s) {
      Self::InscriptionId(s.parse()?)
    } else if AMOUNT.is_match(s) {
      Self::Amount(s.parse()?)
    } else if let Some(captures) = DUNE.captures(s) {
      Self::Dune {
        decimal: captures[1].parse()?,
        dune: captures[2].parse()?,
      }
    } else {
      bail!("unrecognized outgoing: {s}");
    })
  }
}


#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn parse() {
    assert_eq!(
      "0000000000000000000000000000000000000000000000000000000000000000i0"
        .parse::<Outgoing>()
        .unwrap(),
      Outgoing::InscriptionId(
        "0000000000000000000000000000000000000000000000000000000000000000i0"
          .parse()
          .unwrap()
      ),
    );

    assert_eq!(
      "0000000000000000000000000000000000000000000000000000000000000000:0:0"
        .parse::<Outgoing>()
        .unwrap(),
      Outgoing::SatPoint(
        "0000000000000000000000000000000000000000000000000000000000000000:0:0"
          .parse()
          .unwrap()
      ),
    );

    assert_eq!(
      "0 sat".parse::<Outgoing>().unwrap(),
      Outgoing::Amount("0 sat".parse().unwrap()),
    );

    assert_eq!(
      "0sat".parse::<Outgoing>().unwrap(),
      Outgoing::Amount("0 sat".parse().unwrap()),
    );

    assert!("0".parse::<Outgoing>().is_err());
  }
}
