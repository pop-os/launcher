// Copyright 2021 System76 <info@system76.com>
// SPDX-License-Identifier: MPL-2.0

use futures::{Stream, StreamExt};
use pop_launcher::{Request, Response};
use std::io;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt};
use tokio::process;
use tokio_stream::wrappers::LinesStream;

#[derive(Debug)]
pub struct IpcClient {
    pub child: process::Child,
    pub stdin: process::ChildStdin,
}

impl IpcClient {
    pub fn new() -> io::Result<(Self, impl Stream<Item = Response>)> {
        let mut child = process::Command::new("pop-launcher")
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .spawn()?;

        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| io::Error::new(io::ErrorKind::Other, "failed to find child stdin"))?;

        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| io::Error::new(io::ErrorKind::Other, "failed to find child stdout"))?;

        let responses = LinesStream::new(tokio::io::BufReader::new(stdout).lines()).filter_map(
            |result| async move {
                let Ok(line) = result else {
                    return None;
                };

                serde_json::from_str::<Response>(&line).ok()
            },
        );

        let client = Self { child, stdin };

        Ok((client, responses))
    }

    pub async fn send(&mut self, request: Request) -> io::Result<()> {
        let mut request_json = serde_json::to_string(&request)
            .map_err(|err| io::Error::new(io::ErrorKind::InvalidInput, err))?;

        request_json.push('\n');

        self.stdin.write_all(request_json.as_bytes()).await
    }

    pub async fn exit(mut self) {
        let _res = self.send(Request::Exit).await;
        let _res = self.child.wait().await;
    }
}
