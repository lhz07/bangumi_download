use crate::UTC_8;
use bitcode::{Decode, Encode};
use chrono::{DateTime, FixedOffset};
use serde::{Deserialize, Serialize};
use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, Default)]
pub struct TimeStamp(DateTime<FixedOffset>);

#[derive(Debug, Clone, Copy, Encode, Decode)]
pub struct TimeStampCoder(i64);

impl From<TimeStamp> for TimeStampCoder {
    fn from(value: TimeStamp) -> Self {
        Self(value.0.timestamp_millis())
    }
}

impl From<TimeStampCoder> for TimeStamp {
    fn from(value: TimeStampCoder) -> Self {
        let time =
            DateTime::from_timestamp_millis(value.0).expect("TimeStampCoder is always valid");
        time.with_timezone(&UTC_8).into()
    }
}

impl From<DateTime<FixedOffset>> for TimeStamp {
    fn from(value: DateTime<FixedOffset>) -> Self {
        Self(value)
    }
}

impl fmt::Display for TimeStamp {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0.format("%Y-%m-%d %H:%M:%S"))
    }
}
