pub enum KeyEnvironment {
    Live,
    Test,
}

pub enum KeyType {
    Secret,
    Public,
    Admin,
}

impl KeyEnvironment {
    pub fn as_str(&self) -> &str {
        match self {
            KeyEnvironment::Live => "live",
            KeyEnvironment::Test => "test",
        }
    }
}

impl KeyType {
    pub fn as_str(&self) -> &str {
        match self {
            KeyType::Secret => "sk",
            KeyType::Public => "pk",
            KeyType::Admin => "ak",
        }
    }
}
