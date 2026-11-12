//! Cold surface policy supplied before the default PTY shell starts (M17.embed).
//! Shell options are copied only during creation. They cannot race a running
//! Session, and later settings changes use the checked message endpoint instead.
//!
//! These are creation-time values, not borrowed host services. App can transfer
//! them to its workers without invoking a native callback from those workers.
//! An omitted process option inherits the same login-shell policy as the harness;
//! no environment or current-directory change affects the embedding application.
//!
//! test-support extends this payload only for authored harness observations.
//! Recorder, source revision and workload observer are explicitly not host API.
//! They retain the original test witnesses through the code move, rather than
//! making every application choose or initialize a diagnostics subsystem.
//!
//! Internal accessors normalize these optional fields to None in normal builds.
//! Worker assembly can therefore follow one startup path while the externally
//! visible options remain restricted to process, settings and native geometry.
#[cfg(feature = "test-support")]
use crate::recording::Recorder;
use crate::settings::DesktopSettings;
use puzzle_term::theme::Appearance;
use std::{ffi::OsString, path::PathBuf};

/// One definition generates both ordinary and extended options.
#[macro_use]
mod definition;
// Keep optional fields outside the ordinary process policy implementation.
include!("options/selected.rs");

impl SurfaceOptions {
    /// Native host builds have no diagnostic recorder; probes opt in explicitly.
    pub(super) fn recorder(&self) -> Option<&crate::recording::Recorder> {
        #[cfg(feature = "test-support")]
        {
            self.recorder.as_ref()
        }
        #[cfg(not(feature = "test-support"))]
        {
            None
        }
    }
    /// Revision metadata belongs to private recordings, never terminal policy.
    pub(super) fn revision(&self) -> &str {
        #[cfg(feature = "test-support")]
        {
            self.revision.as_deref().unwrap_or("unknown")
        }
        #[cfg(not(feature = "test-support"))]
        {
            "unknown"
        }
    }
    /// Finite workload observers exist only in the explicitly opted-in harness.
    #[cfg(feature = "lock-metrics")]
    pub(super) fn observer(&mut self) -> Option<Box<dyn crate::window::observer::SurfaceObserver>> {
        #[cfg(feature = "test-support")]
        {
            self.observer.take()
        }
        #[cfg(not(feature = "test-support"))]
        {
            None
        }
    }
}
