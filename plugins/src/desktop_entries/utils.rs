//! Reusable functions for desktop entries

use std::borrow::Cow;

use freedesktop_desktop_entry::{DesktopEntry, PathSource};

pub fn path_string(source: &PathSource) -> Cow<'static, str> {
    match source {
        PathSource::Local | PathSource::LocalDesktop => "Local".into(),
        PathSource::LocalFlatpak => "Flatpak".into(),
        PathSource::LocalNix => "Nix".into(),
        PathSource::Nix => "Nix (System)".into(),
        PathSource::System => "System".into(),
        PathSource::SystemLocal => "Local (System)".into(),
        PathSource::SystemFlatpak => "Flatpak (System)".into(),
        PathSource::SystemSnap => "Snap (System)".into(),
        PathSource::Other(other) => Cow::Owned(other.clone()),
    }
}

pub fn get_description(de: &DesktopEntry, locales: &[String]) -> String {
    let path_source = PathSource::guess_from(&de.path);

    let desc_source = path_string(&path_source).to_string();

    match de.comment(locales) {
        Some(desc) => {
            if desc.is_empty() {
                desc_source
            } else {
                format!("{} - {}", desc_source, desc)
            }
        }
        None => desc_source,
    }
}

// todo: cache
#[must_use]
pub fn is_session_cosmic() -> bool {
    if let Ok(var) = std::env::var("XDG_CURRENT_DESKTOP") {
        return var.contains("COSMIC");
    }

    false
}
