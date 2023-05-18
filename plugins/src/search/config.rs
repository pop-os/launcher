// SPDX-License-Identifier: GPL-3.0-only
// Copyright © 2023 System76

use regex::Regex;
use serde::Deserialize;

#[derive(Debug, Clone)]
pub struct Config {
    pub rules: Vec<CompiledRule>,
}

#[derive(Debug, Clone)]
pub struct CompiledRule {
    pub pattern: Regex,
    pub action: Definition,
    pub split: Option<Regex>,
}

impl Config {
    pub fn append(&mut self, config: RawConfig) {
        let escape = |keywords: &Vec<String>| {
            keywords
                .into_iter()
                .map(|m| regex::escape(&m))
                .collect::<Vec<String>>()
                .join("|")
        };
        for rule in config.rules {
            let pattern_re = match rule.pattern {
                Pattern::StartsWith(keywords) => {
                    Regex::new(&format!("^({})", escape(&keywords))).unwrap()
                }
                Pattern::StartsWithKeyword(keywords) => {
                    Regex::new(&format!("^({})\\b", escape(&keywords))).unwrap()
                }
                Pattern::EndsWith(keywords) => {
                    Regex::new(&format!("({})$", escape(&keywords))).unwrap()
                }
                Pattern::EndsWithKeyword(keywords) => {
                    Regex::new(&format!("\\b({})$", escape(&keywords))).unwrap()
                }
                Pattern::Regex(uncompiled) => Regex::new(&uncompiled).unwrap(),
            };

            let split_re = match rule.split {
                Split::ShellWords => None,
                Split::Whitespace => Regex::new("\\s+").ok(),
                Split::Regex(uncompiled) => Regex::new(&uncompiled).ok(),
            };

            self.rules.push(CompiledRule {
                pattern: pattern_re,
                action: rule.action,
                split: split_re,
            })
        }
        // eprintln!("rules: {:?}", self.rules);
    }

    pub fn match_rule(&self, query_string: &str) -> Option<&CompiledRule> {
        for rule in &self.rules {
            if rule.pattern.is_match(query_string) {
                return Some(&rule);
            }
        }
        None
    }
}


impl Default for Config {
    fn default() -> Self {
        Config { rules: Vec::new() }
    }
}

#[derive(Debug, Deserialize, Clone)]
pub struct RawConfig {
    pub rules: Vec<Rule>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct Rule {
    pub pattern: Pattern,
    pub action: Definition,

    #[serde(default = "split_shell_words")]
    pub split: Split,
}

#[derive(Debug, Deserialize, Clone)]
pub enum Pattern {
    StartsWith(Vec<String>),
    StartsWithKeyword(Vec<String>),
    EndsWith(Vec<String>),
    EndsWithKeyword(Vec<String>),
    Regex(String),
}

#[derive(Debug, Deserialize, Clone)]
pub enum Split {
    ShellWords,
    Whitespace,
    Regex(String),
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
    #[serde(default = "echo_result")]
    pub result_name: String,

    // An optional string; shown as the "description" line of the query result.
    #[serde(default = "blank_string")]
    pub result_desc: String,

    // REQUIRED: The shell command to run when the user selects a result (usually, "Enter" key pressed)
    pub run_command: String,
}

fn regex_match_all() -> String {
    "^.*$".to_string()
}

fn echo_result() -> String {
    "$OUTPUT".to_string()
}

fn blank_string() -> String {
    "".to_string()
}

fn split_shell_words() -> Split {
    Split::ShellWords
}

pub fn load() -> Config {
    let mut config = Config::default();

    for path in pop_launcher::config::find("search") {
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