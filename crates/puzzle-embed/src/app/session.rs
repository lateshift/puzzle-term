//! Initial Session policy and queue-only native services (D-43/D-50).
//!
//! This is a cold transfer boundary. Session is constructed here, configured
//! completely, and moved into its exclusive worker before a host endpoint is
//! returned. No callback ever borrows it from the main or render thread.
//!
//! Clipboard, URL and notification operations are deferred host transactions.
//! Their authorization stays in core policy; their OS implementation remains
//! the embedding host's responsibility during App::tick.
//!
//! Initial appearance is applied before parsing child output. COLORFGBG uses
//! the same resolved theme mode, so applications see a consistent startup
//! environment and later mode-2031 appearance notifications.
//!
//! The existing built-in overlay models are installed on the Session owner.
//! Views move with sealed publications; measured fonts and geometry are copied
//! values and do not create a shared mutable text context on the parser side.
//!
//! The Session outbox is enabled before policy initialization so startup facts
//! cannot disappear merely because the host has not called tick yet. Installing
//! keybindings last keeps the initial policy snapshot consistent at transfer.
use super::*;
use crate::{render_thread::surface::SurfaceInfo, window::live};
use puzzle_term::{
    Origin, Session, SessionConfig, SessionMessage, messages::Command, platform::Services,
    settings::ThemeMode, theme::Appearance,
};
use std::time::{Duration, Instant};

impl App {
    /// Policy, host effects and initial geometry are installed before the parser starts.
    #[allow(
        clippy::too_many_arguments,
        reason = "cold transfer of distinct Session startup values"
    )]
    pub(super) fn session(
        &self,
        id: SurfaceId,
        term_name: &'static str,
        transport: Box<dyn puzzle_term::platform::Transport>,
        prepared: crate::render_thread::resources::Prepared,
        info: SurfaceInfo,
        settings: &DesktopSettings,
        system: Appearance,
    ) -> Result<Session, String> {
        let metrics = prepared.metrics;
        // Transport owns the ordered writer; the waker records publication demand.
        let mut services = Services::new(
            transport,
            Box::<puzzle_term::platform::PublicationWaker>::default(),
        );
        // The host must perform clipboard IO on its own thread after policy approves.
        services.clipboard = Box::<super::services::DeferredClipboard>::default();
        // URL and notification payloads remain bounded by normal core policy.
        let effects = super::services::Effects {
            id,
            posts: self.posts.clone(),
            wake: Arc::clone(&self.config.wakeup),
        };
        services.urls = Box::new(effects.clone());
        services.notifier = Box::new(effects);
        let grid = live::grid_for_surface(
            info.extent,
            metrics.cell,
            live::padding_for(settings, metrics.dpr),
        );
        let config = SessionConfig {
            term_name,
            columns: usize::from(grid.columns),
            rows: usize::from(grid.rows),
            render_settle: Duration::from_millis(u64::from(
                settings.store.effective().render_settle_ms,
            )),
            ..settings.store.effective().session_config()
        };
        let mut session = Session::new(config, services);
        session.set_notification_namespace(id.0);
        session.enable_images(
            (settings.store.effective().images_storage_limit_mb as usize)
                .saturating_mul(1024 * 1024),
        );
        session.set_image_texture_limit(prepared.image_texture_limit);
        // Enable before any host-facing startup fact, not after the child can print.
        session.enable_host_events()?;
        // Session copies measurements only; fonts and caches stay on rendering.
        session.apply(SessionMessage::FontMetrics(metrics), Instant::now())?;
        session.set_padding(live::padding_for(settings, metrics.dpr));
        session.set_surface_size([info.extent.width, info.extent.height]);
        // Initial settings and appearance must precede parsing the first child byte.
        session.apply_settings(settings.store.effective(), Instant::now());
        session.submit(Command::SetSystemAppearance(system), Origin::Platform);
        session.pump(Instant::now());
        // Interaction models belong to Session; views travel with publications.
        session.install_model_overlays();
        session.set_keybindings(settings.store.keybindings());
        Ok(session)
    }
}
/// The child's initial environment follows effective mode rather than host preference.
pub(super) fn shell_appearance(mode: ThemeMode, system: Appearance) -> Appearance {
    match mode {
        ThemeMode::Auto => system,
        ThemeMode::Light => Appearance::Light,
        ThemeMode::Dark => Appearance::Dark,
    }
}
