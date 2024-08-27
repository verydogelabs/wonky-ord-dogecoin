use linked_hash_map::LinkedHashMap;
use rayon::prelude::{IntoParallelRefIterator, ParallelIterator};

use {
  self::{
    deserialize_from_str::DeserializeFromStr,
    error::{OptionExt, ServerError, ServerResult},
  },
  super::*,
  crate::{
    drc20::{script_key::ScriptKey, Tick},
    page_config::PageConfig,
    templates::{
      AddressOutputJson, BlockHtml, BlockJson, DuneAddressJson, DuneBalance, DuneBalancesHtml,
      DuneEntryJson, DuneHtml, DuneJson, DuneOutput, DuneOutputJson, DunesHtml, HomeHtml,
      InputHtml, InscriptionByAddressJson, InscriptionHtml, InscriptionJson, InscriptionsHtml,
      OutputHtml, OutputJson, PageContent, PageHtml, PreviewAudioHtml, PreviewImageHtml,
      PreviewModelHtml, PreviewPdfHtml, PreviewTextHtml, PreviewUnknownHtml, PreviewVideoHtml,
      RangeHtml, RareTxt, SatHtml, ShibescriptionJson, TransactionHtml, Utxo, DRC20,
    },
  },
  axum::{
    body,
    extract::{Extension, Json, Path, Query},
    headers::UserAgent,
    http::{header, HeaderMap, HeaderValue, StatusCode, Uri},
    response::{IntoResponse, Redirect, Response},
    routing::get,
    Router, TypedHeader,
  },
  axum_server::Handle,
  rust_embed::RustEmbed,
  rustls_acme::{
    acme::{LETS_ENCRYPT_PRODUCTION_DIRECTORY, LETS_ENCRYPT_STAGING_DIRECTORY},
    axum::AxumAcceptor,
    caches::DirCache,
    AcmeConfig,
  },
  serde_json::to_string,
  std::collections::HashMap,
  std::{cmp::Ordering, str},
  tokio_stream::StreamExt,
  tower_http::{
    compression::CompressionLayer,
    cors::{Any, CorsLayer},
    set_header::SetResponseHeaderLayer,
  },
};

mod error;
mod query;

// Helper function to get transaction details
fn get_transaction_details(
  input: &TxIn,
  index: &Arc<Index>,
  page_config: &Arc<PageConfig>,
) -> (String, String) {
  let txid = input.previous_output.txid;
  let result = if txid
    == Txid::from_str("0000000000000000000000000000000000000000000000000000000000000000").unwrap()
  {
    (String::new(), String::new())
  } else {
    index
      .get_transaction(txid)
      .map(|transaction| {
        transaction
          .map(|t| {
            let value = t
              .output
              .clone()
              .into_iter()
              .nth(input.previous_output.vout as usize)
              .map(|output| output.value.to_string())
              .unwrap_or_else(|| "0".to_string());

            let script_pubkey = t
              .output
              .into_iter()
              .nth(input.previous_output.vout as usize)
              .map(|output| output.script_pubkey)
              .unwrap_or_else(|| Script::new());

            let address = page_config
              .chain
              .address_from_script(&script_pubkey)
              .map(|address| address.to_string())
              .unwrap_or(String::new());

            (value, address)
          })
          .unwrap_or((String::new(), String::new()))
      })
      .unwrap_or((String::new(), String::new()))
  };

  result
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
pub(crate) struct InscriptionAddressJson {
  pub(crate) inscriptions: Vec<InscriptionByAddressJson>,
  pub(crate) total_inscriptions: usize,
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
pub(crate) struct UtxoAddressJson {
  pub(crate) utxos: Vec<Utxo>,
  pub(crate) total_utxos: usize,
  pub(crate) total_shibes: u128,
  pub(crate) total_inscription_shibes: u128,
}

#[derive(Deserialize)]
struct UtxoBalanceQuery {
  limit: Option<usize>,
  show_all: Option<bool>,
  value_filter: Option<u64>,
}

#[derive(Deserialize)]
struct OutputsQuery {
  outputs: String,
}

#[derive(Deserialize)]
struct JsonQuery {
  json: Option<bool>,
}

enum BlockQuery {
  Height(u32),
  Hash(BlockHash),
}

impl FromStr for BlockQuery {
  type Err = Error;

  fn from_str(s: &str) -> Result<Self, Self::Err> {
    Ok(if s.len() == 64 {
      BlockQuery::Hash(s.parse()?)
    } else {
      BlockQuery::Height(s.parse()?)
    })
  }
}

enum SpawnConfig {
  Https(AxumAcceptor),
  Http,
  Redirect(String),
}

#[derive(Deserialize)]
struct InscriptionsByOutputsQuery {
  outputs: String,
}

#[derive(Deserialize)]
struct BlocksQuery {
  no_inscriptions: Option<bool>,
  no_input_data: Option<bool>,
}

#[derive(Deserialize)]
struct DunesBalanceQuery {
  show_all: Option<bool>,
  list_dunes: Option<bool>,
  filter: Option<SpacedDune>,
}

#[derive(Deserialize)]
struct Search {
  query: String,
}

#[derive(RustEmbed)]
#[folder = "static"]
struct StaticAssets;

struct StaticHtml {
  title: &'static str,
  html: &'static str,
}

impl PageContent for StaticHtml {
  fn title(&self) -> String {
    self.title.into()
  }
}

impl Display for StaticHtml {
  fn fmt(&self, f: &mut Formatter) -> fmt::Result {
    f.write_str(self.html)
  }
}

#[derive(Debug, Parser)]
pub(crate) struct Server {
  #[clap(
    long,
    default_value = "0.0.0.0",
    help = "Listen on <ADDRESS> for incoming requests."
  )]
  address: String,
  #[clap(
    long,
    help = "Request ACME TLS certificate for <ACME_DOMAIN>. This ord instance must be reachable at <ACME_DOMAIN>:443 to respond to Let's Encrypt ACME challenges."
  )]
  acme_domain: Vec<String>,
  #[clap(
    long,
    help = "Listen on <HTTP_PORT> for incoming HTTP requests. [default: 80]."
  )]
  http_port: Option<u16>,
  #[clap(
    long,
    group = "port",
    help = "Listen on <HTTPS_PORT> for incoming HTTPS requests. [default: 443]."
  )]
  https_port: Option<u16>,
  #[clap(long, help = "Store ACME TLS certificates in <ACME_CACHE>.")]
  acme_cache: Option<PathBuf>,
  #[clap(long, help = "Provide ACME contact <ACME_CONTACT>.")]
  acme_contact: Vec<String>,
  #[clap(long, help = "Serve HTTP traffic on <HTTP_PORT>.")]
  http: bool,
  #[clap(long, help = "Serve HTTPS traffic on <HTTPS_PORT>.")]
  https: bool,
  #[clap(long, help = "Redirect HTTP traffic to HTTPS.")]
  redirect_http_to_https: bool,
}

impl Server {
  pub(crate) fn run(self, options: Options, index: Arc<Index>, handle: Handle) -> SubcommandResult {
    Runtime::new()?.block_on(async {
      let index_clone = index.clone();

      let index_thread = thread::spawn(move || loop {
        if SHUTTING_DOWN.load(atomic::Ordering::Relaxed) {
          break;
        }
        if let Err(error) = index_clone.update() {
          log::warn!("{error}");
        }
        thread::sleep(Duration::from_millis(5000));
      });
      INDEXER.lock().unwrap().replace(index_thread);

      let config = options.load_config()?;
      let acme_domains = self.acme_domains()?;

      let page_config = Arc::new(PageConfig {
        chain: options.chain(),
        domain: acme_domains.first().cloned(),
        index_sats: index.has_sat_index(),
        csp_origin: options.csp_origin(),
      });

      let router = Router::new()
        .route("/", get(Self::home))
        .route("/block-count", get(Self::block_count))
        .route("/block/:query", get(Self::block))
        .route("/blocks/:query/:endquery", get(Self::blocks))
        .route("/bounties", get(Self::bounties))
        .route("/content/:inscription_id", get(Self::content))
        .route("/faq", get(Self::faq))
        .route("/favicon.ico", get(Self::favicon))
        .route("/feed.xml", get(Self::feed))
        .route("/input/:block/:transaction/:input", get(Self::input))
        .route("/inscription/:inscription_id", get(Self::inscription))
        .route("/inscriptions", get(Self::inscriptions))
        .route("/inscriptions/:from", get(Self::inscriptions_from))
        .route("/shibescription/:inscription_id", get(Self::inscription))
        .route("/shibescriptions", get(Self::inscriptions))
        .route("/shibescriptions/:from", get(Self::inscriptions_from))
        .route(
          "/shibescriptions_on_outputs",
          get(Self::inscriptions_by_outputs),
        )
        .route(
          "/shibescriptions_by_outputs",
          get(Self::shibescriptions_by_outputs),
        )
        .route("/install.sh", get(Self::install_script))
        .route("/ordinal/:sat", get(Self::ordinal))
        .route("/output/:output", get(Self::output))
        .route("/outputs/:output_list", get(Self::outputs))
        .route("/address/:address", get(Self::outputs_by_address))
        .route("/preview/:inscription_id", get(Self::preview))
        .route("/range/:start/:end", get(Self::range))
        .route("/rare.txt", get(Self::rare_txt))
        .route("/dune/:dune", get(Self::dune))
        .route("/dunes", get(Self::dunes))
        .route("/dunes/balances", get(Self::dunes_balances))
        .route(
          "/dunes/balance/:address",
          get(Self::dunes_by_address_unpaginated),
        )
        .route("/dunes/balance/:address/:page", get(Self::dunes_by_address))
        .route(
          "/utxos/balance/:address",
          get(Self::utxos_by_address_unpaginated),
        )
        .route("/utxos/balance/:address/:page", get(Self::utxos_by_address))
        .route(
          "/inscriptions/balance/:address",
          get(Self::inscriptions_by_address_unpaginated),
        )
        .route(
          "/inscriptions/balance/:address/:page",
          get(Self::inscriptions_by_address),
        )
        .route("/drc20/tick/:tick", get(Self::drc20_tick_info))
        .route("/drc20/tick", get(Self::drc20_all_tick_info))
        .route(
          "/drc20/tick/:tick/address/:address/balance",
          get(Self::drc20_balance),
        )
        .route(
          "/drc20/address/:address/balance",
          get(Self::drc20_all_balance),
        )
        .route("/dunes_on_outputs", get(Self::dunes_by_outputs))
        .route("/sat/:sat", get(Self::sat))
        .route("/search", get(Self::search_by_query))
        .route("/search/*query", get(Self::search_by_path))
        .route("/static/*path", get(Self::static_asset))
        .route("/status", get(Self::status))
        .route("/tx/:txid", get(Self::transaction))
        .layer(Extension(index))
        .layer(Extension(page_config))
        .layer(Extension(Arc::new(config)))
        .layer(SetResponseHeaderLayer::if_not_present(
          header::CONTENT_SECURITY_POLICY,
          HeaderValue::from_static("default-src 'self'"),
        ))
        .layer(SetResponseHeaderLayer::overriding(
          header::STRICT_TRANSPORT_SECURITY,
          HeaderValue::from_static("max-age=31536000; includeSubDomains; preload"),
        ))
        .layer(
          CorsLayer::new()
            .allow_methods([http::Method::GET])
            .allow_origin(Any),
        )
        .layer(CompressionLayer::new());

      match (self.http_port(), self.https_port()) {
        (Some(http_port), None) => {
          self
            .spawn(router, handle, http_port, SpawnConfig::Http)?
            .await??
        }
        (None, Some(https_port)) => {
          self
            .spawn(
              router,
              handle,
              https_port,
              SpawnConfig::Https(self.acceptor(&options)?),
            )?
            .await??
        }
        (Some(http_port), Some(https_port)) => {
          let http_spawn_config = if self.redirect_http_to_https {
            SpawnConfig::Redirect(if https_port == 443 {
              format!("https://{}", acme_domains[0])
            } else {
              format!("https://{}:{https_port}", acme_domains[0])
            })
          } else {
            SpawnConfig::Http
          };

          let (http_result, https_result) = tokio::join!(
            self.spawn(router.clone(), handle.clone(), http_port, http_spawn_config)?,
            self.spawn(
              router,
              handle,
              https_port,
              SpawnConfig::Https(self.acceptor(&options)?),
            )?
          );
          http_result.and(https_result)??;
        }
        (None, None) => unreachable!(),
      }

      Ok(Box::new(Empty {}) as Box<dyn Output>)
    })
  }

  fn spawn(
    &self,
    router: Router,
    handle: Handle,
    port: u16,
    config: SpawnConfig,
  ) -> Result<task::JoinHandle<io::Result<()>>> {
    let addr = (self.address.as_str(), port)
      .to_socket_addrs()?
      .next()
      .ok_or_else(|| anyhow!("failed to get socket addrs"))?;

    if !integration_test() {
      eprintln!(
        "Listening on {}://{addr}",
        match config {
          SpawnConfig::Https(_) => "https",
          _ => "http",
        }
      );
    }

    Ok(tokio::spawn(async move {
      match config {
        SpawnConfig::Https(acceptor) => {
          axum_server::Server::bind(addr)
            .handle(handle)
            .acceptor(acceptor)
            .serve(router.into_make_service())
            .await
        }
        SpawnConfig::Redirect(destination) => {
          axum_server::Server::bind(addr)
            .handle(handle)
            .serve(
              Router::new()
                .fallback(Self::redirect_http_to_https)
                .layer(Extension(destination))
                .into_make_service(),
            )
            .await
        }
        SpawnConfig::Http => {
          axum_server::Server::bind(addr)
            .handle(handle)
            .serve(router.into_make_service())
            .await
        }
      }
    }))
  }

  fn acme_cache(acme_cache: Option<&PathBuf>, options: &Options) -> Result<PathBuf> {
    let acme_cache = if let Some(acme_cache) = acme_cache {
      acme_cache.clone()
    } else {
      options.data_dir()?.join("acme-cache")
    };

    Ok(acme_cache)
  }

  fn acme_domains(&self) -> Result<Vec<String>> {
    if !self.acme_domain.is_empty() {
      Ok(self.acme_domain.clone())
    } else {
      Ok(vec![System::host_name().expect("Host name not found")])
    }
  }

