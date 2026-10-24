// Copyright 2021 System76 <info@system76.com>
// SPDX-License-Identifier: MPL-2.0

mod codec;
pub mod config;

pub use self::codec::*;

use const_format::concatcp;
use serde::{Deserialize, Serialize};
use std::{
    borrow::Cow,
    path::{Path, PathBuf},
};

pub const LOCAL: &str = "~/.local/share/pop-launcher";
pub const LOCAL_PLUGINS: &str = concatcp!(LOCAL, "/plugins");

pub const SYSTEM: &str = "/etc/pop-launcher";
pub const SYSTEM_PLUGINS: &str = concatcp!(SYSTEM, "/plugins");

pub const DISTRIBUTION: &str = "/usr/lib/pop-launcher";
pub const DISTRIBUTION_PLUGINS: &str = concatcp!(DISTRIBUTION, "/plugins");

pub const PLUGIN_PATHS: &[&str] = &[LOCAL_PLUGINS, SYSTEM_PLUGINS, DISTRIBUTION_PLUGINS];

pub fn plugin_paths() -> impl Iterator<Item = Cow<'static, Path>> {
    PLUGIN_PATHS.iter().map(|path| {
        #[allow(deprecated)]
        if let Some(path) = path.strip_prefix("~/") {
            let path = dirs::home_dir()
                .expect("user does not have home dir")
                .join(path);
            Cow::Owned(path)
        } else {
            Cow::Borrowed(Path::new(path))
        }
    })
}

/// u32 value defining the generation of an indice.
pub type Generation = u32;

/// u32 value defining the indice of a slot.
pub type Indice = u32;

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct ContextOption {
    pub id: Indice,
    pub name: String,
}

