//! Authored real-PTY workloads with private child-receipt journals.
//!
//! The shell wrapper ignores the login flag supplied by normal PTY creation.
//! Python is explicitly the interpreter: the script is never executed as a
//! shell command without permission, avoiding an earlier setup error.
//! Both children set raw input before reporting ready. The flood uses one fixed
//! reusable glyph set, and the sibling builds real primary-screen scrollback.
//! No shell startup files, personal commands or third-party assets are generated.
//! The private shell file is executable because portable-pty starts it directly.
//! Its shebang selects /bin/sh, whose only work is an explicit Python exec.
//! Login-shell arguments cannot accidentally become Python command-line options.
//! The role is passed through child environment values, not command interpolation.
//! Every readiness file appears after raw mode, so keys never hit a line editor.
//! The second child never echoes a key: that would move the scrolling viewport.
//! Its read endpoint still records the exact bytes and a shared-clock timestamp.
//! Line-buffered receipts become visible without waiting for child termination.
//! The reported timestamp precedes file IO, separating delivery from journal cost.
//! Flood writes are larger than a PTY read and naturally exercise pool backpressure.
//! Waiting on an idle child consumes no polling loop or periodic output wake.
//! Neither child exits before the host cancels it after the measured interval.
//! That also exercises App shutdown with a live flood and a blocked idle read.
//! The generated files stay under target and are never shipped or staged.
use std::ffi::OsString;

/// Creates only owned ignored fixtures; the probe directory belongs to this run.
pub(super) fn prepare(out: &std::path::Path) -> Result<(), String> {
    std::fs::write(out.join("producer.py"), PROGRAM).map_err(|e| e.to_string())?;
    std::fs::write(
        out.join("shell.sh"),
        "#!/bin/sh\nexec /usr/bin/python3 -u \"$PROBE_DIR/producer.py\"\n",
    )
    .map_err(|e| e.to_string())?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(out.join("shell.sh"), std::fs::Permissions::from_mode(0o755))
            .map_err(|e| e.to_string())?;
    }
    Ok(())
}
/// Child-specific environment is copied at creation, never set process-globally.
pub(super) fn environment(
    out: &std::path::Path,
    index: usize,
    flood: bool,
) -> Vec<(OsString, OsString)> {
    vec![
        ("PROBE_DIR".into(), out.as_os_str().to_owned()),
        (
            "PROBE_ROLE".into(),
            if index == 1 {
                "sibling"
            } else if flood {
                "flood"
            } else {
                "idle"
            }
            .into(),
        ),
    ]
}

/// Unix epoch nanoseconds pair the two processes; monotonic clocks retain intervals independently.
const PROGRAM: &str = r#"import os, select, time, tty
role = os.environ['PROBE_ROLE']
out = os.environ['PROBE_DIR']
tty.setraw(0)
if role == 'sibling':
    os.write(1, b'scroll ABCDEFGHIJKLMNOPQRSTUVWXYZ abcdefghijklmnopqrstuvwxyz\r\n' * 5000)
else:
    os.write(1, b'owned shared-App flood fixture\r\n')
open(out + '/' + role + '.ready', 'w').close()
receipt = open(out + '/' + role + '.keys', 'w', buffering=1)
block = b'\x1b[38;2;80;200;160mshared App flood ABCDEFGHIJKLMNOPQRSTUVWXYZ\r\n' * 256
while True:
    if select.select([0], [], [], 0 if role == 'flood' else 1)[0]:
        data = os.read(0, 4096)
        if not data: break
        for byte in data:
            receipt.write(str(byte) + ' ' + str(time.time_ns()) + '\n')
    if role == 'flood': os.write(1, block)
"#;
