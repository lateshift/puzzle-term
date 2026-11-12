//! Protect D-46's explicit scheduling boundary without launching Chrome.
use super::*;

/// Only a deliberate supported desktop invocation may omit browser execution.
#[test]
fn final_is_full_and_task_skip_is_explicit() {
    assert!(matches!(parse(&["M16".into()]), Ok((_, BrowserGate::Full))));
    assert!(matches!(
        parse(&["M16".into(), "--skip-browser".into()]),
        Ok((_, BrowserGate::TaskSkip))
    ));
    for args in [
        vec!["M16".into(), "--skip-brower".into()],
        vec!["M16".into(), "--skip-browser".into(), "extra".into()],
        vec!["M7".into(), "--skip-browser".into()],
    ] {
        assert!(parse(&args).is_err());
    }
}

/// Skipped results cannot start a child or label an unexecuted failure as PASS.
#[test]
fn skip_does_not_evaluate_browser_criterion() {
    let mut passed = true;
    BrowserGate::TaskSkip.report("browser witness", || panic!("browser started"), &mut passed);
    assert!(passed);
    BrowserGate::Full.report("failed witness", || Err("failure".into()), &mut passed);
    assert!(!passed);
}
