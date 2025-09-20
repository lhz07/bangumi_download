use crate::UTC_8;
use bincode::{BorrowDecode, Decode, Encode};
use chrono::{DateTime, FixedOffset};
use serde::{Deserialize, Serialize};
use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, Default)]
pub struct TimeStamp(DateTime<FixedOffset>);

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

impl Encode for TimeStamp {
    fn encode<E: bincode::enc::Encoder>(
        &self,
        encoder: &mut E,
    ) -> Result<(), bincode::error::EncodeError> {
        self.0.timestamp_millis().encode(encoder)
    }
}

impl<'de, Context> BorrowDecode<'de, Context> for TimeStamp {
    fn borrow_decode<D: bincode::de::BorrowDecoder<'de, Context = Context>>(
        decoder: &mut D,
    ) -> Result<Self, bincode::error::DecodeError> {
        let millis = i64::decode(decoder)?;
        DateTime::from_timestamp_millis(millis)
            .ok_or(bincode::error::DecodeError::Other(
                "Invalid timestamp millis",
            ))
            .map(|t| t.with_timezone(&UTC_8).into())
    }
}

impl<Context> Decode<Context> for TimeStamp {
    fn decode<D: bincode::de::Decoder<Context = Context>>(
        decoder: &mut D,
    ) -> Result<Self, bincode::error::DecodeError> {
        let millis = i64::decode(decoder)?;
        DateTime::from_timestamp_millis(millis)
            .ok_or(bincode::error::DecodeError::Other(
                "Invalid timestamp millis",
            ))
            .map(|t| t.with_timezone(&UTC_8).into())
    }
}
