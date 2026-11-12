//! M7's minimal JS surface delegates to the neutral Session (§12.1).
//!
//! No public buffer, settings facade or middleware API is exposed yet (M10).
//! Disposables carry weak ownership so subscriptions cannot retain a terminal.
/// Public handle types accompany the exported terminal class.
pub use super::handles::{Connection, Disposable};
use super::*;
use puzzle_term::{
    Origin,
    messages::{Command, ExitInfo},
};

/// One rendered terminal; its resources are released by explicit dispose.
#[wasm_bindgen]
pub struct PuzzleTerminal {
    /// Only this wrapper owns the instance; DOM closures refer to it weakly.
    pub(super) owner: Owner,
}

/// The subset is intentionally identical for DOM and programmatic input.
#[wasm_bindgen]
impl PuzzleTerminal {
    /// This query never mounts DOM or starts a terminal session. Hosts can
    /// show an unsupported-browser message before allocating a canvas.
    /// The result describes the browser capability, not a connected backend.
    /// Reports WebGPU availability without creating a canvas or throwing.
    #[wasm_bindgen(js_name = isSupported, unchecked_return_type = "Support")]
    pub async fn is_supported() -> Result<JsValue, JsValue> {
        // Capability discovery does not claim device creation or backend connectivity succeeded.
        let present = value::get(&window()?.navigator().into(), "gpu")?.is_object();
        let result = js_sys::Object::new();
        js_sys::Reflect::set(&result, &"webgpu".into(), &present.into())?;
        // Hosts can show a useful fallback reason without catching a thrown GPU error.
        if !present {
            js_sys::Reflect::set(
                &result,
                &"reason".into(),
                &"WebGPU is unavailable in this browser".into(),
            )?;
        }
        Ok(result.into())
    }
    /// `host` owns CSS dimensions; devicePixelRatio chooses backing pixels.
    /// `options.settings` enters the session layer of the shared validator.
    /// Creation errors return a PuzzleError rather than a partly mounted API.
    /// Fonts remain external assets and can arrive after the initial pack frame.
    /// Mounts a canvas and native textarea, sharing GPU/text resources per page.
    pub async fn create(
        host: web_sys::HtmlElement,
        #[wasm_bindgen(unchecked_optional_param_type = "TerminalOptions")] options: Option<JsValue>,
    ) -> Result<PuzzleTerminal, JsValue> {
        Ok(Self {
            owner: super::create(host, options.unwrap_or(JsValue::UNDEFINED)).await?,
        })
    }
    /// Disposal is idempotent and does not call subscribers with stale state.
    /// Removing the last terminal's strong references permits weak registry
    /// entries to expire; subscriptions cannot keep those resources alive.
    /// Pending output promises reject instead of waiting on a dead parser.
    /// Releases listeners, connections and strong shared-resource references.
    pub fn dispose(&self) -> Result<(), JsValue> {
        // Cancel work first; callbacks and resources leave ownership together after the borrow.
        let (dom, text, surface, pending, transport) = {
            let mut runtime = self.owner.borrow_mut();
            if runtime.disposed {
                return Ok(());
            }
            // Reentrant cleanup and retained Disposables must observe a closed instance.
            runtime.disposed = true;
            // Parser tasks are independent of visibility and rAF; cancel their browser handle too.
            runtime.continuation = None;
            if let Some(id) = runtime.raf.take() {
                window()?.cancel_animation_frame(id)?;
            }
            runtime.draw = None;
            if let Some(id) = runtime.timer_id.take() {
                window()?.clear_timeout_with_handle(id);
            }
            runtime.timer = None;
            runtime.data.clear();
            runtime.events.clear();
            let transport = runtime.transport.take();
            runtime.socket = None;
            (
                runtime.dom.take(),
                runtime.text.take(),
                runtime.surface.take(),
                std::mem::take(&mut runtime.waiting),
                transport,
            )
        };
        drop(dom);
        drop(surface);
        drop(text);
        output::reject(pending);
        // Host cleanup may call back into JS, so it runs after releasing the Session borrow.
        output::close_transport(transport)?;
        Ok(())
    }
    /// Application output changes the grid; it is not sent back as stdin.
    /// A Uint8Array preserves split UTF-8 and VT sequences across writes.
    /// Completion is independent of page visibility and GPU presentation.
    /// A parser-generated reply is handed to the transport before resolving.
    /// Resolves after this output prefix is parsed and its replies flushed.
    #[wasm_bindgen(unchecked_return_type = "Promise<void>")]
    pub fn write(
        &self,
        #[wasm_bindgen(unchecked_param_type = "string | Uint8Array")] data: JsValue,
    ) -> Result<Promise, JsValue> {
        output::write(&self.owner, value::bytes(data)?)
    }
    /// CR returns to the first column and LF advances the line under the
    /// application's active newline/scroll-region modes. This is output,
    /// unlike input(), which sends bytes towards the application.
    /// Appends CR LF as terminal output, retaining the same completion guarantee.
    #[wasm_bindgen(unchecked_return_type = "Promise<void>")]
    pub fn writeln(&self, data: String) -> Result<Promise, JsValue> {
        output::write(&self.owner, format!("{data}\r\n").into_bytes())
    }
    /// The count is actual parser backlog, not WebSocket bytes received.
    /// Hosts can use it to implement backpressure for callback transports.
    /// Bridge output uses its own ACK ledger with the same parsed-byte source.
    /// Queued output not yet parsed; excludes input waiting on server credit.
    #[wasm_bindgen(getter, js_name = pendingBytes)]
    pub fn pending_bytes(&self) -> Result<f64, JsValue> {
        let runtime = self.owner.borrow();
        runtime.ensure_live()?;
        Ok((runtime.parsed.accepted - runtime.parsed.consumed) as f64)
    }
    /// Each callback receives a Uint8Array containing terminal-encoded bytes.
    /// It observes keys, paste and replies in application order; it does not
    /// receive the application's own output. The returned handle removes
    /// exactly this registration, leaving other subscribers unchanged.
    /// Receives encoded keys and replies immediately, before any frame.
    #[wasm_bindgen(js_name = onData)]
    pub fn on_data(
        &self,
        #[wasm_bindgen(unchecked_param_type = "(data: Uint8Array) => void")] callback: Function,
    ) -> Result<Disposable, JsValue> {
        // Each registration gets an identity independent of callback function equality.
        let mut runtime = self.owner.borrow_mut();
        runtime.ensure_live()?;
        runtime.next_id += 1;
        let id = runtime.next_id;
        runtime.data.push((id, callback));
        Ok(Disposable {
            owner: Rc::downgrade(&self.owner),
            id,
        })
    }
    /// Text passes through bus policy with Api origin. It is not parsed as
    /// application output, and therefore cannot paint a character locally.
    /// The program's response is what later changes the terminal grid.
    /// Sends committed text through the typed input bus and current encoder.
    pub fn input(&self, text: String) -> Result<(), JsValue> {
        self.command(Command::Text(text))
    }
    /// Bracketed paste is controlled by DEC mode 2004, not a page heuristic.
    /// Control filtering and delimiter handling use the same core path as
    /// native paste events, so programmatic paste cannot bypass policy.
    /// Sanitizes and brackets text according to the application's paste mode.
    pub fn paste(&self, text: String) -> Result<(), JsValue> {
        self.command(Command::Paste(text))
    }
    /// M8 also supplies scroll, selectionChange and searchRequested facts.
    /// Scroll measures distance above the tail; selection carries expanded cells.
    /// Search requests let the embedding page focus its own DOM field.
    /// Supported names are validated before allocating a subscription.
    /// Terminal facts use bus observers; focus ownership comes from the DOM.
    /// A throwing subscriber is reported without rewriting an existing fact.
    /// Subscribes to one M7 convenience event; unsupported names are rejected.
    #[wasm_bindgen(skip_typescript)]
    // Link callbacks receive a cancelable event before a platform window opens.
    pub fn on(&self, event: String, callback: Function) -> Result<Disposable, JsValue> {
        if ![
            "title",
            "bell",
            "resize",
            "focus",
            "blur",
            "error",
            "scroll",
            "selectionChange",
            "searchRequested",
            "linkOpen",
        ]
        .contains(&event.as_str())
        {
            return Err(value::error("InvalidArgument", "unsupported M7 event"));
        }
        // Each registration gets an identity independent of callback function equality.
        let mut runtime = self.owner.borrow_mut();
        runtime.ensure_live()?;
        runtime.next_id += 1;
        let id = runtime.next_id;
        runtime.events.push((id, event, callback));
        Ok(Disposable {
            owner: Rc::downgrade(&self.owner),
            id,
        })
    }
    /// `key` is the logical W3C identity; optional `code` is its physical key.
    /// Modifiers do not become terminal bytes until current application modes
    /// are sampled. Kitty and legacy output therefore share one encoder.
    /// Encodes one W3C-shaped key using the same table as native DOM events.
    #[wasm_bindgen(js_name = sendKey)]
    pub fn send_key(
        &self,
        #[wasm_bindgen(unchecked_param_type = "KeySpec")] spec: JsValue,
    ) -> Result<(), JsValue> {
        // Validate optional fields before creating an input command with Api origin.
        let event = super::arguments::key_event(&spec)?;
        self.owner.borrow().ensure_live()?;
        let host = host::Host(Rc::downgrade(&self.owner));
        use crate::dom::DomHost;
        // Releases are requested by kitty flag 2; legacy mode intentionally emits nothing.
        if event.type_() == "keyup" && host.key_policy().kitty_flags & 2 == 0 {
            return Ok(());
        }
        let mapped = crate::dom::keyboard::Keyboard::default()
            .map(&event, host.key_policy())
            .ok_or_else(|| value::error("InvalidArgument", "key cannot be encoded"))?;
        self.command(Command::Key(mapped.event))
    }
    /// Focus targets the native editing element, never the rendering canvas.
    /// The runtime borrow ends before focus() dispatches synchronous DOM
    /// events, so application focus reports cannot reenter a borrowed Session.
    /// Focuses the textarea, preserving the browser's IME and clipboard path.
    pub fn focus(&self) -> Result<(), JsValue> {
        let textarea = {
            let r = self.owner.borrow();
            r.ensure_live()?;
            r.dom
                .as_ref()
                .ok_or_else(|| value::error("NotRendered", "no textarea"))?
                .textarea()
                .clone()
        };
        textarea.focus()
    }
    /// A browser blur event clears tracked modifiers and IME composition.
    /// It also lets the Session draw its inactive hollow cursor and send
    /// DEC mode 1004 reports if the application enabled them.
    /// Removes focus without holding a core borrow during native DOM dispatch.
    pub fn blur(&self) -> Result<(), JsValue> {
        let textarea = {
            let r = self.owner.borrow();
            r.ensure_live()?;
            r.dom
                .as_ref()
                .ok_or_else(|| value::error("NotRendered", "no textarea"))?
                .textarea()
                .clone()
        };
        textarea.blur()
    }
    /// This is browser focus, not whether the application's cursor is visible.
    /// Several terminals can share rendering resources while only one owns
    /// the page's active textarea and receives keyboard/IME input.
    /// True exactly when this terminal's textarea owns browser focus.
    #[wasm_bindgen(getter, js_name = hasFocus)]
    pub fn has_focus(&self) -> Result<bool, JsValue> {
        let runtime = self.owner.borrow();
        runtime.ensure_live()?;
        Ok(runtime.dom.as_ref().is_some_and(|dom| {
            window()
                .ok()
                .and_then(|w| w.document())
                .and_then(|d| d.active_element())
                .is_some_and(|active| active == dom.textarea().clone().into())
        }))
    }
    /// The required write callback receives stdin bytes and parser replies.
    /// Optional resize and close callbacks carry host lifecycle operations.
    /// The host feeds application output with write() and reports drained
    /// process exit with exited(); no native PTY is compiled into wasm.
    /// Attaches a host such as Electron/node-pty without shipping a backend.
    #[wasm_bindgen(js_name = attachTransport)]
    pub fn attach_transport(
        &self,
        #[wasm_bindgen(unchecked_param_type = "HostTransport")] transport: JsValue,
    ) -> Result<Disposable, JsValue> {
        super::arguments::transport(&transport)?;
        // Each registration gets an identity independent of callback function equality.
        let mut runtime = self.owner.borrow_mut();
        runtime.ensure_live()?;
        runtime.next_id += 1;
        let id = runtime.next_id;
        runtime.transport = Some(transport);
        runtime.transport_id = id;
        Ok(Disposable {
            owner: Rc::downgrade(&self.owner),
            id,
        })
    }
    /// Raw transports send encoded bytes without development framing.
    /// puzzle-bridge-v1 performs hello, resize and bidirectional credit.
    /// The returned connection controls this socket incarnation only.
    /// Connecting does not bundle or start a server. A raw endpoint supplies
    /// its own authentication; bridge tokens stay in the caller's private URL.
    /// Output acknowledgements return parsed-byte credit, independently of
    /// input credit, so an echoing child cannot strand its own socket controls.
    /// Connects an application-supplied backend or the explicit local dev bridge.
    pub fn connect(
        &self,
        url: String,
        #[wasm_bindgen(unchecked_optional_param_type = "ConnectOptions")] options: Option<JsValue>,
    ) -> Result<Connection, JsValue> {
        // The host selects framing explicitly; raw never emits development resize or credit.
        let protocol = options
            .as_ref()
            .map(|o| arguments::optional_string(o, "protocol"))
            .transpose()?
            .flatten()
            .unwrap_or_else(|| "raw".into());
        if !["raw", "puzzle-bridge-v1"].contains(&protocol.as_str()) {
            return Err(value::error(
                "InvalidArgument",
                "unsupported transport protocol",
            ));
        }
        let id = socket::connect(&self.owner, &url, protocol == "puzzle-bridge-v1")?;
        Ok(Connection {
            owner: Rc::downgrade(&self.owner),
            id,
        })
    }
    /// Call this only after delivering the application's final output.
    /// Unknown codes remain unknown rather than inventing a numeric status.
    /// Input then follows the Session's held-screen close policy.
    /// Reports drained process exit; web defaults hold the final screen.
    pub fn exited(
        &self,
        #[wasm_bindgen(unchecked_param_type = "ExitInfo")] info: JsValue,
    ) -> Result<(), JsValue> {
        // Exit validation cannot invent success for a malformed numeric status.
        let code = super::arguments::exit_code(&info)?;
        let signal = super::arguments::optional_string(&info, "signal")?;
        let runtime_ms = self
            .owner
            .borrow()
            .started
            .elapsed()
            .as_millis()
            .min(u128::from(u64::MAX)) as u64;
        {
            let mut r = self.owner.borrow_mut();
            r.ensure_live()?;
            // Final output may still be queued under the parser budget.
            // The lifecycle notice cannot overtake that accepted prefix.
            r.exit = Some(ExitInfo {
                code,
                signal,
                runtime_ms,
            });
        }
        output::finish_exit(&self.owner)?;
        Ok(())
    }
}

/// Non-exported core helper keeps input origin explicit and synchronous.
impl PuzzleTerminal {
    /// Programmatic input has Api origin and never waits for presentation.
    fn command(&self, command: Command) -> Result<(), JsValue> {
        {
            let mut runtime = self.owner.borrow_mut();
            runtime.ensure_live()?;
            // Programmatic input cannot impersonate a trusted platform mode change.
            runtime
                .apply(puzzle_term::SessionMessage::Command(command, Origin::Api))
                .map_err(|e| value::error("InvalidArgument", &e))?;
        }
        // API keys use the same host-action lane as DOM keys, including zoom/profile actions.
        settings::actions(&self.owner);
        output::flush(&self.owner)?;
        draw::schedule(&self.owner)
    }
}
