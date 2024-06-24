use anyhow::{bail, Result};
use rusqlite::Connection;
use std::{fs, path::PathBuf, process::Command};

pub enum Browser {
    Unknown,
    Firefox,
}

impl Browser {
    pub fn get_default_browser() -> Self {
        let output = Command::new("xdg-settings")
            .arg("get")
            .arg("default-web-browser")
            .output()
            .expect("Failed to execute xdg-settings");

        if output.status.success() {
            let browser = std::str::from_utf8(&output.stdout).unwrap_or("").trim();

            if browser.contains("firefox") {
                Self::Firefox
            } else {
                Self::Unknown
            }
        } else {
            // Print the error if the command failed
            let error_message = std::str::from_utf8(&output.stderr).unwrap_or("Unknown error");
            tracing::error!("Failed to get the default web browser: {}", error_message);
            Self::Unknown
        }
    }
}

fn firefox_db_path() -> Result<PathBuf> {
    let home = std::env::var("HOME")?;

    let mut home = PathBuf::from(home);

    home.push(".mozilla");
    home.push("firefox");

    if !home.is_dir() {
        bail!("no firefox directory detected")
    }

    if let Ok(entries) = fs::read_dir(home) {
        for entry in entries.flatten() {
            let file_name = entry.path();
            if let Some(name) = file_name.to_str() {
                if name.ends_with(".default-release") {
                    return Ok(file_name.join("places.sqlite"));
                }
            }
        }
    }

    bail!("no db found")
}

pub fn open_firefox_db_ro() -> Result<Connection> {
    let firefox_db_path = firefox_db_path()?;

    let tmp_db_path = "/tmp/places_backup.sqlite";

    fs::copy(firefox_db_path, tmp_db_path)?;

    let conn =
        Connection::open_with_flags(tmp_db_path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)?;

    Ok(conn)
}

#[derive(Debug, Clone, PartialEq)]
pub struct F64Ord(pub f64);

impl Eq for F64Ord {}

impl PartialOrd for F64Ord {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for F64Ord {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        other.0.total_cmp(&self.0)
    }
}
