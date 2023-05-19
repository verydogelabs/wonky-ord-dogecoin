use super::*;
use serde_json;
use std::collections::HashMap;
use std::fs::File;
use std::io::BufReader;
use std::path::Path;
use lazy_static::lazy_static;
use serde::{Deserialize, Serialize};
use once_cell::sync::Lazy;

#[derive(Copy, Clone, Eq, PartialEq, Debug, Display, PartialOrd)]
pub(crate) struct Epoch(pub(crate) u64);

fn read_sat_from_file<P: AsRef<Path>>(path: P) -> Result<Vec<Sat>, Box<dyn std::error::Error>> {
    let file = File::open(path)?;
    let reader = BufReader::new(file);
    let sats: Vec<u128> = serde_json::from_reader(reader)?;

    Ok(sats.into_iter().map(Sat).collect())
}

lazy_static! {
    pub(crate) static ref STARTING_SATS: Vec<Sat> = {
        let path = env::var("STARTING_SATS_PATH").expect("STARTING_SATS_PATH must be set");
        read_sat_from_file(&path).expect("Failed to read JSON")
    };
}

#[derive(Debug, Serialize, Deserialize)]
struct Epochs {
    epochs: HashMap<u64, u64>,
}

static EPOCHS: Lazy<Epochs> = Lazy::new(|| {
    let path = env::var("SUBSIDIES_PATH").expect("SUBSIDIES_PATH must be set");
    let data = fs::read_to_string(&path).expect("Unable to read file");
    serde_json::from_str(&data).expect("Unable to parse JSON")
});

impl Epoch {
  pub fn get_starting_sats() -> &'static Vec<Sat> {
    &STARTING_SATS
  }

  pub(crate) fn subsidy(self) -> u64 {
      match EPOCHS.epochs.get(&self.0) {
          Some(&value) => value,
          None => panic!("bad epoch"),
      }
  }

  pub(crate) fn starting_sat(self) -> Sat {
    *Self::get_starting_sats()
      .get(usize::try_from(self.0).unwrap())
      .unwrap_or_else(|| Self::get_starting_sats().last().unwrap())
  }

  pub(crate) fn starting_height(self) -> Height {
    if self.0 < 145_000 {
      Height(self.0)
    } else if self.0 < 145_001 {
      Height(145_000)
    } else if self.0 < 145_002 {
      Height(200_000)
    } else if self.0 < 145_003 {
      Height(300_000)
    } else if self.0 < 145_004 {
      Height(400_000)
    } else if self.0 < 145_005 {
      Height(500_000)
    } else if self.0 < 145_006 {
      Height(600_000)
    } else {
      panic!("bad epoch")
    }
  }
}

impl PartialEq<u64> for Epoch {
  fn eq(&self, other: &u64) -> bool {
    self.0 == *other
  }
}

impl From<Sat> for Epoch {
  fn from(sat: Sat) -> Self {
    let starting_sats = Self::get_starting_sats();

    let len = starting_sats.len();
    for i in 0..len-1 {
      if sat < starting_sats[i+1] {
        return Epoch(i as u64);
      }
    }

    // If none of the starting sats is greater than the given sat, return the last Epoch
    Epoch(145_005)
  }
}

impl From<Height> for Epoch {
  fn from(height: Height) -> Self {
    if height.0 < 145_000 {
      Epoch(height.0)
    } else if height.0 < 200_000 {
      Epoch(145_000)
    } else if height.0 < 300_000 {
      Epoch(145_001)
    } else if height.0 < 400_000 {
      Epoch(145_002)
    } else if height.0 < 500_000 {
      Epoch(145_003)
    } else if height.0 < 600_000 {
      Epoch(145_004)
    } else {
      Epoch(145_005)
    }
  }
}

#[cfg(test)]
mod tests {
  use super::super::*;

  #[test]
  fn starting_sat() {
    assert_eq!(Epoch(0).starting_sat(), 0);
  }

  #[test]
  fn subsidy() {
    assert_eq!(Epoch(0).subsidy(), 1_000_000 * COIN_VALUE);
    assert_eq!(Epoch(1).subsidy(), 500_000 * COIN_VALUE);
    // assert_eq!(Epoch(32).subsidy(), 1);
    // assert_eq!(Epoch(33).subsidy(), 0);
  }

  #[test]
  fn starting_height() {
    assert_eq!(Epoch(0).starting_height(), 0);
    assert_eq!(Epoch(1).starting_height(), 100_000);
    assert_eq!(Epoch(2).starting_height(), 145_000);
  }

  #[test]
  fn from_height() {
    assert_eq!(Epoch::from(Height(0)), 0);
    assert_eq!(Epoch::from(Height(100_000)), 1);
    assert_eq!(Epoch::from(Height(150_000)), 2);
    assert_eq!(Epoch::from(Height(200_000)), 3);
  }

  #[test]
  fn from_sat() {
    for (epoch, starting_sat) in Epoch::get_starting_sats().into_iter().enumerate() {
      if epoch > 0 {
        assert_eq!(
          Epoch::from(Sat(starting_sat.n() - 1)),
          Epoch(epoch as u64 - 1)
        );
      }
      assert_eq!(Epoch::from(starting_sat), Epoch(epoch as u64));
      assert_eq!(Epoch::from(starting_sat + 1), Epoch(epoch as u64));
    }
    assert_eq!(Epoch::from(Sat(0)), 0);
    assert_eq!(Epoch::from(Sat(1)), 0);
    assert_eq!(Epoch::from(Epoch(1).starting_sat()), 1);
    assert_eq!(Epoch::from(Epoch(1).starting_sat() + 1), 1);
    // assert_eq!(Epoch::from(Sat(u128::max_value())), 33);
  }

  #[test]
  fn eq() {
    assert_eq!(Epoch(0), 0);
    assert_eq!(Epoch(100), 100);
  }
}


