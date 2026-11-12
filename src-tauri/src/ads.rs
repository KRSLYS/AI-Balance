//! The store edition's banner ad, on Android. Everything here is a no-op in the full edition and on
//! every other platform: those builds contain no ad code and contact no ad network.
//!
//! The Kotlin side (android-ads/AdsPlugin.kt) asks for consent before any ad is requested.

use crate::model::Edition;

/// The ad unit this build shows, set when a store build is made.
fn unit_id() -> Option<&'static str> {
    option_env!("AIBALANCE_AD_UNIT").filter(|id| !id.is_empty())
}

/// Whether this build shows an ad at all.
pub fn enabled() -> bool {
    cfg!(target_os = "android") && Edition::current() == Edition::Store && unit_id().is_some()
}

#[cfg(target_os = "android")]
mod android {
    use std::sync::OnceLock;

    use serde::{Deserialize, Serialize};
    use tauri::plugin::PluginHandle;

    static PLUGIN: OnceLock<PluginHandle<tauri::Wry>> = OnceLock::new();

    #[derive(Serialize)]
    #[serde(rename_all = "camelCase")]
    struct Banner<'a> {
        unit_id: &'a str,
    }

    #[derive(Deserialize)]
    struct Shown {
        height: u32,
    }

    #[derive(Deserialize)]
    struct Required {
        required: bool,
    }

    pub fn register(handle: PluginHandle<tauri::Wry>) {
        let _ = PLUGIN.set(handle);
    }

    pub fn show_banner(unit_id: &str) -> u32 {
        PLUGIN
            .get()
            .and_then(|plugin| plugin.run_mobile_plugin::<Shown>("showBanner", Banner { unit_id }).ok())
            .map_or(0, |shown| shown.height)
    }

    pub fn privacy_options_required() -> bool {
        PLUGIN
            .get()
            .and_then(|plugin| plugin.run_mobile_plugin::<Required>("privacyOptionsRequired", ()).ok())
            .is_some_and(|answer| answer.required)
    }

    pub fn show_privacy_options() {
        if let Some(plugin) = PLUGIN.get() {
            let _ = plugin.run_mobile_plugin::<serde_json::Value>("showPrivacyOptions", ());
        }
    }
}

#[cfg(target_os = "android")]
pub use android::register;

/// Asks for consent where it is needed, then shows the banner. Returns the banner's height in
/// density-independent pixels, which the interface leaves free at the bottom; 0 means no banner.
/// Blocks until the consent form is answered, so call it off the main thread.
pub fn start() -> u32 {
    #[cfg(target_os = "android")]
    if let (true, Some(unit)) = (enabled(), unit_id()) {
        return android::show_banner(unit);
    }
    0
}

/// Whether to offer the user a way to change their consent answer.
pub fn privacy_options_required() -> bool {
    #[cfg(target_os = "android")]
    if enabled() {
        return android::privacy_options_required();
    }
    false
}

/// Opens the form for changing the consent answer.
pub fn show_privacy_options() {
    #[cfg(target_os = "android")]
    if enabled() {
        android::show_privacy_options();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_desktop_build_never_shows_an_ad() {
        assert!(!enabled());
        assert_eq!(start(), 0);
        assert!(!privacy_options_required());
    }
}
