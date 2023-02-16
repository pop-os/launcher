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
    vars: &[String],
    capture: &str,
) -> Result<String, InterpolateError> {
    let expanded = shellexpand::full_with_context(
        input,
        home_dir,
        |var: &str| -> Result<Option<String>, std::num::ParseIntError> {
            if var.eq("QUERY") {
                // All search terms as one arg
                Ok(Some(vars[1..].join(" ")))
            } else if var.eq("FULLQUERY") {
                // All search terms (including search command) as one arg
                Ok(Some(vars.join(" ")))
            } else if var.eq("CAPTURE") {
                // Use the regex capture (first set of parens in regex)
                Ok(Some(capture.to_owned()))
            } else {
                // If this is a numeric variable (e.g. "$1", "$2", ...), look up the search term
                let idx = var.parse::<usize>()?;
                let value = vars.get(idx);
                Ok(value.map(|s| format!("'{}'", s)))
            }
        },
    )?;

    Ok(expanded.to_string())
}

pub fn interpolate_command(input: &str, vars: &[String]) -> Result<Vec<String>, InterpolateError> {
    let expanded = shellexpand::full_with_context(
        input,
        home_dir,
        |var: &str| -> Result<Option<String>, std::num::ParseIntError> {
            if var.eq("QUERY") {
                // All search terms as one arg
                Ok(Some(format!("'{}'", vars[1..].join(" "))))
            } else if var.eq("FULLQUERY") {
                // All search terms (including search command) as one arg
                Ok(Some(format!("'{}'", vars.join(" "))))
            } else {
                // If this is a numeric variable (e.g. "$1", "$2", ...), look up the search term
                let idx = var.parse::<usize>()?;
                let value = vars.get(idx);
                Ok(value.map(|s| format!("'{}'", s)))
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
