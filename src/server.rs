use crate::{Config, cache::RamFactory};
use anyhow::{Context, Result, ensure};
use axum::{
    Json, Router,
    body::{Body, Bytes},
    extract::{DefaultBodyLimit, FromRequest, Path, Query, Request, State},
    http::{HeaderMap, Method, StatusCode, header},
    middleware::{self, Next},
    response::{Html, IntoResponse, Response},
    routing::{get, post},
};
use futures::StreamExt;
use librqbit::{AddTorrent, AddTorrentOptions, ManagedTorrent, Session};
use serde::{Deserialize, de::DeserializeOwned};
use serde_json::{Value, json};
use std::{
    collections::HashMap,
    io::SeekFrom,
    net::IpAddr,
    sync::{
        Arc, Weak,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
use tokio::{
    io::{AsyncReadExt, AsyncSeekExt},
    sync::{Mutex, Semaphore},
};
use tokio_util::sync::CancellationToken;

const MAX_METADATA: usize = 2 * 1024 * 1024;
const IO_CHUNK: usize = 64 * 1024;
pub struct AppState {
    pub session: Arc<Session>,
    pub factory: RamFactory,
    pub config: Config,
    pub active: Mutex<Option<Active>>,
    pub operation: Mutex<()>,
    pub started: Instant,
    pub client: reqwest::Client,
    pub tv: Mutex<crate::network::TvAccess>,
}
#[derive(Clone)]
pub struct Active {
    handle: Arc<ManagedTorrent>,
    pub cancel: CancellationToken,
    readers: Arc<Semaphore>,
    max_readers: usize,
    touched: Arc<AtomicU64>,
    title: String,
    poster: String,
    category: String,
    data: String,
    timestamp: u64,
    viewed: Arc<parking_lot::Mutex<HashMap<usize, f64>>>,
    preload: Arc<parking_lot::Mutex<Option<Arc<Preload>>>>,
}
struct Preload {
    file: usize,
    target: u64,
    loaded: AtomicU64,
    running: AtomicBool,
    cancel: CancellationToken,
    error: parking_lot::Mutex<Option<String>>,
}
pub struct ApiError(StatusCode, String);
impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (self.0, Json(json!({"error":self.1}))).into_response()
    }
}
impl From<anyhow::Error> for ApiError {
    fn from(e: anyhow::Error) -> Self {
        Self(StatusCode::BAD_REQUEST, format!("{e:#}"))
    }
}
type ApiResult<T> = std::result::Result<T, ApiError>;

// Lampa passes a JSON string to jQuery without overriding its form content
// type. TorrServer's ShouldBindJSON accepts that body; native API routes keep
// Axum's strict application/json requirement.
struct CompatJson<T>(T);
impl<S: Send + Sync, T: DeserializeOwned> FromRequest<S> for CompatJson<T> {
    type Rejection = Response;
    async fn from_request(req: Request, state: &S) -> Result<Self, Self::Rejection> {
        let bytes = Bytes::from_request(req, state)
            .await
            .map_err(IntoResponse::into_response)?;
        serde_json::from_slice(&bytes).map(Self).map_err(|error| {
            (
                StatusCode::BAD_REQUEST,
                Json(json!({"error":format!("Invalid JSON: {error}")})),
            )
                .into_response()
        })
    }
}
fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}
fn conflict(s: &str) -> ApiError {
    ApiError(StatusCode::CONFLICT, s.into())
}

fn link_matches(link: &str, hash: &str) -> bool {
    link.eq_ignore_ascii_case(hash)
        || librqbit::Magnet::parse(link)
            .ok()
            .and_then(|m| m.as_id20())
            .is_some_and(|id| id.as_string() == hash)
}

