use serde::de::Error;
use serde::{Deserialize, Deserializer};

pub trait HexDe: Sized {
    fn deserialize<'de, D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>;
}

impl HexDe for u32 {
    fn deserialize<'de, D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let str: String = Deserialize::deserialize(deserializer)?;
        if str.starts_with("0x") {
            u32::from_str_radix(str.split_at(2).1, 16).map_err(D::Error::custom)
        } else {
            str.parse::<u32>().map_err(D::Error::custom)
        }
    }
}
