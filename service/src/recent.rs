use std::collections::{HashMap, hash_map::DefaultHasher, VecDeque};
use std::hash::{Hasher, Hash};
use serde::{Deserialize, Serialize};

const SHORTTERM_CAP: usize = 20;
const LONGTERM_CAP: usize = 100;

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
        self.trim()
    }

    fn trim(&mut self) {
        while self.short_term.len() > SHORTTERM_CAP {
            self.short_term.pop_front();
        }

        if self.long_term.values().sum::<usize>() > LONGTERM_CAP {
            let mut delete_keys = Vec::new();
            for (k, v) in self.long_term.iter_mut() {
                *v /= 2;
                if *v == 0 {
                    delete_keys.push(*k);
                }
            }
            for k in delete_keys {
                self.long_term.remove(&k);
            }
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