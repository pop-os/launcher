// SPDX-License-Identifier: GPL-3.0-only
// Copyright © 2023 System76

use serde::Deserialize;
use slab::Slab;
use std::collections::HashMap;

#[derive(Default, Clone, Debug)]
pub struct Config {
    matches: HashMap<String, u32>,
    definitions: Slab<Definition>,
}

impl Config {
    pub fn append(&mut self, config: RawConfig) {
        for rule in config.rules {
            let idx = self.definitions.insert(rule.action);
            for keyword in rule.matches {
                self.matches.entry(keyword).or_insert(idx as u32);
            }
        }
    }

    pub fn get(&self, word: &str) -> Option<&Definition> {
        self.matches
            .get(word)
            .and_then(|idx| self.definitions.get(*idx as usize))
    }
}

#[derive(Debug, Deserialize, Clone)]
pub struct RawConfig {
    pub rules: Vec<Rule>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct Rule {
    pub matches: Vec<String>,
    pub action: Definition,
}

/**
 * The DisplayLine configures what to show in the results list, based on what the
 * shell command's STDOUT produces.
 */
#[derive(Debug, Deserialize, Clone)]
pub struct Definition {
    // NOTE: In each field below, the variables $QUERY, $KEYWORDS, and $KEYWORDn are available.

    // REQUIRED: The shell command to run whose STDOUT will be interpreted as a series of query results
    // Each line of output is available as $OUTPUT in result_name, result_desc, and run_command.
    pub query_command: String,

    // An optional regex applied to each STDOUT line; each capture will be available as $CAPTUREn
    // variables in result_name, result_desc, and run_command, where "n" is a number from 1..len(captures)
    #[serde(default = "regex_match_all")]
    pub output_captures: String,

    // An optional string; shown as the "name" line of the query result.
    #[serde(default = "result_echo")]
    pub result_name: String,

    // An optional string; shown as the "description" line of the query result.
    #[serde(default = "string_blank")]
    pub result_desc: String,

    // REQUIRED: The shell command to run when the user selects a result (usually, "Enter" key pressed)
    pub run_command: String,
}

fn regex_match_all() -> String {
    "^.*$".to_string()
}

fn result_echo() -> String {
    "$OUTPUT".to_string()
}

fn string_blank() -> String {
    "".to_string()
}

pub fn load() -> Config {
    eprintln!("load config");
    let mut config = Config::default();

    for path in pop_launcher::config::find("search") {
        let string = match std::fs::read_to_string(&path) {
            Ok(string) => string,
            Err(why) => {
                eprintln!("load config err A");
                tracing::error!("failed to read config: {}", why);
                continue;
            }
        };

        match ron::from_str::<RawConfig>(&string) {
            Ok(raw) => {
                eprintln!("raw: {:?}", raw);
                config.append(raw)
            }
            Err(why) => {
                eprintln!("load config err B: {}", why);
                tracing::error!("failed to deserialize config: {}", why);
            }
        }
    }

    eprintln!("load config: {:?}", config);

    config
}
