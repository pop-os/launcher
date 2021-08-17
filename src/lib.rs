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
            let path = std::env::home_dir()
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

#[derive(Debug, Deserialize, Serialize)]
pub struct ContextOption {
    pub id: Indice,
    pub name: String,
}

#[derive(Debug, Deserialize, Serialize)]
pub enum GpuPreference {
    Default,
    NonDefault,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub enum IconSource {
    // Locate by name or path.
    Name(Cow<'static, str>),
    // Icon is a mime type.
    Mime(Cow<'static, str>),
    // Window Entity ID.
    Window((Generation, Indice)),
}

/// Sent from a plugin to the launcher service.
#[derive(Debug, Deserialize, Serialize)]
pub enum PluginResponse {
    /// Append a new search item to the launcher.
    Append(PluginSearchResult),
    /// Clear all results in the launcher list.
    Clear,
    /// Close the launcher.
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
    },
    /// Update the text in the launcher.
    Fill(String),
    /// Indicoates that a plugin is finished with its queries.
    Finished,
}

/// Search information from a plugin to be sorted and filtered by the launcher service.
#[derive(Debug, Default, Deserialize, Serialize)]
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

// Sent to the input pipe of the launcher service, and disseminated to its plugins.
#[derive(Debug, Deserialize, Serialize)]
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
    /// Requests to cancel any active searches.
    Interrupt,
    /// Request to close the selected item.
    Quit(Indice),
    /// Perform a search in our database.
    Search(String),
}

/// Sent from the launcher service to a frontend.
#[derive(Debug, Deserialize, Serialize)]
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
    },
    // The frontend should clear its search results and display a new list.
    Update(Vec<SearchResult>),
    // An item was selected that resulted in a need to autofill the launcher.
    Fill(String),
}

/// Serialized response to launcher frontend about a search result.
#[derive(Debug, Serialize, Deserialize)]
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
