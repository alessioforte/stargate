use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "lowercase")]
pub enum Protocol {
    Http,
    Https,
    Ws,
    Wss,
}

impl Protocol {
    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "http" => Some(Protocol::Http),
            "https" => Some(Protocol::Https),
            "ws" => Some(Protocol::Ws),
            "wss" => Some(Protocol::Wss),
            _ => None,
        }
    }

    pub fn as_str(&self) -> &str {
        match self {
            Protocol::Http => "http",
            Protocol::Https => "https",
            Protocol::Ws => "ws",
            Protocol::Wss => "wss",
        }
    }
}