  fn http_port(&self) -> Option<u16> {
    if self.http || self.http_port.is_some() || (self.https_port.is_none() && !self.https) {
      Some(self.http_port.unwrap_or(80))
    } else {
      None
    }
  }

  fn https_port(&self) -> Option<u16> {
    if self.https || self.https_port.is_some() {
      Some(self.https_port.unwrap_or(443))
    } else {
      None
    }
  }

  fn acceptor(&self, options: &Options) -> Result<AxumAcceptor> {
    let config = AcmeConfig::new(self.acme_domains()?)
      .contact(&self.acme_contact)
      .cache_option(Some(DirCache::new(Self::acme_cache(
        self.acme_cache.as_ref(),
        options,
      )?)))
      .directory(if cfg!(test) {
        LETS_ENCRYPT_STAGING_DIRECTORY
      } else {
        LETS_ENCRYPT_PRODUCTION_DIRECTORY
      });

    let mut state = config.state();

    let acceptor = state.axum_acceptor(Arc::new(
      rustls::ServerConfig::builder()
        .with_safe_defaults()
        .with_no_client_auth()
        .with_cert_resolver(state.resolver()),
    ));

    tokio::spawn(async move {
      while let Some(result) = state.next().await {
        match result {
          Ok(ok) => log::info!("ACME event: {:?}", ok),
          Err(err) => log::error!("ACME error: {:?}", err),
        }
      }
    });

    Ok(acceptor)
  }

  fn index_height(index: &Index) -> ServerResult<Height> {
    index.height()?.ok_or_not_found(|| "genesis block")
  }

  async fn sat(
    Extension(page_config): Extension<Arc<PageConfig>>,
    Extension(index): Extension<Arc<Index>>,
    Path(DeserializeFromStr(sat)): Path<DeserializeFromStr<Sat>>,
  ) -> ServerResult<PageHtml<SatHtml>> {
    let satpoint = index.rare_sat_satpoint(sat)?;

    Ok(
      SatHtml {
        sat,
        satpoint,
        blocktime: index.blocktime(sat.height())?,
        inscription: index.get_inscription_id_by_sat(sat)?,
      }
      .page(page_config),
    )
  }

  async fn ordinal(Path(sat): Path<String>) -> Redirect {
    Redirect::to(&format!("/sat/{sat}"))
  }

  async fn output(
    Extension(page_config): Extension<Arc<PageConfig>>,
    Extension(index): Extension<Arc<Index>>,
    Path(outpoint): Path<OutPoint>,
  ) -> ServerResult<PageHtml<OutputHtml>> {
    let list = index.list(outpoint)?;

    let output = if outpoint == OutPoint::null() {
      let mut value = 0;

      if let Some(List::Unspent(ranges)) = &list {
        for (start, end) in ranges {
          value += u64::try_from(end - start).unwrap();
        }
      }

      TxOut {
        value,
        script_pubkey: Script::new(),
      }
    } else {
      index
        .get_transaction(outpoint.txid)?
        .ok_or_not_found(|| format!("output {outpoint}"))?
        .output
        .into_iter()
        .nth(outpoint.vout as usize)
        .ok_or_not_found(|| format!("output {outpoint}"))?
    };

    let inscriptions = index.get_inscriptions_on_output(outpoint)?;

    let dunes = index.get_dune_balances_for_outpoint(outpoint)?;

    Ok(
      OutputHtml {
        outpoint,
        inscriptions,
        list,
        chain: page_config.chain,
        output,
        dunes,
      }
      .page(page_config),
    )
  }

  async fn utxos_by_address(
    Extension(index): Extension<Arc<Index>>,
    Path(params): Path<(String, u32)>,
    Query(query): Query<UtxoBalanceQuery>,
  ) -> ServerResult<Response> {
    Self::get_utxos_by_address(index, params.0, Some(params.1), query).await
  }

  async fn utxos_by_address_unpaginated(
    Extension(index): Extension<Arc<Index>>,
    Path(params): Path<String>,
    Query(query): Query<UtxoBalanceQuery>,
  ) -> ServerResult<Response> {
    Self::get_utxos_by_address(index, params, None, query).await
  }

  async fn get_utxos_by_address(
    index: Arc<Index>,
    address: String,
    page: Option<u32>,
    query: UtxoBalanceQuery,
  ) -> ServerResult<Response> {
    let (address, page) = (address, page.unwrap_or(0));
    let show_all = query.show_all.unwrap_or(false);
    let value_filter = query.value_filter.unwrap_or(0);

    let items_per_page = query.limit.unwrap_or(10);
    let page = page as usize;
    let start_index = if page == 0 || page == 1 {
      0
    } else {
      (page - 1) * items_per_page + 1
    };
    let mut element_counter = 0;

    let outpoints: Vec<OutPoint> = index.get_account_outputs(address.clone())?;

    let mut utxos = Vec::new();
    let mut total_shibes = 0u128;
    let mut inscription_shibes = 0u128;

    for outpoint in outpoints {
      if !index.get_dune_balances_for_outpoint(outpoint)?.is_empty() {
        continue;
      }
      if !show_all
        && (element_counter < start_index || element_counter > start_index + items_per_page - 1)
      {
        continue;
      }

      let txid = outpoint.txid;
      let vout = outpoint.vout;
      let output = index
        .get_transaction(txid)?
        .ok_or_not_found(|| format!("{txid} current transaction"))?
        .output
        .into_iter()
        .nth(vout.try_into().unwrap())
        .ok_or_not_found(|| format!("{vout} current transaction output"))?;

      if value_filter > 0 && output.value <= value_filter {
        continue;
      }

      if !index.get_inscriptions_on_output(outpoint)?.is_empty() {
        inscription_shibes += output.value as u128;
        continue;
      }

      element_counter += 1;

      total_shibes += output.value as u128;

      let confirmations = if let Some(block_hash_info) = index.get_transaction_blockhash(txid)? {
        block_hash_info.confirmations
      } else {
        None
      };

      utxos.push(Utxo {
        txid,
        vout,
        script: output.script_pubkey,
        shibes: output.value,
        confirmations,
      });
    }
    Ok(
      Json(UtxoAddressJson {
        utxos,
        total_shibes,
        total_utxos: element_counter,
        total_inscription_shibes: inscription_shibes,
      })
      .into_response(),
    )
  }

  async fn inscriptions_by_address(
    Extension(index): Extension<Arc<Index>>,
    Path(params): Path<(String, u32)>,
    Query(query): Query<UtxoBalanceQuery>,
  ) -> ServerResult<Response> {
    Self::get_inscriptions_by_address(index, params.0, Some(params.1), query).await
  }

  async fn inscriptions_by_address_unpaginated(
    Extension(index): Extension<Arc<Index>>,
    Path(params): Path<String>,
    Query(query): Query<UtxoBalanceQuery>,
  ) -> ServerResult<Response> {
    Self::get_inscriptions_by_address(index, params, None, query).await
  }

  async fn get_inscriptions_by_address(
    index: Arc<Index>,
    address: String,
    page: Option<u32>,
    query: UtxoBalanceQuery,
  ) -> ServerResult<Response> {
    let (address, page) = (address, page.unwrap_or(0));
    let show_all = query.show_all.unwrap_or(false);
    let value_filter = query.value_filter.unwrap_or(0);

    let items_per_page = query.limit.unwrap_or(10);
    let page = page as usize;
    let start_index = if page == 0 || page == 1 {
      0
    } else {
      (page - 1) * items_per_page + 1
    };
    let mut element_counter = 0;

    let mut all_inscriptions_json = Vec::new();
    let outpoints: Vec<OutPoint> = index.get_account_outputs(address)?;

    for outpoint in outpoints {
      let inscriptions = index.get_inscriptions_on_output(outpoint)?;

      if inscriptions.is_empty() {
        continue;
      }

      element_counter += 1;
      if !show_all
        && (element_counter < start_index || element_counter > start_index + items_per_page - 1)
      {
        continue;
      }

      let txid = outpoint.txid;
      let vout = outpoint.vout;

      let output = index
        .get_transaction(txid)?
        .ok_or_not_found(|| format!("dunes {txid} current transaction"))?
        .output
        .into_iter()
        .nth(vout.try_into().unwrap())
        .ok_or_not_found(|| format!("dunes {vout} current transaction output"))?;
      let shibes = output.value;
      let script = output.script_pubkey;

      if value_filter > 0 && shibes <= value_filter {
        element_counter -= 1;
        continue;
      }

      for inscription_id in inscriptions {
        let inscription = index
          .get_inscription_by_id(inscription_id)?
          .ok_or_not_found(|| format!("inscription {inscription_id}"))?;

        let entry = index
          .get_inscription_entry(inscription_id)?
          .ok_or_not_found(|| format!("inscription {inscription_id}"))?;

        let satpoint = index
          .get_inscription_satpoint_by_id(inscription_id)?
          .ok_or_not_found(|| format!("inscription {inscription_id}"))?;

        let content_type = inscription.content_type().map(|s| s.to_string());
        let content_length = inscription.content_length();
        let content = inscription.into_body();

        let str_content = match (content_type.clone(), content) {
          (Some(ref ct), Some(c))
            if ct.starts_with("application/json") || ct.starts_with("text") =>
          {
            Some(String::from_utf8_lossy(c.as_slice()).to_string())
          }
          (None, Some(c)) => Some(String::from_utf8_lossy(c.as_slice()).to_string()),
          _ => None,
        };

        if let Some(content) = str_content.clone() {
          let drc20 = DRC20::from_json_string(content.as_str());
          if drc20.is_some() {
            element_counter -= 1;
            continue;
          }
        };

        let confirmations = if let Some(block_hash_info) = index.get_transaction_blockhash(txid)? {
          block_hash_info.confirmations
        } else {
          None
        };

        let inscription_json = InscriptionByAddressJson {
          utxo: Utxo {
            txid,
            vout,
            script: script.clone(),
            shibes,
            confirmations,
          },
          content: str_content,
          content_length,
          content_type,
          genesis_height: entry.height,
          inscription_id,
          inscription_number: entry.inscription_number,
          timestamp: entry.timestamp,
          offset: satpoint.offset,
        };

        all_inscriptions_json.push(inscription_json);
      }
    }
    Ok(
      Json(InscriptionAddressJson {
        inscriptions: all_inscriptions_json,
        total_inscriptions: element_counter,
      })
      .into_response(),
    )
  }

  async fn dunes_by_address(
    Extension(index): Extension<Arc<Index>>,
    Path(params): Path<(String, u32)>,
    Query(query): Query<DunesBalanceQuery>,
  ) -> ServerResult<Response> {
    Self::get_dunes_by_address(index, params.0, Some(params.1), query).await
  }

  async fn dunes_by_address_unpaginated(
    Extension(index): Extension<Arc<Index>>,
    Path(params): Path<String>,
    Query(query): Query<DunesBalanceQuery>,
  ) -> ServerResult<Response> {
    Self::get_dunes_by_address(index, params, None, query).await
  }

  async fn get_dunes_by_address(
    index: Arc<Index>,
    address: String,
    page: Option<u32>,
    query: DunesBalanceQuery,
  ) -> ServerResult<Response> {
    let (address, page) = (address, page.unwrap_or(0));
    let show_all = query.show_all.unwrap_or(false);
    let list_dunes = query.list_dunes.unwrap_or(false);

    let outpoints = index.get_account_outputs(address)?;

    let items_per_page = 10usize;
    let page = page as usize;
    let mut start_index = if page == 0 {
      0
    } else {
      (page - 1) * items_per_page
    };
    let mut elements_counter = 0;

    let mut dune_balances_map: LinkedHashMap<SpacedDune, DuneBalance> = LinkedHashMap::new();

    for outpoint in outpoints {
      let dunes = index.get_dune_balances_for_outpoint(outpoint)?;
      for (dune, balances) in dunes {
        if let Some(filter) = query.filter {
          if dune != filter {
            continue;
          }
        }
        let dune_balance = dune_balances_map
          .entry(dune.clone())
          .or_insert_with(|| DuneBalance {
            dune: dune.clone(),
            divisibility: balances.divisibility,
            symbol: balances.symbol,
            total_balance: 0,
            total_outputs: 0,
            balances: Vec::new(),
          });

        if !list_dunes {
          let txid = outpoint.txid;
          let vout = outpoint.vout;
          let output = index
            .get_transaction(txid)?
            .ok_or_not_found(|| format!("dunes {txid} current transaction"))?
            .output
            .into_iter()
            .nth(vout.try_into().unwrap())
            .ok_or_not_found(|| format!("dunes {vout} current transaction output"))?;

          dune_balance.balances.push(DuneOutput {
            txid,
            vout,
            script: output.script_pubkey,
            shibes: output.value,
            balance: balances.amount,
          });
        }

        dune_balance.total_balance += balances.amount;
        dune_balance.total_outputs += 1;
        elements_counter += 1;
      }
    }

    let dune_balances: Vec<DuneBalance> = if show_all {
      dune_balances_map.values().cloned().collect()
    } else if list_dunes {
      dune_balances_map
        .values()
        .cloned()
        .skip(start_index)
        .take(items_per_page)
        .collect()
    } else {
      let values: Vec<DuneBalance> = dune_balances_map.values().cloned().collect();
      let mut items_collected = 0;
      let mut result = Vec::new();
      for value in values.iter() {
        let balances: Vec<DuneOutput> = value
          .balances
          .iter()
          .skip(start_index)
          .take(items_per_page - items_collected)
          .cloned()
          .collect();
        items_collected += balances.len();
        start_index -= value.balances.len().min(start_index);
        if balances.is_empty() {
          continue;
        }
        result.push(DuneBalance {
          dune: value.dune.clone(),
          divisibility: value.divisibility,
          symbol: value.symbol.clone(),
          total_balance: value.total_balance,
          total_outputs: value.total_outputs,
          balances,
        });
        if items_collected >= items_per_page {
          break;
        }
      }
      result
    };

    Ok(
      Json(DuneAddressJson {
        dunes: dune_balances,
        total_dunes: dune_balances_map.len(),
        total_elements: elements_counter,
      })
      .into_response(),
    )
  }

  async fn outputs_by_address(
    Extension(index): Extension<Arc<Index>>,
    Path(address): Path<String>,
  ) -> Result<String, ServerError> {
    let mut outputs = vec![];
    let outpoints = index.get_account_outputs(address)?;

    outputs.push(AddressOutputJson::new(outpoints));

    let outputs_json = to_string(&outputs).context("Failed to serialize outputs")?;

    Ok(outputs_json)
  }

