use crate::err::{ErrorCode, ErrorResponse};
use serde_json::Value;

use super::DEFAULT_MFA_STEP_UP_TTL_SECS;
use super::types::{MfaMode, MfaPolicy, OtpMethod};
use super::util::otp_env;

pub(crate) fn admin_step_up_required() -> bool {
    crate::etc::env::bool_or("MFA_ADMIN_STEP_UP_REQUIRED", false)
}

pub(crate) async fn ensure_passwordless_session_allowed(
    user: &db::ent::User,
) -> Result<(), ErrorResponse> {
    let policy = mfa_policy_for_user(user).await?;
    if policy.required {
        return Err(ErrorResponse::new(ErrorCode::PasswordlessMfaRequired));
    }

    Ok(())
}

pub(crate) async fn mfa_policy_for_user(user: &db::ent::User) -> Result<MfaPolicy, ErrorResponse> {
    let mode = mfa_mode()?;
    let mut methods = configured_mfa_methods();
    let user_methods = user_mfa_methods(&user.attrs);

    if !user_methods.is_empty() {
        methods.retain(|method| user_methods.contains(method));
    }

    let user_enabled = user_mfa_enabled(&user.attrs);
    if mode == MfaMode::Off {
        methods.clear();
    }

    let super_admin_required = mfa_required_for_super_admin()
        && crate::fun::is_super_admin_user_id(&user.id)
            .await
            .map_err(ErrorResponse::internal)?;

    let required = match mode {
        MfaMode::Off => false,
        MfaMode::Optional => user_enabled || super_admin_required,
        MfaMode::Required => true,
    };

    let preferred_method = user_preferred_mfa_method(&user.attrs)
        .filter(|method| methods.contains(method))
        .or_else(|| default_mfa_method().filter(|method| methods.contains(method)))
        .or_else(|| methods.first().copied());

    Ok(MfaPolicy {
        mode,
        required,
        methods,
        preferred_method,
    })
}

pub(crate) fn ensure_method_allowed(
    policy: &MfaPolicy,
    method: OtpMethod,
) -> Result<(), ErrorResponse> {
    if policy.mode == MfaMode::Off && policy.methods.is_empty() {
        return Err(ErrorResponse::new(ErrorCode::MfaDisabled));
    }

    if !policy.methods.contains(&method) {
        return Err(ErrorResponse::new(ErrorCode::MfaMethodUnavailable));
    }

    Ok(())
}

pub(crate) fn mfa_step_up_ttl_secs() -> Result<u64, ErrorResponse> {
    let ttl = otp_env("MFA_STEP_UP_TTL_SECS", DEFAULT_MFA_STEP_UP_TTL_SECS)?;
    if ttl == 0 {
        tracing::error!("MFA_STEP_UP_TTL_SECS must be greater than zero");
        return Err(ErrorResponse::new(ErrorCode::MfaUnavailable));
    }
    Ok(ttl)
}

fn mfa_mode() -> Result<MfaMode, ErrorResponse> {
    match crate::etc::env::string_or("MFA_MODE", "optional")
        .trim()
        .to_ascii_lowercase()
        .as_str()
    {
        "off" => Ok(MfaMode::Off),
        "optional" => Ok(MfaMode::Optional),
        "required" => Ok(MfaMode::Required),
        other => {
            tracing::error!(value = other, "invalid MFA_MODE");
            Err(ErrorResponse::new(ErrorCode::MfaUnavailable))
        }
    }
}

fn configured_mfa_methods() -> Vec<OtpMethod> {
    if !crate::etc::env::bool_or("MFA_EMAIL_ENABLED", true) {
        return Vec::new();
    }

    let configured = crate::etc::env::string_or("MFA_METHODS", "email");
    let mut methods = Vec::new();
    for method in configured.split(',').filter_map(parse_method) {
        if !methods.contains(&method) {
            methods.push(method);
        }
    }
    methods
}

fn default_mfa_method() -> Option<OtpMethod> {
    crate::etc::env::optional_string("MFA_DEFAULT_METHOD")
        .as_deref()
        .and_then(parse_method)
        .or(Some(OtpMethod::Email))
}

fn mfa_required_for_super_admin() -> bool {
    crate::etc::env::bool_or("MFA_REQUIRED_FOR_SUPER_ADMIN", true)
}

fn parse_method(value: &str) -> Option<OtpMethod> {
    match value.trim().to_ascii_lowercase().as_str() {
        "email" => Some(OtpMethod::Email),
        _ => None,
    }
}

fn user_mfa_enabled(attrs: &Value) -> bool {
    attrs
        .get("mfa")
        .and_then(|mfa| mfa.get("enabled"))
        .and_then(Value::as_bool)
        .unwrap_or(false)
}

fn user_mfa_methods(attrs: &Value) -> Vec<OtpMethod> {
    attrs
        .get("mfa")
        .and_then(|mfa| mfa.get("methods"))
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(Value::as_str)
                .filter_map(parse_method)
                .collect()
        })
        .unwrap_or_default()
}

fn user_preferred_mfa_method(attrs: &Value) -> Option<OtpMethod> {
    attrs
        .get("mfa")
        .and_then(|mfa| mfa.get("preferredMethod"))
        .and_then(Value::as_str)
        .and_then(parse_method)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn user_mfa_attrs_parse_email_method() {
        let attrs = json!({
            "mfa": {
                "enabled": true,
                "methods": ["email", "totp"],
                "preferredMethod": "email"
            }
        });

        assert!(user_mfa_enabled(&attrs));
        assert_eq!(user_mfa_methods(&attrs), vec![OtpMethod::Email]);
        assert_eq!(user_preferred_mfa_method(&attrs), Some(OtpMethod::Email));
    }
}
