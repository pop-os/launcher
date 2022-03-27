// Copyright 2021 System76 <info@system76.com>
// SPDX-License-Identifier: MPL-2.0

use futures::{Stream, StreamExt};
use serde::Deserialize;
use tokio::io::{AsyncBufReadExt, AsyncRead};

/// stdin with AsyncRead support
pub fn async_stdin() -> tokio::io::Stdin {
    tokio::io::stdin()
}

/// stdout with AsyncWrite support
pub fn async_stdout() -> tokio::io::Stdout {
    tokio::io::stdout()
}

/// Creates a stream that parses JSON input line-by-line
pub fn json_input_stream<I, S>(input: I) -> impl Stream<Item = serde_json::Result<S>> + Unpin + Send
where
    I: AsyncRead + Unpin + Send,
    S: for<'a> Deserialize<'a>,
{
    let line_reader = tokio::io::BufReader::new(input).lines();
    tokio_stream::wrappers::LinesStream::new(line_reader)
        .take_while(|x| futures::future::ready(x.is_ok()))
        .map(Result::unwrap)
        .map(|line| serde_json::from_str::<S>(&line))
}