  async fn outputs(
    Extension(server_config): Extension<Arc<PageConfig>>,
    Extension(index): Extension<Arc<Index>>,
    Path(outpoints_str): Path<String>,
  ) -> Result<String, ServerError> {
    let outpoints: Vec<OutPoint> = outpoints_str
      .split(',')
      .map(|s| OutPoint::from_str(s).expect("Failed to parse OutPoint"))
      .collect();
    let mut outputs = vec![];
    for outpoint in outpoints {
      let list = index.list(outpoint)?;

      let output = if outpoint == OutPoint::null() {
        let mut value = 0;

        if let Some(List::Unspent(ranges)) = &list {
          for (start, end) in ranges {
            value += u64::try_from(end - start).unwrap();
          }
        }

        TxOut {
          value,
          script_pubkey: Script::new(),
        }
      } else {
        index
          .get_transaction(outpoint.txid)?
          .ok_or_not_found(|| format!("output {outpoint}"))?
          .output
          .into_iter()
          .nth(outpoint.vout as usize)
          .ok_or_not_found(|| format!("output {outpoint}"))?
      };

      let inscriptions = index.get_inscriptions_on_output(outpoint)?;

      let dunes = index.get_dune_balances_for_outpoint(outpoint)?;

      outputs.push(OutputJson::new(
        server_config.chain,
        inscriptions,
        outpoint,
        output,
        dunes,
      ))
    }

    let outputs_json = to_string(&outputs).context("Failed to serialize outputs")?;

    Ok(outputs_json)
  }

  async fn drc20_tick_info(
    Extension(index): Extension<Arc<Index>>,
    Path(tick): Path<String>,
  ) -> Result<Response, ServerError> {
    let tick =
      &Tick::from_str(tick.as_str()).map_err(|err| ServerError::BadRequest(err.to_string()))?;
    let token_info = index.get_drc20_token_info(&tick.clone())?;

    if let Some(token_info) = token_info {
      Ok(Json(token_info).into_response())
    } else {
      Err(ServerError::BadRequest("No token info found".to_string()))
    }
  }

  async fn drc20_all_tick_info(
    Extension(index): Extension<Arc<Index>>,
  ) -> Result<Response, ServerError> {
    let token_info = index
      .get_drc20_tokens_info()
      .map_err(|err| ServerError::BadRequest(err.to_string()))?;
    Ok(Json(token_info).into_response())
  }

  async fn drc20_balance(
    Extension(index): Extension<Arc<Index>>,
    Path(params): Path<(String, String)>,
  ) -> Result<Response, ServerError> {
    let tick = params.0;
    let address = params.1;
    let tick = Tick::from_str(&tick).map_err(|err| ServerError::BadRequest(err.to_string()))?;
    let address =
      Address::from_str(&address).map_err(|err| ServerError::BadRequest(err.to_string()))?;

    let balance = index
      .get_drc20_balance(&ScriptKey::from_address(address), &tick)
      .map_err(|err| ServerError::BadRequest(err.to_string()))?;

    /*let available_balance = if let Some(balance) = balance
    {
      balance.overall_balance - balance.transferable_balance
    } else {
      0
    };*/

    Ok(Json(balance).into_response())
  }

  async fn drc20_all_balance(
    Extension(index): Extension<Arc<Index>>,
    Path(address): Path<String>,
  ) -> Result<Response, ServerError> {
    let address =
      Address::from_str(&address).map_err(|err| ServerError::BadRequest(err.to_string()))?;

    let balance = index
      .get_drc20_balances(&ScriptKey::from_address(address))
      .map_err(|err| ServerError::BadRequest(err.to_string()))?;

    /*let available_balance = if let Some(balance) = balance
    {
      balance.overall_balance - balance.transferable_balance
    } else {
      0
    };*/

    Ok(Json(balance).into_response())
  }

  async fn range(
    Extension(page_config): Extension<Arc<PageConfig>>,
    Path((DeserializeFromStr(start), DeserializeFromStr(end))): Path<(
      DeserializeFromStr<Sat>,
      DeserializeFromStr<Sat>,
    )>,
  ) -> ServerResult<PageHtml<RangeHtml>> {
    match start.cmp(&end) {
      Ordering::Equal => Err(ServerError::BadRequest("empty range".to_string())),
      Ordering::Greater => Err(ServerError::BadRequest(
        "range start greater than range end".to_string(),
      )),
      Ordering::Less => Ok(RangeHtml { start, end }.page(page_config)),
    }
  }

  async fn rare_txt(Extension(index): Extension<Arc<Index>>) -> ServerResult<RareTxt> {
    Ok(RareTxt(index.rare_sat_satpoints()?))
  }

  async fn dune(
    Extension(page_config): Extension<Arc<PageConfig>>,
    Extension(index): Extension<Arc<Index>>,
    Path(DeserializeFromStr(dune_query)): Path<DeserializeFromStr<query::Dune>>,
    Query(query): Query<JsonQuery>,
  ) -> ServerResult<Response> {
    let dune = match dune_query {
      query::Dune::SpacedDune(spaced_dune) => spaced_dune.dune,
      query::Dune::DuneId(dune_id) => index
        .get_dune_by_id(dune_id)?
        .ok_or_not_found(|| format!("dune {dune_id}"))?,
    };

    let (id, entry) = index.dune(dune)?.ok_or_else(|| {
      ServerError::NotFound(
        "tracking dunes requires index created with `--index-dunes` flag".into(),
      )
    })?;

    let block_height = index.height()?.unwrap_or(Height(0));

    let mintable = entry
      .mintable(Height(block_height.n() + 1).0.into())
      .is_ok();

    let inscription = InscriptionId {
      txid: entry.etching,
      index: 0,
    };

    let inscription = index
      .inscription_exists(inscription)?
      .then_some(inscription);

    Ok(if !query.json.unwrap_or_default() {
      DuneHtml {
        id,
        entry,
        mintable,
        inscription,
      }
      .page(page_config)
      .into_response()
    } else {
      Json(DuneJson {
        entry: DuneEntryJson {
          burned: entry.burned,
          divisibility: entry.divisibility,
          etching: entry.etching,
          mint: entry.terms,
          mints: entry.mints,
          number: entry.number,
          dune: entry.spaced_dune(),
          supply: entry.supply,
          symbol: entry.symbol,
          timestamp: entry.timestamp,
        },
        id,
        mintable,
        inscription,
      })
      .into_response()
    })
  }

  async fn dunes(
    Extension(page_config): Extension<Arc<PageConfig>>,
    Extension(index): Extension<Arc<Index>>,
  ) -> ServerResult<PageHtml<DunesHtml>> {
    Ok(
      DunesHtml {
        entries: index.dunes()?,
      }
      .page(page_config),
    )
  }

  async fn dunes_balances(
    Extension(page_config): Extension<Arc<PageConfig>>,
    Extension(index): Extension<Arc<Index>>,
  ) -> ServerResult<Response> {
    let balances = index.get_dune_balance_map()?;
    Ok(
      DuneBalancesHtml { balances }
        .page(page_config)
        .into_response(),
    )
  }

  async fn dunes_by_outputs(
    Extension(index): Extension<Arc<Index>>,
    Query(query): Query<OutputsQuery>,
  ) -> ServerResult<Response> {
    let mut all_dunes_jsons = Vec::new();

    // Split the outputs string into individual outputs
    let outputs = query.outputs.split(',');

    for output in outputs {
      // Split the output into tx_id and vout
      let parts: Vec<&str> = output.split(':').collect();
      if parts.len() != 2 {
        return Err(ServerError::BadRequest("wrong output format".to_string()));
      }

      let tx_id = Txid::from_str(parts[0])
        .map_err(|_| ServerError::BadRequest("wrong tx id format".to_string()))?;
      let vout = parts[1]
        .parse::<u32>()
        .map_err(|_| ServerError::BadRequest("wrong vout format".to_string()))?;

      // Create OutPoint
      let outpoint = OutPoint::new(tx_id, vout);

      let dunes = index.get_dune_balances_for_outpoint(outpoint)?;

      for (dune, balances) in dunes {
        all_dunes_jsons.push(DuneOutputJson { dune, balances });
      }
    }

    Ok(Json(all_dunes_jsons).into_response())
  }

  async fn home(
    Extension(page_config): Extension<Arc<PageConfig>>,
    Extension(index): Extension<Arc<Index>>,
  ) -> ServerResult<PageHtml<HomeHtml>> {
    Ok(HomeHtml::new(index.blocks(100)?, index.get_homepage_inscriptions()?).page(page_config))
  }

  async fn install_script() -> Redirect {
    Redirect::to("https://raw.githubusercontent.com/apezord/ord-dogecoin/master/install.sh")
  }

  async fn block(
    Extension(page_config): Extension<Arc<PageConfig>>,
    Extension(index): Extension<Arc<Index>>,
    Path(DeserializeFromStr(query)): Path<DeserializeFromStr<query::Block>>,
  ) -> ServerResult<PageHtml<BlockHtml>> {
    let (block, height) = match query {
      query::Block::Height(height) => {
        let block = index
          .get_block_by_height(height)?
          .ok_or_not_found(|| format!("block {height}"))?;

        (block, height)
      }
      query::Block::Hash(hash) => {
        let info = index
          .block_header_info(hash)?
          .ok_or_not_found(|| format!("block {hash}"))?;

        let block = index
          .get_block_by_hash(hash)?
          .ok_or_not_found(|| format!("block {hash}"))?;

        (block, u32::try_from(info.height).unwrap())
      }
    };

    // Prepare the inputs_per_tx map
    let inputs_per_tx = block
      .txdata
      .iter()
      .map(|tx| {
        let txid = tx.txid();
        let inputs = tx
          .input
          .iter()
          .map(|input| input.previous_output.to_string())
          .collect::<Vec<_>>()
          .join(",");
        (txid, inputs)
      })
      .collect::<HashMap<_, _>>();

    // Parallelize the processing using Rayon
    let results: Vec<_> = block
      .txdata
      .par_iter()
      .flat_map_iter(|tx| {
        let txid = tx.txid();
        tx.input
          .par_iter()
          .map(|input| get_transaction_details(input, &index, &page_config))
          .map(move |(value, address)| (txid.clone(), value, address))
          .collect::<Vec<_>>()
      })
      .collect();

    // Separate the results into the desired HashMaps
    let input_values_per_tx: HashMap<_, _> = results
      .iter()
      .map(|(txid, value, _)| (txid.clone(), value.clone()))
      .collect();

    let input_addresses_per_tx: HashMap<_, _> = results
      .iter()
      .map(|(txid, _, address)| (txid.clone(), address.clone()))
      .collect();

    // Prepare the outputs_per_tx map
    let outputs_per_tx = block
      .txdata
      .iter()
      .map(|tx| {
        let txid = tx.txid();
        let outputs = tx.output.iter()
            .enumerate()  // Enumerate the iterator to get the index of each output
            .map(|(vout, _output)| {
              let outpoint = OutPoint::new(txid, vout as u32);  // Create the OutPoint from txid and vout
              outpoint.to_string()  // Convert the OutPoint to a string
            })
            .collect::<Vec<_>>()
            .join(",");
        (txid, outputs)
      })
      .collect::<HashMap<_, _>>();

    // Prepare the output values per tx
    let output_values_per_tx = block
      .txdata
      .iter()
      .map(|tx| {
        let txid = tx.txid();
        let output_values = tx
          .output
          .iter()
          .map(|output| output.value.to_string())
          .collect::<Vec<_>>()
          .join(",");
        (txid, output_values)
      })
      .collect::<HashMap<_, _>>();

    let output_addresses_per_tx: HashMap<_, _> = block
      .txdata
      .iter()
      .map(|tx| {
        let txid = tx.txid();
        let addresses = tx
          .output
          .iter()
          .map(|output| {
            page_config
              .chain
              .address_from_script(&output.script_pubkey)
              .map(|address| address.to_string())
              .unwrap_or_else(|_| String::new())
          })
          .collect::<Vec<_>>()
          .join(",");
        (txid, addresses)
      })
      .collect();

    let inscriptions_per_tx: HashMap<_, _> = block
      .txdata
      .iter()
      .filter_map(|tx| {
        let txid = tx.txid();
        match index.get_inscription_by_id(txid.into()) {
          Ok(Some(inscription)) => {
            let inscription_id = InscriptionId::from(txid);
            let content_type = inscription.content_type().map(|s| s.to_string()); // Convert content type to Option<String>
            let content = inscription.into_body();
            Some((txid, (inscription_id, content_type, content)))
          }
          _ => None,
        }
      })
      .collect();

    Ok(
      BlockHtml::new(
        block,
        Height(height),
        Self::index_height(&index)?,
        inputs_per_tx,
        input_values_per_tx,
        input_addresses_per_tx,
        outputs_per_tx,
        output_values_per_tx,
        inscriptions_per_tx,
        output_addresses_per_tx,
      )
      .page(page_config),
    )
  }

