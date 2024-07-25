use super::*;

pub mod balances;
pub mod epochs;
pub mod find;
mod index;
pub mod info;
pub mod list;
pub mod parse;
mod preview;
pub mod dunes;
mod server;
pub mod subsidy;
pub mod traits;
pub mod wallet;

fn print_json(output: impl Serialize) -> Result {
  serde_json::to_writer_pretty(io::stdout(), &output)?;
  println!();
  Ok(())
}

#[derive(Debug, Parser)]
pub(crate) enum Subcommand {
  #[command(about = "List all dune balances")]
  Balances,
  #[command(about = "List the first satoshis of each reward epoch")]
  Epochs,
  #[command(about = "Find a satoshi's current location")]
  Find(find::Find),
  #[command(about = "Update the index")]
  Index,
  #[command(about = "Display index statistics")]
  Info(info::Info),
  #[command(about = "List the satoshis in an output")]
  List(list::List),
  #[command(about = "Parse a satoshi from ordinal notation")]
  Parse(parse::Parse),
  #[command(about = "Run an explorer server populated with inscriptions")]
  Preview(preview::Preview),
  #[command(about = "List all dunes")]
  Dunes,
  #[command(about = "Run the explorer server")]
  Server(server::Server),
  #[command(about = "Display information about a block's subsidy")]
  Subsidy(subsidy::Subsidy),
  #[command(about = "Display satoshi traits")]
  Traits(traits::Traits),
  #[command(subcommand, about = "Wallet commands")]
  Wallet(wallet::Wallet),
}

impl Subcommand {
  pub(crate) fn run(self, options: Options) -> SubcommandResult {
    match self {
      Self::Balances => balances::run(options),
      Self::Epochs => epochs::run(),
      Self::Find(find) => find.run(options),
      Self::Index => index::run(options),
      Self::Info(info) => info.run(options),
      Self::List(list) => list.run(options),
      Self::Parse(parse) => parse.run(),
      Self::Preview(preview) => preview.run(),
      Self::Dunes => dunes::run(options),
      Self::Server(server) => {
        let index = Arc::new(Index::open(&options)?);
        let handle = axum_server::Handle::new();
        LISTENERS.lock().unwrap().push(handle.clone());
        server.run(options, index, handle)
      }
      Self::Subsidy(subsidy) => subsidy.run(),
      Self::Traits(traits) => traits.run(),
      Self::Wallet(wallet) => wallet.run(options),
    }
  }
}

#[derive(Serialize, Deserialize)]
pub struct Empty {}

pub(crate) trait Output: Send {
  fn print_json(&self);
}

impl<T> Output for T
  where
      T: Serialize + Send,
{
  fn print_json(&self) {
    serde_json::to_writer_pretty(io::stdout(), self).ok();
    println!();
  }
}

pub(crate) type SubcommandResult = Result<Box<dyn Output>>;
