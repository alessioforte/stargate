use crate::meta::{KeyEnvironment, KeyType};

pub struct KeyMetadata {
    pub key_type: String,
    pub environment: String,
}

pub enum ParseError {
    InvalidFormat,
    UnknownType,
    UnknownEnvironment,
    MissingKey,
}

pub fn parse_key_metadata(key: &str) -> Result<KeyMetadata, ParseError> {
    let parts: Vec<&str> = key.split('_').collect();
    if parts.len() != 3 {
        return Err(ParseError::InvalidFormat);
    }

    let key_type = match parts[0] {
        "sk" => KeyType::Secret,
        "pk" => KeyType::Public,
        "ak" => KeyType::Admin,
        _ => return Err(ParseError::UnknownType),
    };

    let environment = match parts[1] {
        "live" => KeyEnvironment::Live,
        "test" => KeyEnvironment::Test,
        _ => return Err(ParseError::UnknownEnvironment),
    };

    let key_value = parts[2];
    if key_value.is_empty() {
        return Err(ParseError::MissingKey);
    }

    Ok(KeyMetadata {
        key_type: key_type.as_str().to_string(),
        environment: environment.as_str().to_string(),
    })
}
