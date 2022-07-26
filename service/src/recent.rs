use std::collections::{HashMap, hash_map::DefaultHasher};
use std::hash::{Hasher, Hash};
use serde::{Deserialize, Serialize, Serializer, Deserializer};


// Holds a long term storage that tracks how often a search
// result was activated, and a short term storage that stores
// the order of recently activated search results (higher
// vales are more recent).
// Keys for both mappings are hashes of the acvtivated result's
// command string.
#[derive(Debug, Default)]
pub struct RecentUseStorage {
    long_term: HashMap<usize, usize>,
    short_term: HashMap<usize, usize>,
    short_term_queries: usize
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

        self.short_term_queries += 1;
        self.short_term.insert(key, self.short_term_queries);
    }

    pub fn get_recent<K: Hash>(&self, exec: &K) -> usize {
        self.short_term.get(&hash_key(exec)).copied().unwrap_or(0)
    }

    pub fn get_freq<K: Hash>(&self, exec: &K) -> usize {
        self.long_term.get(&hash_key(exec)).copied().unwrap_or(0)
    }
}

impl Serialize for RecentUseStorage {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        // Only serialize the long term storage
        HashMap::serialize(&self.long_term, serializer)
    }
}

impl<'de> Deserialize<'de> for RecentUseStorage {
    fn deserialize<D>(deserializer: D) -> Result<RecentUseStorage, D::Error>
    where
        D: Deserializer<'de>,
    {
        let lt = HashMap::deserialize(deserializer)?;
        Ok(RecentUseStorage{ long_term: lt, short_term: HashMap::new(), short_term_queries: 0})
    }
}