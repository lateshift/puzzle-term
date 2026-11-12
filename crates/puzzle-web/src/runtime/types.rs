//! TypeScript option and event shapes are shipped beside generated class methods.
//! wasm-bindgen inserts these interfaces into its .d.ts, while Rust remains
//! the authority for runtime validation and documentation of method behavior.
use wasm_bindgen::prelude::*;

/// R1 documentation is preserved verbatim inside the generated declaration file.
#[wasm_bindgen(typescript_custom_section)]
const TYPES: &str = include_str!("../../types/terminal.d.ts");
