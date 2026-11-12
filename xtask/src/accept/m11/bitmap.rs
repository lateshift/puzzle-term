//! M11.5 checks native bitmap parsing separately from installed GPU artwork.
//!
//! Synthetic authored tables pin strike choice and absence/error distinctions.
//! A corrupt preferred image must fall through without changing cell placement.
//! The area-filter reference includes hidden green RGB to detect alpha halos.
//! Real PNG streams cover channel conversion and truncation in the host codec.
//! Installed emoji are read back at both DPRs without blessing OS-specific goldens.
//! Missing system installation produces an explicit reason in the retained output.
//! Exact names use the shared helper, preventing silently empty test filters.
use super::{report, test};

/// Neutral tests use authored font tables and therefore run without OS fonts.
/// Desktop tests separately validate the approved codec’s real compressed input.
/// Only the final witness requires Metal and an installed emoji collection.
/// Independent reporting lets filter errors remain visible after a codec failure.
/// No native artwork is copied into golden assets or compared across machines.
/// The final desktop checklist remains a user review after M11.7.
/// Runs independent neutral and desktop witnesses, retaining all failures.
pub(super) fn run(passed: &mut bool) {
    // Table selection, corrupt-source continuation and filter math are independent.
    for (label, name) in [
        (
            "strike selection and authored sbix/CBDT tables",
            "strike_selection_and_table_formats",
        ),
        (
            "corrupt PNG tries next source with one font warning",
            "decoder_failure_tries_next_source",
        ),
        (
            "premultiplied fractional area reference",
            "premultiplied_area_reference",
        ),
    ] {
        report(
            &format!("M11.5 {label}"),
            test("puzzle-text", &format!("bitmap::tests::{name}")),
            passed,
        );
    }
    // Only desktop links png; no native codec is allowed in the neutral witnesses.
    report(
        "M11.5 real PNG decoding and failure reports",
        test("puzzle-embed", "bitmap::tests::png_decoding_and_failures"),
        passed,
    );
    // The GPU guard checks both scales and color without claiming appearance quality.
    report(
        "M11.5 installed emoji two-cell Metal ink (or explicit unavailable SKIP)",
        installed(),
        passed,
    );
}

/// Retains the exact installed-font result, including any concrete skip reason.
/// The Cargo helper validates that one test actually executed successfully.
/// Its stdout includes chosen strike ppem, ink count and atlas bytes per DPR.
/// Writing this evidence does not promote an unavailable font to observed ink.
fn installed() -> Result<(), String> {
    let name = "bitmap::tests::installed_emoji_two_cells";
    let output = crate::commands::capture(
        "cargo",
        &[
            "test",
            "-p",
            "puzzle-desktop",
            name,
            "--",
            "--exact",
            "--nocapture",
        ],
    )?;
    std::fs::write("target/m11-bitmap-installed.log", &output).map_err(|e| e.to_string())?;
    if !output.contains(&format!("test {name} ... ok")) {
        return Err("installed emoji witness did not execute".into());
    }
    // A skip is exposed verbatim rather than being confused with a rendered sample.
    for line in output
        .lines()
        .filter(|line| line.starts_with("bitmap dpr=") || line.starts_with("SKIP installed"))
    {
        println!("{line}");
    }
    Ok(())
}
