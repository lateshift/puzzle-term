//! Public wire catalogue shared by code generation and validation (design §5.7).
//!
//! The table order is fixed because each position is also a hook-mask bit.
//! Command and event domains are grouped by the subsystem that owns them.
//! Names are stable external protocol strings; a new spelling requires a
//! migration instead of changing an existing row. Internal rows have names
//! for diagnostics, but wire decoding rejects them before construction.
//! Reserved rows describe their future wire shape without constructing an
//! untyped message before its Rust payload type exists.
//! The explicit direction field lets generated references distinguish an
//! externally dispatchable request from a fact that may only be observed.
//! The table includes internal platform results to make their invisibility
//! auditable instead of relying on an undocumented omission.
use puzzle_bus::kinds::{Availability, Direction, KindDef, Shape, Visibility};

/// Defines a command with a stable external identity.
/// A macro keeps all fields present in every row without runtime work.
macro_rules! command {
    ($variant:literal, $name:literal, $visibility:ident, $shape:ident, $status:ident $(($milestone:literal))?, $description:literal) => {
        KindDef {
            variant: $variant,
            name: $name,
            direction: Direction::Command,
            visibility: Visibility::$visibility,
            shape: Shape::$shape,
            availability: Availability::$status $(($milestone))?,
            description: $description,
        }
    };
}

/// Defines a fact with the same catalogue fields as a command.
/// The two macros intentionally hard-code different directions.
macro_rules! event {
    ($variant:literal, $name:literal, $visibility:ident, $shape:ident, $status:ident $(($milestone:literal))?, $description:literal) => {
        KindDef {
            variant: $variant,
            name: $name,
            direction: Direction::Event,
            visibility: Visibility::$visibility,
            shape: Shape::$shape,
            availability: Availability::$status $(($milestone))?,
            description: $description,
        }
    };
}

