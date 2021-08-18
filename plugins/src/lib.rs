pub mod calc;
pub mod desktop_entries;
pub mod files;
pub mod find;
pub mod pop_shell;
pub mod scripts;
pub mod web;

use futures_lite::{AsyncWrite, AsyncWriteExt};
use pop_launcher::PluginResponse;
use std::ffi::OsStr;

pub async fn send<W: AsyncWrite + Unpin>(tx: &mut W, response: PluginResponse) {
    if let Ok(mut bytes) = serde_json::to_string(&response) {
        bytes.push('\n');
        let _ = tx.write(bytes.as_bytes()).await;
        let _ = tx.flush().await;
    }
}

/// Launches a file with its default appplication via `xdg-open`.
pub fn xdg_open<S: AsRef<OsStr>>(file: S) {
    let _ = smol::process::Command::new("xdg-open").arg(file).spawn();
}
