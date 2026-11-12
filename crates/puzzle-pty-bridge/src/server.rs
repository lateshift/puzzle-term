//! IPv4-loopback listener with admission before any upgrade (§12.5).
//!
//! The address is not an option: neither CLI nor Config can bind a LAN port.
//! A four-permit semaphore includes handshakes and pre-hello sockets, so
//! opening idle connections cannot grow worker count or start a fifth shell.
use crate::{config::Config, connection, metrics::Metrics, security, shutdown::Shutdown};
use std::{
    net::{Ipv4Addr, SocketAddr, TcpListener},
    sync::{Arc, atomic::Ordering},
};
use tokio::{runtime::Builder, sync::Semaphore};

/// Bound development listener and its process-lifetime random access token.
pub struct Bridge {
    /// OS socket created only from Ipv4Addr::LOCALHOST and a port number.
    listener: TcpListener,
    /// Options are immutable once clients can connect.
    config: Config,
    /// Secret printed once for the user; not part of Debug or tracing fields.
    token: String,
    /// Native event-turn evidence does not inspect tokens or shell bytes.
    metrics: Arc<Metrics>,
}

/// Listener creation and serving never expose a wildcard bind operation.
impl Bridge {
    /// Generates entropy before binding; port zero is useful for isolated tests.
    pub fn bind(config: Config) -> Result<Self, String> {
        // An entropy error fails closed: no listening socket exists yet.
        // There is intentionally no fallback to a timestamp or process ID.
        let token = security::token()?;
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, config.port))
            .map_err(|error| format!("bind bridge: {error}"))?;
        // Both accept and each authorized socket await OS readiness. Only
        // native PTY reads/writes and child waits use blocking workers.
        listener.set_nonblocking(true).map_err(|e| e.to_string())?;
        Ok(Self {
            listener,
            config,
            token,
            metrics: Arc::new(Metrics::default()),
        })
    }

    /// Returns the actual OS address, allowing tests to prove the bind scope.
    pub fn local_addr(&self) -> Result<SocketAddr, String> {
        self.listener
            .local_addr()
            .map_err(|error| error.to_string())
    }

    /// Local startup URL; callers must not put it in persistent logs.
    pub fn url(&self) -> Result<String, String> {
        Ok(format!(
            "ws://{}/pty?token={}",
            self.local_addr()?,
            self.token
        ))
    }

    /// Returns event counters without exposing any credential or private output.
    pub fn metrics(&self) -> Arc<Metrics> {
        self.metrics.clone()
    }

    /// Waits for a host stop, then joins admitted sessions through shell cleanup.
    pub fn serve(self, shutdown: Shutdown) -> Result<(), String> {
        // Async socket tasks share the runtime; the bounded PTY workers are
        // OS threads. Blocking-pool slots are needed only for final cleanup.
        let runtime = Builder::new_multi_thread()
            .worker_threads(1)
            .max_blocking_threads(4)
            .enable_all()
            .build()
            .map_err(|e| e.to_string())?;
        runtime.block_on(self.accept(shutdown))
    }

    /// Counts pre-hello sockets in the quota, rejecting overflow before upgrade.
    async fn accept(self, shutdown: Shutdown) -> Result<(), String> {
        let listener =
            tokio::net::TcpListener::from_std(self.listener).map_err(|e| e.to_string())?;
        // Pre-hello peers reserve the same quota as authorized running shells.
        let slots = Arc::new(Semaphore::new(4));
        // Shared immutable metadata is not copied for each socket readiness event.
        let config = Arc::new(self.config);
        let token = Arc::new(self.token);
        // Joining keeps native cleanup within the lifetime of this runtime.
        let mut tasks = tokio::task::JoinSet::new();
        let mut session = 0_u64;
        loop {
            self.metrics.admissions.fetch_add(1, Ordering::Relaxed);
            // Shutdown keeps a Notify permit if stop races the first await.
            // No timer wakes a listener or a silent admitted socket (P2).
            let (stream, peer) = tokio::select! {
                _ = shutdown.cancelled() => break,
                result = listener.accept() => result.map_err(|e| e.to_string())?,
                _ = tasks.join_next(), if !tasks.is_empty() => continue,
            };
            // Recheck the peer even though the listener itself is loopback-only.
            if !peer.ip().is_loopback() {
                continue;
            }
            let Ok(permit) = slots.clone().try_acquire_owned() else {
                // No PTY or session task is created for rejected admission.
                stream.writable().await.map_err(|e| e.to_string())?;
                let _ = stream.try_write(b"HTTP/1.1 503 Service Unavailable\r\nContent-Length: 0\r\nConnection: close\r\n\r\n");
                continue;
            };
            // IDs identify lifecycle logs without exposing credentials or argv.
            session = session.saturating_add(1);
            let (config, token, metrics) = (config.clone(), token.clone(), self.metrics.clone());
            // No blocking per-session socket worker or idle wakeup is created.
            tasks.spawn(async move {
                // run awaits SIGHUP and actual reaping before this permit drops.
                let _permit = permit;
                if let Err(error) = connection::run(stream, &config, &token, session, metrics).await
                {
                    tracing::warn!(event = "bridge.session_failed", session, %error);
                }
            });
        }
        // Host shutdown stops admission; it does not drop in-flight bytes or
        // orphan a shell by aborting a connection's cleanup future.
        while tasks.join_next().await.is_some() {}
        Ok(())
    }
}