#[derive(Debug, Deserialize, Serialize, Clone, Copy)]
pub enum GpuPreference {
    Default,
    NonDefault,
    SpecificIdx(u32),
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub enum IconSource {
    // Locate by name or path.
    Name(Cow<'static, str>),
    // Icon is a mime type.
    Mime(Cow<'static, str>),
}

/// Sent from a plugin to the launcher service.
#[derive(Debug, Deserialize, Serialize, Clone)]
pub enum PluginResponse {
    /// Append a new search item to the launcher.
    Append(PluginSearchResult),
    /// Clear all results in the launcher list.
    Clear,
    /// Close the launcher.
    Close,
    // Additional options for launching a certain item.
    Context {
        id: Indice,
        options: Vec<ContextOption>,
    },
    /// Instruct the launcher service to deactivate this plugin.
    Deactivate,
    // Notifies that a .desktop entry should be launched by the frontend.
    DesktopEntry {
        path: PathBuf,
        gpu_preference: GpuPreference,
        #[serde(skip_serializing_if = "Option::is_none", default)]
        action_name: Option<String>,
    },
    /// Update the text in the launcher.
    Fill(String),
    /// Indicates that a plugin is finished with its queries.
    Finished,
}

/// Search information from a plugin to be sorted and filtered by the launcher service.
#[derive(Debug, Default, Deserialize, Serialize, Clone)]
pub struct PluginSearchResult {
    /// Numeric identifier tracked by the plugin.
    pub id: Indice,
    /// The name / title.
    pub name: String,
    /// The description / subtitle.
    pub description: String,
    /// Extra words to match when sorting and filtering.
    pub keywords: Option<Vec<String>>,
    /// Icon to display in the frontend.
    pub icon: Option<IconSource>,
    /// Command that is executed by this result, used for sorting and filtering.
    pub exec: Option<String>,
    /// Designates that this search item refers to a window.
    pub window: Option<(Generation, Indice)>,
}

impl PluginSearchResult {
    #[must_use]
    #[inline]
    pub fn cache_identifier(&self) -> Option<String> {
        // the exec field may clash in multiple search results as the arguments
        // are cut from the string
        // self.exec.to_owned().unwrap_or_else(|| self.name.to_owned())
        self.exec.as_ref().map(|_| self.name.clone())
    }
}

// Sent to the input pipe of the launcher service, and disseminated to its plugins.
#[derive(Debug, Deserialize, Serialize, Clone)]
pub enum Request {
    /// Activate on the selected item.
    Activate(Indice),
    /// Activate a context item on an item.
    ActivateContext { id: Indice, context: Indice },
    /// Perform a tab completion from the selected item.
    Complete(Indice),
    /// Request for any context options this result may have.
    Context(Indice),
    /// Request to end the service.
    Exit,
    /// The frontend was closed and the service should release resources
    /// and prepare for the next query
    Close,
    /// Requests to cancel any active searches.
    Interrupt,
    /// Request to close the selected item.
    Quit(Indice),
    /// Perform a search in our database.
    Search(String),
    /// List windows for a window switcher, such as Alt+Tab.
    ///
    /// Only sent to plugins whose config sets `window_switcher: true`, so
    /// plugins that predate this request never receive it.
    WindowSwitcher(WindowSwitcherQuery),
}

/// Parameters of a [`Request::WindowSwitcher`].
#[derive(Debug, Default, Deserialize, Serialize, Clone, PartialEq, Eq)]
// Missing fields fall back to their defaults so the query can grow new
// fields without breaking frontends that don't send them.
#[serde(default)]
pub struct WindowSwitcherQuery {
    /// Which workspaces to list windows from.
    pub scope: WorkspaceScope,
    /// Name of the output (e.g. `DP-1`) the switcher is shown on.
    ///
    /// [`WorkspaceScope::FocusedOutputWorkspace`] lists windows on this
    /// output; without it, the output of the focused window is used.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output: Option<String>,
    /// List only windows of the focused window's app (by app ID), within
    /// `scope`, as for an Alt+` switcher.
    ///
    /// Left out of the JSON when `false`, so plain Alt+Tab queries look
    /// exactly as they did before this field existed.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub same_app: bool,
}

/// Which workspaces a window switcher lists windows from.
#[derive(Debug, Default, Deserialize, Serialize, Clone, Copy, PartialEq, Eq)]
pub enum WorkspaceScope {
    /// Every window on every workspace.
    #[default]
    AllWorkspaces,
    /// Windows on the active workspace of every output.
    ActiveWorkspaces,
    /// Windows on the active workspace of one output: the one named in
    /// [`WindowSwitcherQuery::output`], else the one with the focused window.
    FocusedOutputWorkspace,
}

/// Sent from the launcher service to a frontend.
#[derive(Debug, Deserialize, Serialize, Clone)]
pub enum Response {
    // An operation was performed and the frontend may choose to exit its process.
    Close,
    // Additional options for launching a certain item
    Context {
        id: Indice,
        options: Vec<ContextOption>,
    },
    // Notifies that a .desktop entry should be launched by the frontend.
    DesktopEntry {
        path: PathBuf,
        gpu_preference: GpuPreference,
        action_name: Option<String>,
    },
    // The frontend should clear its search results and display a new list.
    Update(Vec<SearchResult>),
    // An item was selected that resulted in a need to autofill the launcher.
    Fill(String),
}

/// Serialized response to launcher frontend about a search result.
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct SearchResult {
    /// Numeric identifier tracked by the plugin.
    pub id: Indice,
    /// The name / title.
    pub name: String,
    /// The description / subtitle.
    pub description: String,

    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "::serde_with::rust::unwrap_or_skip"
    )]
    /// Icon to display in the frontend for this item
    pub icon: Option<IconSource>,

    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "::serde_with::rust::unwrap_or_skip"
    )]
    /// Icon to display in the frontend for this plugin
    pub category_icon: Option<IconSource>,

    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "::serde_with::rust::unwrap_or_skip"
    )]
    /// Designates that this search item refers to a window.
    pub window: Option<(Generation, Indice)>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn window_switcher_request_json_roundtrip() {
        let req = Request::WindowSwitcher(WindowSwitcherQuery {
            scope: WorkspaceScope::ActiveWorkspaces,
            ..Default::default()
        });
        let json = serde_json::to_string(&req).unwrap();
        assert_eq!(json, r#"{"WindowSwitcher":{"scope":"ActiveWorkspaces"}}"#);
        assert!(matches!(
            serde_json::from_str::<Request>(&json).unwrap(),
            Request::WindowSwitcher(WindowSwitcherQuery {
                scope: WorkspaceScope::ActiveWorkspaces,
                output: None,
                same_app: false,
            })
        ));
    }

    #[test]
    fn window_switcher_query_missing_fields_default_to_all_workspaces() {
        let query: WindowSwitcherQuery = serde_json::from_str("{}").unwrap();
        assert_eq!(query.scope, WorkspaceScope::AllWorkspaces);
        assert_eq!(query.output, None);
    }

    #[test]
    fn window_switcher_query_carries_output_name() {
        let json = r#"{"scope":"FocusedOutputWorkspace","output":"DP-1"}"#;
        let query: WindowSwitcherQuery = serde_json::from_str(json).unwrap();
        assert_eq!(query.output.as_deref(), Some("DP-1"));
    }

    #[test]
    fn same_app_round_trips() {
        let query = WindowSwitcherQuery {
            scope: WorkspaceScope::ActiveWorkspaces,
            same_app: true,
            ..Default::default()
        };
        let json = serde_json::to_string(&Request::WindowSwitcher(query.clone())).unwrap();
        assert_eq!(
            json,
            r#"{"WindowSwitcher":{"scope":"ActiveWorkspaces","same_app":true}}"#
        );
        assert!(matches!(
            serde_json::from_str::<Request>(&json).unwrap(),
            Request::WindowSwitcher(parsed) if parsed == query
        ));
    }

    // Alt+Tab queries from before `same_app` existed must parse, and must
    // still be sent byte for byte as before.
    #[test]
    fn same_app_defaults_to_false_and_is_not_serialised() {
        let query: WindowSwitcherQuery =
            serde_json::from_str(r#"{"scope":"AllWorkspaces"}"#).unwrap();
        assert!(!query.same_app);
        assert_eq!(
            serde_json::to_string(&WindowSwitcherQuery::default()).unwrap(),
            r#"{"scope":"AllWorkspaces"}"#
        );
    }
}
