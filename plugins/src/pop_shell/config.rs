// SPDX-License-Identifier: GPL-3.0-only
// Copyright © 2021 System76

use serde::Deserialize;

pub fn bool_true() -> bool {
    true
}

#[derive(Debug, Deserialize, Clone)]
pub struct SearchScope {
    #[serde(default = "bool_true")]
    pub name: bool,

    #[serde(default = "bool_true")]
    pub description: bool,
}

impl Default for SearchScope {
    fn default() -> SearchScope {
        SearchScope {
            name: true,
            description: true,
        }
    }
}

#[derive(Debug, Default, Deserialize, Clone)]
pub struct Config {
    #[serde(default)]
    pub search: SearchScope,
}

pub fn load() -> Config {
    let mut config = Config::default();

    for path in pop_launcher::config::find("pop_shell") {
        let string = match std::fs::read_to_string(&path) {
            Ok(string) => string,
            Err(why) => {
                tracing::error!("failed to read config: {}", why);
                continue;
            }
        };

        match ron::from_str::<Config>(&string) {
            Ok(raw) => config.search = raw.search,
            Err(why) => {
                tracing::error!("failed to deserialize config: {}", why);
            }
        }
    }

    config
}
