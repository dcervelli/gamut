//! Which unit the user measures paper in: inches where the locale is one of
//! the few that do, millimeters everywhere else. The info panel writes a print
//! size no sheet is in it; a sheet's own size is in the sheet's unit whatever
//! this says.
//!
//! On Linux it is the locale's territory, as glibc's `LC_MEASUREMENT` decides
//! it: the variables `setlocale` would consult, in its order, read directly,
//! since the program never calls it. On a Mac it is the region's setting in
//! System Settings, which `NSLocale` answers.

use std::sync::OnceLock;

use crate::image::density::Unit;

/// The unit, read once: the locale does not change under a running program.
pub fn unit() -> Unit {
    static UNIT: OnceLock<Unit> = OnceLock::new();
    *UNIT.get_or_init(read)
}

#[cfg(target_os = "macos")]
fn read() -> Unit {
    if objc2_foundation::NSLocale::currentLocale().usesMetricSystem() {
        Unit::Millimeters
    } else {
        Unit::Inches
    }
}

#[cfg(not(target_os = "macos"))]
fn read() -> Unit {
    let locale = ["LC_ALL", "LC_MEASUREMENT", "LANG"]
        .into_iter()
        .filter_map(|name| std::env::var(name).ok())
        .find(|value| !value.is_empty())
        .unwrap_or_default();
    of_locale(&locale)
}

/// The unit a locale's name — `en_US.UTF-8`, `my_MM@calendar` — measures in,
/// by its territory: the United States, Liberia and Myanmar, the three
/// glibc's locales give the US system.
#[cfg_attr(target_os = "macos", allow(dead_code))]
fn of_locale(locale: &str) -> Unit {
    let territory = locale
        .split(['.', '@'])
        .next()
        .and_then(|name| name.split_once('_'))
        .map(|(_, territory)| territory);
    match territory {
        Some("US" | "LR" | "MM") => Unit::Inches,
        _ => Unit::Millimeters,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_locale_measures_by_its_territory() {
        assert_eq!(of_locale("en_US.UTF-8"), Unit::Inches);
        assert_eq!(of_locale("es_US"), Unit::Inches);
        assert_eq!(of_locale("my_MM@calendar=buddhist"), Unit::Inches);
        assert_eq!(of_locale("en_GB.UTF-8"), Unit::Millimeters);
        assert_eq!(of_locale("en_CA.UTF-8"), Unit::Millimeters);
        assert_eq!(of_locale("C.UTF-8"), Unit::Millimeters);
        assert_eq!(of_locale(""), Unit::Millimeters);
    }
}
