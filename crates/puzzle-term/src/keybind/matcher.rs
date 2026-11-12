//! Ordered leader matching and public bus interception (§5.2, §14.9).
//!
//! An unsuccessful leader flushes encoder keys, not synthesized text, so
//! kitty modes still apply at dispatch. The bus resumes ordinary Key commands after this hook,
//! so downstream key policies see buffered keys without matching them twice.
//!
//! Leader state contains original encoder events rather than text bytes.
//! If the application changes kitty flags while a leader is pending, its
//! later flush is encoded under the modes active at dispatch, as other keys.
//! A mismatch first releases earlier steps, then tries the current key as
//! a fresh shortcut. Thus an unrelated leader can begin on the same event.
//! A pending continuation is compared before global physical shortcuts;
//! otherwise a physical c binding could steal ctrl+a followed by c.
//! Completed actions clear the leader, so a later key cannot reuse its steps.
//! Unconsumed sequences instead release every buffered step and the final
//! key. Their action still runs, but the application receives the whole chord.
//! Performable future actions have no side effects and leave encoder input
//! intact. Availability is explicit in the catalogue rather than inferred
//! from whether an action happened to produce bytes on this invocation.
//! All unmatched encoder keys share one follow-up round with flushed leaders.
//! That ordering matters for a pump containing several keys: an immediate
//! later key could otherwise overtake earlier keys waiting for the next round.
//! Resume carries the public Key kind through downstream policy masks.
//! It skips policies which already inspected the input before buffering;
//! those policies must not see a press twice when the leader times out.
//! Finite timeouts use injected elapsed milliseconds, not wall-clock reads.
//! The default zero timeout never schedules a wake, preserving idle behavior.
use super::{Action, Binding, Keybindings};
use crate::{
    messages::{Command, Event},
    vt::input::{KeyEvent, KeyPhase},
};
use puzzle_bus::{
    Origin,
    hooks::{Flow, HookCtx, Interceptor},
};

/// Matcher output separates buffered passthrough from a completed action.
#[derive(Debug, Default)]
pub struct MatchResult {
    /// Original keys flushed before any current action executes.
    pub forward: Vec<KeyEvent>,
    /// Typed action with its shared-store routing bit.
    pub action: Option<(Action, bool)>,
    /// Whether the current key should bypass the encoder.
    pub consumed: bool,
}

/// Advances leader state only after a mapped input event arrives.
impl Keybindings {
    /// Delegates plain host text once while accepted query keys bypass app shortcuts.
    /// Only a configured single-key paste keeps its ordinary clipboard operation.
    /// The resulting committed text is edited by Session after clipboard policy.
    /// Reject releases/composition here so they cannot request a second clipboard read.
    fn query_key(&mut self, key: &KeyEvent, ctx: &mut HookCtx<'_, Command, Event>) -> Flow {
        // Preserve the configured one-key paste shortcut: its clipboard completion
        // becomes Command::Paste, which the query mode consumes instead of the PTY.
        // All other shortcuts and leaders remain bypassed while editing the field.
        if !key.composing
            && key.phase != KeyPhase::Release
            && self
                .test(key)
                .is_some_and(|binding| binding.steps.len() == 1 && binding.action == Action::Paste)
        {
            self.native_key = None;
            ctx.submit(
                Command::BindingAction {
                    action: Action::Paste,
                    all: false,
                },
                ctx.origin().unwrap_or(Origin::User),
            );
            return Flow::Stop("query clipboard paste");
        }
        // beforeinput supplies the committed text later; its key must not insert twice.
        if self.native_key.take().as_ref() == Some(key) {
            self.native_passed = true;
            return Flow::Stop("query delegates native text");
        }
        // Query mode bypasses leader matching; its editing keys cannot start app sequences.
        // The Session input mode consumes physical editing keys after ordinary policies.
        Flow::Continue
    }
    /// Combines static feature availability with the Session’s current copy sources.
    fn can_perform(&self, action: &Action) -> bool {
        // Empty selection must preserve Ctrl+C for applications on Windows/Linux.
        match action {
            Action::Copy(_) => action.performable() && self.selection_available,
            // M9 supplies desktop UI; web hosts already own a search request listener.
            Action::Search => self.search_ui,
            // Performable navigation passes through when no results are available.
            Action::SearchNext | Action::SearchPrev => self.search_available,
            Action::CopyCommandOutput => action.performable() && self.output_available,
            _ => action.performable(),
        }
    }
    /// Finds a prefix; physical beats logical at the current sequence step.
    pub(super) fn candidate(&self, keys: &[KeyEvent]) -> Option<&Binding> {
        // Releases belong to the buffered stream, not the leader grammar.
        let count = keys
            .iter()
            .filter(|key| key.phase != KeyPhase::Release)
            .count();
        if count == 0 {
            return None;
        }
        // Prefix length bounds indexing before physical precedence is compared.
        self.entries
            .iter()
            .filter(|binding| {
                binding.steps.len() >= count
                    && binding
                        .steps
                        .iter()
                        .zip(keys.iter().filter(|key| key.phase != KeyPhase::Release))
                        .all(|(step, key)| step.matches(key))
            })
            // Exact code wins only within the current continuation set.
            .max_by_key(|binding| binding.steps[count - 1].physical)
    }
    /// Resolves an event at injected milliseconds, never consuming IME input.
    pub fn match_key(&mut self, key: &KeyEvent, now_ms: u64) -> MatchResult {
        let mut result = MatchResult::default();
        if key.composing {
            return result;
        }
        // Expired prefixes are emitted before examining this new input.
        result.forward = self.expire(now_ms);
        // A kitty release cannot reach the child before its held press.
        if key.phase == KeyPhase::Release {
            if !self.pending.is_empty() {
                self.pending.push(key.clone());
                result.consumed = true;
            }
            return result;
        }
        // A pending leader gets first refusal. On mismatch it is flushed
        // before trying the current key as the start of a different binding.
        self.pending.push(key.clone());
        let mut binding = self.candidate(&self.pending).cloned();
        if binding.is_none() {
            self.pending.pop();
            result.forward.append(&mut self.pending);
            self.pending.push(key.clone());
            binding = self.candidate(&self.pending).cloned();
        }
        // No candidate leaves the current key to the ordinary encoder lane.
        let Some(binding) = binding else {
            self.pending.clear();
            return result;
        };
        if binding.steps.len()
            > self
                .pending
                .iter()
                .filter(|key| key.phase != KeyPhase::Release)
                .count()
        {
            self.started_ms = now_ms;
            result.consumed = true;
            return result;
        }
        // Deferred actions remain listed. performable explicitly preserves
        // all keys of the attempted sequence when its feature is absent.
        if (binding.action == Action::Search && !self.search_ui)
            || (binding.prefixes.performable && !self.can_perform(&binding.action))
        {
            self.pending.pop();
            result.forward.append(&mut self.pending);
            return result;
        }
        self.complete(binding, result)
    }
    /// A completed leader releases original input only under unconsumed policy.
    fn complete(&mut self, binding: Binding, mut result: MatchResult) -> MatchResult {
        result.consumed = !binding.prefixes.unconsumed;
        if binding.prefixes.unconsumed {
            self.pending.pop();
            result.forward.append(&mut self.pending);
        } else {
            self.pending.clear();
        }
        result.action = Some((binding.action, binding.prefixes.all));
        result
    }
    /// Empties a leader during a table reload without dropping input.
    pub fn flush(&mut self) -> Vec<KeyEvent> {
        std::mem::take(&mut self.pending)
    }
    /// Releases held input with its trusted origins during host reload/expiry.
    pub(crate) fn flush_tagged(&mut self) -> Vec<(KeyEvent, Origin)> {
        let origins = std::mem::take(&mut self.pending_origins);
        self.flush()
            .into_iter()
            .enumerate()
            .map(|(index, key)| (key, origins.get(index).copied().unwrap_or(Origin::User)))
            .collect()
    }
    /// Remaining injected milliseconds until a finite leader expires.
    pub fn timeout_remaining(&self) -> Option<u64> {
        (!self.pending.is_empty() && self.timeout_ms != 0).then(|| {
            self.timeout_ms
                .saturating_sub(self.now_ms.saturating_sub(self.started_ms))
        })
    }
    /// Flushes a timed-out leader; zero means indefinitely pending.
    pub fn expire(&mut self, now_ms: u64) -> Vec<KeyEvent> {
        if self.timeout_ms != 0 && now_ms.saturating_sub(self.started_ms) >= self.timeout_ms {
            std::mem::take(&mut self.pending)
        } else {
            Vec::new()
        }
    }
}

