use {super::*, ord::subcommand::epochs::Output, ord::Sat};

#[test]
fn empty() {
  assert_eq!(
    CommandBuilder::new("epochs").output::<Output>(),
    Output {
      starting_sats: vec![
        Sat(0 * COIN_VALUE as u128),
        Sat(100000000000 * COIN_VALUE as u128),
        Sat(122500000000 * COIN_VALUE as u128),
        Sat(136250000000 * COIN_VALUE as u128),
        Sat(148750000000 * COIN_VALUE as u128),
        Sat(155000000000 * COIN_VALUE as u128),
        Sat(158125000000 * COIN_VALUE as u128),
        Sat(159687500000 * COIN_VALUE as u128),
      ]
    }
  );
}
