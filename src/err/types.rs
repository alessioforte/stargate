use http::StatusCode;
use serde::Serialize;
use std::fmt;
use utoipa::ToSchema;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum ErrorType {
    Request,
    Validation,
    Authentication,
    Authorization,
    NotFound,
    Conflict,
    RateLimit,
    Upstream,
    Availability,
    Internal,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ErrorDefinition {
    pub status: StatusCode,
    pub error_type: ErrorType,
    pub message: &'static str,
}

const fn definition(
    status: StatusCode,
    error_type: ErrorType,
    message: &'static str,
) -> ErrorDefinition {
    ErrorDefinition {
        status,
        error_type,
        message,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(usize)]
pub enum ErrorCode {
    RequestMissingContentType,
    RequestInvalidContentType,
    RequestMissingPayload,
    RequestMalformedPayload,
    RequestInvalid,
    RequestInvalidQuery,
    RequestInvalidJson,
    RequestPayloadTooLarge,
    RequestMethodNotAllowed,
    AuthTokenMissing,
    AuthTokenInvalid,
    AuthInvalidCredentials,
    AuthInsufficientPermissions,
    AuthUserSessionRequired,
    AuthUntrustedOrigin,
    AuthOpenidScopeRequired,
    AuthLoginRateLimited,
    AuthLoginUnavailable,
    SessionOrganizationMembershipRequired,
    SessionNotFound,
    UserNotFound,
    UserEmailAlreadyExists,
    UserNicknameAlreadyExists,
    UserSuperAdminDeleteForbidden,
    OrganizationNotFound,
    OrganizationNameRequired,
    MembershipRoleRequired,
    MembershipRoleTooLong,
    ApiKeyNotFound,
    ApiKeyOwnerTypeInvalid,
    ApiKeyOwnerRequired,
    ApiKeyOrganizationUnsupported,
    ApiKeyOwnerNotMember,
    ApiKeyAlreadyRevoked,
    AdminKeyNotFound,
    AdminKeyPermissionsRequired,
    AdminKeyPermissionInvalid,
    AdminKeyRevoked,
    AdminKeyAlreadyRevoked,
    ServiceAccountNotFound,
    ServiceAccountNameRequired,
    OAuthClientIdInvalid,
    OAuthClientNameRequired,
    OAuthClientNameTooLong,
    OAuthClientListValueEmpty,
    OAuthClientValueUnsupported,
    OAuthClientAuthMethodUnsupported,
    OAuthClientRedirectUriInvalid,
    OAuthClientRedirectUriInsecure,
    OAuthClientGrantTypesRequired,
    OAuthClientConfidentialRequired,
    OAuthClientCodeResponseTypeRequired,
    OAuthClientRedirectUriRequired,
    OAuthClientAuthorizationCodeGrantRequired,
    OAuthClientRefreshTokenGrantInvalid,
    OAuthClientOfflineAccessGrantInvalid,
    OAuthClientTypeChangeForbidden,
    OAuthClientNotFound,
    OAuthClientAlreadyExists,
    OAuthClientSecretUnsupported,
    OutboxEventNotFound,
    ConfigurationNotFound,
    ConfigurationInvalid,
    AccessControlRevisionConflict,
    AccessControlPoliciesInvalid,
    AccessControlEvaluationUnavailable,
    AccessControlActionConflict,
    AccessControlActionInvalid,
    AccessControlRuntimeUnavailable,
    SignupTokenInvalid,
    SignupRequestNotFound,
    SignupEmailMismatch,
    SignupUserAlreadyExists,
    SignupRequestAlreadyExists,
    PasswordPolicyViolation,
    PasswordResetTokenInvalid,
    PasswordResetRequestInvalid,
    PasswordUpdateFailed,
    OtpChallengeIdInvalid,
    OtpCodeInvalidOrExpired,
    OtpCodeInvalid,
    OtpAttemptsExceeded,
    OtpRequestRateLimited,
    OtpChallengeChanged,
    OtpUnavailable,
    EmailOtpUnavailable,
    MfaUnavailable,
    MfaDisabled,
    MfaMethodUnavailable,
    PasswordlessMfaRequired,
    SocialAuthorizationCodeMissing,
    SocialStateInvalid,
    SocialTokenExchangeFailed,
    SocialUserInfoFailed,
    SocialStateGenerationFailed,
    GatewayRouteNotFound,
    GatewayPolicyNotFound,
    GatewayMiddlewareNotFound,
    GatewayUnauthorized,
    GatewayAccessDenied,
    GatewayPolicyConfigurationInvalid,
    GatewayRequestPreparationFailed,
    GatewayHttpClientUnavailable,
    GatewayNoHealthyUpstream,
    GatewayOrganizationContextRequired,
    GatewayLimiterUnavailable,
    GatewayLimitConfigurationInvalid,
    GatewayRateLimitExceeded,
    GatewayQuotaExceeded,
    GatewayIngressRateLimitExceeded,
    GatewayIngressUnavailable,
    GatewayOverloaded,
    GatewayReplayMemoryExhausted,
    GatewayTimeout,
    GatewayUploadTimeout,
    GatewayRequestBodyFailed,
    GatewayCancelled,
    GatewayReplayBufferFailed,
    GatewayReplayPayloadTooLarge,
    GatewayWebsocketUpgradeInvalid,
    GatewayDirectResponseStatusInvalid,
    UpstreamConnectionFailed,
    InternalContextUnavailable,
    I18nLocaleNotSupported,
    SystemInternal,
}

impl ErrorCode {
    pub const ALL: &'static [Self] = &[
        Self::RequestMissingContentType,
        Self::RequestInvalidContentType,
        Self::RequestMissingPayload,
        Self::RequestMalformedPayload,
        Self::RequestInvalid,
        Self::RequestInvalidQuery,
        Self::RequestInvalidJson,
        Self::RequestPayloadTooLarge,
        Self::RequestMethodNotAllowed,
        Self::AuthTokenMissing,
        Self::AuthTokenInvalid,
        Self::AuthInvalidCredentials,
        Self::AuthInsufficientPermissions,
        Self::AuthUserSessionRequired,
        Self::AuthUntrustedOrigin,
        Self::AuthOpenidScopeRequired,
        Self::AuthLoginRateLimited,
        Self::AuthLoginUnavailable,
        Self::SessionOrganizationMembershipRequired,
        Self::SessionNotFound,
        Self::UserNotFound,
        Self::UserEmailAlreadyExists,
        Self::UserNicknameAlreadyExists,
        Self::UserSuperAdminDeleteForbidden,
        Self::OrganizationNotFound,
        Self::OrganizationNameRequired,
        Self::MembershipRoleRequired,
        Self::MembershipRoleTooLong,
        Self::ApiKeyNotFound,
        Self::ApiKeyOwnerTypeInvalid,
        Self::ApiKeyOwnerRequired,
        Self::ApiKeyOrganizationUnsupported,
        Self::ApiKeyOwnerNotMember,
        Self::ApiKeyAlreadyRevoked,
        Self::AdminKeyNotFound,
        Self::AdminKeyPermissionsRequired,
        Self::AdminKeyPermissionInvalid,
        Self::AdminKeyRevoked,
        Self::AdminKeyAlreadyRevoked,
        Self::ServiceAccountNotFound,
        Self::ServiceAccountNameRequired,
        Self::OAuthClientIdInvalid,
        Self::OAuthClientNameRequired,
        Self::OAuthClientNameTooLong,
        Self::OAuthClientListValueEmpty,
        Self::OAuthClientValueUnsupported,
        Self::OAuthClientAuthMethodUnsupported,
        Self::OAuthClientRedirectUriInvalid,
        Self::OAuthClientRedirectUriInsecure,
        Self::OAuthClientGrantTypesRequired,
        Self::OAuthClientConfidentialRequired,
        Self::OAuthClientCodeResponseTypeRequired,
        Self::OAuthClientRedirectUriRequired,
        Self::OAuthClientAuthorizationCodeGrantRequired,
        Self::OAuthClientRefreshTokenGrantInvalid,
        Self::OAuthClientOfflineAccessGrantInvalid,
        Self::OAuthClientTypeChangeForbidden,
        Self::OAuthClientNotFound,
        Self::OAuthClientAlreadyExists,
        Self::OAuthClientSecretUnsupported,
        Self::OutboxEventNotFound,
        Self::ConfigurationNotFound,
        Self::ConfigurationInvalid,
        Self::AccessControlRevisionConflict,
        Self::AccessControlPoliciesInvalid,
        Self::AccessControlEvaluationUnavailable,
        Self::AccessControlActionConflict,
        Self::AccessControlActionInvalid,
        Self::AccessControlRuntimeUnavailable,
        Self::SignupTokenInvalid,
        Self::SignupRequestNotFound,
        Self::SignupEmailMismatch,
        Self::SignupUserAlreadyExists,
        Self::SignupRequestAlreadyExists,
        Self::PasswordPolicyViolation,
        Self::PasswordResetTokenInvalid,
        Self::PasswordResetRequestInvalid,
        Self::PasswordUpdateFailed,
        Self::OtpChallengeIdInvalid,
        Self::OtpCodeInvalidOrExpired,
        Self::OtpCodeInvalid,
        Self::OtpAttemptsExceeded,
        Self::OtpRequestRateLimited,
        Self::OtpChallengeChanged,
        Self::OtpUnavailable,
        Self::EmailOtpUnavailable,
        Self::MfaUnavailable,
        Self::MfaDisabled,
        Self::MfaMethodUnavailable,
        Self::PasswordlessMfaRequired,
        Self::SocialAuthorizationCodeMissing,
        Self::SocialStateInvalid,
        Self::SocialTokenExchangeFailed,
        Self::SocialUserInfoFailed,
        Self::SocialStateGenerationFailed,
        Self::GatewayRouteNotFound,
        Self::GatewayPolicyNotFound,
        Self::GatewayMiddlewareNotFound,
        Self::GatewayUnauthorized,
        Self::GatewayAccessDenied,
        Self::GatewayPolicyConfigurationInvalid,
        Self::GatewayRequestPreparationFailed,
        Self::GatewayHttpClientUnavailable,
        Self::GatewayNoHealthyUpstream,
        Self::GatewayOrganizationContextRequired,
        Self::GatewayLimiterUnavailable,
        Self::GatewayLimitConfigurationInvalid,
        Self::GatewayRateLimitExceeded,
        Self::GatewayQuotaExceeded,
        Self::GatewayIngressRateLimitExceeded,
        Self::GatewayIngressUnavailable,
        Self::GatewayOverloaded,
        Self::GatewayReplayMemoryExhausted,
        Self::GatewayTimeout,
        Self::GatewayUploadTimeout,
        Self::GatewayRequestBodyFailed,
        Self::GatewayCancelled,
        Self::GatewayReplayBufferFailed,
        Self::GatewayReplayPayloadTooLarge,
        Self::GatewayWebsocketUpgradeInvalid,
        Self::GatewayDirectResponseStatusInvalid,
        Self::UpstreamConnectionFailed,
        Self::InternalContextUnavailable,
        Self::I18nLocaleNotSupported,
        Self::SystemInternal,
    ];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::RequestMissingContentType => "request.missing_content_type",
            Self::RequestInvalidContentType => "request.invalid_content_type",
            Self::RequestMissingPayload => "request.missing_payload",
            Self::RequestMalformedPayload => "request.malformed_payload",
            Self::RequestInvalid => "request.invalid",
            Self::RequestInvalidQuery => "request.invalid_query",
            Self::RequestInvalidJson => "request.invalid_json",
            Self::RequestPayloadTooLarge => "request.payload_too_large",
            Self::RequestMethodNotAllowed => "request.method_not_allowed",
            Self::AuthTokenMissing => "auth.token_missing",
            Self::AuthTokenInvalid => "auth.token_invalid",
            Self::AuthInvalidCredentials => "auth.invalid_credentials",
            Self::AuthInsufficientPermissions => "auth.insufficient_permissions",
            Self::AuthUserSessionRequired => "auth.user_session_required",
            Self::AuthUntrustedOrigin => "auth.untrusted_origin",
            Self::AuthOpenidScopeRequired => "auth.openid_scope_required",
            Self::AuthLoginRateLimited => "auth.login_rate_limited",
            Self::AuthLoginUnavailable => "auth.login_unavailable",
            Self::SessionOrganizationMembershipRequired => {
                "session.organization_membership_required"
            }
            Self::SessionNotFound => "session.not_found",
            Self::UserNotFound => "user.not_found",
            Self::UserEmailAlreadyExists => "user.email_already_exists",
            Self::UserNicknameAlreadyExists => "user.nickname_already_exists",
            Self::UserSuperAdminDeleteForbidden => "user.super_admin_delete_forbidden",
            Self::OrganizationNotFound => "organization.not_found",
            Self::OrganizationNameRequired => "organization.name_required",
            Self::MembershipRoleRequired => "membership.role_required",
            Self::MembershipRoleTooLong => "membership.role_too_long",
            Self::ApiKeyNotFound => "api_key.not_found",
            Self::ApiKeyOwnerTypeInvalid => "api_key.owner_type_invalid",
            Self::ApiKeyOwnerRequired => "api_key.owner_required",
            Self::ApiKeyOrganizationUnsupported => "api_key.organization_unsupported",
            Self::ApiKeyOwnerNotMember => "api_key.owner_not_organization_member",
            Self::ApiKeyAlreadyRevoked => "api_key.already_revoked",
            Self::AdminKeyNotFound => "admin_key.not_found",
            Self::AdminKeyPermissionsRequired => "admin_key.permissions_required",
            Self::AdminKeyPermissionInvalid => "admin_key.permission_invalid",
            Self::AdminKeyRevoked => "admin_key.revoked",
            Self::AdminKeyAlreadyRevoked => "admin_key.already_revoked",
            Self::ServiceAccountNotFound => "service_account.not_found",
            Self::ServiceAccountNameRequired => "service_account.name_required",
            Self::OAuthClientIdInvalid => "oauth_client.id_invalid",
            Self::OAuthClientNameRequired => "oauth_client.name_required",
            Self::OAuthClientNameTooLong => "oauth_client.name_too_long",
            Self::OAuthClientListValueEmpty => "oauth_client.list_value_empty",
            Self::OAuthClientValueUnsupported => "oauth_client.value_unsupported",
            Self::OAuthClientAuthMethodUnsupported => "oauth_client.auth_method_unsupported",
            Self::OAuthClientRedirectUriInvalid => "oauth_client.redirect_uri_invalid",
            Self::OAuthClientRedirectUriInsecure => "oauth_client.redirect_uri_insecure",
            Self::OAuthClientGrantTypesRequired => "oauth_client.grant_types_required",
            Self::OAuthClientConfidentialRequired => "oauth_client.confidential_required",
            Self::OAuthClientCodeResponseTypeRequired => "oauth_client.code_response_type_required",
            Self::OAuthClientRedirectUriRequired => "oauth_client.redirect_uri_required",
            Self::OAuthClientAuthorizationCodeGrantRequired => {
                "oauth_client.authorization_code_grant_required"
            }
            Self::OAuthClientRefreshTokenGrantInvalid => "oauth_client.refresh_token_grant_invalid",
            Self::OAuthClientOfflineAccessGrantInvalid => {
                "oauth_client.offline_access_grant_invalid"
            }
            Self::OAuthClientTypeChangeForbidden => "oauth_client.type_change_forbidden",
            Self::OAuthClientNotFound => "oauth_client.not_found",
            Self::OAuthClientAlreadyExists => "oauth_client.already_exists",
            Self::OAuthClientSecretUnsupported => "oauth_client.secret_unsupported",
            Self::OutboxEventNotFound => "outbox_event.not_found",
            Self::ConfigurationNotFound => "configuration.not_found",
            Self::ConfigurationInvalid => "configuration.invalid",
            Self::AccessControlRevisionConflict => "access_control.revision_conflict",
            Self::AccessControlPoliciesInvalid => "access_control.policies_invalid",
            Self::AccessControlEvaluationUnavailable => "access_control.evaluation_unavailable",
            Self::AccessControlActionConflict => "access_control.action_conflict",
            Self::AccessControlActionInvalid => "access_control.action_invalid",
            Self::AccessControlRuntimeUnavailable => "access_control.runtime_unavailable",
            Self::SignupTokenInvalid => "signup.token_invalid",
            Self::SignupRequestNotFound => "signup.request_not_found",
            Self::SignupEmailMismatch => "signup.email_mismatch",
            Self::SignupUserAlreadyExists => "signup.user_already_exists",
            Self::SignupRequestAlreadyExists => "signup.request_already_exists",
            Self::PasswordPolicyViolation => "password.policy_violation",
            Self::PasswordResetTokenInvalid => "password.reset_token_invalid",
            Self::PasswordResetRequestInvalid => "password.reset_request_invalid",
            Self::PasswordUpdateFailed => "password.update_failed",
            Self::OtpChallengeIdInvalid => "otp.challenge_id_invalid",
            Self::OtpCodeInvalidOrExpired => "otp.code_invalid_or_expired",
            Self::OtpCodeInvalid => "otp.code_invalid",
            Self::OtpAttemptsExceeded => "otp.attempts_exceeded",
            Self::OtpRequestRateLimited => "otp.request_rate_limited",
            Self::OtpChallengeChanged => "otp.challenge_changed",
            Self::OtpUnavailable => "otp.unavailable",
            Self::EmailOtpUnavailable => "otp.email_unavailable",
            Self::MfaUnavailable => "mfa.unavailable",
            Self::MfaDisabled => "mfa.disabled",
            Self::MfaMethodUnavailable => "mfa.method_unavailable",
            Self::PasswordlessMfaRequired => "passwordless.mfa_required",
            Self::SocialAuthorizationCodeMissing => "social.authorization_code_missing",
            Self::SocialStateInvalid => "social.state_invalid",
            Self::SocialTokenExchangeFailed => "social.token_exchange_failed",
            Self::SocialUserInfoFailed => "social.user_info_failed",
            Self::SocialStateGenerationFailed => "social.state_generation_failed",
            Self::GatewayRouteNotFound => "gateway.route_not_found",
            Self::GatewayPolicyNotFound => "gateway.policy_not_found",
            Self::GatewayMiddlewareNotFound => "gateway.middleware_not_found",
            Self::GatewayUnauthorized => "gateway.unauthorized",
            Self::GatewayAccessDenied => "gateway.access_denied",
            Self::GatewayPolicyConfigurationInvalid => "gateway.policy_configuration_invalid",
            Self::GatewayRequestPreparationFailed => "gateway.request_preparation_failed",
            Self::GatewayHttpClientUnavailable => "gateway.http_client_unavailable",
            Self::GatewayNoHealthyUpstream => "gateway.no_healthy_upstream",
            Self::GatewayOrganizationContextRequired => "gateway.organization_context_required",
            Self::GatewayLimiterUnavailable => "gateway.limiter_unavailable",
            Self::GatewayLimitConfigurationInvalid => "gateway.limit_configuration_invalid",
            Self::GatewayRateLimitExceeded => "gateway.rate_limit_exceeded",
            Self::GatewayQuotaExceeded => "gateway.quota_exceeded",
            Self::GatewayIngressRateLimitExceeded => "gateway.ingress_rate_limit_exceeded",
            Self::GatewayIngressUnavailable => "gateway.ingress_unavailable",
            Self::GatewayOverloaded => "gateway.overloaded",
            Self::GatewayReplayMemoryExhausted => "gateway.replay_memory_exhausted",
            Self::GatewayTimeout => "gateway.timeout",
            Self::GatewayUploadTimeout => "gateway.upload_timeout",
            Self::GatewayRequestBodyFailed => "gateway.request_body_failed",
            Self::GatewayCancelled => "gateway.cancelled",
            Self::GatewayReplayBufferFailed => "gateway.replay_buffer_failed",
            Self::GatewayReplayPayloadTooLarge => "gateway.replay_payload_too_large",
            Self::GatewayWebsocketUpgradeInvalid => "gateway.websocket_upgrade_invalid",
            Self::GatewayDirectResponseStatusInvalid => "gateway.direct_response_status_invalid",
            Self::UpstreamConnectionFailed => "upstream.connection_failed",
            Self::InternalContextUnavailable => "internal_context.unavailable",
            Self::I18nLocaleNotSupported => "i18n.locale_not_supported",
            Self::SystemInternal => "system.internal",
        }
    }

    pub const fn definition(self) -> ErrorDefinition {
        use ErrorType::*;
        use StatusCode as Status;

        match self {
            Self::RequestMissingContentType => definition(
                Status::UNSUPPORTED_MEDIA_TYPE,
                Request,
                "Content-Type header is required",
            ),
            Self::RequestInvalidContentType => definition(
                Status::UNSUPPORTED_MEDIA_TYPE,
                Request,
                "Content-Type is not supported",
            ),
            Self::RequestMissingPayload => {
                definition(Status::BAD_REQUEST, Request, "Request payload is required")
            }
            Self::RequestMalformedPayload => {
                definition(Status::BAD_REQUEST, Request, "Request payload is malformed")
            }
            Self::RequestInvalid => definition(Status::BAD_REQUEST, Request, "Request is invalid"),
            Self::RequestInvalidQuery => {
                definition(Status::BAD_REQUEST, Request, "Query parameters are invalid")
            }
            Self::RequestInvalidJson => {
                definition(Status::BAD_REQUEST, Request, "JSON payload is invalid")
            }
            Self::RequestPayloadTooLarge => definition(
                Status::PAYLOAD_TOO_LARGE,
                Request,
                "Request payload is too large",
            ),
            Self::RequestMethodNotAllowed => definition(
                Status::METHOD_NOT_ALLOWED,
                Request,
                "Request method is not allowed",
            ),
            Self::AuthTokenMissing => definition(
                Status::UNAUTHORIZED,
                Authentication,
                "Authentication token is required",
            ),
            Self::AuthTokenInvalid => definition(
                Status::UNAUTHORIZED,
                Authentication,
                "Authentication token is invalid or expired",
            ),
            Self::AuthInvalidCredentials => definition(
                Status::UNAUTHORIZED,
                Authentication,
                "Invalid username or password",
            ),
            Self::AuthInsufficientPermissions => {
                definition(Status::FORBIDDEN, Authorization, "Insufficient permissions")
            }
            Self::AuthUserSessionRequired => definition(
                Status::FORBIDDEN,
                Authorization,
                "A user session is required",
            ),
            Self::AuthUntrustedOrigin => definition(
                Status::FORBIDDEN,
                Authorization,
                "Request origin is not trusted",
            ),
            Self::AuthOpenidScopeRequired => definition(
                Status::FORBIDDEN,
                Authorization,
                "The openid scope is required",
            ),
            Self::AuthLoginRateLimited => definition(
                Status::TOO_MANY_REQUESTS,
                RateLimit,
                "Too many failed login attempts",
            ),
            Self::AuthLoginUnavailable => definition(
                Status::SERVICE_UNAVAILABLE,
                Availability,
                "Login is temporarily unavailable",
            ),
            Self::SessionOrganizationMembershipRequired => definition(
                Status::BAD_REQUEST,
                Validation,
                "User is not a member of the requested organization",
            ),
            Self::SessionNotFound => definition(Status::NOT_FOUND, NotFound, "Session not found"),
            Self::UserNotFound => definition(Status::NOT_FOUND, NotFound, "User not found"),
            Self::UserEmailAlreadyExists => definition(
                Status::CONFLICT,
                Conflict,
                "A user with this email already exists",
            ),
            Self::UserNicknameAlreadyExists => definition(
                Status::CONFLICT,
                Conflict,
                "A user with this nickname already exists",
            ),
            Self::UserSuperAdminDeleteForbidden => definition(
                Status::FORBIDDEN,
                Authorization,
                "Super-admin users cannot be deleted",
            ),
            Self::OrganizationNotFound => {
                definition(Status::NOT_FOUND, NotFound, "Organization not found")
            }
            Self::OrganizationNameRequired => definition(
                Status::BAD_REQUEST,
                Validation,
                "Organization name is required",
            ),
            Self::MembershipRoleRequired => definition(
                Status::BAD_REQUEST,
                Validation,
                "Membership role is required",
            ),
            Self::MembershipRoleTooLong => definition(
                Status::BAD_REQUEST,
                Validation,
                "Membership role is too long",
            ),
            Self::ApiKeyNotFound => definition(Status::NOT_FOUND, NotFound, "API key not found"),
            Self::ApiKeyOwnerTypeInvalid => definition(
                Status::BAD_REQUEST,
                Validation,
                "API key owner type is invalid",
            ),
            Self::ApiKeyOwnerRequired => {
                definition(Status::BAD_REQUEST, Validation, "API key owner is required")
            }
            Self::ApiKeyOrganizationUnsupported => definition(
                Status::BAD_REQUEST,
                Validation,
                "Organization context is not supported for this API key owner",
            ),
            Self::ApiKeyOwnerNotMember => definition(
                Status::BAD_REQUEST,
                Validation,
                "API key owner is not a member of the organization",
            ),
            Self::ApiKeyAlreadyRevoked => {
                definition(Status::CONFLICT, Conflict, "API key is already revoked")
            }
            Self::AdminKeyNotFound => {
                definition(Status::NOT_FOUND, NotFound, "Admin key not found")
            }
            Self::AdminKeyPermissionsRequired => definition(
                Status::BAD_REQUEST,
                Validation,
                "At least one admin-key permission is required",
            ),
            Self::AdminKeyPermissionInvalid => definition(
                Status::BAD_REQUEST,
                Validation,
                "Admin-key permission is invalid",
            ),
            Self::AdminKeyRevoked => definition(Status::CONFLICT, Conflict, "Admin key is revoked"),
            Self::AdminKeyAlreadyRevoked => {
                definition(Status::CONFLICT, Conflict, "Admin key is already revoked")
            }
            Self::ServiceAccountNotFound => {
                definition(Status::NOT_FOUND, NotFound, "Service account not found")
            }
            Self::ServiceAccountNameRequired => definition(
                Status::BAD_REQUEST,
                Validation,
                "Service account name is required",
            ),
            Self::OAuthClientIdInvalid => definition(
                Status::BAD_REQUEST,
                Validation,
                "OAuth client ID is invalid",
            ),
            Self::OAuthClientNameRequired => definition(
                Status::BAD_REQUEST,
                Validation,
                "OAuth client name is required",
            ),
            Self::OAuthClientNameTooLong => definition(
                Status::BAD_REQUEST,
                Validation,
                "OAuth client name is too long",
            ),
            Self::OAuthClientListValueEmpty => definition(
                Status::BAD_REQUEST,
                Validation,
                "OAuth client list contains an empty value",
            ),
            Self::OAuthClientValueUnsupported => definition(
                Status::BAD_REQUEST,
                Validation,
                "OAuth client value is not supported",
            ),
            Self::OAuthClientAuthMethodUnsupported => definition(
                Status::BAD_REQUEST,
                Validation,
                "OAuth client authentication method is not supported",
            ),
            Self::OAuthClientRedirectUriInvalid => definition(
                Status::BAD_REQUEST,
                Validation,
                "OAuth client redirect URI is invalid",
            ),
            Self::OAuthClientRedirectUriInsecure => definition(
                Status::BAD_REQUEST,
                Validation,
                "OAuth client redirect URI must use HTTPS",
            ),
            Self::OAuthClientGrantTypesRequired => definition(
                Status::BAD_REQUEST,
                Validation,
                "At least one OAuth grant type is required",
            ),
            Self::OAuthClientConfidentialRequired => definition(
                Status::BAD_REQUEST,
                Validation,
                "This OAuth grant requires a confidential client",
            ),
            Self::OAuthClientCodeResponseTypeRequired => definition(
                Status::BAD_REQUEST,
                Validation,
                "Authorization Code clients must support the code response type",
            ),
            Self::OAuthClientRedirectUriRequired => definition(
                Status::BAD_REQUEST,
                Validation,
                "Authorization Code clients require a redirect URI",
            ),
            Self::OAuthClientAuthorizationCodeGrantRequired => definition(
                Status::BAD_REQUEST,
                Validation,
                "The code response type requires the Authorization Code grant",
            ),
            Self::OAuthClientRefreshTokenGrantInvalid => definition(
                Status::BAD_REQUEST,
                Validation,
                "The Refresh Token grant requires the Authorization Code grant",
            ),
            Self::OAuthClientOfflineAccessGrantInvalid => definition(
                Status::BAD_REQUEST,
                Validation,
                "The offline_access scope requires the Refresh Token grant",
            ),
            Self::OAuthClientTypeChangeForbidden => definition(
                Status::CONFLICT,
                Conflict,
                "OAuth client type cannot be changed in place",
            ),
            Self::OAuthClientNotFound => {
                definition(Status::NOT_FOUND, NotFound, "OAuth client not found")
            }
            Self::OAuthClientAlreadyExists => definition(
                Status::CONFLICT,
                Conflict,
                "An OAuth client with this ID already exists",
            ),
            Self::OAuthClientSecretUnsupported => definition(
                Status::BAD_REQUEST,
                Validation,
                "Public OAuth clients do not use client secrets",
            ),
            Self::OutboxEventNotFound => {
                definition(Status::NOT_FOUND, NotFound, "Outbox event not found")
            }
            Self::ConfigurationNotFound => {
                definition(Status::NOT_FOUND, NotFound, "Configuration file not found")
            }
            Self::ConfigurationInvalid => definition(
                Status::BAD_REQUEST,
                Validation,
                "Gateway configuration is invalid",
            ),
            Self::AccessControlRevisionConflict => definition(
                Status::CONFLICT,
                Conflict,
                "Access-control policies changed since they were loaded",
            ),
            Self::AccessControlPoliciesInvalid => definition(
                Status::BAD_REQUEST,
                Validation,
                "Access-control policies are invalid",
            ),
            Self::AccessControlEvaluationUnavailable => definition(
                Status::BAD_REQUEST,
                Validation,
                "Invalid access-control policies cannot be evaluated",
            ),
            Self::AccessControlActionConflict => definition(
                Status::BAD_REQUEST,
                Validation,
                "Resource action conflicts with the explicit action",
            ),
            Self::AccessControlActionInvalid => definition(
                Status::BAD_REQUEST,
                Validation,
                "Access-control resource action is invalid",
            ),
            Self::AccessControlRuntimeUnavailable => definition(
                Status::INTERNAL_SERVER_ERROR,
                Internal,
                "Access-control runtime is unavailable",
            ),
            Self::SignupTokenInvalid => definition(
                Status::BAD_REQUEST,
                Validation,
                "Signup token is invalid or expired",
            ),
            Self::SignupRequestNotFound => {
                definition(Status::NOT_FOUND, NotFound, "Signup request not found")
            }
            Self::SignupEmailMismatch => definition(
                Status::UNAUTHORIZED,
                Authentication,
                "Signup email does not match the token",
            ),
            Self::SignupUserAlreadyExists => definition(
                Status::CONFLICT,
                Conflict,
                "A user already exists for this signup request",
            ),
            Self::SignupRequestAlreadyExists => definition(
                Status::CONFLICT,
                Conflict,
                "A signup request already exists for this email",
            ),
            Self::PasswordPolicyViolation => definition(
                Status::BAD_REQUEST,
                Validation,
                "Password does not satisfy the password policy",
            ),
            Self::PasswordResetTokenInvalid => definition(
                Status::UNAUTHORIZED,
                Authentication,
                "Password-reset token is invalid or expired",
            ),
            Self::PasswordResetRequestInvalid => definition(
                Status::UNAUTHORIZED,
                Authentication,
                "Password-reset request is invalid or expired",
            ),
            Self::PasswordUpdateFailed => definition(
                Status::INTERNAL_SERVER_ERROR,
                Internal,
                "Password could not be updated",
            ),
            Self::OtpChallengeIdInvalid => definition(
                Status::BAD_REQUEST,
                Validation,
                "OTP challenge ID is invalid",
            ),
            Self::OtpCodeInvalidOrExpired => definition(
                Status::UNAUTHORIZED,
                Authentication,
                "Verification code is invalid or expired",
            ),
            Self::OtpCodeInvalid => definition(
                Status::UNAUTHORIZED,
                Authentication,
                "Verification code is invalid",
            ),
            Self::OtpAttemptsExceeded => definition(
                Status::TOO_MANY_REQUESTS,
                RateLimit,
                "Too many invalid verification attempts",
            ),
            Self::OtpRequestRateLimited => definition(
                Status::TOO_MANY_REQUESTS,
                RateLimit,
                "Too many verification-code requests",
            ),
            Self::OtpChallengeChanged => definition(
                Status::CONFLICT,
                Conflict,
                "OTP challenge changed; retry verification",
            ),
            Self::OtpUnavailable => definition(
                Status::SERVICE_UNAVAILABLE,
                Availability,
                "OTP is temporarily unavailable",
            ),
            Self::EmailOtpUnavailable => definition(
                Status::SERVICE_UNAVAILABLE,
                Availability,
                "Email OTP is temporarily unavailable",
            ),
            Self::MfaUnavailable => definition(
                Status::SERVICE_UNAVAILABLE,
                Availability,
                "MFA is temporarily unavailable",
            ),
            Self::MfaDisabled => definition(Status::FORBIDDEN, Authorization, "MFA is disabled"),
            Self::MfaMethodUnavailable => definition(
                Status::BAD_REQUEST,
                Validation,
                "Requested MFA method is unavailable",
            ),
            Self::PasswordlessMfaRequired => definition(
                Status::FORBIDDEN,
                Authorization,
                "Passwordless login is unavailable when MFA is required",
            ),
            Self::SocialAuthorizationCodeMissing => definition(
                Status::BAD_REQUEST,
                Request,
                "OAuth authorization code is required",
            ),
            Self::SocialStateInvalid => definition(
                Status::BAD_REQUEST,
                Authentication,
                "OAuth state is invalid or expired",
            ),
            Self::SocialTokenExchangeFailed => definition(
                Status::BAD_GATEWAY,
                Upstream,
                "Identity-provider token exchange failed",
            ),
            Self::SocialUserInfoFailed => definition(
                Status::BAD_GATEWAY,
                Upstream,
                "Identity-provider user information could not be retrieved",
            ),
            Self::SocialStateGenerationFailed => definition(
                Status::INTERNAL_SERVER_ERROR,
                Internal,
                "OAuth state could not be generated",
            ),
            Self::GatewayRouteNotFound => {
                definition(Status::NOT_FOUND, NotFound, "Gateway route not found")
            }
            Self::GatewayPolicyNotFound => definition(
                Status::INTERNAL_SERVER_ERROR,
                Internal,
                "Configured gateway policy was not found",
            ),
            Self::GatewayMiddlewareNotFound => definition(
                Status::INTERNAL_SERVER_ERROR,
                Internal,
                "Configured gateway middleware was not found",
            ),
            Self::GatewayUnauthorized => definition(
                Status::UNAUTHORIZED,
                Authentication,
                "Gateway authentication is required",
            ),
            Self::GatewayAccessDenied => definition(
                Status::FORBIDDEN,
                Authorization,
                "Gateway access was denied",
            ),
            Self::GatewayPolicyConfigurationInvalid => definition(
                Status::INTERNAL_SERVER_ERROR,
                Internal,
                "Gateway policy configuration is invalid",
            ),
            Self::GatewayRequestPreparationFailed => definition(
                Status::INTERNAL_SERVER_ERROR,
                Internal,
                "Upstream request preparation failed",
            ),
            Self::GatewayHttpClientUnavailable => definition(
                Status::INTERNAL_SERVER_ERROR,
                Internal,
                "Gateway HTTP client is unavailable",
            ),
            Self::GatewayNoHealthyUpstream => definition(
                Status::SERVICE_UNAVAILABLE,
                Availability,
                "No healthy upstream is available",
            ),
            Self::GatewayOrganizationContextRequired => definition(
                Status::FORBIDDEN,
                Authorization,
                "Organization context is required",
            ),
            Self::GatewayLimitConfigurationInvalid => definition(
                Status::INTERNAL_SERVER_ERROR,
                Internal,
                "Selected gateway limit configuration is invalid",
            ),
            Self::GatewayLimiterUnavailable => definition(
                Status::INTERNAL_SERVER_ERROR,
                Internal,
                "Gateway limiter is unavailable",
            ),
            Self::GatewayRateLimitExceeded => definition(
                Status::TOO_MANY_REQUESTS,
                RateLimit,
                "Gateway rate limit exceeded",
            ),
            Self::GatewayQuotaExceeded => definition(
                Status::TOO_MANY_REQUESTS,
                RateLimit,
                "Gateway quota exceeded",
            ),
            Self::GatewayIngressRateLimitExceeded => definition(
                Status::TOO_MANY_REQUESTS,
                RateLimit,
                "Gateway ingress rate limit exceeded",
            ),
            Self::GatewayIngressUnavailable => definition(
                Status::SERVICE_UNAVAILABLE,
                Availability,
                "Gateway ingress admission is unavailable",
            ),
            Self::GatewayOverloaded => definition(
                Status::SERVICE_UNAVAILABLE,
                Availability,
                "Gateway concurrency budget is exhausted",
            ),
            Self::GatewayReplayMemoryExhausted => definition(
                Status::SERVICE_UNAVAILABLE,
                Availability,
                "Gateway replay memory budget is exhausted",
            ),
            Self::GatewayTimeout => definition(
                Status::GATEWAY_TIMEOUT,
                Upstream,
                "Gateway deadline exceeded",
            ),
            Self::GatewayUploadTimeout => definition(
                Status::REQUEST_TIMEOUT,
                Request,
                "Request body upload timed out",
            ),
            Self::GatewayRequestBodyFailed => definition(
                Status::BAD_REQUEST,
                Request,
                "Request body could not be read",
            ),
            Self::GatewayCancelled => definition(
                Status::SERVICE_UNAVAILABLE,
                Availability,
                "Gateway request was cancelled",
            ),
            Self::GatewayReplayBufferFailed => definition(
                Status::BAD_REQUEST,
                Request,
                "Request body could not be buffered for replay",
            ),
            Self::GatewayReplayPayloadTooLarge => definition(
                Status::PAYLOAD_TOO_LARGE,
                Request,
                "Request body exceeds the gateway replay limit",
            ),
            Self::GatewayWebsocketUpgradeInvalid => definition(
                Status::BAD_REQUEST,
                Request,
                "WebSocket upgrade request is invalid",
            ),
            Self::GatewayDirectResponseStatusInvalid => definition(
                Status::INTERNAL_SERVER_ERROR,
                Internal,
                "Configured direct-response status is invalid",
            ),
            Self::UpstreamConnectionFailed => definition(
                Status::BAD_GATEWAY,
                Upstream,
                "Failed to connect to the upstream service",
            ),
            Self::InternalContextUnavailable => definition(
                Status::SERVICE_UNAVAILABLE,
                Availability,
                "Internal-context signing is unavailable",
            ),
            Self::I18nLocaleNotSupported => {
                definition(Status::NOT_FOUND, NotFound, "Locale is not supported")
            }
            Self::SystemInternal => definition(
                Status::INTERNAL_SERVER_ERROR,
                Internal,
                "Internal server error",
            ),
        }
    }

    pub const fn status(self) -> StatusCode {
        self.definition().status
    }

    pub const fn error_type(self) -> ErrorType {
        self.definition().error_type
    }

    pub const fn message(self) -> &'static str {
        self.definition().message
    }

    pub fn url(self) -> String {
        let base =
            std::env::var("DOCS_URL").unwrap_or_else(|_| "https://docs.stargate.dev".to_string());
        format!("{}/errors/{}", base.trim_end_matches('/'), self.as_str())
    }
}

impl fmt::Display for ErrorCode {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

impl Serialize for ErrorCode {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(self.as_str())
    }
}
