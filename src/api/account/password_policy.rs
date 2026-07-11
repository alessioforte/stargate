use axum::Json;
use serde::Serialize;
use utoipa::ToSchema;

/// Global password requirements, for rendering signup/reset checklists.
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct PasswordPolicyResponse {
    /// Minimum length in characters, if enforced.
    pub min_length: Option<usize>,
    /// Maximum length in characters, if enforced.
    pub max_length: Option<usize>,
    pub lowercase: bool,
    pub uppercase: bool,
    pub digits: bool,
    pub special_chars: bool,
    /// Password must not contain the username.
    pub not_username: bool,
    /// Password must not contain the email address or its local part.
    pub not_email: bool,
    /// Password must not be a known-common password.
    pub banned_passwords: bool,
    /// Regex the password must match, if configured.
    pub regex_pattern: Option<String>,
    /// Maximum allowed run of the same character, if enforced.
    pub max_repeats: Option<usize>,
    /// Password age in days after which login forces a change, if enforced.
    pub max_age_days: Option<u32>,
    /// Number of recent passwords (including the current one) that cannot be
    /// reused, if enforced.
    pub history: Option<usize>,
}

#[utoipa::path(
    get,
    path = "/account/password-policy",
    tags = ["Account"],
    summary = "Password Policy",
    description = "Global password requirements applied at signup and password changes. Organizations may enforce different rules for their members; those are validated server-side and surfaced as 400 responses.",
    responses(
        (status = 200, description = "OK", body = PasswordPolicyResponse),
    )
)]
pub async fn get_password_policy() -> Json<PasswordPolicyResponse> {
    let set = &crate::act::password_policy::global().set;
    Json(PasswordPolicyResponse {
        min_length: set.min_length,
        max_length: set.max_length,
        lowercase: set.lowercase,
        uppercase: set.uppercase,
        digits: set.digits,
        special_chars: set.special_chars,
        not_username: set.not_username,
        not_email: set.not_email,
        banned_passwords: set.banned_passwords,
        regex_pattern: set.regex_pattern.clone(),
        max_repeats: set.max_repeats,
        max_age_days: set.max_age_days,
        history: set.history,
    })
}
