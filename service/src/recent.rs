use std::collections::{HashMap, hash_map::DefaultHasher};
use std::hash::{Hasher, Hash};
use serde::{Deserialize, Serialize};


#[derive(Serialize, Deserialize, Debug)]
pub struct RecentUseStorage {
    map: HashMap<usize, usize>
}

impl RecentUseStorage {
    pub fn new() -> Self {
        Self{ map: HashMap::new() }
    }

    pub fn add<K: Hash + std::fmt::Debug>(&mut self, exec: &K) {
        let mut hasher = DefaultHasher::new();
        exec.hash(&mut hasher);
        let key = hasher.finish();

        let count = self.map.entry(key as usize).or_insert(0);
        *count += 1;
    }

    pub fn get<K: Hash + std::fmt::Debug>(&self, exec: &K) -> usize {
        let mut hasher = DefaultHasher::new();
        exec.hash(&mut hasher);
        let key = hasher.finish() as usize;
        return self.map.get(&key).copied().unwrap_or(0);
    }
}