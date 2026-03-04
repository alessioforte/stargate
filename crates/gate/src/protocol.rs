use serde::{Deserialize, Serialize};
use std::str::FromStr;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "lowercase")]
pub enum Protocol {
    Http,
    Https,
    Ws,
    Wss,
}

pub const PROTOCOL_COUNT: usize = 4;

impl Protocol {
    pub fn as_str(&self) -> &str {
        match self {
            Protocol::Http => "http",
            Protocol::Https => "https",
            Protocol::Ws => "ws",
            Protocol::Wss => "wss",
        }
    }

    #[inline]
    pub fn index(&self) -> usize {
        *self as usize
    }
}

impl FromStr for Protocol {
    type Err = ();

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "http" => Ok(Protocol::Http),
            "https" => Ok(Protocol::Https),
            "ws" => Ok(Protocol::Ws),
            "wss" => Ok(Protocol::Wss),
            _ => Err(()),
        }
    }
}