  async fn blocks(
    Extension(page_config): Extension<Arc<PageConfig>>,
    Extension(index): Extension<Arc<Index>>,
    Path(path): Path<(u32, u32)>,
    Query(query): Query<BlocksQuery>,
  ) -> Result<String, ServerError> {
    let (height, endheight) = path;
    let mut blocks = vec![];
    for height in height..endheight {
      let block = index
        .get_block_by_height(height)?
        .ok_or_not_found(|| format!("block {}", height))?;

      let txids = block
        .txdata
        .iter()
        .map(|tx| tx.txid().to_string())
        .collect::<Vec<_>>()
        .join(",");

      // Prepare the inputs_per_tx map
      let inputs_per_tx = block
        .txdata
        .iter()
        .map(|tx| {
          let txid = tx.txid();
          let inputs = tx
            .input
            .iter()
            .map(|input| input.previous_output.to_string())
            .collect::<Vec<_>>()
            .join(",");
          (txid, inputs)
        })
        .collect::<HashMap<_, _>>();

      let mut input_values_per_tx: HashMap<_, _> = HashMap::new();
      let mut input_addresses_per_tx: HashMap<_, _> = HashMap::new();

      if !query.no_input_data.unwrap_or(true) {
        // Parallelize the processing using Rayon
        let results: Vec<_> = block
          .txdata
          .par_iter()
          .flat_map_iter(|tx| {
            let txid = tx.txid();
            tx.input
              .par_iter()
              .map(|input| get_transaction_details(input, &index, &page_config))
              .map(move |(value, address)| (txid.clone(), value, address))
              .collect::<Vec<_>>()
          })
          .collect();

        // Separate the results into the desired HashMaps
        input_values_per_tx = results
          .iter()
          .map(|(txid, value, _)| (txid.clone(), value.clone()))
          .collect();

        input_addresses_per_tx = results
          .iter()
          .map(|(txid, _, address)| (txid.clone(), address.clone()))
          .collect();
      }

      // Prepare the outputs_per_tx map
      let outputs_per_tx = block
        .txdata
        .iter()
        .map(|tx| {
          let txid = tx.txid();
          let outputs = tx.output.iter()
            .enumerate()  // Enumerate the iterator to get the index of each output
            .map(|(vout, _output)| {
              let outpoint = OutPoint::new(txid, vout as u32);  // Create the OutPoint from txid and vout
              outpoint.to_string()  // Convert the OutPoint to a string
            })
            .collect::<Vec<_>>()
            .join(",");
          (txid, outputs)
        })
        .collect::<HashMap<_, _>>();

      // Prepare the output values per tx
      let output_values_per_tx = block
        .txdata
        .iter()
        .map(|tx| {
          let txid = tx.txid();
          let output_values = tx
            .output
            .iter()
            .map(|output| output.value.to_string())
            .collect::<Vec<_>>()
            .join(",");
          (txid, output_values)
        })
        .collect::<HashMap<_, _>>();

      let output_addresses_per_tx: HashMap<_, _> = block
        .txdata
        .iter()
        .map(|tx| {
          let txid = tx.txid();
          let addresses = tx
            .output
            .iter()
            .map(|output| {
              page_config
                .chain
                .address_from_script(&output.script_pubkey)
                .map(|address| address.to_string())
                .unwrap_or_else(|_| String::new())
            })
            .collect::<Vec<_>>()
            .join(",");
          (txid, addresses)
        })
        .collect();

      let output_scripts_per_tx: HashMap<_, _> = block
        .txdata
        .iter()
        .map(|tx| {
          let txid = tx.txid();
          let scripts = tx
            .output
            .iter()
            .map(|output| {
              // Convert the byte array to a hexadecimal string.
              // If the byte array is empty, this will result in an empty string.
              hex::encode(&output.script_pubkey)
            })
            .collect::<Vec<_>>()
            .join(",");
          (txid, scripts)
        })
        .collect();

      let inscriptions_per_tx: HashMap<_, _> = if !query.no_inscriptions.unwrap_or_default() {
        block
          .txdata
          .iter()
          .filter_map(|tx| {
            let txid = tx.txid();
            match index.get_inscription_by_id(txid.into()) {
              Ok(Some(inscription)) => {
                let inscription_id = InscriptionId::from(txid);
                let content_type = inscription.content_type().map(|s| s.to_string()); // Convert content type to Option<String>

                // Check if content_type starts with "image" or "video"
                let content = if let Some(ref ct) = content_type {
                  if ct.starts_with("application/json") || ct.starts_with("text") {
                    // If it's an image or video, set content to None
                    None
                  } else {
                    // Otherwise, use the actual content
                    inscription.into_body()
                  }
                } else {
                  // If there's no content type, use the actual content
                  inscription.into_body()
                };

                Some((txid, (inscription_id, content_type, content)))
              }
              _ => None,
            }
          })
          .collect()
      } else {
        HashMap::new()
      };

      blocks.push(BlockJson::new(
        block,
        Height(height).0,
        txids,
        inputs_per_tx,
        input_values_per_tx,
        input_addresses_per_tx,
        outputs_per_tx,
        output_values_per_tx,
        inscriptions_per_tx,
        output_addresses_per_tx,
        output_scripts_per_tx,
      ));
    }

    // This will convert the Vec<BlocksJson> into a JSON string
    let blocks_json = to_string(&blocks).context("Failed to serialize blocks")?;

    Ok(blocks_json)
  }

  async fn transaction(
    Extension(page_config): Extension<Arc<PageConfig>>,
    Extension(index): Extension<Arc<Index>>,
    Path(txid): Path<Txid>,
    Query(query): Query<JsonQuery>,
  ) -> ServerResult<Response> {
    let json = query.json.unwrap_or(false);
    let inscription = index.get_inscription_by_id(txid.into())?;

    let mut blockhash = None;
    let mut confirmations = None;

    if let Some(block_hash_info) = index.get_transaction_blockhash(txid)? {
      blockhash = block_hash_info.hash;
      confirmations = block_hash_info.confirmations;
    }

    let tx_object = TransactionHtml::new(
      index
        .get_transaction(txid)?
        .ok_or_not_found(|| format!("transaction {txid}"))?,
      blockhash,
      confirmations,
      inscription.map(|_| txid.into()),
      page_config.chain,
      None,
    );

    Ok(if !json {
      tx_object.page(page_config).into_response()
    } else {
      Json(tx_object.to_json()).into_response()
    })
  }

  async fn status(Extension(index): Extension<Arc<Index>>) -> (StatusCode, &'static str) {
    if index.is_unrecoverably_reorged() {
      (
        StatusCode::OK,
        "unrecoverable reorg detected, please rebuild the database.",
      )
    } else {
      (
        StatusCode::OK,
        StatusCode::OK.canonical_reason().unwrap_or_default(),
      )
    }
  }

  async fn search_by_query(
    Extension(index): Extension<Arc<Index>>,
    Query(search): Query<Search>,
  ) -> ServerResult<Redirect> {
    Self::search(&index, &search.query).await
  }

  async fn search_by_path(
    Extension(index): Extension<Arc<Index>>,
    Path(search): Path<Search>,
  ) -> ServerResult<Redirect> {
    Self::search(&index, &search.query).await
  }

  async fn search(index: &Index, query: &str) -> ServerResult<Redirect> {
    Self::search_inner(index, query)
  }

  fn search_inner(index: &Index, query: &str) -> ServerResult<Redirect> {
    lazy_static! {
      static ref HASH: Regex = Regex::new(r"^[[:xdigit:]]{64}$").unwrap();
      static ref OUTPOINT: Regex = Regex::new(r"^[[:xdigit:]]{64}:\d+$").unwrap();
      static ref INSCRIPTION_ID: Regex = Regex::new(r"^[[:xdigit:]]{64}i\d+$").unwrap();
      static ref DUNE: Regex = Regex::new(r"^[A-Z•.]+$").unwrap();
      static ref DUNE_ID: Regex = Regex::new(r"^[0-9]+:[0-9]+$").unwrap();
    }

    let query = query.trim();

    if HASH.is_match(query) {
      if index.block_header(query.parse().unwrap())?.is_some() {
        Ok(Redirect::to(&format!("/block/{query}")))
      } else {
        Ok(Redirect::to(&format!("/tx/{query}")))
      }
    } else if OUTPOINT.is_match(query) {
      Ok(Redirect::to(&format!("/output/{query}")))
    } else if INSCRIPTION_ID.is_match(query) {
      Ok(Redirect::to(&format!("/shibescription/{query}")))
    } else if DUNE.is_match(query) {
      Ok(Redirect::to(&format!("/dune/{query}")))
    } else if DUNE_ID.is_match(query) {
      let id = query
        .parse::<DuneId>()
        .map_err(|err| ServerError::BadRequest(err.to_string()))?;

      let dune = index.get_dune_by_id(id)?.ok_or_not_found(|| "dune ID")?;

      Ok(Redirect::to(&format!("/dune/{dune}")))
    } else {
      Ok(Redirect::to(&format!("/sat/{query}")))
    }
  }

  async fn favicon(user_agent: Option<TypedHeader<UserAgent>>) -> ServerResult<Response> {
    if user_agent
      .map(|user_agent| {
        user_agent.as_str().contains("Safari/")
          && !user_agent.as_str().contains("Chrome/")
          && !user_agent.as_str().contains("Chromium/")
      })
      .unwrap_or_default()
    {
      Ok(
        Self::static_asset(Path("/favicon.png".to_string()))
          .await
          .into_response(),
      )
    } else {
      Ok(
        (
          [(
            header::CONTENT_SECURITY_POLICY,
            HeaderValue::from_static("default-src 'unsafe-inline'"),
          )],
          Self::static_asset(Path("/favicon.svg".to_string())).await?,
        )
          .into_response(),
      )
    }
  }

  async fn feed(
    Extension(page_config): Extension<Arc<PageConfig>>,
    Extension(index): Extension<Arc<Index>>,
  ) -> ServerResult<Response> {
    let mut builder = rss::ChannelBuilder::default();

    let chain = page_config.chain;
    match chain {
      Chain::Mainnet => builder.title("Shibescriptions"),
      _ => builder.title(format!("Shibescriptions – {chain:?}")),
    };

    builder.generator(Some("ord".to_string()));

    for (number, id) in index.get_feed_inscriptions(300)? {
      builder.item(
        rss::ItemBuilder::default()
          .title(format!("Shibescription {number}"))
          .link(format!("/shibescription/{id}"))
          .guid(Some(rss::Guid {
            value: format!("/shibescription/{id}"),
            permalink: true,
          }))
          .build(),
      );
    }

    Ok(
      (
        [
          (header::CONTENT_TYPE, "application/rss+xml"),
          (
            header::CONTENT_SECURITY_POLICY,
            "default-src 'unsafe-inline'",
          ),
        ],
        builder.build().to_string(),
      )
        .into_response(),
    )
  }

  async fn static_asset(Path(path): Path<String>) -> ServerResult<Response> {
    let content = StaticAssets::get(if let Some(stripped) = path.strip_prefix('/') {
      stripped
    } else {
      &path
    })
    .ok_or_not_found(|| format!("asset {path}"))?;
    let body = body::boxed(body::Full::from(content.data));
    let mime = mime_guess::from_path(path).first_or_octet_stream();
    Ok(
      Response::builder()
        .header(header::CONTENT_TYPE, mime.as_ref())
        .body(body)
        .unwrap(),
    )
  }

  async fn block_count(Extension(index): Extension<Arc<Index>>) -> ServerResult<String> {
    Ok(index.block_count()?.to_string())
  }

  async fn input(
    Extension(page_config): Extension<Arc<PageConfig>>,
    Extension(index): Extension<Arc<Index>>,
    Path(path): Path<(u32, usize, usize)>,
  ) -> Result<PageHtml<InputHtml>, ServerError> {
    let not_found = || format!("input /{}/{}/{}", path.0, path.1, path.2);

    let block = index
      .get_block_by_height(path.0)?
      .ok_or_not_found(not_found)?;

    let transaction = block
      .txdata
      .into_iter()
      .nth(path.1)
      .ok_or_not_found(not_found)?;

    let input = transaction
      .input
      .into_iter()
      .nth(path.2)
      .ok_or_not_found(not_found)?;

    Ok(InputHtml { path, input }.page(page_config))
  }

  async fn faq() -> Redirect {
    Redirect::to("https://docs.ordinals.com/faq/")
  }

  async fn bounties() -> Redirect {
    Redirect::to("https://docs.ordinals.com/bounty/")
  }

  async fn content(
    Extension(index): Extension<Arc<Index>>,
    Extension(config): Extension<Arc<Config>>,
    Path(inscription_id): Path<InscriptionId>,
    Extension(page_config): Extension<Arc<PageConfig>>,
  ) -> ServerResult<Response> {
    if config.is_hidden(inscription_id) {
      return Ok(PreviewUnknownHtml.into_response());
    }

    let mut inscription = index
      .get_inscription_by_id(inscription_id)?
      .ok_or_not_found(|| format!("inscription {inscription_id}"))?;

    if let Some(delegate) = inscription.delegate() {
      inscription = index
        .get_inscription_by_id(delegate)?
        .ok_or_not_found(|| format!("delegate {inscription_id}"))?
    }

    Ok(
      Self::content_response(inscription, &page_config)
        .ok_or_not_found(|| format!("inscription {inscription_id} content"))?
        .into_response(),
    )
  }

  fn content_response(
    inscription: Inscription,
    page_config: &PageConfig,
  ) -> Option<(HeaderMap, Vec<u8>)> {
    let mut headers = HeaderMap::new();
    match &page_config.csp_origin {
      None => {
        headers.insert(
          header::CONTENT_SECURITY_POLICY,
          HeaderValue::from_static("default-src 'self' 'unsafe-eval' 'unsafe-inline' data: blob:"),
        );
        headers.append(
          header::CONTENT_SECURITY_POLICY,
          HeaderValue::from_static("default-src *:*/content/ *:*/blockheight *:*/blockhash *:*/blockhash/ *:*/blocktime *:*/r/ 'unsafe-eval' 'unsafe-inline' data: blob:"),
        );
      }
      Some(origin) => {
        let csp = format!("default-src {origin}/content/ {origin}/blockheight {origin}/blockhash {origin}/blockhash/ {origin}/blocktime {origin}/r/ 'unsafe-eval' 'unsafe-inline' data: blob:");
        headers.insert(
          header::CONTENT_SECURITY_POLICY,
          HeaderValue::from_str(&csp)
            .map_err(|err| ServerError::Internal(Error::from(err)))
            .ok()?,
        );
      }
    }
    headers.insert(
      header::CACHE_CONTROL,
      HeaderValue::from_static("max-age=31536000, immutable"),
    );
    headers.insert(
      header::CONTENT_TYPE,
      inscription
        .content_type()
        .and_then(|content_type| content_type.parse().ok())
        .unwrap_or(HeaderValue::from_static("application/octet-stream")),
    );

    Some((headers, inscription.into_body()?))
  }