/// `lan` is true for listeners that face the home network: TV access, or `--listen`
/// on a non-loopback address. Only they accept the opaque origin of TV apps.
pub fn router(state: Arc<AppState>, lan: bool) -> Router {
    let guard = Guard {
        state: state.clone(),
        lan,
    };
    Router::new()
        .route(
            "/",
            get(|| async { Html(include_str!("../ui/index.html")) }),
        )
        .route(
            "/styles.css",
            get(|| async {
                (
                    [(header::CONTENT_TYPE, "text/css; charset=utf-8")],
                    include_str!("../ui/styles.css"),
                )
            }),
        )
        .route(
            "/app.js",
            get(|| async {
                (
                    [(header::CONTENT_TYPE, "text/javascript; charset=utf-8")],
                    include_str!("../ui/app.js"),
                )
            }),
        )
        .route(
            "/echo",
            get(|| async { format!("TorrStreamLite.{}", env!("CARGO_PKG_VERSION")) }),
        )
        .route("/api/status", get(status))
        .route("/api/peers", get(peer_details))
        .route(
            "/peer-rates.js",
            get(|| async {
                (
                    [(header::CONTENT_TYPE, "text/javascript; charset=utf-8")],
                    include_str!("../ui/peer-rates.js"),
                )
            }),
        )
        .route(
            "/licenses/app",
            get(|| async {
                (
                    [(header::CONTENT_TYPE, "text/plain; charset=utf-8")],
                    include_str!("../LICENSE"),
                )
            }),
        )
        .route(
            "/licenses/third-party",
            get(|| async {
                (
                    [(header::CONTENT_TYPE, "text/plain; charset=utf-8")],
                    concat!(
                        include_str!("../THIRD_PARTY_NOTICES.txt"),
                        "\n\n",
                        include_str!("../licenses/Apache-2.0.txt")
                    ),
                )
            }),
        )
        .route(
            "/api/network",
            get(crate::network::status).post(crate::network::update),
        )
        .route("/api/torrents", post(add_json).delete(remove_active))
        .route("/api/torrents/file", post(add_file))
        .route("/api/stream/{file}", get(native_stream))
        .route("/torrents", post(compat_torrents))
        .route("/settings", post(settings))
        .route("/cache", post(compat_cache))
        .route("/viewed", post(compat_viewed))
        .route("/stream", get(compat_stream))
        .route("/stream/{name}", get(compat_stream))
        .layer(DefaultBodyLimit::max(MAX_METADATA))
        .layer(middleware::from_fn_with_state(guard, request_guard))
        .with_state(state)
}

#[derive(Clone)]
struct Guard {
    state: Arc<AppState>,
    lan: bool,
}

async fn request_guard(State(guard): State<Guard>, req: Request, next: Next) -> Response {
    let config = &guard.state.config;
    let host = req
        .headers()
        .get(header::HOST)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_owned();
    // Clients without a Host header aren't browsers, so they can't be a rebound page.
    if !host.is_empty() && !host_allowed(&host, &config.allow_host) {
        return (
            StatusCode::FORBIDDEN,
            "Unknown host name. Use the server's IP address, or start it with --allow-host <name>",
        )
            .into_response();
    }
    let private_network = req
        .headers()
        .get("access-control-request-private-network")
        .is_some_and(|value| value == "true");
    let origin = req
        .headers()
        .get(header::ORIGIN)
        .and_then(|v| v.to_str().ok())
        .map(str::to_owned);
    if let Some(origin) = &origin
        && !origin_allowed(
            origin,
            &host,
            req.uri().path(),
            &config.allow_origin,
            guard.lan,
        )
    {
        return (StatusCode::FORBIDDEN, "Browser origin is not allowed").into_response();
    }
    let mut response = if req.method() == Method::OPTIONS {
        StatusCode::NO_CONTENT.into_response()
    } else {
        next.run(req).await
    };
    if let Some(origin) = origin {
        if let Ok(v) = origin.parse() {
            response
                .headers_mut()
                .insert(header::ACCESS_CONTROL_ALLOW_ORIGIN, v);
        }
        response.headers_mut().insert(
            header::ACCESS_CONTROL_ALLOW_METHODS,
            "GET, HEAD, POST, DELETE, OPTIONS".parse().unwrap(),
        );
        response.headers_mut().insert(
            header::ACCESS_CONTROL_ALLOW_HEADERS,
            "Content-Type, Range, Authorization, X-Requested-With"
                .parse()
                .unwrap(),
        );
        response.headers_mut().insert(
            header::ACCESS_CONTROL_EXPOSE_HEADERS,
            "Content-Length, Content-Range, Accept-Ranges"
                .parse()
                .unwrap(),
        );
        if private_network {
            response.headers_mut().insert(
                "access-control-allow-private-network",
                "true".parse().unwrap(),
            );
        }
        response
            .headers_mut()
            .insert(header::VARY, "Origin".parse().unwrap());
    }
    response
}

fn origin_allowed(origin: &str, host: &str, path: &str, configured: &[String], lan: bool) -> bool {
    if origin == format!("http://{host}") || configured.iter().any(|item| item == origin) {
        return true;
    }
    // Lampa's defaults apply only to the compatibility API, never to network
    // controls or /api/*. Packaged TV apps have the opaque "null" origin, but so
    // does any web page's sandboxed frame. It is accepted only on listeners that
    // face the home network, where the TV is, never on the Mac's loopback server.
    let compat = matches!(
        path,
        "/" | "/echo" | "/settings" | "/torrents" | "/cache" | "/viewed" | "/stream"
    ) || path.starts_with("/stream/");
    compat
        && match origin {
            "null" => lan,
            "http://lampa.mx" | "https://lampa.mx" | "http://lampa.app" | "https://lampa.app" => {
                true
            }
            _ => false,
        }
}

