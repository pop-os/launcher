use std::cmp::Ordering;

use crate::PluginPriority;


pub struct Priority {
    pub plugin_priority: PluginPriority,
    pub match_score: f64,
    pub recent_use_index: usize,
    pub use_freq: usize,
}

impl Priority {
    fn compute_value(&self, other: &Self) -> f64{
        fn signum(val: i32) -> f64 {
            if val > 0 { return  1.0; }
            if val < 0 { return -1.0; }
            return 0.0;
        }

        self.match_score
            + 0.2 * signum(self.recent_use_index as i32 - other.recent_use_index as i32)
            + 0.1 * signum(self.use_freq as i32 - other.use_freq as i32)
    }
}

impl PartialEq for Priority {
    fn eq(&self, other: &Self) -> bool {        
        self.plugin_priority == other.plugin_priority
            && self.compute_value(other) == other.match_score
    }
}

impl Eq for Priority {}

impl PartialOrd for Priority {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {        
        (self.plugin_priority, self.compute_value(other)).partial_cmp(&(other.plugin_priority, other.match_score))
    }
}

impl Ord for Priority {
    fn cmp(&self, other: &Self) -> Ordering {
        self.partial_cmp(other).unwrap()
    }
}