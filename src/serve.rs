//! Serves the router on one listener, like `axum::serve`, with the limits it doesn't have.
//!
//! - A connection has 30 s to send a request's headers. The same timer runs while it's idle
//!   between requests, so a client that went away without closing (a TV switched off)
//!   doesn't leave its connection open for good. Clients open a new one for their next
//!   request. A response in progress has no limit: a paused player may wait for hours.
//! - At most 64 connections are served at once on a listener. More wait in the system's
//!   queue until one ends, so each listener's tasks and buffers stay bounded.
//! - A connection that ends while the client hasn't taken all that was sent keeps its
//!   socket, and its place among the 64, until the client has. A response is finished for
//!   the server once its last byte is in the system's buffer; a paused player may read it
//!   much later, and the system throws away what a closed socket couldn't deliver.
use axum::{Router, extract::ConnectInfo};
use hyper_util::rt::{TokioIo, TokioTimer};
use std::{
    future::Future,
    io::{self, ErrorKind},
    pin::{Pin, pin},
    sync::Arc,
    task::{Context, Poll},
    time::Duration,
};
use tokio::{
    io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt, ReadBuf},
    net::{TcpListener, TcpStream},
    sync::{OwnedSemaphorePermit, Semaphore},
};
use tokio_util::sync::CancellationToken;
use tower_service::Service as _;

/// How often a socket kept for an unread response is looked at again.
const LINGER_CHECK: Duration = Duration::from_millis(250);

#[derive(Clone, Copy)]
pub struct Limits {
    /// For a request's headers to arrive, counted from the connection's start or from the
    /// end of the response before.
    pub header_timeout: Duration,
    /// Connections served at once.
    pub connections: usize,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            header_timeout: Duration::from_secs(30),
            connections: 64,
        }
    }
}

/// Serves `app` until `shutdown` completes, then lets the requests in progress finish.
/// Handlers get the client's address as `ConnectInfo<SocketAddr>`.
pub async fn serve(
    listener: TcpListener,
    app: Router,
    limits: Limits,
    shutdown: impl Future<Output = ()>,
) {
    let slots = Arc::new(Semaphore::new(limits.connections));
    let closing = CancellationToken::new();
    let mut shutdown = pin!(shutdown);
    loop {
        // A free slot comes first: connections over the limit stay in the system's queue.
        let slot = tokio::select! {
            slot = slots.clone().acquire_owned() => slot.expect("the semaphore is never closed"),
            _ = &mut shutdown => break,
        };
        let (stream, peer) = tokio::select! {
            accepted = listener.accept() => match accepted {
                Ok(connection) => connection,
                // The client gave up before it was accepted.
                Err(error) if matches!(
                    error.kind(),
                    ErrorKind::ConnectionRefused | ErrorKind::ConnectionAborted | ErrorKind::ConnectionReset
                ) => continue,
                Err(error) => {
                    // Out of file descriptors, most likely: wait for some to be closed.
                    tracing::warn!(%error, "Couldn't accept a connection");
                    tokio::time::sleep(Duration::from_secs(1)).await;
                    continue;
                }
            },
            _ = &mut shutdown => break,
        };
        let app = app.clone();
        let closing = closing.clone();
        let socket = Socket {
            stream: Some(stream),
            slot: Some(slot),
            closing: closing.clone(),
        };
        tokio::spawn(async move {
            let service = hyper::service::service_fn(
                move |mut request: hyper::Request<hyper::body::Incoming>| {
                    request.extensions_mut().insert(ConnectInfo(peer));
                    // The router is always ready, and a clone shares its routes.
                    app.clone().call(request)
                },
            );
            let mut connection = pin!(
                hyper::server::conn::http1::Builder::new()
                    .timer(TokioTimer::new())
                    .header_read_timeout(limits.header_timeout)
                    .serve_connection(TokioIo::new(socket), service)
            );
            tokio::select! {
                // An error here is the client's: it went away, or sent something that
                // isn't HTTP, or ran out of time.
                _ = connection.as_mut() => {}
                _ = closing.cancelled() => {
                    connection.as_mut().graceful_shutdown();
                    let _ = connection.await;
                }
            }
        });
    }
    // Stop accepting, then wait until every connection has finished its request and closed.
    drop(listener);
    closing.cancel();
    let _ = slots.acquire_many(limits.connections as u32).await;
}

