//! One theme-default/override table for xterm and kitty colour protocols (M18.1).
//! Cells keep tagged colours; updates invalidate paint without rewriting rows.
//! Both protocol families address these same entries. A palette query after a
//! kitty set therefore observes the exact value an xterm set would install.
//! This table owns values, not rendering resources, and can cross the Session
//! publication boundary by copy. No renderer borrows mutable terminal state.
//! Palette indexes stay in cells, letting a colour edit recolour existing text
//! without rewriting thousands of cells or changing their logical identities.
//! RIS clears overrides, preserving host defaults. Theme replacement installs
//! defaults and clears every override deliberately, as design §14.7 requires.

/// Historical unconfigured VT defaults and the standard xterm colour cube.
mod defaults;
/// Typed palette, special and dynamic addresses.
mod roles;
pub use defaults::palette;
pub use roles::{ColorSlot, DynamicColor, SpecialColor};

/// A concrete colour, or a role whose colour depends on the rendered cell.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ColorValue {
    /// Opaque sRGB bytes, quantized once by the shared colour-spec parser.
    Rgb([u8; 3]),
    /// No fixed value; OSC 21 reports an empty value for this state.
    Dynamic,
}

/// Converts protocol state to fixed bytes without inventing a dynamic value.
impl ColorValue {
    /// Returns only fixed RGB values; dynamic roles use a rendering fallback.
    pub fn rgb(self) -> Option<[u8; 3]> {
        match self {
            Self::Rgb(rgb) => Some(rgb),
            Self::Dynamic => None,
        }
    }
}

/// One colour's theme default and optional application override.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct Entry {
    /// Installed by the host theme, unchanged by OSC requests.
    default: ColorValue,
    /// Removed by a matching reset, RIS or any theme/appearance change.
    override_value: Option<ColorValue>,
}

/// Keeps default installation separate from application changes.
impl Entry {
    /// Creates an unmodified role; construction is independent of protocol input.
    fn new(default: ColorValue) -> Self {
        Self {
            default,
            override_value: None,
        }
    }
    /// Resolves an application value before the theme fallback.
    fn effective(self) -> ColorValue {
        self.override_value.unwrap_or(self.default)
    }
}

/// Complete terminal colour state, copied once into each sealed publication.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ColorTable {
    /// Palette addresses 0–255; aliases 256–260 address `special` instead.
    palette: [Entry; 256],
    /// OSC 5's five special roles, including their OSC 4 aliases.
    special: [Entry; 5],
    /// OSC 10–19 and kitty cursor_text/visual_bell share these entries.
    dynamic: [Entry; 12],
}

/// Initializes the standalone VT before optional host theme installation.
impl Default for ColorTable {
    /// Preserves the pre-M18 unconfigured harness appearance.
    fn default() -> Self {
        Self {
            palette: std::array::from_fn(|i| Entry::new(ColorValue::Rgb(palette(i as u8)))),
            special: [Entry::new(ColorValue::Rgb([229; 3])); 5],
            dynamic: defaults::dynamic(),
        }
    }
}

/// Shared operations; wire parsers never maintain their own colour copies.
impl ColorTable {
    /// Resolves a slot identically for xterm queries, OSC 21 and drawing.
    pub fn get(&self, slot: ColorSlot) -> ColorValue {
        self.entry(slot).effective()
    }
    /// Exposes override presence for cursor-setting precedence and compatibility views.
    pub fn override_value(&self, slot: ColorSlot) -> Option<ColorValue> {
        self.entry(slot).override_value
    }
    /// Sets one application override; palette entries cannot become dynamic.
    pub fn set(&mut self, slot: ColorSlot, value: ColorValue) {
        // Kitty permits undefined semantic roles, but palette entries need RGB.
        if matches!(slot, ColorSlot::Palette(_)) && value == ColorValue::Dynamic {
            return;
        }
        self.entry_mut(slot).override_value = Some(value);
    }
    /// Resets one slot without altering any other colour or default.
    pub fn reset(&mut self, slot: ColorSlot) {
        // Reset reveals the currently installed theme, not hard-coded xterm bytes.
        self.entry_mut(slot).override_value = None;
    }
    /// Replaces a theme default; callers clear overrides once for the whole theme.
    pub fn set_default(&mut self, slot: ColorSlot, value: ColorValue) {
        // Fade snapshots reuse this operation while keeping their own overrides.
        self.entry_mut(slot).default = value;
    }
    /// Clears every application change, including roles the renderer does not draw.
    pub fn clear_overrides(&mut self) {
        // This includes stored-only roles so switching appearance is one transaction.
        for entry in self
            .palette
            .iter_mut()
            .chain(&mut self.special)
            .chain(&mut self.dynamic)
        {
            entry.override_value = None;
        }
    }
    /// Resets OSC 104/105's entire palette or special group independently.
    pub fn reset_group(&mut self, special: bool) {
        // OSC 104 must not reset OSC 5 state, and OSC 105 must not reset the palette.
        let entries: &mut [Entry] = if special {
            &mut self.special
        } else {
            &mut self.palette
        };
        for entry in entries {
            entry.override_value = None;
        }
    }
    /// Range-checked numeric address used by OSC 4, including xterm special aliases.
    pub fn indexed(index: u16) -> Option<ColorSlot> {
        // Xterm reserves the next five numeric addresses as OSC 5 aliases.
        if index < 256 {
            Some(ColorSlot::Palette(index as u8))
        } else {
            SpecialColor::from_index(index - 256).map(ColorSlot::Special)
        }
    }
    /// Borrows a typed slot; enum construction guarantees all array bounds.
    fn entry(&self, slot: ColorSlot) -> &Entry {
        match slot {
            ColorSlot::Palette(i) => &self.palette[usize::from(i)],
            ColorSlot::Special(role) => &self.special[role as usize],
            ColorSlot::Dynamic(role) => &self.dynamic[role as usize],
        }
    }
    /// Mutably borrows the same slot, never a protocol-specific duplicate.
    fn entry_mut(&mut self, slot: ColorSlot) -> &mut Entry {
        match slot {
            ColorSlot::Palette(i) => &mut self.palette[usize::from(i)],
            ColorSlot::Special(role) => &mut self.special[role as usize],
            ColorSlot::Dynamic(role) => &mut self.dynamic[role as usize],
        }
    }
}
