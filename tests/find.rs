use {super::*, ord::subcommand::find::Output};

#[test]
fn find_command_returns_satpoint_for_sat() {
  let rpc_server = test_bitcoincore_rpc::spawn();
  assert_eq!(
    CommandBuilder::new("--index-sats find 0")
      .rpc_server(&rpc_server)
      .output::<Output>(),
    Output {
      satpoint: "5b2a3f53f605d62c53e62932dac6925e3d74afa5a4b459745c36d42d0ed26a69:0:0"
        .parse()
        .unwrap()
    }
  );
}

#[test]
fn unmined_sat() {
  let rpc_server = test_bitcoincore_rpc::spawn();
  CommandBuilder::new("--index-sats find 10000000000000000")
    .rpc_server(&rpc_server)
    .expected_stderr("error: sat has not been mined as of index height\n")
    .expected_exit_code(1)
    .run();
}

#[test]
fn no_satoshi_index() {
  let rpc_server = test_bitcoincore_rpc::spawn();
  CommandBuilder::new("find 0")
    .rpc_server(&rpc_server)
    .expected_stderr("error: find requires index created with `--index-sats` flag\n")
    .expected_exit_code(1)
    .run();
}
