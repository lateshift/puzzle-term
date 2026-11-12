//! Native shell startup with a bounded reader and optional byte tap.
//!
//! The child may print before Session or the parser thread is ready. The
//! fixed pool owns those bytes until the parser starts; no unbounded queue
//! is created by recording. The master writer remains separate so keyboard
//! input can still reach the child when its output side is backpressured.
//!
//! There are two transfer stages. start() owns all IO immediately after spawn;
//! start_pipeline() attaches the Session's join handle before returning any
//! capability. Every error after that transfer drops one Pipeline that can
//! cancel and join the child, drain, writer and Session in the shutdown order.
//!
//! The renderer needs frames while App needs events. Attached separates those
//! receivers after the worker handle has moved into Pipeline, so neither the
//! host nor a render driver can accidentally detach the lifetime barrier.

use super::{SurfaceOptions, record};
use crate::window::live;
use crate::{
    pty::{self, LocalPty, SpawnedPty},
    pty_pool::{self, Consumer},
};
use puzzle_term::messages::ExitInfo;
use puzzle_term::platform::PtySize;
use puzzle_term::theme::Appearance;
use std::sync::mpsc::{self, Receiver};

/// Transport, child and parser pool have distinct thread ownership.
type Started = (
    LocalPty,
    Box<dyn portable_pty::ChildKiller + Send + Sync>,
    Consumer,
    &'static str,
    Receiver<ExitInfo>,
    crate::pipeline::Pipeline,
);

/// Channels remaining after the Session's join barrier moves to Pipeline.
pub(super) struct Attached {
    /// Checked Surface assembly retains this nonblocking input capability.
    pub(super) handle: crate::session_thread::Handle,
    /// Main-thread callbacks consume only this policy-completed outbox.
    pub(super) events: crate::session_thread::Events,
    /// Sealed publications move permanently to the rendering driver.
    pub(super) frames: crate::session_thread::Frames,
    /// Bootstrap directory lifetime of an approved producer launch, if any.
    pub(super) provisioned: Provisioned,
}

/// Spawns the login shell and starts reading before the parser thread exists.
pub(super) fn start(
    grid: PtySize,
    appearance: Appearance,
    options: &SurfaceOptions,
    shell_integration: &str,
    bootstrap: Option<&std::path::Path>,
) -> Result<Started, String> {
    let recorder = options.recorder();
    // Exactly 64 reusable buffers bound unread application output at 4 MiB.
    let (drain_pool, parse_pool) = pty_pool::pair();
    // Host environment and cwd are finalized before the child can execute;
    // the terminal identity is still installed by the common PTY constructor.
    // Integration is a next-session policy: this constructor installs its
    // startup environment once, shared by the harness and native C hosts.
    // A host-provided transport below bypasses this child startup entirely.
    let SpawnedPty {
        mut transport,
        reader,
        child,
        term_name,
        started,
    } = spawn_child(grid, appearance, options, shell_integration, bootstrap)?;
    if let Some(recorder) = recorder {
        // The transport records only successful writes, including replies.
        transport.set_recorder(recorder.clone());
    }
    // Only the Session owner produces ordered input; only the writer may block on the OS.
    transport.set_metrics(parse_pool.metrics_handle());
    // IO ownership starts before Session registration, so every later error
    // follows the same cancel-and-join path as normal application shutdown.
    let writer = transport.start_writer()?;
    // The optional wrapper observes OS reads before the unchanged pool
    // slices them for the sole Session owner.
    // The drain thread owns wait() so it can send status only after EOF.
    // The window keeps a separate kill handle for explicit close requests.
    let killer = child.clone_killer();
    // Status travels separately from byte buffers: the parser receives it
    // only after the reader has released its final producer handle.
    let (exit_tx, exit_rx) = mpsc::channel();
    let drain = live::start_reader(
        record::reader(reader, recorder),
        drain_pool,
        child,
        started,
        exit_tx,
    );
    // Return the transport immediately; keyboard input must not wait for
    // the child to finish producing its first prompt or terminal query.
    // Retain both joins before returning; early Session startup failure kills the
    // child and releases IO rather than detaching a reader or writer indefinitely.
    let workers = crate::pipeline::Pipeline::new(killer.clone_killer(), writer, drain);
    Ok((transport, killer, parse_pool, term_name, exit_rx, workers))
}