  async fn preview(
    Extension(index): Extension<Arc<Index>>,
    Extension(config): Extension<Arc<Config>>,
    Extension(page_config): Extension<Arc<PageConfig>>,
    Path(inscription_id): Path<InscriptionId>,
  ) -> ServerResult<Response> {
    if config.is_hidden(inscription_id) {
      return Ok(PreviewUnknownHtml.into_response());
    }

    let mut inscription = index
      .get_inscription_by_id(inscription_id)?
      .ok_or_not_found(|| format!("inscription {inscription_id}"))?;

    if let Some(delegate) = inscription.delegate() {
      inscription = index
        .get_inscription_by_id(delegate)?
        .ok_or_not_found(|| format!("delegate {inscription_id}"))?
    }

    return match inscription.media() {
      Media::Audio => Ok(PreviewAudioHtml { inscription_id }.into_response()),
      Media::Iframe => Ok(
        Self::content_response(inscription, &page_config)
          .ok_or_not_found(|| format!("inscription {inscription_id} content"))?
          .into_response(),
      ),
      Media::Model => Ok(
        (
          [(
            header::CONTENT_SECURITY_POLICY,
            "script-src-elem 'self' https://ajax.googleapis.com",
          )],
          PreviewModelHtml { inscription_id },
        )
          .into_response(),
      ),
      Media::Image => Ok(
        (
          [(
            header::CONTENT_SECURITY_POLICY,
            "default-src 'self' 'unsafe-inline'",
          )],
          PreviewImageHtml { inscription_id },
        )
          .into_response(),
      ),
      Media::Pdf => Ok(
        (
          [(
            header::CONTENT_SECURITY_POLICY,
            "script-src-elem 'self' https://cdn.jsdelivr.net",
          )],
          PreviewPdfHtml { inscription_id },
        )
          .into_response(),
      ),
      Media::Text => Ok(PreviewTextHtml { inscription_id }.into_response()),
      Media::Unknown => Ok(PreviewUnknownHtml.into_response()),
      Media::Video => Ok(PreviewVideoHtml { inscription_id }.into_response()),
    };
  }

  async fn inscription(
    Extension(page_config): Extension<Arc<PageConfig>>,
    Extension(index): Extension<Arc<Index>>,
    Path(inscription_id): Path<InscriptionId>,
    Query(query): Query<JsonQuery>,
  ) -> ServerResult<Response> {
    let entry = index
      .get_inscription_entry(inscription_id)?
      .ok_or_not_found(|| format!("inscription {inscription_id}"))?;

    let mut inscription = index
      .get_inscription_by_id(inscription_id)?
      .ok_or_not_found(|| format!("inscription {inscription_id}"))?;

    if let Some(delegate) = inscription.delegate() {
      let delegate_inscription = index
        .get_inscription_by_id(delegate)?
        .ok_or_not_found(|| format!("delegate {inscription_id}"))?;
      inscription.body = Some(Vec::new());
      inscription.content_type = delegate_inscription.content_type;
    }

    let satpoint = index
      .get_inscription_satpoint_by_id(inscription_id)?
      .ok_or_not_found(|| format!("inscription {inscription_id}"))?;

    let output = index
      .get_transaction(satpoint.outpoint.txid)?
      .ok_or_not_found(|| format!("inscription {inscription_id} current transaction"))?
      .output
      .into_iter()
      .nth(satpoint.outpoint.vout.try_into().unwrap())
      .ok_or_not_found(|| format!("inscription {inscription_id} current transaction output"))?;

    let previous = if let Some(previous) = entry.inscription_number.checked_sub(1) {
      Some(
        index
          .get_inscription_id_by_inscription_number(previous)?
          .ok_or_not_found(|| format!("inscription {previous}"))?,
      )
    } else {
      None
    };

    let next = index.get_inscription_id_by_inscription_number(entry.inscription_number + 1)?;

    let dune = index.get_dune_by_inscription_id(inscription_id)?;

    if !query.json.unwrap_or_default() {
      Ok(
        InscriptionHtml {
          chain: page_config.chain,
          genesis_fee: entry.fee,
          genesis_height: entry.height,
          inscription,
          inscription_id,
          next,
          inscription_number: entry.inscription_number,
          output,
          previous,
          sat: entry.sat,
          satpoint,
          timestamp: timestamp(entry.timestamp.into()),
          dune,
        }
        .page(page_config)
        .into_response(),
      )
    } else {
      let mut address: Option<String> = None;

      match page_config.chain.address_from_script(&output.script_pubkey) {
        Ok(add) => {
          address = Some(add.to_string());
        }
        Err(_error) => {
          // do nothing
        }
      }

      Ok(
        Json(ShibescriptionJson {
          chain: page_config.chain,
          genesis_fee: entry.fee,
          genesis_height: entry.height,
          inscription,
          inscription_id,
          next,
          inscription_number: entry.inscription_number,
          output,
          address,
          previous,
          sat: entry.sat,
          satpoint,
          timestamp: Default::default(),
          dune,
        })
        .into_response(),
      )
    }
  }

  async fn inscriptions(
    Extension(page_config): Extension<Arc<PageConfig>>,
    Extension(index): Extension<Arc<Index>>,
  ) -> ServerResult<PageHtml<InscriptionsHtml>> {
    Self::inscriptions_inner(page_config, index, None).await
  }

  async fn shibescriptions_by_outputs(
    Extension(index): Extension<Arc<Index>>,
    Query(query): Query<OutputsQuery>,
  ) -> ServerResult<Response> {
    let mut all_inscription_jsons = Vec::new();

    // Split the outputs string into individual outputs
    let outputs = query.outputs.split(',');

    for output in outputs {
      // Split the output into tx_id and vout
      let parts: Vec<&str> = output.split(':').collect();
      if parts.len() != 2 {
        return Err(ServerError::BadRequest("wrong output format".to_string()));
      }

      let tx_id = Txid::from_str(parts[0])
        .map_err(|_| ServerError::BadRequest("wrong tx id format".to_string()))?;
      let vout = parts[1]
        .parse::<u32>()
        .map_err(|_| ServerError::BadRequest("wrong vout format".to_string()))?;

      // Create OutPoint
      let outpoint = OutPoint::new(tx_id, vout);

      // Query the index for inscriptions on this OutPoint
      let inscriptions = index.get_inscriptions_on_output(outpoint)?;

      let output = index
        .get_transaction(outpoint.txid)?
        .ok_or_not_found(|| format!("inscription {tx_id} current transaction"))?
        .output
        .into_iter()
        .nth(outpoint.vout.try_into().unwrap())
        .ok_or_not_found(|| format!("inscription {vout} current transaction output"))?;

      for inscription_id in inscriptions {
        let inscription = index
          .get_inscription_by_id(inscription_id)?
          .ok_or_not_found(|| format!("inscription {inscription_id}"))?;

        let entry = index
          .get_inscription_entry(inscription_id)?
          .ok_or_not_found(|| format!("inscription {inscription_id}"))?;

        let satpoint = index
          .get_inscription_satpoint_by_id(inscription_id)?
          .ok_or_not_found(|| format!("inscription {inscription_id}"))?;

        let content_type = inscription.content_type().map(|s| s.to_string());
        let content_length = inscription.content_length();
        let content = inscription.into_body();

        let str_content = match (content_type.clone(), content) {
          (Some(ref ct), Some(c))
            if ct.starts_with("application/json") || ct.starts_with("text") =>
          {
            Some(String::from_utf8_lossy(c.as_slice()).to_string())
          }
          (None, Some(c)) => Some(String::from_utf8_lossy(c.as_slice()).to_string()),
          _ => None,
        };

        let confirmations =
          if let Some(block_hash_info) = index.get_transaction_blockhash(outpoint.txid)? {
            block_hash_info.confirmations
          } else {
            None
          };

        let inscription_json = InscriptionByAddressJson {
          utxo: Utxo {
            txid: tx_id,
            vout,
            script: output.script_pubkey.clone(),
            shibes: output.value,
            confirmations,
          },
          content: str_content,
          content_length,
          content_type,
          genesis_height: entry.height,
          inscription_id,
          inscription_number: entry.inscription_number,
          timestamp: entry.timestamp,
          offset: satpoint.offset,
        };

        all_inscription_jsons.push(inscription_json);
      }
    }

    // Build your response
    Ok(Json(all_inscription_jsons).into_response())
  }

  async fn inscriptions_by_outputs(
    Extension(index): Extension<Arc<Index>>,
    Query(query): Query<OutputsQuery>,
  ) -> ServerResult<Response> {
    let mut all_inscription_jsons = Vec::new();

    // Split the outputs string into individual outputs
    let outputs = query.outputs.split(',');

    for output in outputs {
      // Split the output into tx_id and vout
      let parts: Vec<&str> = output.split(':').collect();
      if parts.len() != 2 {
        return Err(ServerError::BadRequest("wrong output format".to_string()));
      }

      let tx_id = Txid::from_str(parts[0])
        .map_err(|_| ServerError::BadRequest("wrong tx id format".to_string()))?;
      let vout = parts[1]
        .parse::<u32>()
        .map_err(|_| ServerError::BadRequest("wrong vout format".to_string()))?;

      // Create OutPoint
      let outpoint = OutPoint::new(tx_id, vout);

      // Query the index for inscriptions on this OutPoint
      let inscriptions = index.get_inscriptions_on_output(outpoint)?;

      for inscription_id in inscriptions {
        let inscription = index
          .get_inscription_by_id(inscription_id)?
          .ok_or_not_found(|| format!("inscription {inscription_id}"))?;

        let entry = index
          .get_inscription_entry(inscription_id)?
          .ok_or_not_found(|| format!("inscription {inscription_id}"))?;

        let content_type = inscription.content_type().map(|s| s.to_string());
        let content_length = inscription.content_length();
        let content = inscription.into_body();

        let str_content = if let Some(ref ct) = content_type {
          if ct.starts_with("application/json") || ct.starts_with("text") {
            content
          } else {
            // Otherwise, don't serve it
            None
          }
        } else {
          // If there's no content type, use the actual content
          content
        };

        let inscription_json = InscriptionJson {
          content: str_content,
          content_length,
          content_type,
          genesis_height: entry.height,
          inscription_id,
          inscription_number: entry.inscription_number,
          //dune: None,
          timestamp: entry.timestamp,
          tx_id: tx_id.to_string(),
          vout,
        };

        all_inscription_jsons.push(inscription_json);
      }
    }

    // Build your response
    Ok(Json(all_inscription_jsons).into_response())
  }

  async fn inscriptions_from(
    Extension(page_config): Extension<Arc<PageConfig>>,
    Extension(index): Extension<Arc<Index>>,
    Path(from): Path<u64>,
  ) -> ServerResult<PageHtml<InscriptionsHtml>> {
    Self::inscriptions_inner(page_config, index, Some(from)).await
  }

  async fn inscriptions_inner(
    page_config: Arc<PageConfig>,
    index: Arc<Index>,
    from: Option<u64>,
  ) -> ServerResult<PageHtml<InscriptionsHtml>> {
    let (inscriptions, prev, next) = index.get_latest_inscriptions_with_prev_and_next(100, from)?;
    Ok(
      InscriptionsHtml {
        inscriptions,
        next,
        prev,
      }
      .page(page_config),
    )
  }

  async fn redirect_http_to_https(
    Extension(mut destination): Extension<String>,
    uri: Uri,
  ) -> Redirect {
    if let Some(path_and_query) = uri.path_and_query() {
      destination.push_str(path_and_query.as_str());
    }

    Redirect::to(&destination)
  }
}

#[cfg(test)]
mod tests {
  use bitcoin::blockdata::constants::COIN_VALUE;

  use {super::*, reqwest::Url, std::net::TcpListener};

  use crate::dunes::{Dunestone, Edict, Etching};

  struct TestServer {
    dogecoin_rpc_server: test_bitcoincore_rpc::Handle,
    index: Arc<Index>,
    ord_server_handle: Handle,
    url: Url,
    #[allow(unused)]
    tempdir: TempDir,
  }

  impl TestServer {
    fn new() -> Self {
      Self::new_with_args(&[], &[])
    }

    fn new_with_sat_index() -> Self {
      Self::new_with_args(&["--index-sats"], &[])
    }

    fn new_with_args(ord_args: &[&str], server_args: &[&str]) -> Self {
      Self::new_server(test_bitcoincore_rpc::spawn(), None, ord_args, server_args)
    }

    fn new_with_dogecoin_rpc_server_and_config(
      dogecoin_rpc_server: test_bitcoincore_rpc::Handle,
      config: String,
    ) -> Self {
      Self::new_server(dogecoin_rpc_server, Some(config), &[], &[])
    }

    fn new_server(
      dogecoin_rpc_server: test_bitcoincore_rpc::Handle,
      config: Option<String>,
      ord_args: &[&str],
      server_args: &[&str],
    ) -> Self {
      let tempdir = TempDir::new().unwrap();

      let cookiefile = tempdir.path().join("cookie");

      fs::write(&cookiefile, "username:password").unwrap();

      let port = TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port();

      let url = Url::parse(&format!("http://127.0.0.1:{port}")).unwrap();

      let config_args = match config {
        Some(config) => {
          let config_path = tempdir.path().join("ord.yaml");
          fs::write(&config_path, config).unwrap();
          format!("--config {}", config_path.display())
        }
        None => "".to_string(),
      };

      let (options, server) = parse_server_args(&format!(
        "ord --chain regtest --rpc-url {} --cookie-file {} --data-dir {} {config_args} {} server --http-port {} --address 127.0.0.1 {}",
        dogecoin_rpc_server.url(),
        cookiefile.to_str().unwrap(),
        tempdir.path().to_str().unwrap(),
        ord_args.join(" "),
        port,
        server_args.join(" "),
      ));

      let index = Arc::new(Index::open(&options).unwrap());
      let ord_server_handle = Handle::new();

      {
        let index = index.clone();
        let ord_server_handle = ord_server_handle.clone();
        thread::spawn(|| server.run(options, index, ord_server_handle).unwrap());
      }

      while index.statistic(crate::index::Statistic::Commits) == 0 {
        thread::sleep(Duration::from_millis(25));
      }

      let client = reqwest::blocking::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .unwrap();

      for i in 0.. {
        match client.get(format!("http://127.0.0.1:{port}/status")).send() {
          Ok(_) => break,
          Err(err) => {
            if i == 400 {
              panic!("server failed to start: {err}");
            }
          }
        }

        thread::sleep(Duration::from_millis(25));
      }

      Self {
        dogecoin_rpc_server,
        index,
        ord_server_handle,
        tempdir,
        url,
      }
    }

    fn get(&self, path: impl AsRef<str>) -> reqwest::blocking::Response {
      if let Err(error) = self.index.update() {
        log::error!("{error}");
      }
      reqwest::blocking::get(self.join_url(path.as_ref())).unwrap()
    }

    fn join_url(&self, url: &str) -> Url {
      self.url.join(url).unwrap()
    }

