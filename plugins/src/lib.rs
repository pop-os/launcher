use futures_lite::{AsyncWrite, AsyncWriteExt};

use pop_launcher::PluginResponse;

pub async fn send<W: AsyncWrite + Unpin>(tx: &mut W, response: PluginResponse) {
    if let Ok(mut bytes) = serde_json::to_string(&response) {
        bytes.push('\n');
        let _ = tx.write(bytes.as_bytes()).await;
        let _ = tx.flush().await;
    }
}