/// Session transfer immediately attaches cancellation ownership to the IO pipeline.
impl super::App {
    /// Starts the child and sole Session owner with their initial input policy.
    pub(super) fn start_pipeline(
        &mut self,
        id: crate::SurfaceId,
        info: crate::SurfaceInfo,
        settings: &crate::DesktopSettings,
        options: &SurfaceOptions,
        prepared: crate::render_thread::resources::Prepared,
        initial: (PtySize, &crate::Visibility),
    ) -> Result<(Attached, crate::pipeline::Pipeline), String> {
        let (grid, visibility) = initial;
        // An approved producer's bootstrap exists before its child (None for a shell).
        let launch = self
            .extensions
            .provision(options)
            .map_err(|error| format!("{error:?}"))?;
        // Explicit light/dark settings win over the host preference for COLORFGBG.
        let appearance = super::session::shell_appearance(
            settings.store.effective().theme_mode,
            options.appearance,
        );
        // Source selection happens once: the Session never mixes host bytes with a PTY.
        // Both alternatives return the same pool consumer and EOF/status barrier;
        // only process ownership and the outbound transport implementation differ.
        let (transport, pool, term_name, exit, mut pipeline) = match options.transport {
            super::TransportMode::Pty => {
                let (transport, _, pool, term, exit, pipeline) = start(
                    grid,
                    appearance,
                    options,
                    &settings.store.effective().shell_integration,
                    // Present only for an approved producer launch.
                    bootstrap_path(&launch),
                )?;
                (
                    Box::new(transport) as Box<dyn puzzle_term::platform::Transport>,
                    pool,
                    term,
                    exit,
                    pipeline,
                )
            }
            super::TransportMode::Host => {
                let (transport, pool, exit, pipeline) = super::transport::start(self, id);
                (transport, pool, "xterm-puzzle", exit, pipeline)
            }
        };
        // Native services publish values; they never call AppKit from Session.
        let mut session = self.session(
            id,
            term_name,
            transport,
            prepared,
            info,
            settings,
            options.appearance,
        )?;
        session.set_search_ui(options.search_ui);
        let provisioned = initialize(&mut session, visibility, launch)?;
        let started = self.start_session(session, pool, exit, options.recorder().cloned())?;
        // Retain the join barrier before later render registration can fail.
        pipeline.attach(started.handle.clone(), started.worker);
        // Raw-input policy is queued before the capability can reach the host.
        started
            .handle
            .input_policy(settings.store.effective().option_as_alt())?;
        Ok((
            Attached {
                handle: started.handle,
                events: started.events,
                frames: started.frames,
                provisioned,
            },
            pipeline,
        ))
    }
}

/// Owner state that must precede the first parsed child byte.
///
/// Notification occasion and focus come from the host. An approved producer's
/// handler and grant are installed here too, on the still-local Session, so the
/// producer's first hello can never race its own authorization. Everything is
/// applied before the Session moves to its worker thread.
fn initialize(
    session: &mut puzzle_term::Session,
    visibility: &crate::Visibility,
    launch: Launch,
) -> Result<Provisioned, String> {
    // Visibility is supplied by the host, not inferred from renderer activity.
    session.apply(
        puzzle_term::SessionMessage::NotificationVisibility(
            visibility.visible != Some(false) && visibility.occluded != Some(true),
        ),
        std::time::Instant::now(),
    )?;
    // Input focus and visual occlusion differ; an unfocused visible pane stays visible.
    session.submit(
        puzzle_term::messages::Command::Focus(visibility.focused),
        puzzle_term::Origin::Platform,
    );
    let provisioned = install(launch, session)?;
    // Apply the owner messages before transferring Session to its worker thread.
    session.pump(std::time::Instant::now());
    Ok(provisioned)
}

// Child selection and lifecycle are replaceable whole-file hooks.
include!("pty_bridge/selected.rs");
