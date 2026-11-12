//! A copied-policy host for DOM ownership tests; Session is tested separately.
#![allow(
    clippy::unwrap_used,
    reason = "isolated DOM setup is a test prerequisite"
)]
use crate::dom::{
    DomHost, DomInput, Geometry, KeyDisposition,
    keyboard::{KeyPolicy, MappedKey},
};
use puzzle_term::{
    keybind::Platform,
    messages::Command,
    session::OptionAsAlt,
    theme::Appearance,
    vt::input::{MouseEncoding, MouseModes, MouseTracking},
};
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};
use wasm_bindgen::JsCast;
use web_sys::{HtmlCanvasElement, HtmlElement};

/// An isolated mounted subtree and its externally observable command lane.
pub(super) struct Fixture {
    /// The production owner detaches listeners when dropped.
    pub input: DomInput<Host>,
    /// Candidate, text and pointer commands can be counted independently.
    pub commands: Rc<RefCell<Vec<Command>>>,
    /// Tests can switch the application's requested tracking mode mid-gesture.
    pub tracking: Rc<Cell<MouseTracking>>,
    /// Every fixture removes its host from the document after assertions.
    parent: HtmlElement,
}

/// Test state stays in the platform crate; no Rc enters Session or its services.
pub(super) struct Host {
    /// Records only the neutral commands emitted by real listeners.
    commands: Rc<RefCell<Vec<Command>>>,
    /// Active mode changes are sampled per DOM event.
    tracking: Rc<Cell<MouseTracking>>,
}

/// Device coordinates are intentionally different from CSS coordinates at 2×.
pub(super) fn geometry() -> Geometry {
    Geometry {
        dpr: 2.0,
        padding: [4.0; 2],
        cell: [20.0, 40.0],
        grid: [80, 24],
        cursor: [44.0, 84.0],
    }
}

/// Uses actual elements and registrations; no test calls listener helpers directly.
impl Fixture {
    /// A new parent avoids sharing focus/value state between browser witnesses.
    pub(super) fn new() -> Self {
        let document = web_sys::window().unwrap().document().unwrap();
        let parent: HtmlElement = document.create_element("div").unwrap().dyn_into().unwrap();
        // The runner page has a body margin; tests need a known CSS origin.
        parent
            .style()
            .set_css_text("position:fixed;left:0;top:0;width:800px;height:480px");
        let canvas: HtmlCanvasElement = document
            .create_element("canvas")
            .unwrap()
            .dyn_into()
            .unwrap();
        document.body().unwrap().append_child(&parent).unwrap();
        let commands = Rc::new(RefCell::new(Vec::new()));
        let tracking = Rc::new(Cell::new(MouseTracking::None));
        let host = Host {
            commands: commands.clone(),
            tracking: tracking.clone(),
        };
        let input = DomInput::mount(&parent, canvas, host).unwrap();
        Self {
            input,
            commands,
            tracking,
            parent,
        }
    }
}

/// The browser test document must not retain nodes from completed witnesses.
impl Drop for Fixture {
    /// DomInput also removes its inner container and exact listener callbacks.
    fn drop(&mut self) {
        self.parent.remove();
    }
}

/// Fixed query answers isolate DOM ownership from the already-tested encoder.
impl DomHost for Host {
    /// Default macOS Option produces layout text without a layout map.
    fn key_policy(&self) -> KeyPolicy {
        KeyPolicy {
            platform: Platform::Mac,
            option_as_alt: OptionAsAlt::None,
            kitty_flags: 0,
            locked: false,
        }
    }
    /// Plain keys leave insertion native; encoded keys enter the command lane.
    fn key(&mut self, mapped: MappedKey) -> KeyDisposition {
        if mapped.native_text {
            return KeyDisposition::Text;
        }
        self.command(Command::Key(mapped.event));
        KeyDisposition::Consumed
    }
    /// A command is observed at callback time, without a requestAnimationFrame.
    fn command(&mut self, command: Command) {
        self.commands.borrow_mut().push(command);
    }
    /// This DOM-only host has no history; rendered tests cover local boundaries.
    fn wheel_consumable(
        &self,
        _: puzzle_term::viewport::ScrollInput,
        pointer: puzzle_term::vt::input::MouseEvent,
    ) -> bool {
        self.tracking.get() != MouseTracking::None
            && !pointer
                .modifiers
                .has(puzzle_term::vt::input::Modifiers::SHIFT)
    }
    /// Deterministic DOM fixtures allow momentum unless explicitly canceled.
    fn motion_reduced(&self) -> bool {
        false
    }
    /// Fixed padding/cell geometry makes physical mapping errors unambiguous.
    fn geometry(&self) -> Geometry {
        geometry()
    }
    /// Changing tracking through Cell exercises live cancellation decisions.
    fn mouse_modes(&self) -> MouseModes {
        MouseModes {
            tracking: self.tracking.get(),
            encoding: MouseEncoding::Sgr,
        }
    }
    /// Local Shift selection must never cancel the page's default wheel here.
    fn shift_selects(&self) -> bool {
        true
    }
    /// Rendering is absent in this DOM-only fixture.
    fn visibility(&mut self, _: bool) {}
    /// Theme switches are observed separately by the Session integration tests.
    fn appearance(&mut self, _: Appearance) {}
    /// This fixture has no timers or animations to stop.
    fn reduced_motion(&mut self, _: bool) {}
    /// System palette resolution belongs to the rendered host, not input mapping.
    fn forced_colors(&mut self, _: bool) {}
    /// Geometry is fixed for input tests; resize is tested by the browser runtime.
    fn resize(&mut self, _: [u32; 2], _: f64) {}
}