/// The same public hook contract is used by application and built-in policies.
impl Interceptor<Command, Event> for Keybindings {
    /// Rewrites ordinary keys to typed actions before the VT encoder runs.
    fn intercept(&mut self, command: &mut Command, ctx: &mut HookCtx<'_, Command, Event>) -> Flow {
        let Command::Key(key) = command else {
            return Flow::Continue;
        };
        if self.query_input {
            return self.query_key(key, ctx);
        }
        // IME events neither advance the leader nor change its provenance.
        if key.composing {
            return Flow::Continue;
        }
        let mut origins = std::mem::take(&mut self.pending_origins);
        let result = self.match_key(key, self.now_ms);
        // Native ownership applies only to this original dispatch. Flushed
        // older keys resume after this policy and must still encode, even
        // when they have the same character as the present textarea press.
        let native = self.native_key.take().as_ref() == Some(key);
        let origin = ctx.origin().unwrap_or(Origin::User);
        for (index, key) in result.forward.into_iter().enumerate() {
            ctx.resume(
                Command::Key(key),
                origins.get(index).copied().unwrap_or(origin),
            );
        }
        // Continuations retain prior sources; a mismatched new leader starts
        // a fresh provenance list after all older keys have been released.
        if !self.pending.is_empty() {
            // Press and release origins follow their events until replay.
            origins.truncate(self.pending.len().saturating_sub(1));
            origins.push(origin);
            self.pending_origins = origins;
        }
        if let Some((action, all)) = result.action {
            // Input payloads enter this same round as resumed keys. An extra
            // action-to-Text cascade would reorder a later unbound key ahead.
            let command = match action {
                Action::SendText(text) => Command::Text(text),
                Action::Csi(text) => Command::Text(format!("\x1b[{text}")),
                Action::Esc(text) => Command::Text(format!("\x1b{text}")),
                action => Command::BindingAction { action, all },
            };
            ctx.submit(command, origin);
        }
        // Forward all keys after a flushed prefix in the same FIFO cascade
        // round. Otherwise the current key could overtake its leader.
        if !result.consumed {
            if native {
                self.native_passed = true;
            } else {
                ctx.resume(Command::Key(key.clone()), origin);
            }
        }
        // All encoder keys share the deferred lane. A later unbound key in
        // this input batch cannot overtake a leader flushed by its predecessor.
        Flow::Stop("keybinding sequence/action")
    }
}
