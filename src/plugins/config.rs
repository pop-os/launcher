use regex::Regex;
use serde::Deserialize;
use std::{
    borrow::Cow,
    path::{Path, PathBuf},
};

#[derive(Debug, Default, Deserialize)]
pub struct PluginConfig {
    pub name: Cow<'static, str>,
    pub description: Cow<'static, str>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "::serde_with::rust::unwrap_or_skip"
    )]
    pub bin: Option<PluginBinary>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "::serde_with::rust::unwrap_or_skip"
    )]
    pub icon: Option<crate::IconSource>,

    #[serde(default)]
    pub query: PluginQuery,
}

#[derive(Debug, Default, Deserialize)]
pub struct PluginBinary {
    path: Cow<'static, str>,

    #[serde(default)]
    args: Vec<Cow<'static, str>>,
}

#[derive(Debug, Default, Deserialize)]
pub struct PluginQuery {
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "::serde_with::rust::unwrap_or_skip"
    )]
    pub help: Option<Cow<'static, str>>,

    #[serde(default)]
    pub isolate: bool,

    #[serde(default)]
    pub no_sort: bool,

    #[serde(default)]
    pub persistent: bool,

    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "::serde_with::rust::unwrap_or_skip"
    )]
    pub regex: Option<Cow<'static, str>>,
}

pub fn load(source: &Path, config_path: &Path) -> Option<(PathBuf, PluginConfig, Option<Regex>)> {
    if let Ok(config_bytes) = std::fs::read_to_string(&config_path) {
        let config = match ron::from_str::<PluginConfig>(&config_bytes) {
            Ok(config) => config,
            Err(why) => {
                tracing::error!("malformed config at {}: {}", config_path.display(), why);
                return None;
            }
        };

        let exec = if let Some(bin) = config.bin.as_ref() {
            if bin.path.starts_with('/') {
                PathBuf::from((*bin.path).to_owned())
            } else {
                source.join(bin.path.as_ref())
            }
        } else {
            tracing::error!(
                "bin field is missing from config at {}",
                config_path.display()
            );
            return None;
        };

        let regex = config
            .query
            .regex
            .as_ref()
            .and_then(|p| Regex::new(&*p).ok());

        return Some((exec, config, regex));
    }

    tracing::error!("I/O error reading config at {}", config_path.display());

    None
}
