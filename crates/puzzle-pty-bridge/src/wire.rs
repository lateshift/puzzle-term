//! Async upgrade and v1 hello, both bounded before any PTY is created.
//!
//! Timeouts exist only while doing work: idle authenticated sessions have
//! no heartbeat or polling timer. Error formatting excludes HTTP metadata,
//! since an upgrade request contains the secret access URL.
//!
//! Upgrade and hello are separate gates. HTTP proves the caller has the
//! credential and an allowed browser Origin; hello selects protocol version
//! and initial appearance. Neither alone permits a child to be spawned.
//! Authorization still uses the same callback as the security tests; moving
//! to async I/O must not add a second interpretation of paths or headers.
//! A total deadline bounds a slow sender that dribbles one header byte at a
//! time. Admission is already held, so such a peer cannot create more tasks.
//! Message/frame limits include the one-byte bridge tag. The untrusted input
//! decoder checks its own lengths too, before sizes reach native PTY calls.
//! Error classification deliberately discards HTTP response/request objects:
//! their Debug/Display forms can expose the access URL in lifecycle logs.
//! Safe protocol errors describe framing, not terminal output or typed input.
use crate::{
    config::Config,
    protocol::{self, Client},
    security,
};
use futures_util::StreamExt;
use std::time::Duration;
use tokio::net::TcpStream;
use tokio_tungstenite::{
    WebSocketStream,
    tungstenite::{self, Message, protocol::WebSocketConfig},
};

/// The session uses async socket readiness; OS PTY I/O stays on its workers.
pub(crate) type Socket = WebSocketStream<TcpStream>;

/// Authenticates an exact Origin/token with a finite HTTP deadline.
pub(crate) async fn handshake(
    stream: TcpStream,
    config: &Config,
    token: &str,
) -> Result<Socket, String> {
    // Tungstenite also bounds headers at 64 KiB/512 reads. A total five-second
    // deadline now bounds slow partial headers as well as a fully silent peer.
    let limits = WebSocketConfig::default()
        .max_message_size(Some(protocol::MAX_MESSAGE))
        .max_frame_size(Some(protocol::MAX_MESSAGE))
        .write_buffer_size(0)
        .max_write_buffer_size(2 * protocol::MAX_MESSAGE);
    #[allow(
        clippy::result_large_err,
        reason = "upgrade callback returns Tungstenite ErrorResponse"
    )]
    let upgrade = tokio_tungstenite::accept_hdr_async_with_config(
        stream,
        |request: &_, response| security::authorize(request, response, config, token),
        Some(limits),
    );
    tokio::time::timeout(Duration::from_secs(5), upgrade)
        .await
        .map_err(|_| "WebSocket handshake timeout".to_string())?
        .map_err(safe_error)
}

/// Supplies transport diagnoses without logging requests, responses or tokens.
pub(crate) fn safe_error(error: tungstenite::Error) -> String {
    match error {
        // Only these payloads describe protocol/OS failures rather than HTTP
        // metadata. The catch-all intentionally avoids the error's Display.
        tungstenite::Error::Io(error) => format!("WebSocket I/O: {error}"),
        tungstenite::Error::Protocol(error) => format!("WebSocket protocol: {error}"),
        tungstenite::Error::Http(_) => "WebSocket authorization denied".into(),
        tungstenite::Error::AttackAttempt => "WebSocket parsing limit exceeded".into(),
        _ => "WebSocket closed or rejected".into(),
    }
}

/// First application message must supply the supported version and appearance.
pub(crate) async fn hello(socket: &mut Socket) -> Result<bool, String> {
    // Keep one deadline across Ping/Pong: keepalive cannot extend admission.
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            match socket.next().await {
                Some(Ok(Message::Binary(bytes))) => {
                    return match protocol::decode_client(&bytes) {
                        Ok(Client::Hello { light }) => Ok(light),
                        _ => Err("first message must be v1 hello".into()),
                    };
                }
                Some(Ok(Message::Ping(_) | Message::Pong(_))) => {}
                _ => return Err("socket closed before hello".into()),
            }
        }
    })
    .await
    .map_err(|_| "hello timeout".to_string())?
}