    fn assert_response(&self, path: impl AsRef<str>, status: StatusCode, expected_response: &str) {
      let response = self.get(path);
      assert_eq!(response.status(), status, "{}", response.text().unwrap());
      pretty_assert_eq!(response.text().unwrap(), expected_response);
    }

    fn assert_response_regex(
      &self,
      path: impl AsRef<str>,
      status: StatusCode,
      regex: impl AsRef<str>,
    ) {
      let response = self.get(path);
      assert_eq!(response.status(), status);
      assert_regex_match!(response.text().unwrap(), regex.as_ref());
    }

    fn assert_response_csp(
      &self,
      path: impl AsRef<str>,
      status: StatusCode,
      content_security_policy: &str,
      regex: impl AsRef<str>,
    ) {
      let response = self.get(path);
      assert_eq!(response.status(), status);
      assert_eq!(
        response
          .headers()
          .get(header::CONTENT_SECURITY_POLICY,)
          .unwrap(),
        content_security_policy
      );
      assert_regex_match!(response.text().unwrap(), regex.as_ref());
    }

    fn assert_redirect(&self, path: &str, location: &str) {
      let response = reqwest::blocking::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .unwrap()
        .get(self.join_url(path))
        .send()
        .unwrap();

      assert_eq!(response.status(), StatusCode::SEE_OTHER);
      assert_eq!(response.headers().get(header::LOCATION).unwrap(), location);
    }

    fn mine_blocks(&self, n: u64) -> Vec<bitcoin::Block> {
      let blocks = self.dogecoin_rpc_server.mine_blocks(n);
      self.index.update().unwrap();
      blocks
    }

    fn mine_blocks_with_subsidy(&self, n: u64, subsidy: u64) -> Vec<Block> {
      let blocks = self
        .dogecoin_rpc_server
        .mine_blocks_with_subsidy(n, subsidy);
      self.index.update().unwrap();
      blocks
    }
  }

  impl Drop for TestServer {
    fn drop(&mut self) {
      self.ord_server_handle.shutdown();
    }
  }

  fn parse_server_args(args: &str) -> (Options, Server) {
    match Arguments::try_parse_from(args.split_whitespace()) {
      Ok(arguments) => match arguments.subcommand {
        Subcommand::Server(server) => (arguments.options, server),
        subcommand => panic!("unexpected subcommand: {subcommand:?}"),
      },
      Err(err) => panic!("error parsing arguments: {err}"),
    }
  }

  #[test]
  fn http_and_https_port_dont_conflict() {
    parse_server_args(
      "ord server --http-port 0 --https-port 0 --acme-cache foo --acme-contact bar --acme-domain baz",
    );
  }

  #[test]
  fn http_port_defaults_to_80() {
    assert_eq!(parse_server_args("ord server").1.http_port(), Some(80));
  }

  #[test]
  fn https_port_defaults_to_none() {
    assert_eq!(parse_server_args("ord server").1.https_port(), None);
  }

  #[test]
  fn https_sets_https_port_to_443() {
    assert_eq!(
      parse_server_args("ord server --https --acme-cache foo --acme-contact bar --acme-domain baz")
        .1
        .https_port(),
      Some(443)
    );
  }

  #[test]
  fn https_disables_http() {
    assert_eq!(
      parse_server_args("ord server --https --acme-cache foo --acme-contact bar --acme-domain baz")
        .1
        .http_port(),
      None
    );
  }

  #[test]
  fn https_port_disables_http() {
    assert_eq!(
      parse_server_args(
        "ord server --https-port 433 --acme-cache foo --acme-contact bar --acme-domain baz"
      )
      .1
      .http_port(),
      None
    );
  }

  #[test]
  fn https_port_sets_https_port() {
    assert_eq!(
      parse_server_args(
        "ord server --https-port 1000 --acme-cache foo --acme-contact bar --acme-domain baz"
      )
      .1
      .https_port(),
      Some(1000)
    );
  }

  #[test]
  fn http_with_https_leaves_http_enabled() {
    assert_eq!(
      parse_server_args(
        "ord server --https --http --acme-cache foo --acme-contact bar --acme-domain baz"
      )
      .1
      .http_port(),
      Some(80)
    );
  }

  #[test]
  fn http_with_https_leaves_https_enabled() {
    assert_eq!(
      parse_server_args(
        "ord server --https --http --acme-cache foo --acme-contact bar --acme-domain baz"
      )
      .1
      .https_port(),
      Some(443)
    );
  }

  #[test]
  fn acme_contact_accepts_multiple_values() {
    assert!(Arguments::try_parse_from([
      "ord",
      "server",
      "--address",
      "127.0.0.1",
      "--http-port",
      "0",
      "--acme-contact",
      "foo",
      "--acme-contact",
      "bar"
    ])
    .is_ok());
  }

  #[test]
  fn acme_domain_accepts_multiple_values() {
    assert!(Arguments::try_parse_from([
      "ord",
      "server",
      "--address",
      "127.0.0.1",
      "--http-port",
      "0",
      "--acme-domain",
      "foo",
      "--acme-domain",
      "bar"
    ])
    .is_ok());
  }

  #[test]
  fn acme_cache_defaults_to_data_dir() {
    let arguments = Arguments::try_parse_from(["ord", "--data-dir", "foo", "server"]).unwrap();
    let acme_cache = Server::acme_cache(None, &arguments.options)
      .unwrap()
      .display()
      .to_string();
    assert!(
      acme_cache.contains(if cfg!(windows) {
        r"foo\acme-cache"
      } else {
        "foo/acme-cache"
      }),
      "{acme_cache}"
    )
  }

  #[test]
  fn acme_cache_flag_is_respected() {
    let arguments =
      Arguments::try_parse_from(["ord", "--data-dir", "foo", "server", "--acme-cache", "bar"])
        .unwrap();
    let acme_cache = Server::acme_cache(Some(&"bar".into()), &arguments.options)
      .unwrap()
      .display()
      .to_string();
    assert_eq!(acme_cache, "bar")
  }

  #[test]
  fn acme_domain_defaults_to_hostname() {
    let (_, server) = parse_server_args("ord server");
    assert_eq!(
      server.acme_domains().unwrap(),
      &[System::host_name().unwrap()]
    );
  }

  #[test]
  fn acme_domain_flag_is_respected() {
    let (_, server) = parse_server_args("ord server --acme-domain example.com");
    assert_eq!(server.acme_domains().unwrap(), &["example.com"]);
  }

  #[test]
  fn install_sh_redirects_to_github() {
    TestServer::new().assert_redirect(
      "/install.sh",
      "https://raw.githubusercontent.com/apezord/ord-dogecoin/master/install.sh",
    );
  }

  #[test]
  fn ordinal_redirects_to_sat() {
    TestServer::new().assert_redirect("/ordinal/0", "/sat/0");
  }

  #[test]
  fn bounties_redirects_to_docs_site() {
    TestServer::new().assert_redirect("/bounties", "https://docs.ordinals.com/bounty/");
  }

  #[test]
  fn faq_redirects_to_docs_site() {
    TestServer::new().assert_redirect("/faq", "https://docs.ordinals.com/faq/");
  }

  #[test]
  fn search_by_query_returns_sat() {
    TestServer::new().assert_redirect("/search?query=0", "/sat/0");
  }

  #[test]
  fn search_by_query_returns_dune() {
    TestServer::new().assert_redirect("/search?query=ABCD", "/dune/ABCD");
  }

  #[test]
  fn search_by_query_returns_inscription() {
    TestServer::new().assert_redirect(
      "/search?query=0000000000000000000000000000000000000000000000000000000000000000i0",
      "/shibescription/0000000000000000000000000000000000000000000000000000000000000000i0",
    );
  }

  #[test]
  fn search_is_whitespace_insensitive() {
    TestServer::new().assert_redirect("/search/ 0 ", "/sat/0");
  }

  #[test]
  fn search_by_path_returns_sat() {
    TestServer::new().assert_redirect("/search/0", "/sat/0");
  }

  #[test]
  fn search_for_blockhash_returns_block() {
    TestServer::new().assert_redirect(
      "/search/1a91e3dace36e2be3bf030a65679fe821aa1d6ef92e7c9902eb318182c355691",
      "/block/1a91e3dace36e2be3bf030a65679fe821aa1d6ef92e7c9902eb318182c355691",
    );
  }

  #[test]
  fn search_for_txid_returns_transaction() {
    TestServer::new().assert_redirect(
      "/search/0000000000000000000000000000000000000000000000000000000000000000",
      "/tx/0000000000000000000000000000000000000000000000000000000000000000",
    );
  }

  #[test]
  fn search_for_outpoint_returns_output() {
    TestServer::new().assert_redirect(
      "/search/0000000000000000000000000000000000000000000000000000000000000000:0",
      "/output/0000000000000000000000000000000000000000000000000000000000000000:0",
    );
  }

  #[test]
  fn search_for_inscription_id_returns_inscription() {
    TestServer::new().assert_redirect(
      "/search/0000000000000000000000000000000000000000000000000000000000000000i0",
      "/shibescription/0000000000000000000000000000000000000000000000000000000000000000i0",
    );
  }

  #[test]
  fn search_by_path_returns_dune() {
    TestServer::new().assert_redirect("/search/ABCD", "/dune/ABCD");
  }

  #[test]
  fn search_by_dune_id_returns_dune() {
    let server = TestServer::new_with_regtest_with_index_dunes();

    server.mine_blocks(1);

    let dune = Dune(u128::from(21_000_000 * COIN_VALUE));

    server.assert_response_regex(format!("/dune/{dune}"), StatusCode::NOT_FOUND, ".*");

    server.bitcoin_rpc_server.broadcast_tx(TransactionTemplate {
      inputs: &[(1, 0, 0)],
      witness: inscription("text/plain", "hello").to_witness(),
      op_return: Some(
        Dunestone {
          edicts: vec![Edict {
            id: 0,
            amount: u128::max_value(),
            output: 0,
          }],
          etching: Some(Etching {
            dune,
            ..Default::default()
          }),
          ..Default::default()
        }
        .encipher(),
      ),
      ..Default::default()
    });

    server.mine_blocks(1);

    server.assert_redirect("/search/2/1", "/dune/NVTDIJZYIPU");
    server.assert_redirect("/search?query=2/1", "/dune/NVTDIJZYIPU");

    server.assert_response_regex("/dune/100/200", StatusCode::NOT_FOUND, ".*");

    server.assert_response_regex(
      "/search/100000000000000000000/200000000000000000",
      StatusCode::BAD_REQUEST,
      ".*",
    );
  }

  #[test]
  fn http_to_https_redirect_with_path() {
    TestServer::new_with_args(&[], &["--redirect-http-to-https", "--https"]).assert_redirect(
      "/sat/0",
      &format!("https://{}/sat/0", System::host_name().unwrap()),
    );
  }

  #[test]
  fn http_to_https_redirect_with_empty() {
    TestServer::new_with_args(&[], &["--redirect-http-to-https", "--https"])
      .assert_redirect("/", &format!("https://{}/", System::host_name().unwrap()));
  }

  #[test]
  fn status() {
    TestServer::new().assert_response("/status", StatusCode::OK, "OK");
  }

  #[test]
  fn block_count_endpoint() {
    let test_server = TestServer::new();

    let response = test_server.get("/block-count");

    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.text().unwrap(), "1");

    test_server.mine_blocks(1);

