// Copyright 2021 System76 <info@system76.com>
// SPDX-License-Identifier: MPL-2.0

use blocking::Unblock;
use futures_lite::{AsyncBufReadExt, AsyncRead, Stream, StreamExt};
use serde::Deserialize;
use std::io;

/// stdin with AsyncRead support
pub fn async_stdin() -> Unblock<io::Stdin> {
    Unblock::new(io::stdin())
}

/// stdout with AsyncWrite support
pub fn async_stdout() -> Unblock<io::Stdout> {
    Unblock::new(io::stdout())
}

/// Creates a stream that parses JSON input line-by-line
pub fn json_input_stream<I, S>(input: I) -> impl Stream<Item = serde_json::Result<S>> + Unpin + Send
where
    I: AsyncRead + Unpin + Send,
    S: for<'a> Deserialize<'a>,
{
    futures_lite::io::BufReader::new(input)
        .lines()
        .take_while(Result::is_ok)
        .map(Result::unwrap)
        .map(|line| serde_json::from_str::<S>(&line))
}
