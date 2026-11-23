//! Canonical names for the state keys Stargate writes.
//!
//! Every key starts with the versioned schema root and carries a hash tag for
//! its entity:
//!
//! ```text
//! [REDIS_KEY_PREFIX]stargate:v1:{<entity>}:<name>[:<qualifier>...]
//! ```
//!
//! * The version is part of the prefix, so a schema change starts from empty
//!   sessions and counters instead of reading incompatible state.
//! * Keys used together in one command, script or transaction share the same
//!   `{<entity>}` tag and therefore the same Redis Cluster slot. Keys of one
//!   entity may only be combined when the names below put them in one tag.
//! * The Redis store prepends `REDIS_KEY_PREFIX` (operator setting, empty by
//!   default) before the root; that prefix is outside every tag and does not
//!   move a key to another slot.
//!
//! The values that form a tag are generated identifiers (ULIDs, digests) or an
//! IP address. Free-form input (an email, an identifier typed at login, an OTP
//! subject) is hashed first: the caller canonicalizes it and the builders hash
//! it with SHA-256. A tag never contains `{` or `}`.
//!
//! Audit streams are the one exception: they are external contracts named by
//! `AUDIT_REDIS_STREAM` and deliberately not namespaced.

use sha2::{Digest, Sha256};

/// Root of every state key, including its schema version.
pub const ROOT: &str = "stargate:v1";

/// A key under an entity tag. Callers own tag uniqueness and safety.
pub fn scoped(tag: &str, name: &str) -> String {
    debug_assert!(!tag.is_empty(), "state key tag cannot be empty");
    debug_assert!(
        !tag.contains('{') && !tag.contains('}'),
        "state key tag cannot contain braces: {tag}"
    );
    debug_assert!(!name.is_empty(), "state key name cannot be empty");
    format!("{ROOT}:{{{tag}}}:{name}")
}

fn digest(value: &str) -> String {
    let hash = Sha256::digest(value.as_bytes());
    hash.iter().map(|byte| format!("{byte:02x}")).collect()
}

/// Login sessions and their registry.
pub mod session {
    use super::scoped;

    /// The signed-in subject, keyed by the stable session id.
    pub fn subject(sid: &str) -> String {
        scoped(&format!("s:{sid}"), "subject")
    }

    /// The current refresh-token `jti` of the session (rotation state).
    pub fn refresh(sid: &str) -> String {
        scoped(&format!("s:{sid}"), "refresh")
    }

    /// The user's session index: sid to org and expiry.
    pub fn index(user_id: &str) -> String {
        scoped(&format!("u:{user_id}"), "sessions")
    }

    /// The global session directory: sid to session record.
    pub fn directory() -> String {
        scoped("g", "sessions")
    }

    /// A revoked JWT `jti`, until its expiry.
    pub fn revoked_jti(jti: &str) -> String {
        scoped(&format!("j:{jti}"), "revoked")
    }
}

/// Verified API key subjects, cached until their attributes change.
pub mod api_key {
    use super::scoped;

    pub fn subject(key_hash: &str) -> String {
        scoped(&format!("k:{key_hash}"), "subject")
    }
}

/// OAuth and OpenID Connect provider state.
pub mod oauth {
    use super::scoped;

    /// A pending or redeemed authorization code, keyed by its hash.
    pub fn authorization_code(code_hash: &str) -> String {
        scoped(&format!("c:{code_hash}"), "code")
    }

    /// A rotating refresh-token family.
    pub fn refresh_family(family_id: &str) -> String {
        scoped(&format!("f:{family_id}"), "refresh")
    }

    /// A parked authorization request (login, consent or step-up).
    pub fn authorize_interaction(id: &str) -> String {
        scoped(&format!("i:{id}"), "authorize")
    }

    /// A parked RP-initiated logout request.
    pub fn logout_request(id: &str) -> String {
        scoped(&format!("i:{id}"), "logout")
    }

    /// A social sign-in `state`.
    pub fn state(id: &str) -> String {
        scoped(&format!("o:{id}"), "state")
    }

    /// A client assertion `jti` already accepted, until its expiry.
    pub fn client_assertion(client_id: &str, jti_hash: &str) -> String {
        scoped(&format!("a:{client_id}"), &format!("assertion:{jti_hash}"))
    }
}

/// Email OTP challenges and their request throttles.
pub mod otp {
    use super::{digest, scoped};

    /// A passwordless email login challenge.
    pub fn passwordless(challenge_id: &str) -> String {
        scoped(&format!("otp:{challenge_id}"), "passwordless")
    }

    /// A login waiting for its MFA challenge.
    pub fn pending_login(challenge_id: &str) -> String {
        scoped(&format!("otp:{challenge_id}"), "login")
    }

