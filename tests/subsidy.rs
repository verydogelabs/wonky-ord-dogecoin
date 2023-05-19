use {super::*, ord::subcommand::subsidy::Output};

#[test]
fn genesis() {
  assert_eq!(
    CommandBuilder::new("subsidy 0").output::<Output>(),
    Output {
      first: 0,
      subsidy: 100000000000000,
    }
  );
}

#[test]
fn second_block() {
  assert_eq!(
    CommandBuilder::new("subsidy 1").output::<Output>(),
    Output {
      first: 100000000000000,
      subsidy: 100000000000000,
    }
  );
}

#[test]
#[ignore]
fn second_to_last_block_with_subsidy() {
  assert_eq!(
    CommandBuilder::new("subsidy 6929998").output::<Output>(),
    Output {
      first: 2099999997689998,
      subsidy: 1,
    }
  );
}

#[test]
#[ignore]
fn last_block_with_subsidy() {
  assert_eq!(
    CommandBuilder::new("subsidy 6929999").output::<Output>(),
    Output {
      first: 2099999997689999,
      subsidy: 1,
    }
  );
}

#[test]
#[ignore]
fn first_block_without_subsidy() {
  CommandBuilder::new("subsidy 6930000")
    .expected_stderr("error: block 6930000 has no subsidy\n")
    .expected_exit_code(1)
    .run();
}
