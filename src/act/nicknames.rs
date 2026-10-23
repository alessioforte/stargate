use crate::err::{ErrorCode, ErrorResponse};
use db::ent::{canonical_nickname, nickname_is_valid};

/// Longest nickname the schema stores (`VARCHAR(50)`).
const MAX_NICKNAME_CHARS: usize = 50;
/// Numbered handles (`name_2` ... `name_9`) tried before random ones.
const LAST_NUMBERED_SUFFIX: u32 = 9;
/// Random handles tried after the numbered ones.
const RANDOM_SUFFIXES: usize = 3;

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

/// A free nickname for a new account whose handle comes from elsewhere (a
/// GitHub username): `preferred` itself, else `preferred_2` ... `preferred_9`,
/// else `preferred_<random>`, else the account's email, which is the default
/// nickname and free while no account has that address. GitHub usernames
/// cannot contain `_`, so a suffixed handle is never another GitHub username.
pub(crate) async fn available_nickname(
    preferred: &str,
    email: &str,
) -> Result<String, ErrorResponse> {
    for candidate in nickname_candidates(preferred) {
        if !nickname_is_valid(&candidate, email) {
            continue;
        }
        if !nickname_taken(&candidate).await? {
            return Ok(candidate);
        }
    }
    Ok(db::ent::canonical_email(email))
}

pub(crate) async fn nickname_taken(nickname: &str) -> Result<bool, ErrorResponse> {
    Ok(crate::db::get_user_by_nickname(nickname)
        .await
        .map_err(ErrorResponse::internal)?
        .is_some())
}

fn nickname_candidates(preferred: &str) -> Vec<String> {
    let base = canonical_nickname(preferred);
    if base.is_empty() {
        return Vec::new();
    }
    let numbered = (2..=LAST_NUMBERED_SUFFIX).map(|n| n.to_string());
    let random = std::iter::repeat_with(|| {
        let id = ulid::Ulid::generate().to_string();
        // The trailing characters of a ULID are random (the leading ones are
        // its timestamp).
        id[id.len() - 6..].to_lowercase()
    })
    .take(RANDOM_SUFFIXES);
    let suffixed: Vec<String> = numbered
        .chain(random)
        .map(|suffix| with_suffix(&base, &suffix))
        .collect();
    std::iter::once(base).chain(suffixed).collect()
}

/// `base_suffix`, shortening `base` so the result fits the column.
fn with_suffix(base: &str, suffix: &str) -> String {
    let room = MAX_NICKNAME_CHARS - suffix.chars().count() - 1;
    let base: String = base.chars().take(room).collect();
    format!("{base}_{suffix}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn candidates_start_with_the_canonical_handle_then_add_suffixes() {
        let candidates = nickname_candidates(" Octocat ");
        assert_eq!(candidates[0], "octocat");
        assert_eq!(candidates[1], "octocat_2");
        assert_eq!(candidates[8], "octocat_9");
        assert_eq!(
            candidates.len(),
            1 + (LAST_NUMBERED_SUFFIX as usize - 1) + RANDOM_SUFFIXES
        );
        for random in &candidates[9..] {
            let suffix = random.strip_prefix("octocat_").unwrap();
            assert_eq!(suffix.len(), 6);
            assert_eq!(suffix, suffix.to_lowercase());
        }
    }

    #[test]
    fn suffixed_candidates_fit_the_column() {
        let long = "a".repeat(MAX_NICKNAME_CHARS);
        for candidate in nickname_candidates(&long).iter().skip(1) {
            assert_eq!(candidate.chars().count(), MAX_NICKNAME_CHARS);
        }
        assert_eq!(with_suffix("ada", "2"), "ada_2");
    }

    #[test]
    fn an_empty_handle_has_no_candidates() {
        assert!(nickname_candidates("  ").is_empty());
    }
}