    let response = test_server.get("/block-count");

    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.text().unwrap(), "2");
  }

  #[test]
  fn range_end_before_range_start_returns_400() {
    TestServer::new().assert_response(
      "/range/1/0",
      StatusCode::BAD_REQUEST,
      "range start greater than range end",
    );
  }

  #[test]
  fn invalid_range_start_returns_400() {
    TestServer::new().assert_response(
      "/range/=/0",
      StatusCode::BAD_REQUEST,
      "Invalid URL: invalid digit found in string",
    );
  }

  #[test]
  fn invalid_range_end_returns_400() {
    TestServer::new().assert_response(
      "/range/0/=",
      StatusCode::BAD_REQUEST,
      "Invalid URL: invalid digit found in string",
    );
  }

  #[test]
  fn empty_range_returns_400() {
    TestServer::new().assert_response("/range/0/0", StatusCode::BAD_REQUEST, "empty range");
  }

  #[test]
  fn range() {
    TestServer::new().assert_response_regex(
      "/range/0/1",
      StatusCode::OK,
      r".*<title>Sat range 0–1</title>.*<h1>Sat range 0–1</h1>
<dl>
  <dt>value</dt><dd>1</dd>
  <dt>first</dt><dd><a href=/sat/0 class=mythic>0</a></dd>
</dl>.*",
    );
  }
  #[test]
  fn sat_number() {
    TestServer::new().assert_response_regex("/sat/0", StatusCode::OK, ".*<h1>Sat 0</h1>.*");
  }

  #[test]
  fn sat_decimal() {
    TestServer::new().assert_response_regex("/sat/0.0", StatusCode::OK, ".*<h1>Sat 0</h1>.*");
  }

  #[test]
  fn sat() {
    TestServer::new().assert_response_regex(
      "/sat/0",
      StatusCode::OK,
      ".*<title>Sat 0</title>.*<h1>Sat 0</h1>.*",
    );
  }

  #[test]
  fn block() {
    TestServer::new().assert_response_regex(
      "/block/0",
      StatusCode::OK,
      ".*<title>Block 0</title>.*<h1>Block 0</h1>.*",
    );
  }

  #[test]
  #[ignore]
  fn sat_out_of_range() {
    TestServer::new().assert_response(
      "/sat/2099999997690000",
      StatusCode::BAD_REQUEST,
      "Invalid URL: invalid sat",
    );
  }

  #[test]
  fn invalid_outpoint_hash_returns_400() {
    TestServer::new().assert_response(
      "/output/foo:0",
      StatusCode::BAD_REQUEST,
      "Invalid URL: error parsing TXID",
    );
  }

  #[test]
  fn output_with_sat_index() {
    let txid = "5b2a3f53f605d62c53e62932dac6925e3d74afa5a4b459745c36d42d0ed26a69";
    TestServer::new_with_sat_index().assert_response_regex(
      format!("/output/{txid}:0"),
      StatusCode::OK,
      format!(
        ".*<title>Output {txid}:0</title>.*<h1>Output <span class=monospace>{txid}:0</span></h1>
<dl>
  <dt>value</dt><dd>8800000000</dd>
  <dt>script pubkey</dt><dd class=monospace>OP_PUSHBYTES_65 [[:xdigit:]]{{130}} OP_CHECKSIG</dd>
  <dt>transaction</dt><dd><a class=monospace href=/tx/{txid}>{txid}</a></dd>
</dl>
<h2>1 Sat Range</h2>
<ul class=monospace>
  <li><a href=/range/0/8800000000 class=mythic>0–8800000000</a></li>
</ul>.*"
      ),
    );
  }

  #[test]
  fn output_without_sat_index() {
    let txid = "5b2a3f53f605d62c53e62932dac6925e3d74afa5a4b459745c36d42d0ed26a69";
    TestServer::new().assert_response_regex(
      format!("/output/{txid}:0"),
      StatusCode::OK,
      format!(
        ".*<title>Output {txid}:0</title>.*<h1>Output <span class=monospace>{txid}:0</span></h1>
<dl>
  <dt>value</dt><dd>8800000000</dd>
  <dt>script pubkey</dt><dd class=monospace>OP_PUSHBYTES_65 [[:xdigit:]]{{130}} OP_CHECKSIG</dd>
  <dt>transaction</dt><dd><a class=monospace href=/tx/{txid}>{txid}</a></dd>
</dl>.*"
      ),
    );
  }

  #[test]
  #[ignore]
  fn null_output_is_initially_empty() {
    let txid = "0000000000000000000000000000000000000000000000000000000000000000";
    TestServer::new_with_sat_index().assert_response_regex(
      format!("/output/{txid}:4294967295"),
      StatusCode::OK,
      format!(
        ".*<title>Output {txid}:4294967295</title>.*<h1>Output <span class=monospace>{txid}:4294967295</span></h1>
<dl>
  <dt>value</dt><dd>0</dd>
  <dt>script pubkey</dt><dd class=monospace></dd>
  <dt>transaction</dt><dd><a class=monospace href=/tx/{txid}>{txid}</a></dd>
</dl>
<h2>0 Sat Ranges</h2>
<ul class=monospace>
</ul>.*"
      ),
    );
  }

  #[test]
  #[ignore]
  fn null_output_receives_lost_sats() {
    let server = TestServer::new_with_sat_index();

    server.mine_blocks_with_subsidy(1, 0);

    let txid = "0000000000000000000000000000000000000000000000000000000000000000";

    server.assert_response_regex(
      format!("/output/{txid}:4294967295"),
      StatusCode::OK,
      format!(
        ".*<title>Output {txid}:4294967295</title>.*<h1>Output <span class=monospace>{txid}:4294967295</span></h1>
<dl>
  <dt>value</dt><dd>5000000000</dd>
  <dt>script pubkey</dt><dd class=monospace></dd>
  <dt>transaction</dt><dd><a class=monospace href=/tx/{txid}>{txid}</a></dd>
</dl>
<h2>1 Sat Range</h2>
<ul class=monospace>
  <li><a href=/range/5000000000/10000000000 class=uncommon>5000000000–10000000000</a></li>
</ul>.*"
      ),
    );
  }

  #[test]
  fn unknown_output_returns_404() {
    TestServer::new().assert_response(
      "/output/0000000000000000000000000000000000000000000000000000000000000000:0",
      StatusCode::NOT_FOUND,
      "output 0000000000000000000000000000000000000000000000000000000000000000:0 not found",
    );
  }

  #[test]
  fn invalid_output_returns_400() {
    TestServer::new().assert_response(
      "/output/foo:0",
      StatusCode::BAD_REQUEST,
      "Invalid URL: error parsing TXID",
    );
  }

  #[test]
  #[ignore]
  fn home() {
    let test_server = TestServer::new();

    test_server.mine_blocks(1);

    test_server.assert_response_regex(
    "/",
    StatusCode::OK,
    ".*<title>Dunes</title>.*
<h2>Latest Blocks</h2>
<ol start=1 reversed class=blocks>
  <li><a href=/block/[[:xdigit:]]{64}>[[:xdigit:]]{64}</a></li>
  <li><a href=/block/000000000019d6689c085ae165831e934ff763ae46a2a6c172b3f1b60a8ce26f>000000000019d6689c085ae165831e934ff763ae46a2a6c172b3f1b60a8ce26f</a></li>
</ol>.*",
  );
  }

  #[test]
  fn nav_displays_chain() {
    TestServer::new().assert_response_regex(
      "/",
      StatusCode::OK,
      ".*<a href=/>Dunes<sup>regtest</sup></a>.*",
    );
  }

  #[test]
  fn home_block_limit() {
    let test_server = TestServer::new();

    test_server.mine_blocks(101);

    test_server.assert_response_regex(
    "/",
    StatusCode::OK,
    ".*<ol start=101 reversed class=blocks>\n(  <li><a href=/block/[[:xdigit:]]{64}>[[:xdigit:]]{64}</a></li>\n){100}</ol>.*"
  );
  }

  #[test]
  fn block_not_found() {
    TestServer::new().assert_response(
      "/block/467a86f0642b1d284376d13a98ef58310caa49502b0f9a560ee222e0a122fe16",
      StatusCode::NOT_FOUND,
      "block 467a86f0642b1d284376d13a98ef58310caa49502b0f9a560ee222e0a122fe16 not found",
    );
  }

  #[test]
  #[ignore]
  fn unmined_sat() {
    TestServer::new().assert_response_regex(
      "/sat/0",
      StatusCode::OK,
      ".*<dt>timestamp</dt><dd><time>2009-01-03 18:15:05 UTC</time></dd>.*",
    );
  }

  #[test]
  #[ignore]
  fn mined_sat() {
    TestServer::new().assert_response_regex(
      "/sat/5000000000",
      StatusCode::OK,
      ".*<dt>timestamp</dt><dd><time>.*</time> \\(expected\\)</dd>.*",
    );
  }

  #[test]
  fn static_asset() {
    TestServer::new().assert_response_regex(
      "/static/index.css",
      StatusCode::OK,
      r".*\.rare \{
  background-color: var\(--rare\);
}.*",
    );
  }

  #[test]
  fn favicon() {
    TestServer::new().assert_response_regex("/favicon.ico", StatusCode::OK, r".*");
  }

  #[test]
  fn block_by_hash() {
    let test_server = TestServer::new();

    test_server.mine_blocks(1);
    let transaction = TransactionTemplate {
      inputs: &[(1, 0, 0)],
      fee: 0,
      ..Default::default()
    };
    test_server.dogecoin_rpc_server.broadcast_tx(transaction);
    let block_hash = test_server.mine_blocks(1)[0].block_hash();

    test_server.assert_response_regex(
      format!("/block/{block_hash}"),
      StatusCode::OK,
      ".*<h1>Block 2</h1>.*",
    );
  }

  #[test]
  fn block_by_height() {
    let test_server = TestServer::new();

    test_server.assert_response_regex("/block/0", StatusCode::OK, ".*<h1>Block 0</h1>.*");
  }

  #[test]
  fn transaction() {
    let test_server = TestServer::new();

    let coinbase_tx = test_server.mine_blocks(1)[0].txdata[0].clone();
    let txid = coinbase_tx.txid();

    test_server.assert_response_regex(
      format!("/tx/{txid}"),
      StatusCode::OK,
      format!(
        ".*<title>Transaction {txid}</title>.*<h1>Transaction <span class=monospace>{txid}</span></h1>
<h2>1 Input</h2>
<ul>
  <li><a class=monospace href=/output/0000000000000000000000000000000000000000000000000000000000000000:4294967295>0000000000000000000000000000000000000000000000000000000000000000:4294967295</a></li>
</ul>
<h2>1 Output</h2>
<ul class=monospace>
  <li>
    <a href=/output/30f2f037629c6a21c1f40ed39b9bd6278df39762d68d07f49582b23bcb23386a:0 class=monospace>
      30f2f037629c6a21c1f40ed39b9bd6278df39762d68d07f49582b23bcb23386a:0
    </a>
    <dl>
      <dt>value</dt><dd>5000000000</dd>
      <dt>script pubkey</dt><dd class=monospace></dd>
    </dl>
  </li>
</ul>.*"
      ),
    );
  }

  #[test]
  fn detect_unrecoverable_reorg() {
    let test_server = TestServer::new();

    test_server.mine_blocks(21);

    test_server.assert_response("/status", StatusCode::OK, "OK");

    for _ in 0..15 {
      test_server.bitcoin_rpc_server.invalidate_tip();
    }

    test_server.bitcoin_rpc_server.mine_blocks(21);

    test_server.assert_response_regex("/status", StatusCode::OK, "unrecoverable reorg detected.*");
  }

  #[test]
  fn rare_with_sat_index() {
    TestServer::new_with_sat_index().assert_response(
      "/rare.txt",
      StatusCode::OK,
      "sat\tsatpoint
0\t5b2a3f53f605d62c53e62932dac6925e3d74afa5a4b459745c36d42d0ed26a69:0:0
",
    );
  }

  #[test]
  fn rare_without_sat_index() {
    TestServer::new().assert_response(
      "/rare.txt",
      StatusCode::OK,
      "sat\tsatpoint
",
    );
  }

  #[test]
  fn show_rare_txt_in_header_with_sat_index() {
    TestServer::new_with_sat_index().assert_response_regex(
      "/",
      StatusCode::OK,
      ".*
      <a href=/rare.txt>rare.txt</a>
      <form action=/search method=get>.*",
    );
  }

  #[test]
  fn rare_sat_location() {
    TestServer::new_with_sat_index().assert_response_regex(
      "/sat/0",
      StatusCode::OK,
      ".*>5b2a3f53f605d62c53e62932dac6925e3d74afa5a4b459745c36d42d0ed26a69:0:0<.*",
    );
  }

  #[test]
  fn dont_show_rare_txt_in_header_without_sat_index() {
    TestServer::new().assert_response_regex(
      "/",
      StatusCode::OK,
      ".*
      <form action=/search method=get>.*",
    );
  }

  #[test]
  fn input() {
    TestServer::new().assert_response_regex(
      "/input/0/0/0",
      StatusCode::OK,
      ".*<title>Input /0/0/0</title>.*<h1>Input /0/0/0</h1>.*<dt>text</dt><dd>.*Nintondo</dd>.*",
    );
  }

  #[test]
  fn input_missing() {
    TestServer::new().assert_response(
      "/input/1/1/1",
      StatusCode::NOT_FOUND,
      "input /1/1/1 not found",
    );
  }

  #[test]
  fn commits_are_tracked() {
    let server = TestServer::new();

    thread::sleep(Duration::from_millis(25));
    assert_eq!(server.index.statistic(crate::index::Statistic::Commits), 3);

    let info = server.index.info().unwrap();
    assert_eq!(info.transactions.len(), 1);
    assert_eq!(info.transactions[0].starting_block_count, 0);

    server.index.update().unwrap();

    assert_eq!(server.index.statistic(crate::index::Statistic::Commits), 3);

    let info = server.index.info().unwrap();
    assert_eq!(info.transactions.len(), 1);
    assert_eq!(info.transactions[0].starting_block_count, 0);

    server.mine_blocks(1);

    thread::sleep(Duration::from_millis(10));
    server.index.update().unwrap();

    assert_eq!(server.index.statistic(crate::index::Statistic::Commits), 6);

    let info = server.index.info().unwrap();
    assert_eq!(info.transactions.len(), 2);
    assert_eq!(info.transactions[0].starting_block_count, 0);
    assert_eq!(info.transactions[1].starting_block_count, 1);
    assert!(
      info.transactions[1].starting_timestamp - info.transactions[0].starting_timestamp >= 10
    );
  }

  #[test]
  fn outputs_traversed_are_tracked() {
    let server = TestServer::new_with_sat_index();

    assert_eq!(
      server
        .index
        .statistic(crate::index::Statistic::OutputsTraversed),
      1
    );

    server.index.update().unwrap();

    assert_eq!(
      server
        .index
        .statistic(crate::index::Statistic::OutputsTraversed),
      1
    );

    server.mine_blocks(2);

    server.index.update().unwrap();

    assert_eq!(
      server
        .index
        .statistic(crate::index::Statistic::OutputsTraversed),
      3
    );
  }

  #[test]
  fn coinbase_sat_ranges_are_tracked() {
    let server = TestServer::new_with_sat_index();

    assert_eq!(
      server.index.statistic(crate::index::Statistic::SatRanges),
      2
    );

    server.mine_blocks(1);

    assert_eq!(
      server.index.statistic(crate::index::Statistic::SatRanges),
      4
    );

    server.mine_blocks(1);

    assert_eq!(
      server.index.statistic(crate::index::Statistic::SatRanges),
      6
    );
  }

  #[test]
  fn split_sat_ranges_are_tracked() {
    let server = TestServer::new_with_sat_index();

    assert_eq!(
      server.index.statistic(crate::index::Statistic::SatRanges),
      2
    );

    server.mine_blocks(1);
    server
      .dogecoin_rpc_server
      .broadcast_tx(TransactionTemplate {
        inputs: &[(1, 0, 0)],
        outputs: 2,
        fee: 0,
        ..Default::default()
      });
    server.mine_blocks(1);

    assert_eq!(
      server.index.statistic(crate::index::Statistic::SatRanges),
      7,
    );
  }

  #[test]
  fn fee_sat_ranges_are_tracked() {
    let server = TestServer::new_with_sat_index();

    assert_eq!(
      server.index.statistic(crate::index::Statistic::SatRanges),
      2
    );

    server.mine_blocks(1);
    server
      .dogecoin_rpc_server
      .broadcast_tx(TransactionTemplate {
        inputs: &[(1, 0, 0)],
        outputs: 2,
        fee: 2,
        ..Default::default()
      });
    server.mine_blocks(1);

    assert_eq!(
      server.index.statistic(crate::index::Statistic::SatRanges),
      8,
    );
  }

  #[test]
  fn content_response_no_content() {
    assert_eq!(
      Server::content_response(Inscription::new(
        Some("text/plain".as_bytes().to_vec()),
        None
      )),
      None
    );
  }

  #[test]
  fn content_response_with_content() {
    let (headers, body) = Server::content_response(Inscription::new(
      Some("text/plain".as_bytes().to_vec()),
      Some(vec![1, 2, 3]),
    ))
    .unwrap();

    assert_eq!(headers["content-type"], "text/plain");
    assert_eq!(body, vec![1, 2, 3]);
  }

  #[test]
  fn content_response_no_content_type() {
    let (headers, body) =
      Server::content_response(Inscription::new(None, Some(Vec::new()))).unwrap();

    assert_eq!(headers["content-type"], "application/octet-stream");
    assert!(body.is_empty());
  }

  #[test]
  fn text_preview() {
    let server = TestServer::new();
    server.mine_blocks(1);

    let txid = server
      .dogecoin_rpc_server
      .broadcast_tx(TransactionTemplate {
        inputs: &[(1, 0, 0)],
        witness: inscription("text/plain;charset=utf-8", "hello").to_witness(),
        ..Default::default()
      });

    server.mine_blocks(1);

    server.assert_response_csp(
      format!("/preview/{}", InscriptionId::from(txid)),
      StatusCode::OK,
      "default-src 'self'",
      ".*<pre>hello</pre>.*",
    );
  }

  #[test]
  fn text_preview_returns_error_when_content_is_not_utf8() {
    let server = TestServer::new();
    server.mine_blocks(1);

    let txid = server
      .dogecoin_rpc_server
      .broadcast_tx(TransactionTemplate {
        inputs: &[(1, 0, 0)],
        witness: inscription("text/plain;charset=utf-8", b"\xc3\x28").to_witness(),
        ..Default::default()
      });

    server.mine_blocks(1);

    server.assert_response(
      format!("/preview/{}", InscriptionId::from(txid)),
      StatusCode::INTERNAL_SERVER_ERROR,
      "Internal Server Error",
    );
  }

  #[test]
  fn text_preview_text_is_escaped() {
    let server = TestServer::new();
    server.mine_blocks(1);

    let txid = server
      .dogecoin_rpc_server
      .broadcast_tx(TransactionTemplate {
        inputs: &[(1, 0, 0)],
        witness: inscription(
          "text/plain;charset=utf-8",
          "<script>alert('hello');</script>",
        )
        .to_witness(),
        ..Default::default()
      });

    server.mine_blocks(1);

    server.assert_response_csp(
      format!("/preview/{}", InscriptionId::from(txid)),
      StatusCode::OK,
      "default-src 'self'",
      r".*<pre>&lt;script&gt;alert\(&apos;hello&apos;\);&lt;/script&gt;</pre>.*",
    );
  }

  #[test]
  fn audio_preview() {
    let server = TestServer::new();
    server.mine_blocks(1);

    let txid = server
      .dogecoin_rpc_server
      .broadcast_tx(TransactionTemplate {
        inputs: &[(1, 0, 0)],
        witness: inscription("audio/flac", "hello").to_witness(),
        ..Default::default()
      });
    let inscription_id = InscriptionId::from(txid);

    server.mine_blocks(1);

    server.assert_response_regex(
      format!("/preview/{inscription_id}"),
      StatusCode::OK,
      format!(r".*<audio .*>\s*<source src=/content/{inscription_id}>.*"),
    );
  }

  #[test]
  fn pdf_preview() {
    let server = TestServer::new();
    server.mine_blocks(1);

    let txid = server
      .dogecoin_rpc_server
      .broadcast_tx(TransactionTemplate {
        inputs: &[(1, 0, 0)],
        witness: inscription("application/pdf", "hello").to_witness(),
        ..Default::default()
      });
    let inscription_id = InscriptionId::from(txid);

    server.mine_blocks(1);

    server.assert_response_regex(
      format!("/preview/{inscription_id}"),
      StatusCode::OK,
      format!(r".*<canvas data-inscription={inscription_id}></canvas>.*"),
    );
  }

  #[test]
  fn image_preview() {
    let server = TestServer::new();
    server.mine_blocks(1);

    let txid = server
      .dogecoin_rpc_server
      .broadcast_tx(TransactionTemplate {
        inputs: &[(1, 0, 0)],
        witness: inscription("image/png", "hello").to_witness(),
        ..Default::default()
      });
    let inscription_id = InscriptionId::from(txid);

    server.mine_blocks(1);

    server.assert_response_csp(
      format!("/preview/{inscription_id}"),
      StatusCode::OK,
      "default-src 'self' 'unsafe-inline'",
      format!(r".*background-image: url\(/content/{inscription_id}\);.*"),
    );
  }

  #[test]
  fn iframe_preview() {
    let server = TestServer::new();
    server.mine_blocks(1);

    let txid = server
      .dogecoin_rpc_server
      .broadcast_tx(TransactionTemplate {
        inputs: &[(1, 0, 0)],
        witness: inscription("text/html;charset=utf-8", "hello").to_witness(),
        ..Default::default()
      });

    server.mine_blocks(1);

    server.assert_response_csp(
      format!("/preview/{}", InscriptionId::from(txid)),
      StatusCode::OK,
      "default-src 'unsafe-eval' 'unsafe-inline' data:",
      "hello",
    );
  }

  #[test]
  fn unknown_preview() {
    let server = TestServer::new();
    server.mine_blocks(1);

    let txid = server
      .dogecoin_rpc_server
      .broadcast_tx(TransactionTemplate {
        inputs: &[(1, 0, 0)],
        witness: inscription("text/foo", "hello").to_witness(),
        ..Default::default()
      });

    server.mine_blocks(1);

    server.assert_response_csp(
      format!("/preview/{}", InscriptionId::from(txid)),
      StatusCode::OK,
      "default-src 'self'",
      fs::read_to_string("templates/preview-unknown.html").unwrap(),
    );
  }

  #[test]
  fn video_preview() {
    let server = TestServer::new();
    server.mine_blocks(1);

    let txid = server
      .dogecoin_rpc_server
      .broadcast_tx(TransactionTemplate {
        inputs: &[(1, 0, 0)],
        witness: inscription("video/webm", "hello").to_witness(),
        ..Default::default()
      });
    let inscription_id = InscriptionId::from(txid);

    server.mine_blocks(1);

    server.assert_response_regex(
      format!("/preview/{inscription_id}"),
      StatusCode::OK,
      format!(r".*<video .*>\s*<source src=/content/{inscription_id}>.*"),
    );
  }

  #[test]
  fn inscription_page_title() {
    let server = TestServer::new_with_sat_index();
    server.mine_blocks(1);

    let txid = server
      .dogecoin_rpc_server
      .broadcast_tx(TransactionTemplate {
        inputs: &[(1, 0, 0)],
        witness: inscription("text/foo", "hello").to_witness(),
        ..Default::default()
      });

    server.mine_blocks(1);

    server.assert_response_regex(
      format!("/shibescription/{}", InscriptionId::from(txid)),
      StatusCode::OK,
      ".*<title>Shibescription 0</title>.*",
    );
  }

  #[test]
  fn inscription_page_has_sat_when_sats_are_tracked() {
    let server = TestServer::new_with_sat_index();
    server.mine_blocks(1);

    let txid = server
      .dogecoin_rpc_server
      .broadcast_tx(TransactionTemplate {
        inputs: &[(1, 0, 0)],
        witness: inscription("text/foo", "hello").to_witness(),
        ..Default::default()
      });

    server.mine_blocks(1);

    server.assert_response_regex(
      format!("/shibescription/{}", InscriptionId::from(txid)),
      StatusCode::OK,
      r".*<dt>sat</dt>\s*<dd><a href=/sat/100000000000000>100000000000000</a></dd>\s*<dt>preview</dt>.*",
    );
  }

  #[test]
  fn inscription_page_does_not_have_sat_when_sats_are_not_tracked() {
    let server = TestServer::new();
    server.mine_blocks(1);

    let txid = server
      .dogecoin_rpc_server
      .broadcast_tx(TransactionTemplate {
        inputs: &[(1, 0, 0)],
        witness: inscription("text/foo", "hello").to_witness(),
        ..Default::default()
      });

    server.mine_blocks(1);

    server.assert_response_regex(
      format!("/shibescription/{}", InscriptionId::from(txid)),
      StatusCode::OK,
      r".*<dt>output value</dt>\s*<dd>5000000000</dd>\s*<dt>preview</dt>.*",
    );
  }

  #[test]
  fn strict_transport_security_header_is_set() {
    assert_eq!(
      TestServer::new()
        .get("/status")
        .headers()
        .get(header::STRICT_TRANSPORT_SECURITY)
        .unwrap(),
      "max-age=31536000; includeSubDomains; preload",
    );
  }

  #[test]
  fn feed() {
    let server = TestServer::new_with_sat_index();
    server.mine_blocks(1);

    server
      .dogecoin_rpc_server
      .broadcast_tx(TransactionTemplate {
        inputs: &[(1, 0, 0)],
        witness: inscription("text/foo", "hello").to_witness(),
        ..Default::default()
      });

    server.mine_blocks(1);

    server.assert_response_regex(
      "/feed.xml",
      StatusCode::OK,
      ".*<title>Shibescription 0</title>.*",
    );
  }

  #[test]
  fn inscription_with_unknown_type_and_no_body_has_unknown_preview() {
    let server = TestServer::new_with_sat_index();
    server.mine_blocks(1);

    let txid = server
      .dogecoin_rpc_server
      .broadcast_tx(TransactionTemplate {
        inputs: &[(1, 0, 0)],
        witness: Inscription::new(Some("foo/bar".as_bytes().to_vec()), None).to_witness(),
        ..Default::default()
      });

    let inscription_id = InscriptionId::from(txid);

    server.mine_blocks(1);

    server.assert_response(
      format!("/preview/{inscription_id}"),
      StatusCode::OK,
      &fs::read_to_string("templates/preview-unknown.html").unwrap(),
    );
  }

  #[test]
  fn inscription_with_known_type_and_no_body_has_unknown_preview() {
    let server = TestServer::new_with_sat_index();
    server.mine_blocks(1);

    let txid = server
      .dogecoin_rpc_server
      .broadcast_tx(TransactionTemplate {
        inputs: &[(1, 0, 0)],
        witness: Inscription::new(Some("image/png".as_bytes().to_vec()), None).to_witness(),
        ..Default::default()
      });

    let inscription_id = InscriptionId::from(txid);

    server.mine_blocks(1);

    server.assert_response(
      format!("/preview/{inscription_id}"),
      StatusCode::OK,
      &fs::read_to_string("templates/preview-unknown.html").unwrap(),
    );
  }

  #[test]
  fn content_responses_have_cache_control_headers() {
    let server = TestServer::new();
    server.mine_blocks(1);

    let txid = server
      .dogecoin_rpc_server
      .broadcast_tx(TransactionTemplate {
        inputs: &[(1, 0, 0)],
        witness: inscription("text/foo", "hello").to_witness(),
        ..Default::default()
      });

    server.mine_blocks(1);

    let response = server.get(format!("/content/{}", InscriptionId::from(txid)));

    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
      response.headers().get(header::CACHE_CONTROL).unwrap(),
      "max-age=31536000, immutable"
    );
  }

  #[test]
  fn inscriptions_page_with_no_prev_or_next() {
    TestServer::new_with_sat_index().assert_response_regex(
      "/shibescriptions",
      StatusCode::OK,
      ".*prev\nnext.*",
    );
  }

  #[test]
  fn inscriptions_page_with_no_next() {
    let server = TestServer::new_with_sat_index();

    for i in 0..101 {
      server.mine_blocks(1);
      server
        .dogecoin_rpc_server
        .broadcast_tx(TransactionTemplate {
          inputs: &[(i + 1, 0, 0)],
          witness: inscription("text/foo", "hello").to_witness(),
          ..Default::default()
        });
    }

    server.mine_blocks(1);

    server.assert_response_regex(
      "/shibescriptions",
      StatusCode::OK,
      ".*<a class=prev href=/shibescriptions/0>prev</a>\nnext.*",
    );
  }

  #[test]
  fn inscriptions_page_with_no_prev() {
    let server = TestServer::new_with_sat_index();

    for i in 0..101 {
      server.mine_blocks(1);
      server
        .dogecoin_rpc_server
        .broadcast_tx(TransactionTemplate {
          inputs: &[(i + 1, 0, 0)],
          witness: inscription("text/foo", "hello").to_witness(),
          ..Default::default()
        });
    }

    server.mine_blocks(1);

    server.assert_response_regex(
      "/shibescriptions/0",
      StatusCode::OK,
      ".*prev\n<a class=next href=/shibescriptions/100>next</a>.*",
    );
  }

  #[test]
  fn resonses_are_gzipped() {
    let server = TestServer::new();

    let mut headers = HeaderMap::new();

    headers.insert(header::ACCEPT_ENCODING, "gzip".parse().unwrap());

    let response = reqwest::blocking::Client::builder()
      .default_headers(headers)
      .build()
      .unwrap()
      .get(server.join_url("/"))
      .send()
      .unwrap();

    assert_eq!(
      response.headers().get(header::CONTENT_ENCODING).unwrap(),
      "gzip"
    );
  }

  #[test]
  fn resonses_are_brotlied() {
    let server = TestServer::new();

    let mut headers = HeaderMap::new();

    headers.insert(header::ACCEPT_ENCODING, "br".parse().unwrap());

    let response = reqwest::blocking::Client::builder()
      .default_headers(headers)
      .build()
      .unwrap()
      .get(server.join_url("/"))
      .send()
      .unwrap();

    assert_eq!(
      response.headers().get(header::CONTENT_ENCODING).unwrap(),
      "br"
    );
  }

  #[test]
  fn inscriptions_can_be_hidden_with_config() {
    let dogecoin_rpc_server = test_bitcoincore_rpc::spawn();
    dogecoin_rpc_server.mine_blocks(1);
    let txid = dogecoin_rpc_server.broadcast_tx(TransactionTemplate {
      inputs: &[(1, 0, 0)],
      witness: inscription("text/plain;charset=utf-8", "hello").to_witness(),
      ..Default::default()
    });
    let inscription = InscriptionId::from(txid);
    dogecoin_rpc_server.mine_blocks(1);

    let server = TestServer::new_with_dogecoin_rpc_server_and_config(
      dogecoin_rpc_server,
      format!("\"hidden\":\n - {inscription}"),
    );

    server.assert_response(
      format!("/preview/{inscription}"),
      StatusCode::OK,
      &fs::read_to_string("templates/preview-unknown.html").unwrap(),
    );

    server.assert_response(
      format!("/content/{inscription}"),
      StatusCode::OK,
      &fs::read_to_string("templates/preview-unknown.html").unwrap(),
    );
  }
}
