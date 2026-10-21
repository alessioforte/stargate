use crate::err::{ErrorCode, ErrorResponse};
use db::ent::{canonical_nickname, nickname_is_valid};

/// Checks a nickname before it is written, so callers answer `400`/`409`
/// rather than surfacing the repository's own invariant check as a storage
/// error. `owner_id` is the account being updated, which keeps its handle.
pub(crate) async fn ensure_nickname_available(
    nickname: &str,
    email: &str,
    owner_id: Option<&str>,
) -> Result<(), ErrorResponse> {
    if !nickname_is_valid(nickname, email) {
        return Err(ErrorResponse::new(ErrorCode::UserNicknameInvalid));
    }
    let taken = crate::db::get_user_by_nickname(nickname)
        .await
        .map_err(ErrorResponse::internal)?
        .is_some_and(|user| Some(user.id.as_str()) != owner_id);
    if taken {
        return Err(ErrorResponse::new(ErrorCode::UserNicknameAlreadyExists)
            .with_param("nickname", canonical_nickname(nickname)));
    }
    Ok(())
}
