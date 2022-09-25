use std::cmp::Ordering;

use crate::PluginPriority;


// holds all values used for ordering search results
pub struct Priority {
    pub plugin_priority: PluginPriority,
    pub match_score: f64,
    pub recent_use_index: usize,
    pub use_freq: usize,
    pub execlen: usize,
}


fn signum(val: i32) -> f64 {
    if val > 0 { return  1.0; }
    if val < 0 { return -1.0; }
    0.0
}

impl Priority {
    fn compute_value(&self, other: &Self) -> f64{
        // increases compared jw-score if this search result
        // was activated more frequent or recent by constant values
        let score = self.match_score
            + 0.06 * signum(self.recent_use_index as i32 - other.recent_use_index as i32)
            + 0.03 * signum(self.use_freq as i32 - other.use_freq as i32);
        // score cannot surpass exact matches
        if self.match_score < 1.0 {
            return score.min(0.99);
        }
        return score;
    }
}

impl PartialEq for Priority {
    fn eq(&self, other: &Self) -> bool {        
        self.plugin_priority == other.plugin_priority
            && self.compute_value(other) == other.match_score
            && self.execlen == other.execlen
    }
}

impl Eq for Priority {}

impl PartialOrd for Priority {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {        
        (other.plugin_priority, self.compute_value(other), self.execlen).partial_cmp(
            &(self.plugin_priority, other.match_score, other.execlen)
        )
    }
}

impl Ord for Priority {
    fn cmp(&self, other: &Self) -> Ordering {
        self.partial_cmp(other).unwrap()
    }
}