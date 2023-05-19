use super::*;

#[derive(Deserialize, Serialize)]
pub struct Output {
  pub address: Address,
}

pub(crate) fn run(options: Options) -> Result {
  let address = options
    .dogecoin_rpc_client_for_wallet_command(false)?
    .get_new_address(None, Some(bitcoincore_rpc::json::AddressType::Bech32m))?;

  print_json(Output { address })?;

  Ok(())
}
