use std::cmp::Ordering;

use crate::PluginPriority;

// holds all values used for ordering search results
pub struct Priority {
    pub plugin_priority: PluginPriority,
    pub match_score: f64,
    pub recent_score: f64,
    pub freq_score: f64,
    pub execlen: usize,
}

fn falloff(x: f64) -> f64 {
    x.clamp(0., 1.).powi(3)
}

impl Priority {
    fn compute_value(&self) -> f64 {
        let score = if self.match_score > 1. {
            self.match_score + 0.1
        } else {
            self.match_score
        };
        score + 0.06 * falloff(self.recent_score) + 0.03 * falloff(self.freq_score)
    }
}

impl PartialEq for Priority {
    fn eq(&self, other: &Self) -> bool {
        self.plugin_priority == other.plugin_priority
            && self.compute_value() == other.compute_value()
            && self.execlen == other.execlen
    }
}

impl Eq for Priority {}

impl PartialOrd for Priority {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Priority {
    fn cmp(&self, other: &Self) -> Ordering {
        match other.plugin_priority.cmp(&self.plugin_priority) {
            Ordering::Equal => match self.compute_value().total_cmp(&other.compute_value()) {
                Ordering::Equal => self.execlen.cmp(&other.execlen),
                p => p,
            },
            p => p,
        }
    }
}
