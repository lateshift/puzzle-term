//! Desktop GPU fixtures join the embedding tests’ exclusion domain.
//!
//! Holding this guard across device teardown prevents a harness readback from
//! racing a moved engine recovery fixture in the same test process.
pub(crate) use puzzle_embed::diagnostics::gpu_test::lock;
