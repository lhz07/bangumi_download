use std::{fmt, sync::atomic::AtomicU64};

use bincode::{Decode, Encode};
use once_cell::sync::Lazy;

static ORIGIN: Lazy<AtomicU64> = Lazy::new(|| AtomicU64::new(0));

#[derive(Encode, Decode, PartialEq, PartialOrd, Ord, Eq, Clone, Copy, Hash, Debug)]
pub struct Id(u64);

impl fmt::Display for Id {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "ID: {}", self.0)
    }
}

impl Id {
    pub fn generate() -> Self {
        Id(ORIGIN.fetch_add(1, std::sync::atomic::Ordering::Relaxed))
    }
}
