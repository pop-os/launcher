use blocking::Unblock;
use futures_codec::{FramedRead, LinesCodec};
use futures_lite::{AsyncRead, Stream, StreamExt};
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
    FramedRead::new(input, LinesCodec)
        .take_while(Result::is_ok)
        .map(Result::unwrap)
        .map(|line| serde_json::from_str::<S>(&line))
}