/// A connection's socket, with its place among the listener's connections. Dropped with
/// data the client hasn't taken yet, it hands both to `linger` instead of closing.
struct Socket {
    stream: Option<TcpStream>,
    slot: Option<OwnedSemaphorePermit>,
    closing: CancellationToken,
}

impl Socket {
    fn stream(&mut self) -> Pin<&mut TcpStream> {
        Pin::new(self.stream.as_mut().expect("there until dropped"))
    }
}

impl AsyncRead for Socket {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buffer: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        self.stream().poll_read(cx, buffer)
    }
}

impl AsyncWrite for Socket {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buffer: &[u8],
    ) -> Poll<io::Result<usize>> {
        self.stream().poll_write(cx, buffer)
    }

    // Headers and body go out in one call, as with the stream itself.
    fn poll_write_vectored(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buffers: &[io::IoSlice<'_>],
    ) -> Poll<io::Result<usize>> {
        self.stream().poll_write_vectored(cx, buffers)
    }

    fn is_write_vectored(&self) -> bool {
        self.stream
            .as_ref()
            .is_some_and(TcpStream::is_write_vectored)
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        self.stream().poll_flush(cx)
    }

    fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        self.stream().poll_shutdown(cx)
    }
}

impl Drop for Socket {
    fn drop(&mut self) {
        let Some(stream) = self.stream.take() else {
            return;
        };
        // The client has everything, or the server is stopping: close now.
        let waiting = unsent(&stream);
        if self.closing.is_cancelled() || waiting == 0 {
            return;
        }
        if let Ok(runtime) = tokio::runtime::Handle::try_current() {
            tracing::debug!(
                waiting,
                "A connection ended with data its client hasn't taken; keeping its socket"
            );
            runtime.spawn(linger(stream, self.slot.take(), self.closing.clone()));
        }
    }
}

/// Keeps a socket whose connection has ended until the client has taken what was sent, has
/// gone, or the server stops. A paused player answers the system's probes for as long as
/// it's there, so its socket lives as long as it did when nothing closed idle connections.
async fn linger(
    mut stream: TcpStream,
    _slot: Option<OwnedSemaphorePermit>,
    closing: CancellationToken,
) {
    // No more requests here: after the last byte the client sees the connection end.
    let _ = stream.shutdown().await;
    let mut ignored = [0u8; 512];
    loop {
        tokio::select! {
            _ = closing.cancelled() => return,
            read = stream.read(&mut ignored) => {
                // It closed its side, or the system gave up on it.
                if !matches!(read, Ok(length) if length > 0) {
                    tracing::debug!(left = unsent(&stream), "The client of a kept socket closed it");
                    return;
                }
            }
            _ = tokio::time::sleep(LINGER_CHECK) => {
                if unsent(&stream) == 0 {
                    tracing::debug!("The client of a kept socket has taken everything");
                    return;
                }
            }
        }
    }
}

