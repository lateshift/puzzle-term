//! Mounted DOM lifetime: elements and callbacks are disposed together (§12.2).
//!
//! Only this platform layer owns JS objects. A host keeps its neutral Session
//! outside the adapter and borrows it when a command arrives; no frame is drawn
//! by any listener. The textarea stays focusable while visually transparent.
//!
//! Browser editing and terminal rendering have different coordinate systems.
//! The container establishes the absolute-position origin; the hidden control
//! is moved in CSS pixels while the renderer supplies device-pixel geometry.
//! Keeping both children under that origin makes page scrolling affect them
//! together, including the native input-method candidate window.
//!
//! Autocorrection is disabled because shell input must not change spelling or
//! capitalization. A textarea, rather than a contenteditable canvas wrapper,
//! also gives mobile keyboards and clipboard events an ordinary editing target.
//! Its value is transient: committed strings leave through the typed Text lane,
//! and the text listener clears it after every non-composing input.
//!
//! Callbacks retain only a weak state reference. The owner holds registrations
//! strongly and tears them down first, so disposal cannot leave a JS closure
//! retaining a Session after its canvas has gone away.
//! Resize/media observers likewise disconnect before host state is released.
use super::{DomHost, KeyboardState, listener::Listener};
use std::{cell::RefCell, rc::Rc};
use wasm_bindgen::{JsCast, JsValue};
use web_sys::{HtmlCanvasElement, HtmlElement, HtmlTextAreaElement};
/// Optional Chromium layout/lock APIs are accessed without browser-specific imports.
mod layout;
pub use layout::platform;

/// A terminal's browser elements and their event registrations.
pub struct DomInput<H: DomHost + 'static> {
    /// Relative positioning anchors the candidate window beside the cursor.
    container: HtmlElement,
    /// The canvas never receives keyboard focus or editable DOM text.
    canvas: HtmlCanvasElement,
    /// One focus target covers IME, dead keys, mobile typing and paste.
    textarea: HtmlTextAreaElement,
    /// Shared only within the page thread, never within the neutral core.
    state: Rc<RefCell<KeyboardState<H>>>,
    /// Dropping registrations before the elements releases every JS closure.
    listeners: Vec<Listener>,
    /// Observer disconnects before the DOM owner releases its subtree.
    monitor: Option<super::monitor::Monitor>,
    /// This input-only rAF lane coalesces high-rate motion while keys stay immediate.
    motion: Rc<super::pointer::motion::Motion<H>>,
}

/// The host supplies policies immediately; rendering remains a separate rAF lane.
impl<H: DomHost + 'static> DomInput<H> {
    /// Mounts a rendering canvas and an accessible, transparent input target.
    /// DOM/observer errors return to the host and tear down partial registrations.
    /// The caller supplies rendering resources; mounting does not create a GPU.
    pub fn mount(
        parent: &HtmlElement,
        canvas: HtmlCanvasElement,
        host: H,
    ) -> Result<Self, JsValue> {
        let (container, textarea) = elements(parent, &canvas)?;
        // Pointer Events own basic one-finger scrolling instead of native page panning.
        canvas.style().set_property("touch-action", "none")?;
        let state = Rc::new(RefCell::new(KeyboardState::new(host)));
        let motion = super::pointer::motion::Motion::new(&state);
        let mut mounted = Self {
            container,
            canvas,
            textarea,
            state,
            listeners: Vec::new(),
            monitor: None,
            motion,
        };
        // A setup error drops this owner, removing registrations already installed.
        mounted
            .listeners
            .extend(super::text::install(&mounted.state, &mounted.textarea)?);
        mounted.listeners.extend(super::pointer::install(
            &mounted.state,
            &mounted.canvas,
            &mounted.textarea,
            &mounted.motion,
        )?);
        mounted.anchor()?;
        mounted.monitor = Some(super::monitor::Monitor::new(
            &mounted.state,
            &mounted.container,
        )?);
        Ok(mounted)
    }

    /// Moves the native IME candidate anchor using current device-pixel geometry.
    pub fn anchor(&self) -> Result<(), JsValue> {
        let state = self
            .state
            .try_borrow()
            .map_err(|_| JsValue::from_str("Input host is busy"))?;
        let geometry = state.host.geometry();
        let dpr = geometry.dpr.max(f64::EPSILON);
        // Candidate UI uses CSS pixels; render geometry includes the grid padding.
        self.textarea
            .style()
            .set_property("left", &format!("{}px", geometry.cursor[0] / dpr))?;
        self.textarea
            .style()
            .set_property("top", &format!("{}px", geometry.cursor[1] / dpr))
    }

    /// Exposes the focus target for host API focus/blur without a second input path.
    pub fn textarea(&self) -> &HtmlTextAreaElement {
        &self.textarea
    }

    /// The rendering host owns size changes; event handlers never resize a surface.
    pub fn canvas(&self) -> &HtmlCanvasElement {
        &self.canvas
    }

    /// Loads Chromium's exact layout base; unsupported browsers keep the US fallback.
    pub async fn refresh_layout(&self) -> Result<(), JsValue> {
        layout::load(&self.state).await
    }

    /// Acquires keyboard lock only within a host-initiated fullscreen user gesture.
    pub async fn lock_keyboard(&self) -> Result<(), JsValue> {
        layout::lock(&self.state).await
    }

    /// Releases browser chrome chords immediately when the host leaves fullscreen.
    pub fn unlock_keyboard(&self) -> Result<(), JsValue> {
        layout::unlock(&self.state)
    }
}

/// A disposed terminal cannot leave detached focus targets or callbacks behind.
impl<H: DomHost + 'static> Drop for DomInput<H> {
    /// Listener removal precedes removing the DOM subtree and dropping host state.
    fn drop(&mut self) {
        self.listeners.clear();
        self.motion.cancel();
        self.monitor = None;
        let _ = self.unlock_keyboard();
        self.container.remove();
    }
}

/// Editable focus and rendering must share one CSS coordinate origin.
fn elements(
    parent: &HtmlElement,
    canvas: &HtmlCanvasElement,
) -> Result<(HtmlElement, HtmlTextAreaElement), JsValue> {
    let document = parent
        .owner_document()
        .ok_or_else(|| JsValue::from_str("Detached host"))?;
    let container: HtmlElement = document.create_element("div")?.dyn_into()?;
    let textarea: HtmlTextAreaElement = document.create_element("textarea")?.dyn_into()?;
    container.style().set_property("position", "relative")?;
    container.style().set_property("width", "100%")?;
    container.style().set_property("height", "100%")?;
    canvas.style().set_property("display", "block")?;
    // Host CSS determines size; the backing dimensions never grow the container.
    canvas.style().set_property("width", "100%")?;
    canvas.style().set_property("height", "100%")?;
    textarea.set_attribute("aria-label", "Terminal input")?;
    for (key, value) in [
        ("autocapitalize", "off"),
        ("autocomplete", "off"),
        ("autocorrect", "off"),
        ("spellcheck", "false"),
    ] {
        textarea.set_attribute(key, value)?;
    }
    // display:none would prevent focus and the OS input-method candidate UI.
    textarea.style().set_css_text(
        "position:absolute;opacity:0;width:1px;height:1px;padding:0;border:0;resize:none",
    );
    container.append_child(canvas)?;
    container.append_child(&textarea)?;
    parent.append_child(&container)?;
    Ok((container, textarea))
}
