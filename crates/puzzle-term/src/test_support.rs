//! Deterministic Slug, font-pack and shaping fixtures for test hosts.
//!
//! Production wasm excludes this module. Native harnesses and the optional
//! browser review build enable it explicitly when rendering milestone scenes.
/// First-frame text from validated prebuilt Slug packs, without a font file.
pub mod pack_scene;
/// Shared 100,000-line incremental search workload for native and Chrome probes.
pub mod search_probe;
/// Deterministic M3 geometry and real-font scenes.
pub mod slug_scene;
/// M4 text scenes built from backend-neutral placements.
pub mod text_scene;

/// Approved test-font layers, brushes and CPU oracle evidence.
pub mod color_scene;

/// Public overlay callbacks used by native and browser review fixtures.
pub mod overlay_scene;