/// How much of what was written to the socket the client hasn't acknowledged yet, in
/// bytes. Zero where the system doesn't say.
fn unsent(stream: &TcpStream) -> usize {
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    {
        use std::os::fd::AsRawFd;
        let mut bytes: libc::c_int = 0;
        #[cfg(target_os = "linux")]
        let known = unsafe { libc::ioctl(stream.as_raw_fd(), libc::TIOCOUTQ, &mut bytes) } == 0;
        #[cfg(target_os = "macos")]
        let known = {
            let mut size = std::mem::size_of::<libc::c_int>() as libc::socklen_t;
            let result = unsafe {
                libc::getsockopt(
                    stream.as_raw_fd(),
                    libc::SOL_SOCKET,
                    libc::SO_NWRITE,
                    &mut bytes as *mut libc::c_int as *mut libc::c_void,
                    &mut size,
                )
            };
            result == 0
        };
        if known {
            return bytes.max(0) as usize;
        }
    }
    let _ = stream;
    0
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{body::Body, routing::get};
    use std::{net::SocketAddr, time::Instant};
    use tokio::{
        io::{AsyncReadExt, AsyncWriteExt},
        net::TcpStream,
        time::{sleep, timeout},
    };

    const REQUEST: &[u8] = b"GET / HTTP/1.1\r\nHost: test\r\n\r\n";

    /// A server with `/` (the caller's address) and `/slow`, whose response pauses for
    /// `pause` in the middle. Returns its address, the token that stops it, and its task.
    async fn start(
        limits: Limits,
        pause: Duration,
    ) -> (SocketAddr, CancellationToken, tokio::task::JoinHandle<()>) {
        let app = Router::new()
            .route(
                "/",
                get(|ConnectInfo(peer): ConnectInfo<SocketAddr>| async move { peer.to_string() }),
            )
            .route(
                "/slow",
                get(move || async move {
                    Body::from_stream(futures::stream::unfold(0, move |part| async move {
                        match part {
                            0 => Some((Ok::<_, std::io::Error>("first,"), 1)),
                            1 => {
                                sleep(pause).await;
                                Some((Ok("second"), 2))
                            }
                            _ => None,
                        }
                    }))
                }),
            );
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let stop = CancellationToken::new();
        let stopped = stop.clone();
        let task = tokio::spawn(async move {
            serve(listener, app, limits, stopped.cancelled_owned()).await;
        });
        (address, stop, task)
    }

    fn limits(header_timeout_ms: u64, connections: usize) -> Limits {
        Limits {
            header_timeout: Duration::from_millis(header_timeout_ms),
            connections,
        }
    }

    /// Reads until the text read so far ends with `end`.
    async fn read_until(stream: &mut TcpStream, end: &str) -> String {
        let mut text = String::new();
        let mut buffer = [0u8; 1024];
        while !text.ends_with(end) {
            let read = timeout(Duration::from_secs(5), stream.read(&mut buffer))
                .await
                .expect("the server answered in time")
                .unwrap();
            assert!(read > 0, "the connection closed after {text:?}");
            text.push_str(std::str::from_utf8(&buffer[..read]).unwrap());
        }
        text
    }

    /// How long until the server closes the connection, which must happen within 5 s.
    async fn closed_after(stream: &mut TcpStream) -> Duration {
        let started = Instant::now();
        let mut buffer = [0u8; 1024];
        loop {
            match timeout(Duration::from_secs(5), stream.read(&mut buffer)).await {
                // A reset closes it as well as an orderly end does.
                Ok(Ok(0)) | Ok(Err(_)) => return started.elapsed(),
                Ok(Ok(_)) => {}
                Err(_) => panic!("the server left the connection open"),
            }
        }
    }

    #[tokio::test]
    async fn handlers_get_the_callers_address() {
        let (address, stop, task) = start(limits(5_000, 4), Duration::ZERO).await;
        let mut client = TcpStream::connect(address).await.unwrap();
        client.write_all(REQUEST).await.unwrap();
        let own = client.local_addr().unwrap().to_string();
        let response = read_until(&mut client, &own).await;
        assert!(response.starts_with("HTTP/1.1 200 OK\r\n"), "{response}");
        stop.cancel();
        task.await.unwrap();
    }

    #[tokio::test]
    async fn a_connection_gets_only_so_long_to_send_its_headers() {
        let (address, _stop, _task) = start(limits(300, 4), Duration::ZERO).await;
        // One that sends nothing, and one that sends a request line and stops.
        let mut silent = TcpStream::connect(address).await.unwrap();
        let mut slow = TcpStream::connect(address).await.unwrap();
        slow.write_all(b"GET / HTTP/1.1\r\nHo").await.unwrap();
        for client in [&mut silent, &mut slow] {
            let waited = closed_after(client).await;
            assert!(waited < Duration::from_secs(3), "closed after {waited:?}");
        }
    }

    #[tokio::test]
    async fn an_idle_connection_is_closed_but_a_response_in_progress_is_not() {
        // The response pauses for four times as long as headers may take.
        let (address, _stop, _task) = start(limits(300, 4), Duration::from_millis(1_200)).await;
        let mut client = TcpStream::connect(address).await.unwrap();
        client
            .write_all(b"GET /slow HTTP/1.1\r\nHost: test\r\n\r\n")
            .await
            .unwrap();
        let started = Instant::now();
        // The end of a chunked body.
        let response = read_until(&mut client, "0\r\n\r\n").await;
        assert!(started.elapsed() >= Duration::from_millis(1_200));
        assert!(
            response.contains("first,") && response.contains("second"),
            "{response}"
        );
        // The same connection takes another request, then is closed for sitting idle.
        client.write_all(REQUEST).await.unwrap();
        let own = client.local_addr().unwrap().to_string();
        read_until(&mut client, &own).await;
        let waited = closed_after(&mut client).await;
        assert!(
            waited >= Duration::from_millis(200) && waited < Duration::from_secs(3),
            "closed after {waited:?}"
        );
    }

    #[tokio::test]
    async fn connections_over_the_limit_wait_for_a_free_slot() {
        let (address, _stop, _task) = start(limits(60_000, 2), Duration::ZERO).await;
        let first = TcpStream::connect(address).await.unwrap();
        let _second = TcpStream::connect(address).await.unwrap();
        // The system accepts the third; the server doesn't serve it yet.
        let mut third = TcpStream::connect(address).await.unwrap();
        third.write_all(REQUEST).await.unwrap();
        let mut buffer = [0u8; 16];
        assert!(
            timeout(Duration::from_millis(500), third.read(&mut buffer))
                .await
                .is_err(),
            "the third connection was served over the limit"
        );
        drop(first);
        let own = third.local_addr().unwrap().to_string();
        let response = read_until(&mut third, &own).await;
        assert!(response.starts_with("HTTP/1.1 200 OK\r\n"), "{response}");
    }

    #[tokio::test]
    async fn stopping_lets_a_response_finish_and_closes_the_rest() {
        let (address, stop, task) = start(limits(60_000, 4), Duration::from_millis(1_500)).await;
        let mut idle = TcpStream::connect(address).await.unwrap();
        let mut busy = TcpStream::connect(address).await.unwrap();
        busy.write_all(b"GET /slow HTTP/1.1\r\nHost: test\r\n\r\n")
            .await
            .unwrap();
        read_until(&mut busy, "first,\r\n").await;
        stop.cancel();
        // The idle connection is closed at once, long before its 60 s are over.
        assert!(closed_after(&mut idle).await < Duration::from_secs(3));
        assert!(!task.is_finished(), "stopped in the middle of a response");
        let rest = read_until(&mut busy, "0\r\n\r\n").await;
        assert!(rest.contains("second"), "{rest}");
        timeout(Duration::from_secs(5), task)
            .await
            .expect("the server stops once the response is done")
            .unwrap();
    }

    /// A connection without HTTP: the client's end and the server's.
    async fn pair() -> (TcpStream, TcpStream) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let client = TcpStream::connect(listener.local_addr().unwrap())
            .await
            .unwrap();
        let (server, _) = listener.accept().await.unwrap();
        (client, server)
    }

    /// Writes to a client that doesn't read until the system takes no more: the client's
    /// buffer is full then, and so is the sender's. Returns how much was written.
    async fn fill(server: &TcpStream) -> usize {
        let chunk = [7u8; 16 * 1024];
        let mut written = 0;
        // Full is when the socket hasn't become writable again for a while.
        while timeout(Duration::from_millis(300), server.writable())
            .await
            .is_ok_and(|ready| ready.is_ok())
        {
            match server.try_write(&chunk) {
                Ok(length) => written += length,
                Err(error) if error.kind() == ErrorKind::WouldBlock => {}
                Err(error) => panic!("{error}"),
            }
        }
        written
    }

    /// The server's end as `serve` holds it, with the only place of a listener.
    fn socket(stream: TcpStream) -> (Socket, Arc<Semaphore>, CancellationToken) {
        let slots = Arc::new(Semaphore::new(1));
        let closing = CancellationToken::new();
        let socket = Socket {
            stream: Some(stream),
            slot: Some(slots.clone().try_acquire_owned().unwrap()),
            closing: closing.clone(),
        };
        (socket, slots, closing)
    }

    #[tokio::test]
    async fn unsent_counts_what_the_client_has_not_taken() {
        let (mut client, server) = pair().await;
        assert_eq!(unsent(&server), 0);
        let written = fill(&server).await;
        let waiting = unsent(&server);
        assert!(waiting > 0 && waiting <= written, "{waiting} of {written}");
        let mut received = vec![0u8; written];
        timeout(Duration::from_secs(10), client.read_exact(&mut received))
            .await
            .expect("what was written arrives")
            .unwrap();
        // The acknowledgement of the last bytes is on its way.
        for _ in 0..100 {
            if unsent(&server) == 0 {
                return;
            }
            sleep(Duration::from_millis(20)).await;
        }
        panic!("{} bytes still count as unsent", unsent(&server));
    }

    #[tokio::test]
    async fn a_socket_with_unread_data_is_kept_until_the_client_has_it() {
        let (mut client, server) = pair().await;
        let written = fill(&server).await;
        let (socket, slots, _closing) = socket(server);
        // The connection ends, as when it sat idle for too long after its response.
        drop(socket);
        sleep(LINGER_CHECK * 3).await;
        assert_eq!(
            slots.available_permits(),
            0,
            "closed with data the client hadn't read"
        );
        // The client reads on: all of it arrives, then the end of the connection.
        let mut received = Vec::new();
        timeout(Duration::from_secs(10), client.read_to_end(&mut received))
            .await
            .expect("the connection ends after the data")
            .unwrap();
        assert_eq!(received.len(), written);
        assert!(received.iter().all(|&byte| byte == 7));
        let _place = timeout(Duration::from_secs(5), slots.acquire())
            .await
            .expect("its place is free again");
    }

    #[tokio::test]
    async fn a_socket_the_client_has_emptied_closes_at_once() {
        let (mut client, server) = pair().await;
        server.writable().await.unwrap();
        assert_eq!(server.try_write(b"hello").unwrap(), 5);
        let mut hello = [0u8; 5];
        client.read_exact(&mut hello).await.unwrap();
        for _ in 0..100 {
            if unsent(&server) == 0 {
                break;
            }
            sleep(Duration::from_millis(20)).await;
        }
        let (socket, slots, _closing) = socket(server);
        drop(socket);
        assert_eq!(slots.available_permits(), 1);
        let end = timeout(Duration::from_secs(5), client.read(&mut hello)).await;
        assert_eq!(end.expect("the connection ends").unwrap(), 0);
    }

    #[tokio::test]
    async fn stopping_lets_go_of_a_kept_socket() {
        let (_client, server) = pair().await;
        fill(&server).await;
        let (socket, slots, closing) = socket(server);
        drop(socket);
        sleep(Duration::from_millis(100)).await;
        assert_eq!(slots.available_permits(), 0);
        closing.cancel();
        let place = timeout(Duration::from_secs(5), slots.clone().acquire_owned())
            .await
            .expect("its place is free again")
            .unwrap();
        // And while the server is stopping, nothing is kept at all.
        let (_other, server) = pair().await;
        fill(&server).await;
        drop(Socket {
            stream: Some(server),
            slot: Some(place),
            closing,
        });
        assert_eq!(slots.available_permits(), 1);
    }

    #[tokio::test]
    async fn a_connection_in_use_stays_open() {
        // Six requests, 250 ms apart: longer in all than headers may take, never idle that long.
        let (address, _stop, _task) = start(limits(600, 4), Duration::ZERO).await;
        let mut client = TcpStream::connect(address).await.unwrap();
        let own = client.local_addr().unwrap().to_string();
        for _ in 0..6 {
            client.write_all(REQUEST).await.unwrap();
            let response = read_until(&mut client, &own).await;
            assert!(response.starts_with("HTTP/1.1 200 OK\r\n"), "{response}");
            sleep(Duration::from_millis(250)).await;
        }
    }
}
