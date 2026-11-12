//! Real loopback clients and isolated temporary evidence for bridge contracts.
#![allow(
    clippy::unwrap_used,
    reason = "assertions expose setup or contract failures in tests"
)]
#![allow(
    dead_code,
    reason = "integration targets share helpers but exercise different contracts"
)]
use puzzle_pty_bridge::{
    config::Config,
    metrics::Metrics,
    protocol::{self, Client},
    server::Bridge,
    shutdown::Shutdown,
};
use std::{
    net::{SocketAddr, TcpStream},
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    thread::{self, JoinHandle},
    time::Duration,
};
use tokio_tungstenite::tungstenite::{self, Message, WebSocket, client::IntoClientRequest};

/// Monotonic suffix prevents parallel tests from sharing temporary files.
static NEXT: AtomicU64 = AtomicU64::new(0);

/// Listener runs independently of the synchronous test client.
pub(crate) struct Host {
    /// Token-bearing URL exists only in this test's memory.
    pub url: String,
    /// Actual socket address reported by the bound listener.
    pub address: SocketAddr,
    /// Event counts from the actual production accept and session loops.
    pub metrics: Arc<Metrics>,
    /// Test-only graceful listener stop, never controlled by a network frame.
    stop: Shutdown,
    /// Joined after every client drops, proving tests leave no listener behind.
    worker: Option<JoinHandle<Result<(), String>>>,
}

/// Starts the production listener on an ephemeral loopback port.
impl Host {
    /// Synthetic shell contents avoid invoking the user's startup files in tests.
    pub(crate) fn start(shell: &str, record: Option<PathBuf>) -> Self {
        let _ = tracing_subscriber::fmt().with_test_writer().try_init();
        let bridge = Bridge::bind(Config {
            port: 0,
            shell: vec!["/bin/sh".into(), "-c".into(), shell.into()],
            record,
            ..Config::default()
        })
        .unwrap();
        let address = bridge.local_addr().unwrap();
        let url = bridge.url().unwrap();
        let metrics = bridge.metrics();
        let stop = Shutdown::default();
        let signal = stop.clone();
        let worker = Some(thread::spawn(move || bridge.serve(signal)));
        Self {
            url,
            address,
            stop,
            metrics,
            worker,
        }
    }

    /// Authenticates HTTP metadata but deliberately does not send hello yet.
    pub(crate) fn connect(&self) -> WebSocket<TcpStream> {
        connect(&self.url, "http://127.0.0.1:8080").unwrap()
    }
}

/// All sockets in each test are declared after Host and drop before it.
impl Drop for Host {
    /// A hung session cleanup surfaces as a test failure instead of leaking workers.
    fn drop(&mut self) {
        self.stop.stop();
        if let Some(worker) = self.worker.take() {
            worker.join().unwrap().unwrap();
        }
    }
}

/// Makes a real HTTP upgrade, with a finite receive timeout for failures.
pub(crate) fn connect(url: &str, origin: &str) -> Result<WebSocket<TcpStream>, tungstenite::Error> {
    let mut request = url.into_client_request()?;
    request
        .headers_mut()
        .insert("Origin", origin.parse().unwrap());
    let authority = url
        .strip_prefix("ws://")
        .unwrap()
        .split('/')
        .next()
        .unwrap();
    let started = std::time::Instant::now();
    let stream = TcpStream::connect(authority)?;
    let connect_ms = started.elapsed().as_secs_f64() * 1000.0;
    stream.set_read_timeout(Some(Duration::from_secs(10)))?;
    stream.set_write_timeout(Some(Duration::from_secs(10)))?;
    let result = tungstenite::client(request, stream)
        .map(|(socket, _)| socket)
        .map_err(|error| match error {
            tungstenite::HandshakeError::Failure(error) => error,
            _ => panic!("blocking test client unexpectedly interrupted"),
        });
    // Timings distinguish delayed OS connect completion from HTTP processing;
    // credentials and URLs must never be formatted into this diagnostic.
    tracing::info!(
        connect_ms,
        total_ms = started.elapsed().as_secs_f64() * 1000.0,
        succeeded = result.is_ok(),
        "test client upgrade timing"
    );
    result
}

/// Writes one well-formed protocol request; tests choose precisely when to ACK.
pub(crate) fn send(socket: &mut WebSocket<TcpStream>, message: Client<'_>) {
    let mut bytes = Vec::new();
    protocol::encode_client(message, &mut bytes).unwrap();
    socket.send(Message::Binary(bytes.into())).unwrap();
}

/// Receives the next application message without interpreting output as UTF-8.
pub(crate) fn receive(socket: &mut WebSocket<TcpStream>) -> Vec<u8> {
    loop {
        match socket.read().unwrap() {
            Message::Binary(bytes) => return bytes.to_vec(),
            Message::Ping(_) | Message::Pong(_) => {}
            message => panic!("unexpected {message:?}"),
        }
    }
}

/// Sends appearance before the shell starts and checks the server's version.
pub(crate) fn hello(socket: &mut WebSocket<TcpStream>, light: bool) -> u32 {
    send(socket, Client::Hello { light });
    assert_eq!(receive(socket), [0x82, 1]);
    let credit = receive(socket);
    assert_eq!(
        protocol::decode_server(&credit).unwrap(),
        protocol::Server::InputCredit(puzzle_pty_bridge::protocol::INPUT_WINDOW)
    );
    puzzle_pty_bridge::protocol::INPUT_WINDOW
}

/// A unique temp directory owns all generated recordings and synthetic byte files.
pub(crate) struct Temp(
    /// Directory owned exclusively by this test.
    pub(crate) PathBuf,
);

/// Files live outside the crate tree and are removed even on assertion unwinding.
impl Temp {
    /// Combines process identity and a counter, without a new randomness dependency.
    pub(crate) fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "puzzle-bridge-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&path).unwrap();
        Self(path)
    }
}

/// Leaves no private captures or 100 MB probe inputs in the checkout.
impl Drop for Temp {
    /// Synthetic paths created by this test are safe to remove recursively.
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.0).unwrap();
    }
}

/// Requires an HTTP denial, not a connection error that could mask a dead server.
pub(crate) fn rejected(result: Result<WebSocket<TcpStream>, tungstenite::Error>, status: u16) {
    let response = match result {
        Err(tungstenite::Error::Http(response)) => response,
        Err(error) => panic!("expected HTTP denial, got {error}"),
        Ok(_) => panic!("unauthorized HTTP upgrade succeeded"),
    };
    assert_eq!(response.status().as_u16(), status);
}

/// Collects raw output until drained exit, ACKing each bounded message.
pub(crate) fn drain(socket: &mut WebSocket<TcpStream>) -> (Vec<u8>, i32) {
    let mut output = Vec::new();
    loop {
        let bytes = receive(socket);
        match protocol::decode_server(&bytes).unwrap() {
            protocol::Server::Output(bytes) => {
                output.extend_from_slice(bytes);
                send(socket, Client::Ack(bytes.len() as u32));
            }
            protocol::Server::Exit(code) => return (output, code),
            protocol::Server::InputCredit(_) => {}
            message => panic!("unexpected protocol response: {message:?}"),
        }
    }
}
