use {super::*, ord::subcommand::traits::Output, ord::Rarity};

#[test]
fn traits_command_prints_sat_traits() {
  assert_eq!(
    CommandBuilder::new("traits 0").output::<Output>(),
    Output {
      number: 0,
      decimal: "0.0".into(),
      height: 0,
      epoch: 0,
      offset: 0,
      rarity: Rarity::Mythic,
    }
  );
}

#[test]
#[ignore]
fn traits_command_for_last_sat() {
  assert_eq!(
    CommandBuilder::new("traits 2099999997689999").output::<Output>(),
    Output {
      number: 2099999997689999,
      decimal: "6929999.0".into(),
      height: 6929999,
      epoch: 32,
      offset: 0,
      rarity: Rarity::Uncommon,
    }
  );
}
