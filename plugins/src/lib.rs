// SPDX-License-Identifier: GPL-3.0-only
// Copyright © 2021 System76

pub mod calc;
pub mod cosmic_toplevel;
pub mod desktop_entries;
pub mod files;
pub mod find;
pub mod pop_shell;
pub mod pulse;
pub mod recent;
pub mod scripts;
pub mod terminal;
pub mod web;

use pop_launcher::PluginResponse;
use std::{borrow::Cow, ffi::OsStr, future::Future, path::Path, process::Stdio};
use tokio::io::{AsyncWrite, AsyncWriteExt};

pub async fn send<W: AsyncWrite + Unpin>(tx: &mut W, response: PluginResponse) {
    if let Ok(mut bytes) = serde_json::to_string(&response) {
        bytes.push('\n');
        let _ = tx.write_all(bytes.as_bytes()).await;
    }
}

/// Run both futures and take the output of the first one to finish.
pub async fn or<T>(future1: impl Future<Output = T>, future2: impl Future<Output = T>) -> T {
    futures::pin_mut!(future1);
    futures::pin_mut!(future2);

    futures::future::select(future1, future2)
        .await
        .factor_first()
        .0
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
    let _ = tokio::process::Command::new("xdg-open")
        .arg(file)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn();
}
