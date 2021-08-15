use serde::Deserialize;
use slab::Slab;
use std::collections::HashMap;

#[derive(Default)]
pub struct Config {
    matches: HashMap<String, u32>,
    queries: Slab<Vec<Definition>>,
}

impl Config {
    pub fn new(rules: RawConfig) -> Self {
        let mut config = Self::default();

        for rule in rules.rules {
            let idx = config.queries.insert(rule.queries);
            for keyword in rule.matches {
                config.matches.insert(keyword, idx as u32);
            }
        }

        config
    }

    pub fn get(&self, word: &str) -> Option<&[Definition]> {
        self.matches
            .get(word)
            .and_then(|idx| self.queries.get(*idx as usize))
            .map(|vec| &vec[..])
    }
}

#[derive(Debug, Deserialize)]
pub struct RawConfig {
    pub rules: Vec<Rule>,
}

#[derive(Debug, Deserialize)]
pub struct Rule {
    pub matches: Vec<String>,
    pub queries: Vec<Definition>,
}

#[derive(Debug, Deserialize)]
pub struct Definition {
    pub name: String,
    pub query: String,
}

pub fn load() -> Config {
    pop_launcher::config::find("web")
        .next()
        .and_then(|path| {
            let string = match std::fs::read_to_string(&path) {
                Ok(string) => string,
                Err(why) => {
                    tracing::error!("failed to read config: {}", why);
                    return None;
                }
            };

            match ron::from_str::<RawConfig>(&string) {
                Ok(config) => Some(Config::new(config)),
                Err(why) => {
                    tracing::error!("failed to deserialize config: {}", why);
                    None
                }
            }
        })
        .unwrap_or_default()
}
