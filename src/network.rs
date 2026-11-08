//! Optional LAN listener. The loopback control server and torrent stay alive
//! when TV access is switched off or the Mac changes networks.
use crate::server::{self, AppState};
use axum::{
    Json,
    extract::{ConnectInfo, Request, State},
    http::StatusCode,
    middleware::{self, Next},
    response::Response,
};
use futures::StreamExt;
use serde::{Deserialize, Serialize};
use std::{
    ffi::CStr,
    net::{Ipv4Addr, SocketAddr},
    sync::{Arc, Weak},
    time::Duration,
};
use tokio::net::TcpListener;
use tokio_util::sync::CancellationToken;

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct LanAddress {
    interface: String,
    ip: Ipv4Addr,
}

fn suitable_interface(name: &str, flags: u32, ip: Ipv4Addr) -> bool {
    flags & libc::IFF_UP as u32 != 0
        && flags & (libc::IFF_LOOPBACK | libc::IFF_POINTOPOINT) as u32 == 0
        && ip.is_private()
        && ![
            "utun",
            "tun",
            "tap",
            "wg",
            "tailscale",
            "ppp",
            "docker",
            "veth",
            "br-",
            "awdl",
            "llw",
        ]
        .iter()
        .any(|prefix| name.starts_with(prefix))
}

pub fn lan_addresses() -> Vec<LanAddress> {
    let mut result = Vec::new();
    // getifaddrs owns this list until freeifaddrs; all strings and addresses
    // are copied before it is freed. AF_INET validates the sockaddr cast.
    unsafe {
        let mut head = std::ptr::null_mut();
        if libc::getifaddrs(&mut head) != 0 {
            return result;
        }
        let mut cursor = head;
        while !cursor.is_null() {
            let entry = &*cursor;
            if !entry.ifa_addr.is_null()
                && !entry.ifa_name.is_null()
                && (*entry.ifa_addr).sa_family as i32 == libc::AF_INET
            {
                let addr = &*(entry.ifa_addr as *const libc::sockaddr_in);
                let ip = Ipv4Addr::from(addr.sin_addr.s_addr.to_ne_bytes());
                let name = CStr::from_ptr(entry.ifa_name)
                    .to_string_lossy()
                    .into_owned();
                if suitable_interface(&name, entry.ifa_flags, ip) {
                    result.push(LanAddress {
                        interface: name,
                        ip,
                    });
                }
            }
            cursor = entry.ifa_next;
        }
        libc::freeifaddrs(head);
    }
    result.sort_by(|a, b| (&a.interface, a.ip).cmp(&(&b.interface, b.ip)));
    result.dedup();
    result
}

struct Listener {
    address: LanAddress,
    cancel: CancellationToken,
    task: tokio::task::JoinHandle<()>,
}

pub struct TvAccess {
    pub enabled: bool,
    pub port: u16,
    listeners: Vec<Listener>,
    errors: Vec<String>,
}

impl TvAccess {
    pub fn new(port: u16) -> Self {
        Self {
            enabled: false,
            port,
            listeners: Vec::new(),
            errors: Vec::new(),
        }
    }

    pub fn stop(&mut self) {
        for listener in self.listeners.drain(..) {
            listener.cancel.cancel();
            // Graceful shutdown stops accepting; the middleware below also
            // closes ongoing TV responses without touching Mac readers.
        }
    }

