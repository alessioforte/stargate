use serde::{Deserialize, Serialize};

// TODO: Add more fields as needed and handle code for i18n
// Json Response Messages for the API
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
