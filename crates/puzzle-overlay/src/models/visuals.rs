//! Transient UI sources are sampled by Session, never by a rendering callback.
//! Strings keep their capacity between rotating publications; status is Copy data.

/// Sources for cursor annotations and finite application status overlays.
#[derive(Clone)]
pub struct Visuals {
    /// Uncommitted candidate; a separate input commit owns all application bytes.
    pub preedit: String,
    /// UTF-8 byte boundary supplied by the input method, if known.
    pub preedit_cursor: Option<usize>,
    /// Suggested input remains outside the grid and transport until accepted.
    pub ghost: String,
    /// Logical em size; the view uses the same primary and fallback faces as grid text.
    pub font_size: f32,
    /// Monotonic accepted BEL identity, so an unrelated frame cannot restart its flash.
    pub bell: u64,
    /// Live policy can suppress the flash without suppressing the Bell fact.
    pub bell_visual: bool,
    /// ConEmu state and percent; zero state means no progress affordance.
    pub progress: Option<(u8, u8)>,
}

/// The default follows bell.visual without needing a platform settings store.
impl Default for Visuals {
    /// Empty sources schedule no work or timers.
    fn default() -> Self {
        Self {
            preedit: String::new(),
            preedit_cursor: None,
            ghost: String::new(),
            font_size: 14.0,
            bell: 0,
            bell_visual: true,
            progress: None,
        }
    }
}
/// Dirty captures reuse string storage even across rotating slots (P1).
impl Visuals {
    /// Copies immutable source data without replacing warmed text buffers.
    pub fn copy_from(&mut self, source: &Self) {
        self.preedit.clone_from(&source.preedit);
        self.preedit_cursor = source.preedit_cursor;
        self.ghost.clone_from(&source.ghost);
        self.font_size = source.font_size;
        self.bell = source.bell;
        self.bell_visual = source.bell_visual;
        self.progress = source.progress;
    }
}
