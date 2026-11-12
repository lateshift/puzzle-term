//! Literal search remains incremental in Session, never synchronous in JS.
//! SearchState is a live read-only view of the active query: a page can retain it
//! and read count/complete after refreshes without a polling loop in the engine.
//! Navigation commands visit ordinary policy and centre the selected occurrence.
//! Keeping progress in Rust avoids allocating JS maps on every search frame.
//!
//! A synchronous find over long history would starve DOM input. Instead this
//! facade installs a query, and the existing Session waker schedules bounded
//! work at refresh boundaries. Getters read progress without advancing work.
//! Count can grow across slices or shrink after retention and row rewrites.
//! Completion names the current source, rather than an immutable old result.
//!
//! Every state handle refers to the terminal's active query. Replacing a query
//! therefore updates existing handles as well; no stale match owners remain
//! alive merely because a page kept an earlier return value. The same policy
//! lets clear remove all fills without managing another JS result registry.
//! Hosts may refresh their count while complete is false, then stop their UI
//! callback until another user action or output event requires a new read.
//!
//! Smart case belongs to the VT engine. The adapter accepts only the explicit
//! caseSensitive boolean and rejects unknown options, including regex. It
//! cannot silently reinterpret a requested regular expression as literal text.
//! Find options are bounded plain data, so cycles and throwing getters become
//! errors before the active query is replaced. No JSON parser is linked here.
//! next and prev use the shared Match target and its normal interceptor chain;
//! they centre the occurrence rather than changing the terminal cursor.
//! All facade and state reads check disposal before accessing their Session.

use super::{api::PuzzleTerminal, api_viewport::command, *};
use puzzle_term::{
    messages::Command, search::SearchCommand, viewport::ScrollTarget, vt::search::FindOptions,
};

/// One terminal's search operations, independent of the host page's search field.
#[wasm_bindgen]
pub struct PuzzleSearch {
    /// Retaining the facade does not defeat explicit terminal disposal.
    owner: Owner,
}
/// A live progress view, rather than a synchronous full-history search result.
#[wasm_bindgen]
pub struct PuzzleSearchState {
    /// Queries borrow only the small reconciled progress value from Session.
    owner: Owner,
}
/// Search UI belongs to the embedding page; this getter supplies its Rust model.
#[wasm_bindgen]
impl PuzzleTerminal {
    /// Returns operations over this terminal's active literal query.
    #[wasm_bindgen(getter)]
    pub fn search(&self) -> Result<PuzzleSearch, JsValue> {
        self.owner.borrow().ensure_live()?;
        Ok(PuzzleSearch {
            owner: self.owner.clone(),
        })
    }
}
/// Mutations never scan rows; only scheduled frame boundaries advance the engine.
#[wasm_bindgen]
impl PuzzleSearch {
    /// Starts a query with smart case unless caseSensitive is explicitly supplied.
    /// Progress getters update after refreshes; result count is capped at 10,000.
    #[wasm_bindgen(unchecked_return_type = "SearchState")]
    pub fn find(
        &self,
        query: String,
        #[wasm_bindgen(unchecked_optional_param_type = "FindOptions")] options: Option<JsValue>,
    ) -> Result<PuzzleSearchState, JsValue> {
        let mut case_sensitive = None;
        if let Some(options) = options {
            // Regex and misspelled options must not silently turn into literal searches.
            let plain = value::plain(&options, 0, &mut 32)?;
            let puzzle_term::bus::Value::Map(map) = plain else {
                return Err(value::error(
                    "InvalidArgument",
                    "find options must be an object",
                ));
            };
            for (key, value) in map {
                if key != "caseSensitive" {
                    return Err(value::error("InvalidArgument", "unknown find option"));
                }
                let puzzle_term::bus::Value::Bool(value) = value else {
                    return Err(value::error(
                        "InvalidArgument",
                        "caseSensitive must be boolean",
                    ));
                };
                case_sensitive = Some(value);
            }
        }
        command(
            &self.owner,
            Command::Search(SearchCommand::Find {
                query,
                options: FindOptions { case_sensitive },
            }),
        )?;
        Ok(PuzzleSearchState {
            owner: self.owner.clone(),
        })
    }
    /// Selects the next surviving occurrence, wrapping at the end.
    #[wasm_bindgen(unchecked_return_type = "SearchState")]
    pub fn next(&self) -> Result<PuzzleSearchState, JsValue> {
        command(&self.owner, Command::ScrollTo(ScrollTarget::Match(1)))?;
        Ok(PuzzleSearchState {
            owner: self.owner.clone(),
        })
    }
    /// Selects the previous surviving occurrence, wrapping at the beginning.
    #[wasm_bindgen(unchecked_return_type = "SearchState")]
    pub fn prev(&self) -> Result<PuzzleSearchState, JsValue> {
        command(&self.owner, Command::ScrollTo(ScrollTarget::Match(-1)))?;
        Ok(PuzzleSearchState {
            owner: self.owner.clone(),
        })
    }
    /// Releases tracked hits and removes their fills and markers.
    pub fn clear(&self) -> Result<(), JsValue> {
        command(&self.owner, Command::Search(SearchCommand::Clear))
    }
}
/// Progress is a direct Rust query and cannot start a frame or scan by itself.
#[wasm_bindgen]
impl PuzzleSearchState {
    /// Number of surviving occurrences found by completed slices so far.
    #[wasm_bindgen(getter)]
    pub fn count(&self) -> Result<u32, JsValue> {
        let r = self.owner.borrow();
        r.ensure_live()?;
        Ok(r.session.search_state().count as u32)
    }
    /// Zero-based navigated occurrence, undefined while no result is current.
    #[wasm_bindgen(getter)]
    pub fn current(&self) -> Result<Option<u32>, JsValue> {
        let r = self.owner.borrow();
        r.ensure_live()?;
        Ok(r.session.search_state().current.map(|n| n as u32))
    }
    /// True when all current content has been scanned or the cap was reached.
    #[wasm_bindgen(getter)]
    pub fn complete(&self) -> Result<bool, JsValue> {
        let r = self.owner.borrow();
        r.ensure_live()?;
        Ok(r.session.search_state().complete)
    }
}
