struct UserSettings {
    mfa_enabled: bool,
    mfa_type: String, // sms, email, totp
    mfa_code: String,
    mfa_expires_at: i64,
}
