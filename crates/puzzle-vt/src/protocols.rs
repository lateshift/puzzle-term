//! Extension wire handlers grouped by defining project (D-58).
//! Common ECMA-48, DEC and xterm dispatch stays under perform; models stay shared.
/// apple origin-specific interpretation; see the module reference.
pub(crate) mod apple;
/// conemu origin-specific interpretation; see the module reference.
pub(crate) mod conemu;
/// contour origin-specific interpretation; see the module reference.
pub(crate) mod contour;
/// finalterm origin-specific interpretation; see the module reference.
pub(crate) mod finalterm;
/// iterm2 origin-specific interpretation; see the module reference.
pub(crate) mod iterm2;
/// kitty origin-specific interpretation; see the module reference.
pub(crate) mod kitty;
/// rockorager origin-specific interpretation; see the module reference.
pub(crate) mod rockorager;
/// rxvt origin-specific interpretation; see the module reference.
pub(crate) mod rxvt;
/// Shared string sanitation, separate from origin-specific interpretation.
pub(crate) mod strings;
/// vte origin-specific interpretation; see the module reference.
pub(crate) mod vte;

/// UAPI hierarchical execution-context signalling (OSC 3008).
pub(crate) mod uapi;
