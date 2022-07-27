use std::collections::{HashMap, hash_map::DefaultHasher, VecDeque};
use std::hash::{Hasher, Hash};
use serde::{Deserialize, Serialize};

const N_RECENT_ITEMS: usize = 20;

// Holds a long term storage that tracks how often a search
// result was activated, and a short term storage that stores
// the order of recently activated search results (higher
// vales are more recent).
// Keys for both mappings are hashes of the acvtivated result's
// command string.
#[derive(Debug, Default, Serialize, Deserialize)]
pub struct RecentUseStorage {
    long_term: HashMap<usize, usize>,
    short_term: VecDeque<usize>
}


fn hash_key<K: Hash>(key: K) -> usize {
    let mut hasher = DefaultHasher::new();
    key.hash(&mut hasher);
    hasher.finish() as usize
}


impl RecentUseStorage {
    pub fn add<K: Hash>(&mut self, exec: &K) {
        let key = hash_key(exec);
        let count = self.long_term.entry(key).or_insert(0);
        *count += 1;
        self.short_term.push_back(key);
        if self.short_term.len() > N_RECENT_ITEMS {
            self.short_term.pop_front();
        }
    }

    pub fn get_recent<K: Hash>(&self, exec: &K) -> usize {
        self.short_term.iter().enumerate().rev().filter_map(
            |(i, k)| if *k == hash_key(exec) { Some(i+1) } else { None }
        ).next().unwrap_or(0)
    }

    pub fn get_freq<K: Hash>(&self, exec: &K) -> usize {
        self.long_term.get(&hash_key(exec)).copied().unwrap_or(0)
    }
}