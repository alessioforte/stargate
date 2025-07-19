use serde::{Deserialize, Serialize};

/// A response structure for messages, typically used for API responses.
#[derive(Debug, Serialize, Deserialize)]
pub struct MessageResponse {
    pub message: String,
    pub code: String,
}

impl MessageResponse {
    pub fn new(message: String, code: String) -> Self {
        Self { message, code }
    }
}
