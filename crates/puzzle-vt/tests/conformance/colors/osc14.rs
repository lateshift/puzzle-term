//! OSC 14: xterm ctlseqs colour set/query/reset with BEL and ST.
/// OSC 14 must share the same grammar and table as all other colour numbers.
#[test]
fn set_query_reset_and_malformed() {
    super::dynamic(14);
}
