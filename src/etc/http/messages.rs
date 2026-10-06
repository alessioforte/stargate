use serde::Serialize;
use serde_json::Value;
use std::collections::BTreeMap;
use std::fmt;
use utoipa::ToSchema;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(usize)]
pub enum MessageCode {
    SignupCompleted,
    SignupRequested,
    LogoutCompleted,
    AllSessionsLoggedOut,
    OrganizationDeleted,
    OAuthClientDeleted,
    AdminKeyRevoked,
    AdminKeyDeleted,
    ServiceAccountDeleted,
    UserInvitationSent,
    UserDeleted,
    UserAddedToOrganization,
    UserRemovedFromOrganization,
    ApiKeyDeleted,
    ApiKeyRevoked,
    PasswordResetAlreadyRequested,
    PasswordResetRequested,
    PasswordChanged,
}

impl MessageCode {
    pub const ALL: &'static [Self] = &[
        Self::SignupCompleted,
        Self::SignupRequested,
        Self::LogoutCompleted,
        Self::AllSessionsLoggedOut,
        Self::OrganizationDeleted,
        Self::OAuthClientDeleted,
        Self::AdminKeyRevoked,
        Self::AdminKeyDeleted,
        Self::ServiceAccountDeleted,
        Self::UserInvitationSent,
        Self::UserDeleted,
        Self::UserAddedToOrganization,
        Self::UserRemovedFromOrganization,
        Self::ApiKeyDeleted,
        Self::ApiKeyRevoked,
        Self::PasswordResetAlreadyRequested,
        Self::PasswordResetRequested,
        Self::PasswordChanged,
    ];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::SignupCompleted => "signup.completed",
            Self::SignupRequested => "signup.requested",
            Self::LogoutCompleted => "auth.logout_completed",
            Self::AllSessionsLoggedOut => "auth.all_sessions_logged_out",
            Self::OrganizationDeleted => "organization.deleted",
            Self::OAuthClientDeleted => "oauth_client.deleted",
            Self::AdminKeyRevoked => "admin_key.revoked",
            Self::AdminKeyDeleted => "admin_key.deleted",
            Self::ServiceAccountDeleted => "service_account.deleted",
            Self::UserInvitationSent => "user.invitation_sent",
            Self::UserDeleted => "user.deleted",
            Self::UserAddedToOrganization => "user.added_to_organization",
            Self::UserRemovedFromOrganization => "user.removed_from_organization",
            Self::ApiKeyDeleted => "api_key.deleted",
            Self::ApiKeyRevoked => "api_key.revoked",
            Self::PasswordResetAlreadyRequested => "password.reset_already_requested",
            Self::PasswordResetRequested => "password.reset_requested",
            Self::PasswordChanged => "password.changed",
        }
    }

    pub const fn message(self) -> &'static str {
        match self {
            Self::SignupCompleted => "User created successfully",
            Self::SignupRequested => "A signup request has been sent. Please check your email.",
            Self::LogoutCompleted => "User logged out successfully",
            Self::AllSessionsLoggedOut => "All sessions logged out successfully",
            Self::OrganizationDeleted => "Organization deleted successfully",
            Self::OAuthClientDeleted => "OAuth client deleted successfully",
            Self::AdminKeyRevoked => "Admin key revoked successfully",
            Self::AdminKeyDeleted => "Admin key deleted successfully",
            Self::ServiceAccountDeleted => "Service account deleted successfully",
            Self::UserInvitationSent => "User invitation sent successfully",
            Self::UserDeleted => "User deleted successfully",
            Self::UserAddedToOrganization => "User added to organization successfully",
            Self::UserRemovedFromOrganization => "User removed from organization successfully",
            Self::ApiKeyDeleted => "API key deleted successfully",
            Self::ApiKeyRevoked => "API key revoked successfully",
            Self::PasswordResetAlreadyRequested => {
                "A password-reset request has already been sent. Please check your email."
            }
            Self::PasswordResetRequested => "Password-reset request sent. Please check your email.",
            Self::PasswordChanged => "Password changed successfully",
        }
    }
}

impl fmt::Display for MessageCode {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

impl Serialize for MessageCode {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(self.as_str())
    }
}

#[derive(Debug, Serialize, ToSchema)]
pub struct MessageResponse {
    pub message: String,
    #[schema(value_type = String, example = "user.deleted")]
    pub code: MessageCode,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub params: BTreeMap<String, Value>,
}

impl MessageResponse {
    pub fn new(code: MessageCode) -> Self {
        Self {
            message: code.message().to_string(),
            code,
            params: BTreeMap::new(),
        }
    }

    pub fn with_param(mut self, key: impl Into<String>, value: impl Into<Value>) -> Self {
        self.params.insert(key.into(), value.into());
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn message_codes_are_unique_and_complete() {
        assert_eq!(
            MessageCode::ALL.len(),
            MessageCode::PasswordChanged as usize + 1
        );
        let mut names = HashSet::new();
        for code in MessageCode::ALL {
            assert!(names.insert(code.as_str()), "duplicate code: {code}");
            assert!(!code.message().is_empty(), "missing message: {code}");
        }
    }

    #[test]
    fn message_response_uses_code_metadata() {
        let response = MessageResponse::new(MessageCode::UserDeleted).with_param("id", "usr_123");
        let json = serde_json::to_value(response).unwrap();

        assert_eq!(json["code"], "user.deleted");
        assert_eq!(json["message"], "User deleted successfully");
        assert_eq!(json["params"]["id"], "usr_123");
    }
}