/// A page that rebinds its own domain to this server still sends that domain as
/// the Host. Only IP addresses, localhost, mDNS `.local` names and names passed
/// with `--allow-host` are accepted; none of these can be rebound from outside.
fn host_allowed(host: &str, allowed: &[String]) -> bool {
    let name = host_name(host);
    let name = name.strip_suffix('.').unwrap_or(name).to_ascii_lowercase();
    !name.is_empty()
        && (name.parse::<IpAddr>().is_ok()
            || name == "localhost"
            || name.ends_with(".localhost")
            || name.ends_with(".local")
            || allowed.iter().any(|item| item.eq_ignore_ascii_case(&name)))
}

/// The name in a Host header: `[::1]:8090` → `::1`, `mac.local:8090` → `mac.local`.
fn host_name(host: &str) -> &str {
    match host.strip_prefix('[') {
        Some(rest) => rest.split_once(']').map_or("", |(ip, _)| ip),
        None => host.rsplit_once(':').map_or(host, |(name, _)| name),
    }
}

/// Browsers send fetch metadata; players and the native code of TV apps don't.
/// Anything but a same-origin request or one the user typed counts as cross-site.
fn cross_site(headers: &HeaderMap) -> bool {
    headers
        .get("sec-fetch-site")
        .is_some_and(|value| !matches!(value.as_bytes(), b"same-origin" | b"none"))
}

