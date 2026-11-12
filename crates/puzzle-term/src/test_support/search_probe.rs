//! Measures the same bounded Session search turns on native and browser hosts.
//!
//! Setup feeds real parser bytes into 100,000 retained physical rows. Every
//! hundredth row contains one literal hit, so the independently known result
//! is 1,000 matches. A final blank live row explains the 51st frame slice.
//! Construction and query installation happen outside the measured interval.
//! Hosts call step at their own frame boundaries and retain every observation.
//! No injected clock fabricates elapsed CPU time: Instant observes actual work.
//! This probe measures search-turn CPU, excluding font preparation and GPU work.
//! Its no-op waker leaves refresh scheduling to the benchmark host, not Session.
//! No production terminal can activate this deterministic test-support fixture.
//! Each hit owns tracked endpoints just like an interactive search; the probe
//! includes their creation and marker reconciliation rather than timing only
//! a substring helper. Hard rows keep the expected work count independent of
//! backward soft-wrap discovery, which has its own contract witnesses.
use crate::{
    Origin, Session, SessionConfig,
    messages::Command,
    platform::{PtySize, Services, Transport, Waker},
    search::SearchCommand,
    vt::search::FindOptions,
};
use web_time::Instant;

/// One cold query whose work remains owned by the real Session.
pub struct SearchProbe {
    /// Parser, history, tracked matches and the normal per-frame allowance.
    session: Session,
    /// Search turns retain one host-owned slot even before any font metrics arrive.
    slot: crate::frame_snapshot::FrameSnapshot,
}

impl SearchProbe {
    /// Prepares exactly 100,000 history rows and installs a literal query.
    #[must_use]
    pub fn new() -> Self {
        // The large-history fixture pays for cells and boxed row headers. Its
        // explicit capacity preserves all 100,000 rows on native and wasm hosts;
        // production sessions keep their ordinary 32-MiB default.
        let config = SessionConfig {
            columns: 12,
            rows: 1,
            scrollback_lines: 100_000,
            scrollback_bytes: 64 * 1024 * 1024,
            ..SessionConfig::default()
        };
        let services = Services::new(Box::new(Sink), Box::new(RefreshHost));
        let mut session = Session::new(config, services);
        for row in 0..100_000 {
            // CR/LF creates hard boundaries; smart-case/soft-wrap behavior is separate.
            let bytes: &[u8] = if row % 100 == 0 {
                b"hit\r\n"
            } else {
                b"empty\r\n"
            };
            session.feed(bytes, Instant::now());
        }
        session.submit(
            Command::Search(SearchCommand::Find {
                query: "hit".into(),
                options: FindOptions::default(),
            }),
            Origin::Api,
        );
        // Pump installs the query without scanning; the first step is genuinely cold.
        session.pump(Instant::now());
        Self {
            session,
            slot: Default::default(),
        }
    }

    /// Returns actual CPU milliseconds and rows examined by one normal frame turn.
    pub fn step(&mut self) -> (f64, usize) {
        let start = Instant::now();
        // Publication advances the same bounded search slice; absent fonts produce no image.
        if let Err(error) = self.session.publish_frame(start, &mut self.slot, 0, 0) {
            tracing::warn!(event = "search.probe_publication_failed", %error, "search probe publication failed");
        }
        // Stop the timer before copying progress or converting host-facing values.
        let millis = start.elapsed().as_secs_f64() * 1000.0;
        (millis, self.session.search_state().scanned)
    }

    /// Completion is read directly; this query never advances work by polling.
    #[must_use]
    pub fn complete(&self) -> bool {
        self.session.search_state().complete
    }

    /// Independently expected hit count lets hosts reject shortened workloads.
    #[must_use]
    pub fn count(&self) -> usize {
        self.session.search_state().count
    }
}

impl Default for SearchProbe {
    /// Uses the same cold fixture, not an empty default Session.
    fn default() -> Self {
        Self::new()
    }
}

/// Search output contains no application input; replies are intentionally discarded.
struct Sink;
impl Transport for Sink {
    /// This fixture sends neither interactive input nor protocol queries.
    fn write(&mut self, _: &[u8]) -> Result<(), String> {
        Ok(())
    }
    /// Fixed fixture dimensions never need an operating-system PTY.
    fn resize(&mut self, _: PtySize) -> Result<(), String> {
        Ok(())
    }
    /// Releasing the probe owns no external process or socket.
    fn close(&mut self) -> Result<(), String> {
        Ok(())
    }
}

/// The native loop or Chrome rAF drives steps; no scheduler runs inside the probe.
struct RefreshHost;
impl Waker for RefreshHost {
    /// Unfinished query work is reflected by complete(), which the host checks.
    fn request_frame(&mut self) {}
    /// There is no blinking rendered cursor or synthetic timer in this fixture.
    fn wake_at(&mut self, _: Instant) {}
}
