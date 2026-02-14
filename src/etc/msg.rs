use serde::{Deserialize, Serialize};

/// A response structure for messages, typically used for API responses.
#[derive(Debug, Serialize, Deserialize, utoipa::ToSchema)]
pub struct MessageResponse {
    pub message: String,
    pub code: String,
}

impl MessageResponse {
    pub fn new(message: &str, code: &str) -> Self {
        Self {
            message: message.to_string(),
            code: code.to_string(),
        }
    }
}
