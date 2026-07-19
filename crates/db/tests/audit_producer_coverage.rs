const SERVICE_SOURCE: &str = include_str!("../src/service.rs");
const REPOSITORY_MODULE_SOURCE: &str = include_str!("../src/repo/mod.rs");

const ADMIN_SOURCES: &[(&str, &str)] = &[
    ("users", include_str!("../../../src/api/admin/users.rs")),
    (
        "organizations",
        include_str!("../../../src/api/admin/organizations.rs"),
    ),
    (
        "oauth_clients",
        include_str!("../../../src/api/admin/oauth_clients.rs"),
    ),
    (
        "api_keys",
        include_str!("../../../src/api/admin/api_keys.rs"),
    ),
    (
        "admin_keys",
        include_str!("../../../src/api/admin/admin_keys.rs"),
    ),
    (
        "service_accounts",
        include_str!("../../../src/api/admin/service_accounts.rs"),
    ),
];

const ADMIN_MUTATIONS: &[&str] = &[
    "create_user(",
    "update_user(",
    "delete_user(",
    "add_user_to_organization(",
    "remove_user_from_organization(",
    "create_organization(",
    "update_organization(",
    "delete_organization(",
    "create_oauth_client(",
    "update_oauth_client(",
    "set_oauth_client_enabled(",
    "update_oauth_client_secret_hash(",
    "delete_oauth_client(",
    "create_user_api_key(",
    "create_service_account_api_key(",
    "update_api_key(",
    "revoke_api_key(",
    "delete_api_key(",
    "create_admin_key(",
    "update_admin_key(",
    "revoke_admin_key(",
    "delete_admin_key(",
    "create_service_account(",
    "update_service_account(",
    "delete_service_account(",
];

fn contains_identifier(source: &str, identifier: &str) -> bool {
    source
        .split(|character: char| !(character.is_ascii_alphanumeric() || character == '_'))
        .any(|token| token == identifier)
}

#[test]
fn production_service_has_no_legacy_audit_producer() {
    for forbidden in ["build_audit", "record_audit", "AuditContext"] {
        assert!(
            !contains_identifier(SERVICE_SOURCE, forbidden),
            "production service reintroduced legacy producer identifier {forbidden}"
        );
    }
    assert!(!SERVICE_SOURCE.contains("self.audit.insert"));
    assert!(!REPOSITORY_MODULE_SOURCE.contains("mod audit"));
    assert!(!REPOSITORY_MODULE_SOURCE.contains("use audit"));
}

#[test]
fn every_inventory_admin_handler_consumes_the_trusted_admin_boundary() {
    for (file, source) in ADMIN_SOURCES {
        assert!(
            !source.contains("take_audit_context_from"),
            "{file} reintroduced the fallback legacy context"
        );

        for handler in source.split("pub async fn ").skip(1) {
            let mutations: Vec<_> = ADMIN_MUTATIONS
                .iter()
                .filter(|mutation| handler.contains(**mutation))
                .collect();
            if mutations.is_empty() {
                continue;
            }

            let handler_name = handler.split('(').next().unwrap_or("unknown");
            assert!(
                handler.contains("take_admin_audit_context(req.extensions_mut())?"),
                "admin handler {file}::{handler_name} performs {mutations:?} without consuming the trusted admin context"
            );
        }
    }
}

#[test]
fn non_admin_entrypoints_keep_the_frozen_actor_and_scope_construction() {
    let cases = [
        (
            "signup",
            include_str!("../../../src/api/signup/complete.rs"),
            "TrustedAuditActor::anonymous()",
        ),
        (
            "github",
            include_str!("../../../src/api/social/github.rs"),
            "TrustedAuditActor::external_identity(format!(\"github:",
        ),
        (
            "google",
            include_str!("../../../src/api/social/google.rs"),
            "TrustedAuditActor::external_identity(format!(\"google:",
        ),
        (
            "password_reset",
            include_str!("../../../src/api/account/credentials/reset.rs"),
            "TrustedAuditActor::user(&user.id)",
        ),
        (
            "credential_rehash",
            include_str!("../../../src/api/account/login.rs"),
            "TrustedBackgroundActor::system(None)",
        ),
        (
            "cli_bootstrap",
            include_str!("../../../src/fun/super_admin.rs"),
            "TrustedAuditBoundary::control_plane()",
        ),
        (
            "oauth_revoke",
            include_str!("../../../src/api/oidc/token_ops.rs"),
            "oauth_revoke_audit_context(",
        ),
    ];

    for (entrypoint, source, required) in cases {
        assert!(
            source.contains(required),
            "{entrypoint} no longer contains its frozen trusted actor/scope construction"
        );
    }
}
