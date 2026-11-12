//! OSC 114: xterm ctlseqs colour set/query/reset with BEL and ST.
/// OSC 114 must share the same grammar and table as all other colour numbers.
#[test]
fn set_query_reset_and_malformed() {
    super::reset(114);
}