    async fn reconcile(&mut self, state: &Arc<AppState>) {
        let addresses = if self.enabled {
            lan_addresses()
        } else {
            Vec::new()
        };
        self.listeners.retain(|listener| {
            let keep = addresses.contains(&listener.address) && !listener.task.is_finished();
            if !keep {
                listener.cancel.cancel();
            }
            keep
        });
        self.errors.clear();
        for address in addresses {
            if self.listeners.iter().any(|l| l.address == address) {
                continue;
            }
            let socket = SocketAddr::from((address.ip, self.port));
            match TcpListener::bind(socket).await {
                Ok(listener) => {
                    let cancel = CancellationToken::new();
                    let shutdown = cancel.clone();
                    let app = server::router(state.clone(), true)
                        .layer(middleware::from_fn_with_state(cancel.clone(), tv_response));
                    let task = tokio::spawn(async move {
                        if let Err(error) = axum::serve(
                            listener,
                            app.into_make_service_with_connect_info::<SocketAddr>(),
                        )
                        .with_graceful_shutdown(shutdown.cancelled_owned())
                        .await
                        {
                            tracing::warn!(%socket, %error, "TV listener stopped");
                        }
                    });
                    self.listeners.push(Listener {
                        address,
                        cancel,
                        task,
                    });
                }
                Err(error) => self.errors.push(format!("Cannot open {socket}: {error}")),
            }
        }
    }

    fn snapshot(&self, local_control: bool) -> serde_json::Value {
        let urls: Vec<_> = self
            .listeners
            .iter()
            .filter(|l| !l.task.is_finished())
            .map(|l| {
                serde_json::json!({"interface":l.address.interface,
                "url":format!("http://{}:{}", l.address.ip, self.port)})
            })
            .collect();
        serde_json::json!({"enabled":self.enabled,"port":self.port,"addresses":urls,
            "errors":self.errors,"local_control":local_control})
    }
}

async fn tv_response(
    State(cancel): State<CancellationToken>,
    request: Request,
    next: Next,
) -> Response {
    let response = tokio::select! {
        _ = cancel.cancelled() => return Response::builder().status(StatusCode::SERVICE_UNAVAILABLE)
            .body(axum::body::Body::empty()).unwrap(),
        response = next.run(request) => response,
    };
    let (parts, body) = response.into_parts();
    Response::from_parts(
        parts,
        axum::body::Body::from_stream(body.into_data_stream().take_until(cancel.cancelled_owned())),
    )
}

pub async fn status(
    State(state): State<Arc<AppState>>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
) -> Json<serde_json::Value> {
    Json(state.tv.lock().await.snapshot(peer.ip().is_loopback()))
}

#[derive(Deserialize)]
pub struct Update {
    enabled: bool,
    port: Option<u16>,
}

pub async fn update(
    State(state): State<Arc<AppState>>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    Json(update): Json<Update>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    if !peer.ip().is_loopback() {
        return Err((
            StatusCode::FORBIDDEN,
            Json(serde_json::json!({"error":"Change TV access from the Mac app."})),
        ));
    }
    if update.port.is_some_and(|p| p < 1024) {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error":"Choose a TV port from 1024 to 65535."})),
        ));
    }
    let mut tv = state.tv.lock().await;
    if let Some(port) = update.port
        && port != tv.port
    {
        tv.stop();
        tv.port = port;
    }
    tv.enabled = update.enabled;
    tv.reconcile(&state).await;
    Ok(Json(tv.snapshot(true)))
}

pub async fn watch(state: Weak<AppState>, cancel: CancellationToken) {
    loop {
        tokio::select! {
            _ = cancel.cancelled() => break,
            _ = tokio::time::sleep(Duration::from_secs(3)) => {}
        }
        let Some(state) = state.upgrade() else {
            break;
        };
        state.tv.lock().await.reconcile(&state).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn only_private_lan_interfaces_are_advertised() {
        let up = libc::IFF_UP as u32;
        let home = Ipv4Addr::new(192, 168, 1, 50);
        assert!(suitable_interface("en0", up, home));
        assert!(suitable_interface("eth0", up, Ipv4Addr::new(10, 0, 0, 2)));
        assert!(!suitable_interface("utun7", up, home));
        assert!(!suitable_interface(
            "en0",
            up | libc::IFF_POINTOPOINT as u32,
            home
        ));
        assert!(!suitable_interface(
            "lo0",
            up | libc::IFF_LOOPBACK as u32,
            home
        ));
        assert!(!suitable_interface("en0", 0, home));
        assert!(!suitable_interface("en0", up, Ipv4Addr::new(8, 8, 8, 8)));
    }
}
