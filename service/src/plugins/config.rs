// Copyright 2021 System76 <info@system76.com>
// SPDX-License-Identifier: MPL-2.0

use anyhow::{anyhow, bail};
use freedesktop_desktop_entry as fde;
use regex::Regex;
use std::path::{Path, PathBuf};

#[derive(Debug, Default, Clone)]
pub struct PluginConfig {
    pub name: String,
    pub description: Option<String>,
    pub icon: Option<String>,
    pub exec: PluginExec,
    pub regex: Option<Regex>,
    pub isolate: bool,
    pub isolate_with: Option<Regex>,
    pub show_on_empty_query: bool,
    pub no_sort: bool,
    pub generic_query: Option<String>,
    pub long_lived: bool,
    pub history: bool,
    pub priority: PluginPriority,
}

#[derive(Debug, Default, Clone)]
pub struct PluginExec {
    pub path: PathBuf,
    pub args: Vec<String>,
}

#[derive(Debug, Default, Clone)]
pub struct PluginQuery {}

#[derive(Copy, Clone, Debug, Eq, PartialEq, PartialOrd, Ord)]
pub enum PluginPriority {
    High = 0,
    Default = 1,
    Low = 2,
}

impl Default for PluginPriority {
    fn default() -> Self {
        Self::Default
    }
}

impl PluginConfig {
    pub fn from_path(source: &Path, config_path: &Path) -> anyhow::Result<Self> {
        let content = std::fs::read_to_string(config_path)
            .map_err(|e| anyhow!("error reading config at {}: {:?}", config_path.display(), e))?;

        Self::from_str(source, config_path, &content)
    }

    pub fn from_str(source: &Path, config_path: &Path, content: &str) -> anyhow::Result<Self> {
        let locales = fde::get_languages_from_env();

        let desktop_entry = fde::DesktopEntry::from_str(config_path, content, Some(&locales))?;

        let group = desktop_entry
            .groups
            .group("Plugin")
            .ok_or(anyhow!("no Plugin group"))?;

        let mut config = PluginConfig {
            name: group
                .localized_entry("Name", &locales)
                .ok_or(anyhow!("no Name field"))?
                .to_string(),
            exec: {
                let exec = group
                    .localized_entry("Exec", &locales)
                    .ok_or(anyhow!("no Exec field"))?;

                let mut iter = exec.split(" ");

                let mut exec = PluginExec {
                    path: PathBuf::from(iter.next().unwrap()),
                    args: iter.map(|a| a.to_string()).collect(),
                };

                if !exec.path.is_absolute() {
                    exec.path = source.join(&exec.path);
                };

                exec
            },
            ..Default::default()
        };

        if let Some(description) = group.entry("Description") {
            config.description.replace(description.to_string());
        }

        if let Some(icon) = group.entry("Icon") {
            config.icon.replace(icon.to_string());
        }

        if let Some(regex) = group.entry("Regex") {
            match Regex::new(regex) {
                Ok(regex) => {
                    config.regex.replace(regex);
                }
                Err(e) => bail!("can't parse regex: {e:?}"),
            }
        }

        if let Some(isolate) = group.entry_bool("Isolate") {
            config.isolate = isolate;
        }

        if let Some(regex) = group.entry("IsolateWith") {
            match Regex::new(regex) {
                Ok(regex) => {
                    config.isolate_with.replace(regex);
                }
                Err(e) => bail!("can't parse isolate_with: {e:?}"),
            }
        }

        if let Some(persistent) = group.entry_bool("ShowOnEmptyQuery") {
            config.show_on_empty_query = persistent;
        }

        if let Some(no_sort) = group.entry_bool("NoSort") {
            config.no_sort = no_sort;
        }

        if let Some(generic_query) = group.entry("GenericQuery") {
            config.generic_query.replace(generic_query.to_string());
        }

        if let Some(long_lived) = group.entry_bool("LongLived") {
            config.long_lived = long_lived;
        }

        if let Some(history) = group.entry_bool("History") {
            config.history = history;
        }

        if let Some(priority) = group.entry("Priority") {
            match priority {
                "Default" => config.priority = PluginPriority::Default,
                "High" => config.priority = PluginPriority::High,
                "Low" => config.priority = PluginPriority::Low,
                _ => {}
            }
        }

        Ok(config)
    }
}