fn active_json(a: &Active) -> Value {
    let stats = a.handle.stats();
    let files = a
        .handle
        .with_metadata(|m| {
            m.file_infos
                .iter()
                .enumerate()
                .map(|(i, f)| {
                    json!({
                        "id":i+1, "path":f.relative_filename.to_string_lossy(), "length":f.len,
                        "stream_url":format!("/api/stream/{}",i+1)
                    })
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let live = serde_json::to_value(&stats.live).unwrap_or_default();
    let speed = live
        .pointer("/download_speed/mbps")
        .and_then(Value::as_f64)
        .unwrap_or(0.0)
        * 1024.0
        * 1024.0;
    let preload = a.preload.lock().clone();
    let warming = preload
        .as_ref()
        .is_some_and(|p| p.running.load(Ordering::Relaxed));
    let preload_error = preload.as_ref().and_then(|p| p.error.lock().clone());
    let failed = stats.error.is_some() || preload_error.is_some();
    let torrent_size = a
        .handle
        .with_metadata(|m| m.lengths().total_length())
        .unwrap_or(0);
    let peers = stats.live.as_ref().map(|s| &s.snapshot.peer_stats);
    json!({"title":a.title, "name":a.handle.name(), "hash":a.handle.info_hash().as_string(), "poster":a.poster,
        "category":a.category, "data":a.data, "timestamp":a.timestamp,
        "stat":if failed {4} else if warming {2} else {3}, "stat_string":if failed {"Torrent error"} else if warming {"Torrent preload"} else {"Torrent working"},
        "preload_size":preload.as_ref().map_or(0, |p| p.target),
        "preloaded_bytes":preload.as_ref().map_or(0, |p| p.loaded.load(Ordering::Relaxed)),
        "preload_error":preload_error,
        "torrent_size":torrent_size, "loaded_size":cached_bytes(a), "download_speed":speed,
        "active_peers":peers.map_or(0, |p| p.live),
        "total_peers":peers.map_or(0, |p| p.live + p.queued + p.connecting),
        "file_stats":files, "engine":stats, "active_readers":a.max_readers-a.readers.available_permits()})
}
fn cached_bytes(a: &Active) -> u64 {
    // Engine progress tracks selected download files; RAM mode selects none.
    // HAVE accounting measures verified cache contents and subtracts evictions.
    a.handle
        .live()
        .and_then(|live| live.get_hns())
        .map_or(0, |hns| hns.have_bytes)
}
async fn status(State(state): State<Arc<AppState>>) -> Json<Value> {
    let active = state.active.lock().await.as_ref().map(active_json);
    Json(
        json!({"server":"TorrStream Lite", "version":env!("CARGO_PKG_VERSION"), "mode":"RAM only", "resident_bytes":crate::memory::resident_bytes(),
        "cache":state.factory.snapshot(), "peer_limit":state.config.peers, "uptime_seconds":state.started.elapsed().as_secs(),
        "torrent":active, "resolving":state.operation.try_lock().is_err()}),
    )
}
async fn peer_details(State(state): State<Arc<AppState>>) -> Response {
    let active = state.active.lock().await.clone();
    let session = active
        .as_ref()
        .map(|a| format!("{}:{}", a.handle.info_hash().as_string(), a.handle.id()));
    // Connected peers only. No history or sampling task is retained on the
    // server; the open UI computes rates from counters and monotonic time.
    let mut peers = active
        .as_ref()
        .and_then(|a| a.handle.live())
        .map(|live| {
            live.per_peer_stats_snapshot(Default::default())
                .peers
                .into_iter()
                .take(state.config.peers as usize)
                .map(|(address, peer)| {
                    json!({
                        "address":address, "client":peer.client_name,
                        "received_bytes":peer.counters.fetched_bytes,
                        "connections":peer.counters.connections
                    })
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    peers.sort_by(|a, b| a["address"].as_str().cmp(&b["address"].as_str()));
    (
        [(header::CACHE_CONTROL, "no-store")],
        Json(json!({
            "session":session, "sampled_at_ms":state.started.elapsed().as_millis(),
            "peer_limit":state.config.peers, "peers":peers
        })),
    )
        .into_response()
}
#[derive(Default, Deserialize)]
struct AddRequest {
    #[serde(default)]
    link: String,
    #[serde(default)]
    title: String,
    #[serde(default)]
    poster: String,
    #[serde(default)]
    category: String,
    #[serde(default)]
    data: String,
}
async fn input_from_link(state: &AppState, link: &str) -> Result<AddTorrent<'static>> {
    ensure!(link.len() <= 16 * 1024, "Link is too long");
    if link.starts_with("magnet:?") {
        return Ok(AddTorrent::from_url(link.to_owned()));
    }
    let url = url::Url::parse(link).context("Use a magnet link or an HTTP(S) .torrent URL")?;
    ensure!(
        matches!(url.scheme(), "http" | "https"),
        "Only magnet and HTTP(S) links are supported"
    );
    let response = state.client.get(url).send().await?.error_for_status()?;
    ensure!(
        response.content_length().unwrap_or(0) <= MAX_METADATA as u64,
        "Torrent metadata exceeds 2 MiB"
    );
    let mut parts = response.bytes_stream();
    let mut data = Vec::new();
    while let Some(part) = parts.next().await {
        let part = part?;
        ensure!(
            data.len() + part.len() <= MAX_METADATA,
            "Torrent metadata exceeds 2 MiB"
        );
        data.extend_from_slice(&part);
    }
    Ok(AddTorrent::TorrentFileBytes(data.into()))
}
async fn add(state: &Arc<AppState>, req: AddRequest, bytes: Option<Bytes>) -> ApiResult<Value> {
    let _op = state
        .operation
        .try_lock()
        .map_err(|_| conflict("Another add/stop operation is running"))?;
    if let Some(a) = state.active.lock().await.as_ref() {
        let hash = a.handle.info_hash().as_string();
        if link_matches(&req.link, &hash) {
            return Ok(active_json(a));
        }
        return Err(conflict(
            "Stop the current torrent before adding another; the RAM budget is shared",
        ));
    }
    if state.factory.is_occupied() {
        return Err(conflict(
            "Previous torrent is still releasing memory; retry shortly",
        ));
    }
    let input = match bytes {
        Some(b) => AddTorrent::TorrentFileBytes(b),
        None => input_from_link(state, &req.link).await?,
    };
    let result = tokio::time::timeout(
        Duration::from_secs(90),
        state.session.add_torrent(
            input,
            Some(AddTorrentOptions {
                only_files: Some(Vec::new()),
                overwrite: true,
                ..Default::default()
            }),
        ),
    )
    .await;
    let handle = match result {
        Ok(Ok(response)) => response.into_handle().context("Torrent was not added")?,
        other => {
            let ids = state
                .session
                .with_torrents(|it| it.map(|(id, _)| id).collect::<Vec<_>>());
            for id in ids {
                let _ = state.session.delete(id.into(), false).await;
            }
            return Err(match other { Ok(Err(e)) => anyhow::anyhow!(e).into(),
                _ => ApiError(StatusCode::GATEWAY_TIMEOUT,"Magnet resolution timed out. Try a .torrent file or a torrent with reachable peers.".into()) });
        }
    };
    if let Err(e) = handle.wait_until_initialized().await {
        let _ = state.session.delete(handle.id().into(), false).await;
        return Err(e.into());
    }
    let max_readers = state.factory.snapshot().slots.saturating_sub(1).clamp(1, 4);
    let active = Active {
        title: if req.title.is_empty() {
            handle.name().unwrap_or_else(|| "Torrent".into())
        } else {
            req.title
        },
        handle,
        cancel: CancellationToken::new(),
        readers: Arc::new(Semaphore::new(max_readers)),
        max_readers,
        touched: Arc::new(AtomicU64::new(now())),
        poster: req.poster,
        category: req.category,
        data: req.data,
        timestamp: now(),
        viewed: Default::default(),
        preload: Default::default(),
    };
    let value = active_json(&active);
    *state.active.lock().await = Some(active);
    Ok(value)
}
async fn add_json(
    State(state): State<Arc<AppState>>,
    Json(req): Json<AddRequest>,
) -> ApiResult<Json<Value>> {
    Ok(Json(add(&state, req, None).await?))
}
async fn add_file(State(state): State<Arc<AppState>>, bytes: Bytes) -> ApiResult<Json<Value>> {
    Ok(Json(add(&state, AddRequest::default(), Some(bytes)).await?))
}
async fn stop(state: &AppState) -> ApiResult<()> {
    let _op = state
        .operation
        .try_lock()
        .map_err(|_| conflict("Another add/stop operation is running"))?;
    if let Some(active) = state.active.lock().await.take() {
        active.cancel.cancel();
        state
            .session
            .delete(active.handle.id().into(), false)
            .await?;
        drop(active);
        for _ in 0..30 {
            if !state.factory.is_occupied() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    }
    Ok(())
}
async fn remove_active(State(state): State<Arc<AppState>>) -> ApiResult<Json<Value>> {
    stop(&state).await?;
    Ok(Json(json!({"ok":true})))
}
pub async fn idle_watchdog(weak: Weak<AppState>) {
    loop {
        tokio::time::sleep(Duration::from_secs(5)).await;
        let Some(state) = weak.upgrade() else {
            return;
        };
        if state.config.idle_seconds == 0 {
            continue;
        }
        let idle = state.active.lock().await.as_ref().is_some_and(|a| {
            a.readers.available_permits() == a.max_readers
                && now().saturating_sub(a.touched.load(Ordering::Relaxed))
                    >= state.config.idle_seconds
        });
        if idle {
            let _ = stop(&state).await;
        }
    }
}
async fn current(state: &AppState) -> ApiResult<Active> {
    state.active.lock().await.clone().ok_or(ApiError(
        StatusCode::NOT_FOUND,
        "Add a torrent first".into(),
    ))
}
async fn native_stream(
    State(state): State<Arc<AppState>>,
    Path(file): Path<usize>,
    method: Method,
    headers: HeaderMap,
) -> ApiResult<Response> {
    stream_file(current(&state).await?, file, method, headers).await
}
fn mime(path: &str) -> &'static str {
    match path
        .rsplit('.')
        .next()
        .unwrap_or("")
        .to_ascii_lowercase()
        .as_str()
    {
        "mp4" | "m4v" => "video/mp4",
        "mkv" => "video/x-matroska",
        "webm" => "video/webm",
        "ts" | "m2ts" => "video/mp2t",
        "avi" => "video/x-msvideo",
        "mp3" => "audio/mpeg",
        "flac" => "audio/flac",
        _ => "application/octet-stream",
    }
}
async fn stream_file(
    active: Active,
    one_based: usize,
    method: Method,
    headers: HeaderMap,
) -> ApiResult<Response> {
    let file = one_based
        .checked_sub(1)
        .ok_or_else(|| ApiError(StatusCode::BAD_REQUEST, "File indices start at 1".into()))?;
    let (size, name) = active
        .handle
        .with_metadata(|m| {
            m.file_infos
                .get(file)
                .map(|f| (f.len, f.relative_filename.to_string_lossy().into_owned()))
        })?
        .ok_or_else(|| ApiError(StatusCode::NOT_FOUND, "File not found".into()))?;
    let range = match crate::range::parse(&headers, size) {
        Ok(r) => r,
        Err(()) => {
            return Ok(Response::builder()
                .status(StatusCode::RANGE_NOT_SATISFIABLE)
                .header(header::CONTENT_RANGE, format!("bytes */{size}"))
                .body(Body::empty())
                .unwrap());
        }
    };
    let mut builder = Response::builder()
        .status(if range.partial {
            StatusCode::PARTIAL_CONTENT
        } else {
            StatusCode::OK
        })
        .header(header::ACCEPT_RANGES, "bytes")
        .header(header::CONTENT_LENGTH, range.len)
        .header(header::CONTENT_TYPE, mime(&name))
        .header(header::CACHE_CONTROL, "no-store");
    if range.partial {
        builder = builder.header(
            header::CONTENT_RANGE,
            format!(
                "bytes {}-{}/{size}",
                range.start,
                range.start + range.len - 1
            ),
        );
    }
    if method == Method::HEAD || range.len == 0 {
        return Ok(builder.body(Body::empty()).unwrap());
    }
    if let Some(preload) = active.preload.lock().as_ref() {
        preload.cancel.cancel();
    }
    let permit = active.readers.clone().try_acquire_owned().map_err(|_| {
        ApiError(
            StatusCode::TOO_MANY_REQUESTS,
            "Too many simultaneous range requests for this cache size".into(),
        )
    })?;
    active.touched.store(now(), Ordering::Relaxed);
    let mut reader = active.handle.clone().stream(file).await?;
    reader
        .seek(SeekFrom::Start(range.start))
        .await
        .map_err(anyhow::Error::from)?;
    // One queued chunk bounds HTTP buffering. The producer observes cancellation
    // even when a slow player stops polling the response body.
    let (tx, mut rx) = tokio::sync::mpsc::channel::<std::io::Result<Bytes>>(1);
    tokio::spawn(async move {
        let _permit = permit;
        let result: std::io::Result<()> = async {
            let mut remaining = range.len;
            while remaining > 0 {
                let mut buffer = vec![0u8; IO_CHUNK.min(remaining as usize)];
                let read = tokio::select! {
                    _ = active.cancel.cancelled() => return Ok(()),
                    _ = tx.closed() => return Ok(()),
                    r = tokio::time::timeout(Duration::from_secs(45),reader.read(&mut buffer)) => r.map_err(|_|std::io::Error::new(std::io::ErrorKind::TimedOut,"No torrent data for 45 seconds"))??,
                };
                if read==0 { return Err(std::io::Error::new(std::io::ErrorKind::UnexpectedEof,"Torrent stream ended early")); }
                buffer.truncate(read); remaining-=read as u64; active.touched.store(now(),Ordering::Relaxed);
                tokio::select! {
                    _ = active.cancel.cancelled() => return Ok(()),
                    sent = tx.send(Ok(Bytes::from(buffer))) => if sent.is_err() { return Ok(()); },
                }
            }
            Ok(())
        }.await;
        drop(reader);
        if let Err(e) = result {
            let _ = tx.try_send(Err(e));
        }
    });
    let stream = async_stream::stream! { while let Some(chunk) = rx.recv().await { yield chunk; } };
    let body = Body::from_stream(stream);
    Ok(builder.body(body).unwrap())
}
#[derive(Deserialize)]
struct CompatRequest {
    action: String,
    #[serde(default)]
    hash: String,
    #[serde(flatten)]
    add: AddRequest,
}
async fn compat_torrents(
    State(state): State<Arc<AppState>>,
    CompatJson(req): CompatJson<CompatRequest>,
) -> ApiResult<Json<Value>> {
    match req.action.as_str() {
        "add" => Ok(Json(add(&state, req.add, None).await?)),
        "list" => Ok(Json(
            state
                .active
                .lock()
                .await
                .as_ref()
                .map(|a| json!([active_json(a)]))
                .unwrap_or(json!([])),
        )),
        "get" => {
            let a = current(&state).await?;
            ensure_hash(&a, &req.hash)?;
            Ok(Json(active_json(&a)))
        }
        "rem" | "drop" => {
            let a = current(&state).await?;
            ensure_hash(&a, &req.hash)?;
            drop(a);
            stop(&state).await?;
            Ok(Json(json!({})))
        }
        "set" => {
            let mut g = state.active.lock().await;
            let a = g
                .as_mut()
                .ok_or(ApiError(StatusCode::NOT_FOUND, "Torrent not found".into()))?;
            ensure_hash(a, &req.hash)?;
            a.title = req.add.title;
            a.poster = req.add.poster;
            a.category = req.add.category;
            a.data = req.add.data;
            Ok(Json(active_json(a)))
        }
        _ => Err(ApiError(
            StatusCode::BAD_REQUEST,
            "Supported actions: add, list, get, set, rem, drop".into(),
        )),
    }
}
fn ensure_hash(a: &Active, hash: &str) -> ApiResult<()> {
    if hash != a.handle.info_hash().as_string() {
        return Err(ApiError(StatusCode::NOT_FOUND, "Torrent not found".into()));
    }
    Ok(())
}
async fn settings(
    State(state): State<Arc<AppState>>,
    CompatJson(req): CompatJson<Value>,
) -> ApiResult<Json<Value>> {
    if req.get("action").and_then(Value::as_str) != Some("get") {
        return Err(ApiError(
            StatusCode::UNPROCESSABLE_ENTITY,
            "Change memory and peer limits with startup options; live resizing is disabled".into(),
        ));
    }
    Ok(Json(
        json!({"CacheSize":state.config.cache_mib as usize*1024*1024,"ConnectionsLimit":state.config.peers,"UseDisk":false,"DisableUpload":true}),
    ))
}
async fn compat_cache(
    State(state): State<Arc<AppState>>,
    CompatJson(req): CompatJson<CompatRequest>,
) -> ApiResult<Json<Value>> {
    if req.action != "get" {
        return Err(ApiError(
            StatusCode::BAD_REQUEST,
            "Supported action: get".into(),
        ));
    }
    let active = current(&state).await?;
    ensure_hash(&active, &req.hash)?;
    let cache = state.factory.snapshot();
    let (piece_length, piece_count) = active.handle.with_metadata(|m| {
        (
            m.lengths().default_piece_length(),
            m.lengths().total_pieces(),
        )
    })?;
    Ok(Json(json!({
        "Hash":req.hash, "Capacity":cache.budget_bytes,
        "Filled":cached_bytes(&active),
        "PiecesLength":piece_length, "PiecesCount":piece_count,
        "Torrent":active_json(&active), "Pieces":{}, "Readers":[]
    })))
}

#[derive(Deserialize)]
struct ViewedRequest {
    action: String,
    hash: String,
    #[serde(default)]
    file_index: i64,
    #[serde(default)]
    timecode: f64,
}
async fn compat_viewed(
    State(state): State<Arc<AppState>>,
    CompatJson(req): CompatJson<ViewedRequest>,
) -> ApiResult<Json<Value>> {
    let active = state.active.lock().await.clone();
    let Some(active) = active.filter(|a| a.handle.info_hash().as_string() == req.hash) else {
        return if req.action == "list" {
            Ok(Json(json!([])))
        } else {
            Err(ApiError(StatusCode::NOT_FOUND, "Torrent not found".into()))
        };
    };
    if req.action == "list" {
        let mut items = active.viewed.lock().iter().map(|(&index, &timecode)| {
            json!({"hash":req.hash, "file_index":index, "timecode":timecode})
        }).collect::<Vec<_>>();
        items.sort_by_key(|item| item["file_index"].as_u64());
        return Ok(Json(json!(items)));
    }
    if req.action == "rem" && req.file_index == -1 {
        active.viewed.lock().clear();
        return Ok(Json(json!({})));
    }
    let count = active.handle.with_metadata(|m| m.file_infos.len())?;
    if req.file_index < 1 || req.file_index as usize > count {
        return Err(ApiError(
            StatusCode::BAD_REQUEST,
            "Invalid file index".into(),
        ));
    }
    let index = req.file_index as usize;
    match req.action.as_str() {
        "set" if req.timecode.is_finite() && req.timecode >= 0.0 => {
            // At most one entry per file in the current torrent (4096 maximum).
            active.viewed.lock().insert(index, req.timecode);
        }
        "rem" => {
            active.viewed.lock().remove(&index);
        }
        _ => {
            return Err(ApiError(
                StatusCode::BAD_REQUEST,
                "Invalid viewed action or timecode".into(),
            ));
        }
    }
    Ok(Json(json!({})))
}

fn start_preload(active: &Active, index: usize, cache_bytes: usize) -> ApiResult<()> {
    let file = index.checked_sub(1).context("File indices start at 1")?;
    let size = active
        .handle
        .with_metadata(|m| m.file_infos.get(file).map(|f| f.len))?
        .context("File not found")?;
    let mut slot = active.preload.lock();
    if slot
        .as_ref()
        .is_some_and(|p| p.file == file && !p.cancel.is_cancelled() && p.error.lock().is_none())
    {
        return Ok(());
    }
    let permit = active
        .readers
        .clone()
        .try_acquire_owned()
        .map_err(|_| conflict("Playback requests are already using the cache"))?;
    if let Some(previous) = slot.as_ref() {
        previous.cancel.cancel();
    }
    // Warm a small head window within the existing cache, not a second buffer.
    let preload = Arc::new(Preload {
        file,
        target: size.min(8 * 1024 * 1024).min((cache_bytes / 4) as u64),
        loaded: AtomicU64::new(0),
        running: AtomicBool::new(true),
        cancel: active.cancel.child_token(),
        error: Default::default(),
    });
    *slot = Some(preload.clone());
    let active = active.clone();
    active.touched.store(now(), Ordering::Relaxed);
    tokio::spawn(async move {
        let _permit = permit;
        let result: Result<()> = tokio::select! {
            _ = preload.cancel.cancelled() => Ok(()),
            result = tokio::time::timeout(Duration::from_secs(45), async {
                let mut reader = active.handle.clone().stream(file).await?;
                let mut buffer = vec![0; IO_CHUNK];
                let mut loaded = 0;
                while loaded < preload.target {
                    let length = IO_CHUNK.min((preload.target - loaded) as usize);
                    let n = reader.read(&mut buffer[..length]).await?;
                    ensure!(n > 0, "Torrent stream ended during preload");
                    loaded += n as u64;
                    preload.loaded.store(loaded, Ordering::Relaxed);
                    active.touched.store(now(), Ordering::Relaxed);
                }
                Ok(())
            }) => result.unwrap_or_else(|_| Err(anyhow::anyhow!("Preload timed out after 45 seconds"))),
        };
        if let Err(error) = result {
            *preload.error.lock() = Some(format!("{error:#}"));
        }
        preload.running.store(false, Ordering::Relaxed);
    });
    Ok(())
}

async fn compat_stream(
    State(state): State<Arc<AppState>>,
    Query(q): Query<HashMap<String, String>>,
    method: Method,
    headers: HeaderMap,
) -> ApiResult<Response> {
    let link = q.get("link").context("Missing link")?;
    let existing = state.active.lock().await.clone();
    let matches = existing.as_ref().is_some_and(|a| {
        let h = a.handle.info_hash().as_string();
        link_matches(link, &h)
    });
    if !matches {
        // An <img> or <video> tag on another site sends no Origin header. Loading a
        // stream URL that way must not make this computer join a new swarm.
        if cross_site(&headers) {
            return Err(ApiError(
                StatusCode::FORBIDDEN,
                "Add the torrent first with POST /torrents; a stream URL loaded from another site can't add one".into(),
            ));
        }
        add(
            &state,
            AddRequest {
                link: link.clone(),
                ..Default::default()
            },
            None,
        )
        .await?;
    }
    let active = current(&state).await?;
    if q.contains_key("stat") {
        return Ok(Json(active_json(&active)).into_response());
    }
    if q.contains_key("m3u") {
        let host = headers
            .get(header::HOST)
            .and_then(|h| h.to_str().ok())
            .unwrap_or("127.0.0.1:8090");
        let mut playlist = String::from("#EXTM3U\n");
        active.handle.with_metadata(|m| {
            for (i, f) in m.file_infos.iter().enumerate() {
                let name = f
                    .relative_filename
                    .to_string_lossy()
                    .replace(['\r', '\n'], " ");
                playlist.push_str(&format!(
                    "#EXTINF:-1,{name}\nhttp://{host}/stream?link={}&index={}&play\n",
                    active.handle.info_hash().as_string(),
                    i + 1
                ));
            }
        })?;
        return Ok(([(header::CONTENT_TYPE, "audio/x-mpegurl")], playlist).into_response());
    }
    let index = q
        .get("index")
        .map(|s| s.parse::<usize>())
        .transpose()
        .map_err(anyhow::Error::from)?
        .unwrap_or(1);
    if q.contains_key("preload") {
        start_preload(&active, index, state.factory.snapshot().budget_bytes)?;
        return Ok(Json(active_json(&active)).into_response());
    }
    stream_file(active, index, method, headers).await
}

#[cfg(test)]
mod tests {
    use super::*;

    const NONE: [String; 0] = [];

    #[test]
    fn null_origin_only_on_home_network_listeners() {
        assert!(!origin_allowed(
            "null",
            "127.0.0.1:8090",
            "/torrents",
            &NONE,
            false
        ));
        assert!(!origin_allowed(
            "null",
            "127.0.0.1:8090",
            "/stream",
            &NONE,
            false
        ));
        for path in [
            "/",
            "/echo",
            "/settings",
            "/torrents",
            "/cache",
            "/viewed",
            "/stream",
            "/stream/a.mkv",
        ] {
            assert!(
                origin_allowed("null", "192.168.1.35:8090", path, &NONE, true),
                "{path}"
            );
        }
        for path in [
            "/api/torrents",
            "/api/network",
            "/api/status",
            "/licenses/app",
        ] {
            assert!(
                !origin_allowed("null", "192.168.1.35:8090", path, &NONE, true),
                "{path}"
            );
        }
    }

    #[test]
    fn lampa_web_origins_reach_only_the_compatibility_api() {
        for origin in [
            "http://lampa.mx",
            "https://lampa.mx",
            "http://lampa.app",
            "https://lampa.app",
        ] {
            assert!(origin_allowed(
                origin,
                "127.0.0.1:8090",
                "/settings",
                &NONE,
                false
            ));
            assert!(!origin_allowed(
                origin,
                "127.0.0.1:8090",
                "/api/network",
                &NONE,
                true
            ));
        }
        for origin in ["https://lampa.mx.example", "https://example.com", "file://"] {
            assert!(
                !origin_allowed(origin, "192.168.1.35:8090", "/settings", &NONE, true),
                "{origin}"
            );
        }
        assert!(origin_allowed(
            "http://127.0.0.1:8090",
            "127.0.0.1:8090",
            "/api/network",
            &NONE,
            false
        ));
        let configured = ["https://player.example".to_owned()];
        assert!(origin_allowed(
            "https://player.example",
            "127.0.0.1:8090",
            "/api/status",
            &configured,
            false
        ));
    }

    #[test]
    fn refuses_host_names_a_rebound_page_would_send() {
        for host in [
            "127.0.0.1:8090",
            "[::1]:8090",
            "192.168.1.35:8090",
            "0.0.0.0:8090",
            "localhost:8090",
            "LOCALHOST",
            "app.localhost:8090",
            "macbook.local:8090",
            "MacBook.local.:8090",
        ] {
            assert!(host_allowed(host, &NONE), "{host}");
        }
        for host in [
            "",
            "evil.example:8090",
            "127.0.0.1.evil.example:8090",
            "localhost.evil.example",
            "local:8090",
            "[::1:8090",
            "my.keenetic.net:8090",
        ] {
            assert!(!host_allowed(host, &NONE), "{host}");
        }
        let allowed = ["my.keenetic.net".to_owned()];
        assert!(host_allowed("My.Keenetic.Net:8090", &allowed));
        assert!(!host_allowed("keenetic.net", &allowed));
    }

    #[test]
    fn only_same_origin_or_typed_requests_count_as_same_site() {
        let mut headers = HeaderMap::new();
        assert!(!cross_site(&headers), "players send no fetch metadata");
        for (value, expected) in [
            ("same-origin", false),
            ("none", false),
            ("same-site", true),
            ("cross-site", true),
        ] {
            headers.insert("sec-fetch-site", value.parse().unwrap());
            assert_eq!(cross_site(&headers), expected, "{value}");
        }
    }
}
