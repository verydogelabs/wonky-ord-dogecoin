use super::*;

#[derive(Copy, Clone, Debug, Display, FromStr, Ord, Eq, PartialEq, PartialOrd)]
pub(crate) struct Height(pub(crate) u64);

impl Height {
  pub(crate) fn n(self) -> u64 {
    self.0
  }

  pub(crate) fn subsidy(self) -> u64 {
    Epoch::from(self).subsidy()
  }

  pub(crate) fn starting_sat(self) -> Sat {
    let epoch = Epoch::from(self);
    let epoch_starting_sat = epoch.starting_sat();
    let epoch_starting_height = epoch.starting_height();
    epoch_starting_sat
      + ((self - epoch_starting_height.n()).n() as u128) * (epoch.subsidy() as u128)
  }
}

impl Add<u64> for Height {
  type Output = Self;

  fn add(self, other: u64) -> Height {
    Self(self.0 + other)
  }
}

impl Sub<u64> for Height {
  type Output = Self;

  fn sub(self, other: u64) -> Height {
    Self(self.0 - other)
  }
}

impl PartialEq<u64> for Height {
  fn eq(&self, other: &u64) -> bool {
    self.0 == *other
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn n() {
    assert_eq!(Height(0).n(), 0);
    assert_eq!(Height(1).n(), 1);
  }

  #[test]
  fn add() {
    assert_eq!(Height(0) + 1, 1);
    assert_eq!(Height(1) + 100, 101);
  }

  #[test]
  fn sub() {
    assert_eq!(Height(1) - 1, 0);
    assert_eq!(Height(100) - 50, 50);
  }

  #[test]
  fn eq() {
    assert_eq!(Height(0), 0);
    assert_eq!(Height(100), 100);
  }

  #[test]
  fn from_str() {
    assert_eq!("0".parse::<Height>().unwrap(), 0);
    assert!("foo".parse::<Height>().is_err());
  }

  #[test]
  fn subsidy() {
    assert_eq!(Height(0).subsidy(), 1_000_000 * COIN_VALUE);
    assert_eq!(Height(1).subsidy(), 1_000_000 * COIN_VALUE);
    assert_eq!(Height(100_000).subsidy(), 500_000 * COIN_VALUE);
    assert_eq!(Height(199_999).subsidy(), 250_000 * COIN_VALUE);
    assert_eq!(Height(201_000).subsidy(), 125_000 * COIN_VALUE);
  }
}