/// Command catalogue; its indices are the `CommandKinds` mask bits.
/// Keeping each row on one physical line makes the protocol table scannable.
#[rustfmt::skip]
pub const COMMANDS: &[KindDef] = &[
    // A key uses a map because physical code and produced key differ by layout.
    command!("Key", "input/key", Public, Map, Active, "Physical key and modifier state"),
    // Committed text is distinct from physical key events under IME input.
    command!("Text", "input/text", Public, String, Active, "Committed text input"),
    // Preedit stays provisional until an input method commits its text.
    command!("Preedit", "input/preedit", Public, Map, Active, "IME composition update"),
    // Paste enters its own sanitization and bracketed-paste policy path.
    command!("Paste", "input/paste", Public, String, Active, "Pasted text"),
    // Copy names a target rather than exposing the selected bytes directly.
    command!("Copy", "input/copy", Public, Map, Active, "Copy selection request"),
    // Pointer reports combine position, buttons and event phase.
    command!("Mouse", "input/mouse", Public, Map, Active, "Pointer input"),
    // Relative scrolling may be fractional on a high-resolution trackpad.
    command!("Scroll", "viewport/scroll", Public, Number, Active, "Relative viewport movement"),
    // An absolute target needs a line identifier and alignment fields.
    command!("ScrollTo", "viewport/scrollTo", Public, Map, Active, "Absolute viewport target"),
    // Resize can carry both cell counts and physical surface dimensions.
    command!("Resize", "terminal/resize", Public, Map, Reserved("M6"), "Grid or surface dimensions"),
    // Focus is an actual Boolean so browser truthiness cannot alter it.
    command!("Focus", "input/focus", Public, Bool, Active, "Input focus state"),
    // DPR remains a number until the renderer validates device pixels.
    command!("SetDpr", "render/setDpr", Public, Number, Reserved("M11"), "Device-pixel ratio"),
    // Range and search operations get map payloads for future typed options.
    command!("Select", "terminal/select", Public, Map, Active, "Selection operation with inclusive cell endpoints"),
    command!("Search", "terminal/search", Public, Map, Active, "Search operation"),
    // Settings patches and resets keep layer information beside their keys.
    command!("ApplySettings", "settings/apply", Public, Map, Reserved("M6"), "Settings patch and layer"),
    command!("ResetSettings", "settings/reset", Public, Map, Reserved("M6"), "Settings reset and layer"),
    // A profile is named data, not a magic action encoded in the type string.
    command!("ActivateProfile", "settings/activateProfile", Public, String, Reserved("M6"), "Activate a profile"),
    // Zoom steps are small exact integers checked by the wire decoder.
    command!("ZoomFont", "settings/zoomFont", Public, Number, Reserved("M6"), "Font-size step"),
    // System appearance is a platform fact expressed as a command to the core.
    command!("SetSystemAppearance", "settings/systemAppearance", Public, String, Active, "OS light or dark state"),
    // Application requests to the host remain interceptable policy actions.
    command!("ClipboardWrite", "platform/clipboardWrite", Public, Any, Active, "Plain text or HTML with plain alternative clipboard write request"),
    command!("Notify", "platform/notify", Public, Map, Active, "Notification request"),
    command!("SetPointerShape", "platform/setPointerShape", Public, String, Active, "Pointer appearance"),
    command!("RingBell", "platform/ringBell", Public, Null, Active, "Audible bell request"),
    command!("OpenLink", "platform/openLink", Public, String, Active, "Open selected hyperlink"),
    // Async platform results are named for diagnostics but cannot be forged.
    command!("FontLoaded", "internal/fontLoaded", Internal, Bytes, Reserved("M6"), "Loaded font bytes"),
    command!("ClipboardText", "internal/clipboardText", Internal, String, Reserved("M7"), "Clipboard read result"),
    command!("GlyphRasterized", "internal/glyphRasterized", Internal, Null, Active, "Fallback discovery completion wake"),
    // Plugin names are registered individually under this shared mask bit.
    command!("Ext", "ext:<plugin>/<name>", Public, Any, Active, "Registered plugin command"),
    // Child status is a trusted platform fact, not a forgeable JS request.
    command!("ChildExited", "internal/childExited", Internal, Map, Active, "Drained child exit status"),
    // Flushed leader keys remain internal so external callers cannot bypass matching.
    // Shortcut arguments are typed; future JS action registration uses its own API.
    command!("BindingAction", "internal/bindingAction", Internal, Map, Active, "Validated shortcut action"),
    // Unit-tagged hardware deltas preserve OS inertia and notch easing.
    command!("Wheel", "input/wheel", Public, Map, Active, "Hardware wheel delta and pointer hit"),
    // Selection uses signed device coordinates, while Mouse retains xterm's bounds.
    command!("Pointer", "input/pointer", Public, Map, Active, "Pointer gesture with signed grid-relative position and cell geometry"),
    // Session-owned model callbacks return typed operations for the normal bus policy.
    command!("OverlayAction", "internal/overlayAction", Internal, Any, Active, "Built-in pointer operation on Session models"),
    // Read policy is separate from user paste and cannot be forged on the wire.
    // Read policy is trusted platform plumbing.
    command!("ClipboardRead", "internal/clipboardRead", Internal, Null, Active, "Policy-approved MIME clipboard read"),
    // Notification metadata is typed so native hosts never parse OSC themselves.
    command!("Drop", "internal/drop", Internal, Map, Active, "Native drop target lifecycle"),
    command!("DesktopNotification", "internal/desktopNotification", Internal, Map, Active, "Structured notification request"),
];

