use super::*;

#[derive(Copy, Clone, Eq, PartialEq, Debug, Display, Ord, PartialOrd, Deserialize, Serialize)]
#[serde(transparent)]
pub struct Sat(pub u128);

impl Sat {
  pub(crate) fn n(self) -> u128 {
    self.0
  }

  pub(crate) fn height(self) -> Height {
    self.epoch().starting_height()
      + u64::try_from(self.epoch_position() / self.epoch().subsidy() as u128).unwrap()
  }

  pub(crate) fn epoch(self) -> Epoch {
    self.into()
  }

  pub(crate) fn third(self) -> u64 {
    u64::try_from(self.epoch_position() % self.epoch().subsidy() as u128).unwrap()
  }

  pub(crate) fn epoch_position(self) -> u128 {
    self.0 - self.epoch().starting_sat().0
  }

  pub(crate) fn decimal(self) -> Decimal {
    self.into()
  }

  pub(crate) fn rarity(self) -> Rarity {
    self.into()
  }

  pub(crate) fn is_common(self) -> bool {
    let epoch = self.epoch();
    (self.0 - epoch.starting_sat().0) % epoch.subsidy() as u128 != 0
  }

  fn from_decimal(decimal: &str) -> Result<Self> {
    let (height, offset) = decimal
      .split_once('.')
      .ok_or_else(|| anyhow!("missing period"))?;
    let height = Height(height.parse()?);
    let offset = offset.parse::<u64>()?;

    if offset >= height.subsidy() {
      bail!("invalid block offset");
    }

    Ok(height.starting_sat() + offset as u128)
  }
}

impl PartialEq<u128> for Sat {
  fn eq(&self, other: &u128) -> bool {
    self.0 == *other
  }
}

impl PartialOrd<u128> for Sat {
  fn partial_cmp(&self, other: &u128) -> Option<cmp::Ordering> {
    self.0.partial_cmp(other)
  }
}

impl Add<u128> for Sat {
  type Output = Self;

  fn add(self, other: u128) -> Sat {
    Sat(self.0 + other)
  }
}

impl AddAssign<u128> for Sat {
  fn add_assign(&mut self, other: u128) {
    *self = Sat(self.0 + other);
  }
}

impl FromStr for Sat {
  type Err = Error;

  fn from_str(s: &str) -> Result<Self> {
    if s.contains('.') {
      Self::from_decimal(s)
    } else {
      let sat = Self(s.parse()?);
      Ok(sat)
    }
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn n() {
    assert_eq!(Sat(1).n(), 1);
    assert_eq!(Sat(100).n(), 100);
  }

  #[test]
  fn height() {
    assert_eq!(Sat(0).height(), 0);
    assert_eq!(Sat(1).height(), 0);
    assert_eq!(Sat(Epoch(0).subsidy() as u128).height(), 1);
    assert_eq!(Sat(Epoch(0).subsidy() as u128 * 2).height(), 2);
  }

  #[test]
  fn number() {
    assert_eq!(Sat(2099999997689999).n(), 2099999997689999);
  }

  #[test]
  fn epoch() {
    assert_eq!(Sat(0).epoch(), 0);
    assert_eq!(Sat(1).epoch(), 0);
  }

  #[test]
  fn epoch_position() {
    assert_eq!(Epoch(0).starting_sat().epoch_position(), 0);
    assert_eq!((Epoch(0).starting_sat() + 100).epoch_position(), 100);
    assert_eq!(Epoch(1).starting_sat().epoch_position(), 0);
    assert_eq!(Epoch(2).starting_sat().epoch_position(), 0);
  }

  #[test]
  fn subsidy_position() {
    assert_eq!(Sat(0).third(), 0);
    assert_eq!(Sat(1).third(), 1);
    assert_eq!(
      Sat(Height(0).subsidy() as u128 - 1).third(),
      Height(0).subsidy() - 1
    );
    assert_eq!(Sat(Height(0).subsidy() as u128).third(), 0);
    assert_eq!(Sat(Height(0).subsidy() as u128 + 1).third(), 1);
    assert_eq!(
      Sat(Epoch(1).starting_sat().n() + Epoch(1).subsidy() as u128).third(),
      0
    );
  }

  #[test]
  fn eq() {
    assert_eq!(Sat(0), 0);
    assert_eq!(Sat(1), 1);
  }

  #[test]
  fn partial_ord() {
    assert!(Sat(1) > 0);
    assert!(Sat(0) < 1);
  }

  #[test]
  fn add() {
    assert_eq!(Sat(0) + 1, 1);
    assert_eq!(Sat(1) + 100, 101);
  }

  #[test]
  fn add_assign() {
    let mut sat = Sat(0);
    sat += 1;
    assert_eq!(sat, 1);
    sat += 100;
    assert_eq!(sat, 101);
  }

  fn parse(s: &str) -> Result<Sat, String> {
    s.parse::<Sat>().map_err(|e| e.to_string())
  }

  #[test]
  fn from_str_decimal() {
    assert_eq!(parse("0.0").unwrap(), 0);
    assert_eq!(parse("0.1").unwrap(), 1);
    // assert_eq!(parse("1.0").unwrap(), 50 * COIN_VALUE as u128);
    // assert_eq!(parse("6929999.0").unwrap(), 2099999997689999);
    // assert!(parse("0.5000000000").is_err());
    // assert!(parse("6930000.0").is_err());
  }

  #[test]
  fn from_str_number() {
    assert_eq!(parse("0").unwrap(), 0);
    // assert_eq!(parse("2099999997689999").unwrap(), 2099999997689999);
    // assert!(parse("2099999997690000").is_err());
  }

  #[test]
  fn third() {
    assert_eq!(Sat(0).third(), 0);
    // assert_eq!(Sat(50 * COIN_VALUE as u128 - 1).third(), 4999999999);
    // assert_eq!(Sat(50 * COIN_VALUE as u128).third(), 0);
    // assert_eq!(Sat(50 * COIN_VALUE as u128 + 1).third(), 1);
  }

  #[test]
  fn is_common() {
    fn case(n: u128) {
      assert_eq!(Sat(n).is_common(), Sat(n).rarity() == Rarity::Common);
    }

    case(0);
    case(1);
    case(50 * COIN_VALUE as u128 - 1);
    case(50 * COIN_VALUE as u128);
    case(50 * COIN_VALUE as u128 + 1);
    case(2067187500000000 - 1);
    case(2067187500000000);
    case(2067187500000000 + 1);
  }
}