    /// An authenticated MFA challenge, keyed by the user it belongs to.
    pub fn challenge(user_id: &str, challenge_id: &str) -> String {
        scoped(&format!("u:{user_id}"), &format!("mfa:{challenge_id}"))
    }

    /// A step-up marker verified in a session for a purpose.
    pub fn verified(sid: &str, purpose: &str) -> String {
        scoped(&format!("s:{sid}"), &format!("mfa:{purpose}"))
    }

    /// OTP requests from one client address.
    pub fn request_ip(client_ip: &str) -> String {
        scoped(&format!("ip:{client_ip}"), "otp-request")
    }

    /// OTP requests for one subject (user, email, identifier) from an address.
    pub fn request_subject(subject: &str, client_ip: &str) -> String {
        scoped(
            &format!("otp:{}", digest(subject)),
            &format!("requests:{client_ip}"),
        )
    }
}

/// Sign-up, password reset, login guard and email throttles.
pub mod account {
    use super::{digest, scoped};

    /// A pending password reset for an address, keyed by its digest.
    pub fn password_reset(email: &str) -> String {
        scoped(&format!("em:{}", digest(email)), "password-reset")
    }

    /// A pending email verification request, keyed by its id.
    pub fn email_verification(sid: &str) -> String {
        scoped(&format!("v:{sid}"), "verification")
    }

    /// The address-to-request mapping of a pending email verification.
    pub fn email_verification_lookup(email: &str) -> String {
        scoped(&format!("em:{}", digest(email)), "verification-lookup")
    }

    /// The copy of a sign-up request created once the address is verified.
    pub fn signup(sid: &str) -> String {
        scoped(&format!("sg:{sid}"), "signup")
    }

    /// Requests one address may make through an email-sending flow.
    pub fn email_throttle(flow: &str, email: &str) -> String {
        scoped(&format!("em:{}", digest(email)), &format!("email:{flow}"))
    }

    /// Password attempts from one client address.
    pub fn login_ip(client_ip: &str) -> String {
        scoped(&format!("ip:{client_ip}"), "login-attempts")
    }

    /// Password attempts against one account, across all addresses or from one address.
    pub fn login_user(user_id: &str, client_ip: Option<&str>) -> String {
        login_attempts(&format!("u:{user_id}"), client_ip)
    }

    /// Recognized source addresses of completed password logins for this
    /// account, as an address → deadline hash.
    pub fn login_known(user_id: &str) -> String {
        scoped(&format!("u:{user_id}"), "login-known")
    }

    /// Password attempts for an unknown identifier, across all addresses or from one address.
    pub fn login_identifier(identifier: &str, client_ip: Option<&str>) -> String {
        login_attempts(&format!("id:{}", digest(identifier)), client_ip)
    }

    fn login_attempts(tag: &str, client_ip: Option<&str>) -> String {
        match client_ip {
            Some(ip) => scoped(tag, &format!("login-attempts:{ip}")),
            None => scoped(tag, "login-attempts"),
        }
    }
}

/// Gateway traffic limits and organization limit overrides.
pub mod limits {
    use super::scoped;

    /// The per-address ingress allowance shared by gateway and IAM.
    pub fn ingress(client_ip: &str) -> String {
        scoped(&format!("ip:{client_ip}"), "ingress")
    }

    /// A subject's rate limit bucket.
    pub fn subject_rate(subject: &str) -> String {
        scoped(&format!("sub:{subject}"), "rate")
    }

    /// A subject's quota bucket.
    pub fn subject_quota(subject: &str) -> String {
        scoped(&format!("sub:{subject}"), "quota")
    }

    /// An organization's rate limit bucket.
    pub fn org_rate(org_id: &str) -> String {
        scoped(&format!("org:{org_id}"), "rate")
    }

    /// An organization's quota bucket.
    pub fn org_quota(org_id: &str) -> String {
        scoped(&format!("org:{org_id}"), "quota")
    }

    /// An organization rate limit charged to the client address instead.
    pub fn org_ip_rate(client_ip: &str) -> String {
        scoped(&format!("ip:{client_ip}"), "org-rate")
    }

    /// An organization quota charged to the client address instead.
    pub fn org_ip_quota(client_ip: &str) -> String {
        scoped(&format!("ip:{client_ip}"), "org-quota")
    }

    /// An admin principal's own bucket; `principal` is `user:<id>` or
    /// `key:<id>`.
    pub fn admin_principal(principal: &str) -> String {
        scoped(&format!("admin:{principal}"), "requests")
    }

