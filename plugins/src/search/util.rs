use regex::Captures;
use std::io;
use std::process::Stdio;
use tokio::process::{Child, ChildStdout, Command};

use std::{env, fmt};

use shell_words::{self, ParseError};
use shellexpand::{self, LookupError};

/**
 *
 * Suppose `config.ron` contains the following:
 * (
 *   (
 *     matches: ["f", "find"],
 *     action: (
 *       title: Label("File"),
 *       detail: Capture("^.+/([^/]*)$"),
 *       query: "fdfind --ignore-case --glob --full-path $1 --type ${2:-file}"
 *       command: "xdg-open"
 *     )
 *   ),
 *   ...
 * )
 *
 * And in the launcher frontend, the user types the following search:
 *
 *    "find 'My Document'"
 *
 * Perform an interpolation as follows:
 *
 * 1. Search all rules to find a match for the 'find' command
 * 2. Construct the command-line query by interpolating the search terms:
 *    $1: "My Document"
 *    $2: "file" (using default)
 * 3. Final result:
 *    ["fdfind", "--ignore-case", "--glob", "--full-path", "My Document", "--type", "file"]
 *
 */

fn home_dir() -> Option<String> {
    env::var("HOME").ok()
}

#[derive(Debug)]
pub enum InterpolateError {
    LookupError(String),
    SplitError,
}

impl<E: fmt::Display> From<LookupError<E>> for InterpolateError {
    fn from(err: LookupError<E>) -> InterpolateError {
        InterpolateError::LookupError(format!("{}", err))
    }
}
impl From<ParseError> for InterpolateError {
    fn from(_err: ParseError) -> InterpolateError {
        InterpolateError::SplitError
    }
}

pub fn interpolate_result(
    input: &str,
    output: &str,
    query_string: &str,
    keywords: &[String],
    captures: &Captures,
) -> Result<String, InterpolateError> {
    let expanded = shellexpand::full_with_context(
        input,
        home_dir,
        |var: &str| -> Result<Option<String>, std::num::ParseIntError> {
            if var.eq("OUTPUT") {
                Ok(Some(output.to_string()))
            } else if var.eq("QUERY") {
                // The full query string (i.e. all keywords, including the search prefix) as one string
                Ok(Some(query_string.to_string()))
            } else if var.eq("KEYWORDS") {
                // Just the keywords (absent the search prefix) as one string.
                // NOTE: Whitespace may not be preserved
                Ok(Some(keywords[1..].join(" ")))
            } else if let Some(number) = var.strip_prefix("KEYWORD") {
                // Look up an individual keyword, e.g. $KEYWORD1, $KEYWORD2, etc.
                let idx = number.parse::<usize>()?;
                Ok(keywords.get(idx).cloned())
            } else if let Some(number) = var.strip_prefix("CAPTURE") {
                // Look up an individual regex capture, e.g. $CAPTURE0, $CAPTURE1, etc.
                let idx = number.parse::<usize>()?;
                if let Some(capture) = captures.get(idx) {
                    Ok(Some(capture.as_str().to_owned()))
                } else {
                    Ok(None)
                }
            } else {
                // TODO: Add env vars
                Ok(None)
            }
        },
    )?;

    Ok(expanded.to_string())
}

pub fn interpolate_command(
    input: &str,
    query_string: &str,
    keywords: &[String],
) -> Result<Vec<String>, InterpolateError> {
    let expanded = shellexpand::full_with_context(
        input,
        home_dir,
        |var: &str| -> Result<Option<String>, std::num::ParseIntError> {
            if var.eq("QUERY") {
                // The full query string (i.e. all keywords, including the search prefix) as one string
                Ok(Some(format!("'{}'", query_string.to_string())))
            } else if var.eq("KEYWORDS") {
                // Just the keywords (absent the search prefix) as one string.
                // NOTE: Whitespace may not be preserved
                Ok(Some(keywords[1..].join(" ")))
            } else if let Some(number) = var.strip_prefix("KEYWORD") {
                // Look up an individual keyword, e.g. $KEYWORD1, $KEYWORD2, etc.
                let idx = number.parse::<usize>()?;
                Ok(keywords.get(idx).cloned())
            } else {
                // TODO: Add env vars
                Ok(None)
            }
        },
    )?;

    let parts = shell_words::split(&expanded)?;

    Ok(parts)
}

pub async fn exec(program: &str, args: &[String]) -> io::Result<(Child, ChildStdout)> {
    eprintln!("exec {:?} with {:?}", program, args);
    // Closure to spawn the process
    let mut child = Command::new(program)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()?;

    child
        .stdout
        .take()
        .map(move |stdout| (child, stdout))
        .ok_or_else(|| io::Error::new(io::ErrorKind::BrokenPipe, "stdout pipe is missing"))
}
