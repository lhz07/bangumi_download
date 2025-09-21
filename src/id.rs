use bincode::{Decode, Encode};
use std::fmt;
use std::sync::atomic::AtomicU64;

static ORIGIN: AtomicU64 = AtomicU64::new(0);

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
