//! Initial native backing follows the same public theme registry as Session.
use super::*;
use crate::settings::DesktopSettings;

#[test]
fn cold_background_follows_forced_appearance_and_custom_inheritance() -> Result<(), String> {
    let mut settings = DesktopSettings::open(Some("/nonexistent-puzzle-bg.json"), None, &[])?.0;
    assert_eq!(
        initial(&settings, Appearance::Dark)?,
        [14. / 255., 18. / 255., 26. / 255., 1.]
    );
    assert_eq!(
        initial(&settings, Appearance::Light)?,
        [250. / 255., 250. / 255., 250. / 255., 1.]
    );
    settings.apply_json(r##"{"themes":{"custom":{"extends":"puzzle-dark","background":"#123456"}},"theme":{"mode":"dark","dark":"custom"}}"##)?;
    assert_eq!(
        initial(&settings, Appearance::Light)?,
        [18. / 255., 52. / 255., 86. / 255., 1.]
    );
    Ok(())
}
