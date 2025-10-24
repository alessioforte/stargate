#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Protocols {
    Http,
    Https,
    Ws,
    Wss,
}

impl Protocols {
    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "http" => Some(Protocols::Http),
            "https" => Some(Protocols::Https),
            "ws" => Some(Protocols::Ws),
            "wss" => Some(Protocols::Wss),
            _ => None,
        }
    }

    pub fn as_str(&self) -> &str {
        match self {
            Protocols::Http => "http",
            Protocols::Https => "https",
            Protocols::Ws => "ws",
            Protocols::Wss => "wss",
        }
    }
}