/// Event catalogue; its indices are the `EventKinds` mask bits.
/// Facts are externally observable, but cannot be dispatched as commands.
#[rustfmt::skip]
pub const EVENTS: &[KindDef] = &[
    // OSC title and current-directory updates have separate subscribers.
    event!("TitleChanged", "terminal/titleChanged", Public, String, Active, "Visible title changed"),
    event!("IconNameChanged", "terminal/iconNameChanged", Public, String, Active, "Icon title changed"),
    event!("CwdChanged", "terminal/cwdChanged", Public, String, Active, "Working directory changed"),
    // A BEL fact precedes any policy decision to make sound or flash.
    event!("Bell", "terminal/bell", Public, Null, Active, "BEL arrived"),
    // OSC 133 phases stay distinct so shell timelines can be reconstructed.
    event!("PromptStarted", "shell/promptStarted", Public, Null, Active, "OSC 133 prompt began"),
    event!("CommandStarted", "shell/commandStarted", Public, Map, Active, "OSC 133 command began with optional bounded command line"),
    event!("CommandFinished", "shell/commandFinished", Public, Map, Active, "Exit code and duration"),
    // Mode changes and screen switches have different state lifetimes.
    event!("ModeChanged", "terminal/modeChanged", Public, Map, Reserved("M5"), "Terminal mode changed"),
    event!("ScreenSwitched", "terminal/screenSwitched", Public, String, Reserved("M5"), "Active screen changed"),
    event!("GridResized", "terminal/gridResized", Public, Map, Active, "Grid dimensions changed"),
    // Viewport motion is separate from application-visible grid updates.
    event!("ViewportScrolled", "viewport/scrolled", Public, Number, Active, "Viewport offset changed"),
    // UI subscribers can update selection and link affordances independently.
    event!("SelectionChanged", "terminal/selectionChanged", Public, Map, Active, "Selection changed"),
    event!("HyperlinkHovered", "terminal/hyperlinkHovered", Public, Map, Reserved("M8"), "Hovered link changed"),
    event!("SettingsChanged", "settings/changed", Public, Map, Reserved("M6"), "Settings keys changed"),
    event!("ThemeChanged", "settings/themeChanged", Public, Map, Active, "Active theme changed"),
    // Font availability may change after initial rendering is already live.
    event!("FontsChanged", "render/fontsChanged", Public, Null, Reserved("M6"), "Font availability changed"),
    // Frame data and output byte counts are opt-in to protect hot paths.
    event!("FrameRendered", "render/frameRendered", Public, Map, Active, "Opt-in frame statistics"),
    event!("OutputChunk", "terminal/outputChunk", Public, Number, Reserved("M5"), "Feed-call byte count"),
    // Loss and restoration are separate lifecycle edges for consumers.
    event!("DeviceLost", "render/deviceLost", Public, Null, Reserved("M11"), "GPU device lost"),
    event!("DeviceRestored", "render/deviceRestored", Public, Null, Reserved("M11"), "GPU device recovered"),
    // Each concrete plugin event gets a stable registered name at runtime.
    event!("Ext", "ext:<plugin>/<name>", Public, Any, Active, "Registered plugin event"),
    // OSC 133 B distinguishes prompt editing from command output.
    event!("InputStarted", "shell/inputStarted", Public, Null, Active, "OSC 133 input began"),
    // ConEmu-style progress is a fact, not a host notification request.
    event!("Progress", "terminal/progress", Public, Map, Active, "OSC 9;4 progress changed"),
    // Exit and close requests remain distinct so hosts can implement hold.
    event!("SessionExited", "session/exited", Public, Map, Active, "Drained process exit"),
    event!("SessionCloseRequested", "session/closeRequested", Public, Null, Active, "Key pressed after held exit"),
    // M9 observes the M8 model state to draw its new-output pill.
    event!("NewOutputChanged", "viewport/newOutputChanged", Public, Bool, Active, "New output while viewing history"),
    // The embedding page owns search UI; the engine publishes only this request.
    event!("SearchRequested", "terminal/searchRequested", Public, Null, Active, "Focus the host search interface"),
    // Immutable current context path, emitted only on a metadata/lifecycle change.
    event!("ContextsChanged", "terminal/contextsChanged", Public, Map, Active, "Active UAPI context path after an owner parser drain"),
];
