mod cache;
mod memory;
mod network;
mod range;
mod server;

use anyhow::{Context, Result, ensure};
use clap::Parser;
use librqbit::{
    ConnectionOptions, DhtSessionConfig, PeerConnectionOptions, Session, SessionOptions,
    storage::StorageFactoryExt,
};
use std::{net::SocketAddr, sync::Arc, time::Duration};
use tokio::sync::Mutex;

#[derive(Parser, Debug, Clone)]
#[command(version, about = "RAM-only torrent streaming for one playback session")]
pub struct Config {
    #[arg(long, default_value = "127.0.0.1:8090")]
    listen: SocketAddr,
    #[arg(long, default_value_t = 96, value_parser = clap::value_parser!(u16).range(4..=256))]
    cache_mib: u16,
    #[arg(long, default_value_t = 20, value_parser = clap::value_parser!(u16).range(1..=64))]
    peers: u16,
    #[arg(long)]
    no_dht: bool,
    /// Release an idle torrent after this many seconds (0 disables).
    #[arg(long, default_value_t = 300)]
    idle_seconds: u64,
    /// Allow a browser client at this origin. May be repeated.
    #[arg(long)]
    allow_origin: Vec<String>,
    /// Accept requests addressed to this host name. IP addresses, localhost and
    /// .local names always work. May be repeated.
    #[arg(long)]
    allow_host: Vec<String>,
    /// Port for home network access (TV access), switched on from the control page.
    /// 8090 is the port TorrServer apps expect.
    #[arg(long, default_value_t = 8090, value_parser = clap::value_parser!(u16).range(1024..))]
    tv_port: u16,
    /// Exit when standard input closes. The Mac app passes a pipe, so the server never
    /// outlives it, even after a crash or Force Quit.
    #[arg(long)]
    exit_on_stdin_close: bool,
}

fn main() -> Result<()> {
    let config = Config::parse();
    tracing_subscriber::fmt()
        .with_env_filter(
            std::env::var("RUST_LOG")
                .unwrap_or_else(|_| "torrstream_lite=info,librqbit=warn".into()),
        )
        .init();
    tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .max_blocking_threads(8)
        .enable_all()
        .build()?
        .block_on(run(config))
}

async fn run(config: Config) -> Result<()> {
    ensure!(
        config.idle_seconds == 0 || config.idle_seconds >= 10,
        "--idle-seconds must be 0 or at least 10"
    );
    let factory = cache::RamFactory::new(config.cache_mib as usize * 1024 * 1024);
    let session = Session::new_with_opts(
        "__ram_only__".into(),
        SessionOptions {
            dht: if config.no_dht {
                None
            } else {
                Some(DhtSessionConfig {
                    persistence: None,
                    ..Default::default()
                })
            },
            persistence: None,
            fastresume: false,
            default_storage_factory: Some(factory.clone().boxed()),
            peer_limit: Some(config.peers as usize),
            concurrent_init_limit: Some(1),
            runtime_worker_threads: Some(8),
            disable_upload: true,
            disable_local_service_discovery: true,
            connect: Some(ConnectionOptions {
                peer_opts: Some(PeerConnectionOptions {
                    connect_timeout: Some(Duration::from_secs(5)),
                    read_write_timeout: Some(Duration::from_secs(15)),
                    keep_alive_interval: Some(Duration::from_secs(5)),
                    max_request_window: Some(16),
                }),
                ..Default::default()
            }),
            client_name_and_version: Some(format!("TorrStreamLite/{}", env!("CARGO_PKG_VERSION"))),
            ..Default::default()
        },
    )
    .await?;
    let state = Arc::new(server::AppState {
        session: session.clone(),
        factory,
        config: config.clone(),
        active: Mutex::new(None),
        operation: Mutex::new(()),
        started: std::time::Instant::now(),
        tv: Mutex::new(network::TvAccess::new(config.tv_port)),
        resume: Default::default(),
        client: reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(10))
            .timeout(Duration::from_secs(30))
            .build()?,
    });
    let listener = tokio::net::TcpListener::bind(config.listen)
        .await
        .context("Cannot open HTTP port; another server may be running")?;
    let addr = listener.local_addr()?;
    println!("TORRSTREAM_LISTEN=http://{addr}");
    tracing::info!(%addr, cache_mib=config.cache_mib, peers=config.peers, "TorrStream Lite ready");
    let idle = tokio::spawn(server::idle_watchdog(Arc::downgrade(&state)));
    let shutdown = tokio_util::sync::CancellationToken::new();
    let network_watch = tokio::spawn(network::watch(Arc::downgrade(&state), shutdown.clone()));
    // A listener on a LAN address (the router build) serves TVs directly.
    let lan = !config.listen.ip().is_loopback();
    let parent_gone = tokio_util::sync::CancellationToken::new();
    if config.exit_on_stdin_close {
        let gone = parent_gone.clone();
        std::thread::spawn(move || {
            let mut buffer = [0u8; 256];
            while let Ok(read) = std::io::Read::read(&mut std::io::stdin(), &mut buffer) {
                if read == 0 {
                    break;
                }
            }
            gone.cancel();
            // A player still holding a stream must not keep an orphan alive.
            std::thread::sleep(Duration::from_secs(10));
            std::process::exit(0);
        });
    }
    axum::serve(
        listener,
        server::router(state.clone(), lan).into_make_service_with_connect_info::<SocketAddr>(),
    )
    .with_graceful_shutdown(async move {
        let signal = async {
            #[cfg(unix)]
            {
                let mut term =
                    tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
                        .expect("signal handler");
                tokio::select! { _ = tokio::signal::ctrl_c() => {}, _ = term.recv() => {} }
            }
            #[cfg(not(unix))]
            {
                let _ = tokio::signal::ctrl_c().await;
            }
        };
        tokio::select! { _ = signal => {}, _ = parent_gone.cancelled() => {} }
        shutdown.cancel();
        state.tv.lock().await.stop();
        if let Some(active) = state.active.lock().await.as_ref() {
            active.cancel.cancel();
        }
    })
    .await?;
    idle.abort();
    network_watch.abort();
    session.stop().await;
    Ok(())
}
