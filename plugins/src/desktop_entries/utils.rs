//! Reusable functions for desktop entries

use std::borrow::Cow;

use freedesktop_desktop_entry::{DesktopEntry, PathSource};

// todo: subscriptions with notify

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

pub fn get_description<'a>(de: &'a DesktopEntry, locales: &[String]) -> String {
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

pub fn resolve_icon(name: Option<&str>) -> Option<pop_launcher::IconSource> {
    let name = name?;
    if name.is_empty() {
        return None;
    }

    let mut path = std::path::PathBuf::from(name);
    if name.starts_with("~/") {
        if let Some(home) = dirs::home_dir() {
            path = home.join(&name[2..]);
        }
    }

    if path.is_absolute() && path.exists() {
        return Some(pop_launcher::IconSource::Name(Cow::Owned(
            path.to_string_lossy().into_owned(),
        )));
    }

    // Check standard pixmap paths and user local paths
    let mut search_dirs = Vec::new();

    if let Some(home) = dirs::home_dir() {
        search_dirs.push(home.join(".local/share/icons"));
        search_dirs.push(home.join(".local/share/pixmaps"));
        search_dirs.push(home.join(".icons"));
    }

    search_dirs.push(std::path::PathBuf::from("/usr/share/pixmaps"));
    search_dirs.push(std::path::PathBuf::from("/usr/local/share/pixmaps"));

    for dir in search_dirs {
        let p = dir.join(name);
        if p.exists() {
            return Some(pop_launcher::IconSource::Name(Cow::Owned(
                p.to_string_lossy().into_owned(),
            )));
        }

        // Try adding extensions if missing
        if !name.contains('.') {
            for ext in [".png", ".svg", ".xpm"] {
                let p = dir.join(format!("{}{}", name, ext));
                if p.exists() {
                    return Some(pop_launcher::IconSource::Name(Cow::Owned(
                        p.to_string_lossy().into_owned(),
                    )));
                }
            }
        }
    }

    Some(pop_launcher::IconSource::Name(Cow::Owned(name.to_string())))
}
