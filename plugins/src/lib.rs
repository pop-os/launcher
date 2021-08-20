pub mod calc;
pub mod desktop_entries;
pub mod files;
pub mod find;
pub mod pop_shell;
pub mod pulse;
pub mod scripts;
pub mod web;

use futures_lite::{AsyncWrite, AsyncWriteExt};
use pop_launcher::PluginResponse;
use std::{borrow::Cow, ffi::OsStr, path::Path};

pub async fn send<W: AsyncWrite + Unpin>(tx: &mut W, response: PluginResponse) {
    if let Ok(mut bytes) = serde_json::to_string(&response) {
        bytes.push('\n');
        let _ = tx.write_all(bytes.as_bytes()).await;
    }
}

/// Fetch the mime for a given path
pub fn mime_from_path(path: &Path) -> Cow<'static, str> {
    if path.is_dir() {
        Cow::Borrowed("inode/directory")
    } else if let Some(guess) = new_mime_guess::from_path(&path).first() {
        Cow::Owned(guess.essence_str().to_owned())
    } else {
        Cow::Borrowed("text/plain")
    }
}

/// Launches a file with its default appplication via `xdg-open`.
pub fn xdg_open<S: AsRef<OsStr>>(file: S) {
    let _ = smol::process::Command::new("xdg-open").arg(file).spawn();
}
