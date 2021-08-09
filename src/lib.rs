mod codec;
mod plugins;
mod service;

pub use self::codec::*;
pub use self::plugins::*;
pub use self::service::Service;

use serde::{Deserialize, Serialize};
use slab::Slab;
use std::{borrow::Cow, path::PathBuf};

pub type PluginKey = usize;
pub type Generation = u32;
pub type Indice = u32;

pub enum Event {
    Request(Request),
    Response((PluginKey, PluginResponse)),
    PluginExit(PluginKey),
    Help(async_oneshot::Sender<Slab<PluginHelp>>),
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub enum IconSource {
    // Locate by name or path
    Name(Cow<'static, str>),
    // Icon is a mime type
    Mime(Cow<'static, str>),
    // Window Entity ID
    Window((Generation, Indice)),
}

// Launcher frontends shall send these requests to the launcher service.
#[derive(Debug, Deserialize, Serialize)]
pub enum Request {
    /// Activate on the selected item
    Activate(Indice),
    /// Perform a tab completion from the selected item
    Complete(Indice),
    /// Request to end the service
    Exit,
    /// Requests to cancel any active searches
    Interrupt,
    /// Request to close the selected item
    Quit(Indice),
    /// Perform a search in our database
    Search(String),
}

/// Launcher frontends shall react to these responses from the launcher service.
#[derive(Debug, Deserialize, Serialize)]
pub enum Response {
    // An operation was performed and the frontend may choose to exit its process.
    Close,
    // Notifies that a .desktop entry should be launched by the frontend
    DesktopEntry(PathBuf),
    // The frontend should clear its search results and display a new list
    Update(Vec<SearchResult>),
    // An item was selected that resulted in a need to autofill the launcher
    Fill(String),
}

#[derive(Debug, Deserialize, Serialize)]
pub enum PluginResponse {
    /// Append a new search item to the launcher
    Append(SearchMeta),
    /// Clear all results in the launcher list
    Clear,
    /// Close the launcher
    Close,
    // Notifies that a .desktop entry should be launched by the frontend
    DesktopEntry(PathBuf),
    /// Update the text in the launcher
    Fill(String),
    /// Indicoates that a plugin is finished with its queries
    Finished,
}

#[derive(Debug, Default, Deserialize, Serialize)]
pub struct SearchMeta {
    pub id: Indice,
    pub name: String,
    pub description: String,
    pub keywords: Option<Vec<String>>,
    pub icon: Option<IconSource>,
    pub exec: Option<String>,
    pub window: Option<(Generation, Indice)>,
}

/// Serialized response to launcher frontend about a search result.
#[derive(Debug, Serialize, Deserialize)]
pub struct SearchResult {
    pub id: Indice,
    pub name: String,
    pub description: String,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "::serde_with::rust::unwrap_or_skip"
    )]
    pub icon: Option<IconSource>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "::serde_with::rust::unwrap_or_skip"
    )]
    pub category_icon: Option<IconSource>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "::serde_with::rust::unwrap_or_skip"
    )]
    pub window: Option<(Generation, Indice)>,
}
