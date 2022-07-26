use std::collections::{HashMap, hash_map::DefaultHasher};
use std::hash::{Hasher, Hash};
use serde::{Deserialize, Serialize};


#[derive(Serialize, Deserialize, Debug)]
pub struct RecentUseStorage {
    long_term: HashMap<usize, usize>,
    short_term: HashMap<usize, usize>,
    short_term_queries: usize
}


impl RecentUseStorage {
    pub fn new() -> Self {
        Self{ long_term: HashMap::new(), short_term: HashMap::new(), short_term_queries: 0 }
    }

    pub fn add<K: Hash + std::fmt::Debug>(&mut self, exec: &K) {
        let mut hasher = DefaultHasher::new();
        exec.hash(&mut hasher);
        let key = hasher.finish() as usize;

        let count = self.long_term.entry(key).or_insert(0);
        *count += 1;

        self.short_term_queries += 1;
        self.short_term.insert(key, self.short_term_queries);
    }

    pub fn get<K: Hash + std::fmt::Debug>(&self, exec: &K) -> (usize, usize) {
        let mut hasher = DefaultHasher::new();
        exec.hash(&mut hasher);
        let key = hasher.finish() as usize;
        let lt = self.long_term.get(&key).copied().unwrap_or(0);
        let st = self.short_term.get(&key).copied().unwrap_or(0);
        return (st, lt);
    }
}