use crate::err::{ErrorCode, ErrorResponse};
use serde_json::Value;

use super::settings::otp_settings;
use super::types::{MfaMode, MfaPolicy, OtpMethod};

pub(crate) fn admin_step_up_required() -> bool {
    otp_settings().admin_step_up_required
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
    let settings = otp_settings();
    let mode = settings.mfa_mode;
    let mut methods = settings.mfa_methods.clone();
    let user_methods = user_mfa_methods(&user.attrs);

    if !user_methods.is_empty() {
        methods.retain(|method| user_methods.contains(method));
    }

    let user_enabled = user_mfa_enabled(&user.attrs);
    if mode == MfaMode::Off {
        methods.clear();
    }

    let super_admin_required = settings.mfa_required_for_super_admin
        && crate::iam::authz::admin::is_super_admin_user_id(&user.id)
            .await
            .map_err(ErrorResponse::internal)?;

    let required = match mode {
        MfaMode::Off => false,
        MfaMode::Optional => user_enabled || super_admin_required,
        MfaMode::Required => true,
    };

    let preferred_method = user_preferred_mfa_method(&user.attrs)
        .filter(|method| methods.contains(method))
        .or_else(|| Some(settings.mfa_default_method).filter(|method| methods.contains(method)))
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

pub(crate) fn mfa_step_up_ttl_secs() -> u64 {
    otp_settings().step_up_ttl_secs
}

/// A method named in user attrs; unknown names are ignored there.
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
