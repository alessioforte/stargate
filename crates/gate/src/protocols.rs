#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Protocols {
    Http,
    Ws,
}

impl Protocols {
    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "http" => Some(Protocols::Http),
            "https" => Some(Protocols::Http),
            "ws" => Some(Protocols::Ws),
            "wss" => Some(Protocols::Ws),
            _ => None,
        }
    }

    pub fn as_str(&self) -> &str {
        match self {
            Protocols::Http => "http",
            Protocols::Ws => "ws",
        }
    }
}
