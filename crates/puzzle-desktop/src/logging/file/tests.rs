//! Rotating file tests use small caps and unique paths under target.
//!
//! The test writes complete records through the same MakeWriter seam used
//! by tracing-subscriber. A byte-level comparison catches partial lines
//! and a previous run overwritten at the wrong time.

use super::{FileSink, MakeWriter};
use std::{fs, io::Write, path::PathBuf};

/// Each run keeps only its immediately preceding file.
#[test]
fn startup_and_size_rotation_keep_one_previous_run() -> Result<(), Box<dyn std::error::Error>> {
    // Cargo runs tests from the crate directory; keep diagnostics in workspace target.
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../target")
        .join(format!("log-test-{}.txt", std::process::id()));
    let previous = PathBuf::from(format!("{}.1", path.display()));
    // A prior run is moved aside before the first new record arrives.
    fs::create_dir_all(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target"))?;
    fs::write(&path, b"old\n")?;
    let sink = FileSink::open_limit(&path, 8)?;
    assert_eq!(fs::read(&previous)?, b"old\n");
    {
        let mut writer = sink.make_writer();
        writer.write_all(b"first\n")?;
        writer.write_all(b"second\n")?;
    }
    assert_eq!(fs::read(&previous)?, b"first\n");
    assert_eq!(fs::read(&path)?, b"second\n");
    // The test owns these fixed build artifacts and removes them afterward.
    fs::remove_file(&path)?;
    fs::remove_file(&previous)?;
    Ok(())
}
