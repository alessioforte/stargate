use super::registry;
use crate::err::ErrorResponse;
use crate::iam::auth::{jwt::jwt_config, subject::Subject};
use crate::iam::user::profile::format_name;
use store::Store;

/// A session just started: its tokens and the organization it acts in.
pub struct IssuedSession {
    pub access_token: String,
    pub refresh_token: String,
    /// The organization this session acts in, when the user has one.
    pub org_id: Option<String>,
    /// How long the access token lives, in seconds.
    pub access_ttl_secs: i64,
}

/// Sign `user` in: a new session authenticated at `auth_time` with the
/// methods `amr` (OIDC `amr` values, e.g. `pwd`).
pub async fn start_user_session(
    user: db::ent::User,
    auth_time: usize,
    requested_org_id: Option<&str>,
    amr: Vec<String>,
) -> Result<IssuedSession, ErrorResponse> {
    let org = registry::resolve_org_context(&user, requested_org_id).await?;

    let mut subject = Subject::from(user.clone());
    subject.org_id = org.as_ref().map(|org| org.org_id.clone());
    subject.org_role = org.as_ref().map(|org| org.role.clone());

    let given_name = user.given_name.clone().unwrap_or_default();
    let family_name = user.family_name.clone().unwrap_or_default();
    let name = format_name(&given_name, &family_name);

    let sid = ulid::Ulid::generate().to_string();

    let mut claims = jwt::Claims::default()
        .subject(user.email.to_owned())
        .sub_id(user.id.to_owned())
        .name(name)
        .email(user.email.to_owned())
        .email_verified(true)
        .sid(sid.clone());

    claims.auth_time = Some(auth_time);
    claims.org_id = org.as_ref().map(|org| org.org_id.clone());

    let is_super_admin = crate::iam::authz::admin::is_super_admin_user_id(&user.id)
        .await
        .map_err(ErrorResponse::internal)?;

    if is_super_admin {
        claims = claims.role(crate::iam::authz::admin::SUPER_ADMIN_ROLE.to_string());
    }

    let tokens = super::tokens::generate_tokens(claims).map_err(ErrorResponse::internal)?;

    let store = crate::data::store::use_store();
    let refresh_exp = jwt_config().refresh_exp;
    let access_exp = jwt_config().access_exp;
    let sttl: u64 = refresh_exp.as_seconds_f64() as u64;
    let cttl: i64 = access_exp.as_seconds_f64() as i64;

    store
        .set(&sid, &subject, Some(sttl))
        .await
        .map_err(ErrorResponse::internal)?;
    if let Err(error) = registry::register_session(
        registry::NewSession {
            user_id: &user.id,
            sid: &sid,
            org_id: subject.org_id.as_deref(),
            authenticated_at_unix: auth_time as u64,
            amr,
        },
        &tokens.refresh_jti,
        sttl,
    )
    .await
    {
        let _ = store.delete(&sid).await;
        return Err(ErrorResponse::internal(error));
    }

    Ok(IssuedSession {
        access_token: tokens.access_token,
        refresh_token: tokens.refresh_token,
        org_id: subject.org_id,
        access_ttl_secs: cttl,
    })
}