    /// The cached named-limit overrides of an organization.
    pub fn org_overrides(org_id: &str) -> String {
        scoped(&format!("org:{org_id}"), "limit-overrides")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tag(key: &str) -> &str {
        let root = format!("{ROOT}:");
        let rest = key.strip_prefix(&root).expect("schema root");
        let rest = rest.strip_prefix('{').expect("opening brace");
        let (tag, _) = rest.split_once('}').expect("closing brace");
        tag
    }

    #[test]
    fn every_key_has_one_tag_and_the_versioned_root() {
        let sid = "01JZ00000000000000000000AA";
        let keys = [
            session::subject(sid),
            session::refresh(sid),
            session::index("01JZ00000000000000000000U1"),
            session::directory(),
            session::revoked_jti("jti"),
            api_key::subject("hash"),
            oauth::authorization_code("hash"),
            oauth::refresh_family("family"),
            oauth::authorize_interaction("interaction"),
            oauth::logout_request("interaction"),
            oauth::state("state"),
            oauth::client_assertion("client", "jti-hash"),
            otp::passwordless("challenge"),
            otp::pending_login("challenge"),
            otp::challenge("01JZ00000000000000000000U1", "challenge"),
            otp::verified(sid, "admin"),
            otp::request_ip("203.0.113.1"),
            otp::request_subject("alice@example.com", "203.0.113.1"),
            account::password_reset("alice@example.com"),
            account::email_verification(sid),
            account::email_verification_lookup("alice@example.com"),
            account::signup(sid),
            account::email_throttle("signup", "alice@example.com"),
            account::login_ip("203.0.113.1"),
            account::login_user("01JZ00000000000000000000U1", Some("203.0.113.1")),
            account::login_user("01JZ00000000000000000000U1", None),
            account::login_known("01JZ00000000000000000000U1"),
            account::login_identifier("alice@example.com", Some("203.0.113.1")),
            account::login_identifier("alice@example.com", None),
            limits::ingress("203.0.113.1"),
            limits::subject_rate("01JZ00000000000000000000U1"),
            limits::subject_quota("01JZ00000000000000000000U1"),
            limits::org_rate("01JZ00000000000000000000O1"),
            limits::org_quota("01JZ00000000000000000000O1"),
            limits::org_ip_rate("203.0.113.1"),
            limits::org_ip_quota("203.0.113.1"),
            limits::admin_principal("user:01JZ00000000000000000000U1"),
            limits::org_overrides("01JZ00000000000000000000O1"),
        ];
        for key in keys {
            assert!(key.starts_with(&format!("{ROOT}:{{")), "{key}");
            let rest = key.strip_prefix(&format!("{ROOT}:{{")).unwrap();
            assert_eq!(rest.matches('}').count(), 1, "one tag in {key}");
            assert!(!tag(&key).is_empty(), "{key}");
            assert!(!tag(&key).contains(['{', '}']), "{key}");
        }
    }

    #[test]
    fn keys_of_one_entity_share_their_tag() {
        let sid = "01JZ00000000000000000000AA";
        let user = "01JZ00000000000000000000U1";
        assert_eq!(tag(&session::subject(sid)), tag(&session::refresh(sid)));
        assert_eq!(
            tag(&otp::verified(sid, "admin")),
            tag(&otp::verified(sid, "login"))
        );
        assert_eq!(
            tag(&account::login_user(user, Some("203.0.113.1"))),
            tag(&account::login_user(user, None))
        );
        assert_eq!(
            tag(&limits::subject_rate(user)),
            tag(&limits::subject_quota(user))
        );
        assert_eq!(
            tag(&limits::org_rate("01JZ00000000000000000000O1")),
            tag(&limits::org_overrides("01JZ00000000000000000000O1"))
        );
        let email = "Alice@Example.com";
        assert_eq!(
            tag(&account::password_reset(email)),
            tag(&account::email_verification_lookup(email))
        );
    }

    #[test]
    fn free_form_components_are_hashed_out_of_the_tag() {
        let email = "victim{tag}@example.com";
        let key = account::password_reset(email);
        assert!(!key.contains("victim"));
        assert!(!tag(&key).contains('{'));
        assert_ne!(key, account::password_reset("other@example.com"));
    }

    #[test]
    fn exact_shapes_are_stable() {
        let sid = "01JZ00000000000000000000AA";
        assert_eq!(
            session::subject(sid),
            "stargate:v1:{s:01JZ00000000000000000000AA}:subject"
        );
        assert_eq!(api_key::subject("abc"), "stargate:v1:{k:abc}:subject");
        assert_eq!(oauth::authorization_code("abc"), "stargate:v1:{c:abc}:code");
        assert_eq!(
            limits::ingress("203.0.113.8"),
            "stargate:v1:{ip:203.0.113.8}:ingress"
        );
        assert_eq!(
            limits::admin_principal("key:01JZ00000000000000000000K1"),
            "stargate:v1:{admin:key:01JZ00000000000000000000K1}:requests"
        );
    }
}
