use core::num::NonZeroU32;
use core::time::Duration;
use serde::{Deserialize, Serialize};

#[derive(Debug, PartialEq, Eq, Clone, Copy, Serialize, Deserialize)]
pub struct Quota {
    pub max_burst: NonZeroU32,
    pub replenish_1_per: Duration,
}

impl Default for Quota {
    fn default() -> Self {
        Quota {
            max_burst: NonZeroU32::new(2).unwrap(),
            replenish_1_per: Duration::from_secs(5),
        }
    }
}
