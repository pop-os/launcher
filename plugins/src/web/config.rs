// SPDX-License-Identifier: GPL-3.0-only
// Copyright © 2021 System76

use serde::Deserialize;
use slab::Slab;
use std::collections::HashMap;

#[derive(Default, Clone)]
pub struct Config {
    matches: HashMap<String, u32>,
    queries: Slab<Vec<Definition>>,
}

impl Config {
    pub fn append(&mut self, rules: RawConfig) {
        for rule in rules.rules {
            let idx = self.queries.insert(rule.queries);
            for keyword in rule.matches {
                self.matches.insert(keyword, idx as u32);
            }
        }
    }

    pub fn get(&self, word: &str) -> Option<&[Definition]> {
        self.matches
            .get(word)
            .and_then(|idx| self.queries.get(*idx as usize))
            .map(|vec| &vec[..])
    }
}

#[derive(Debug, Deserialize, Clone)]
pub struct RawConfig {
    pub rules: Vec<Rule>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct Rule {
    pub matches: Vec<String>,
    pub queries: Vec<Definition>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct Definition {
    pub name: String,
    pub query: String,
}

pub fn load() -> Config {
    let mut config = Config::default();

    for path in pop_launcher::config::find("web") {
        let string = match std::fs::read_to_string(&path) {
            Ok(string) => string,
            Err(why) => {
                tracing::error!("failed to read config: {}", why);
                continue;
            }
        };

        match ron::from_str::<RawConfig>(&string) {
            Ok(raw) => config.append(raw),
            Err(why) => {
                tracing::error!("failed to deserialize config: {}", why);
            }
        }
    }

    config
}
