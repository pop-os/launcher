use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::collections::{hash_map::DefaultHasher, HashMap};
use std::hash::{Hash, Hasher};

const SHORTTERM_CAP: usize = 20;
const LONGTERM_CAP: usize = 100;

// Holds a long term storage that tracks how often a search
// result was activated, and a short term storage that stores
// the order of recently activated search results (higher
// vales are more recent).
// Keys for both mappings are hashes of the acvtivated result's
// command string.
#[derive(Debug, Default)]
pub struct RecentUseStorage {
    long_term: HashMap<u64, usize>,
    short_term: HashMap<u64, usize>,
}

fn hash_key<K: Hash>(key: K) -> u64 {
    let mut hasher = DefaultHasher::new();
    key.hash(&mut hasher);
    hasher.finish()
}

impl RecentUseStorage {
    pub fn add<K: Hash>(&mut self, exec: &K) {
        let key = hash_key(exec);
        *self.long_term.entry(key).or_insert(0) += 1;
        let short_term_idx = self.short_term.values().max().unwrap_or(&0) + 1;
        self.short_term.insert(key, short_term_idx);
        self.trim();
    }

    fn trim(&mut self) {
        while self.short_term.len() > SHORTTERM_CAP {
            let key = *self.short_term.iter().min_by_key(|kv| kv.1).unwrap().0;
            self.short_term.remove(&key);
        }

        while self.long_term.values().sum::<usize>() > LONGTERM_CAP {
            let mut delete_keys = Vec::new();
            for (k, v) in &mut self.long_term {
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
        let mut stvec: Vec<_> = self.short_term.keys().copied().collect();
        stvec.sort_by_key(|k| self.short_term[k]);
        (&self.long_term, stvec).serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for RecentUseStorage {
    fn deserialize<D>(deserializer: D) -> Result<RecentUseStorage, D::Error>
    where
        D: Deserializer<'de>,
    {
        type SerType = (HashMap<u64, usize>, Vec<u64>);
        let (long_term, stv) = SerType::deserialize(deserializer)?;
        let short_term: HashMap<_, _> = stv.into_iter().enumerate().map(|(v, k)| (k, v)).collect();
        Ok(RecentUseStorage {
            long_term,
            short_term,
        })
    }
}
