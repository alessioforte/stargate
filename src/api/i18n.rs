use crate::err::{ErrorCode, ErrorResponse};
use crate::etc::http::messages::MessageCode;
use axum::Json;
use axum::extract::Path;
use axum::response::{IntoResponse, Response};
use http::StatusCode;
use http::header::{CACHE_CONTROL, CONTENT_LANGUAGE, ETAG, IF_NONE_MATCH};
use once_cell::sync::Lazy;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use utoipa::ToSchema;

const CATALOG_SCHEMA: &str = "stargate/i18n/v1";
const FALLBACK_LOCALE: &str = "en";
const ITALIAN_OVERRIDES: &str = include_str!("i18n/it.json");

#[derive(Debug, Clone, Default, Deserialize, Serialize, ToSchema)]
pub struct ApiMessages {
    pub errors: BTreeMap<String, String>,
    pub messages: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ApiMessageCatalog {
    pub schema: &'static str,
    pub locale: String,
    pub fallback_locale: &'static str,
    pub version: String,
    pub messages: ApiMessages,
}

static ENGLISH_MESSAGES: Lazy<ApiMessages> = Lazy::new(|| ApiMessages {
    errors: ErrorCode::ALL
        .iter()
        .map(|code| (code.as_str().to_string(), code.message().to_string()))
        .collect(),
    messages: MessageCode::ALL
        .iter()
        .map(|code| (code.as_str().to_string(), code.message().to_string()))
        .collect(),
});

static ITALIAN_MESSAGES: Lazy<ApiMessages> = Lazy::new(|| {
    let overrides: ApiMessages =
        serde_json::from_str(ITALIAN_OVERRIDES).expect("Italian API translations must be valid");
    merge_with_english(overrides)
});

fn merge_with_english(overrides: ApiMessages) -> ApiMessages {
    let mut messages = ENGLISH_MESSAGES.clone();
    messages.errors.extend(overrides.errors);
    messages.messages.extend(overrides.messages);
    messages
}

fn normalize_locale(locale: &str) -> Option<&'static str> {
    match locale.to_ascii_lowercase().as_str() {
        "en" | "en-us" | "en-gb" => Some("en"),
        "it" | "it-it" => Some("it"),
        _ => None,
    }
}

fn catalog(locale: &str) -> Option<ApiMessageCatalog> {
    let locale = normalize_locale(locale)?;
    let messages = match locale {
        "it" => ITALIAN_MESSAGES.clone(),
        _ => ENGLISH_MESSAGES.clone(),
    };
    let version = catalog_version(locale, &messages);

    Some(ApiMessageCatalog {
        schema: CATALOG_SCHEMA,
        locale: locale.to_string(),
        fallback_locale: FALLBACK_LOCALE,
        version,
        messages,
    })
}

fn catalog_version(locale: &str, messages: &ApiMessages) -> String {
    let mut hasher = Sha256::new();
    hasher.update(CATALOG_SCHEMA);
    hasher.update(locale);
    hasher.update(serde_json::to_vec(messages).expect("API messages must serialize"));
    let digest = hasher.finalize();
    format!("sha256:{}", hex::encode(digest))
}

#[utoipa::path(
    get,
    path = "/i18n/{locale}",
    tags = ["Internationalization"],
    params(("locale" = String, Path, description = "Locale code, for example en or it")),
    responses(
        (status = 200, description = "Versioned API message catalog", body = ApiMessageCatalog),
        (status = 304, description = "Catalog has not changed"),
        (status = 404, description = "Locale is not supported", body = ErrorResponse)
    )
)]
pub async fn get_api_messages(
    Path(locale): Path<String>,
    headers: http::HeaderMap,
) -> Result<Response, ErrorResponse> {
    let catalog = catalog(&locale).ok_or_else(|| {
        ErrorResponse::new(ErrorCode::I18nLocaleNotSupported).with_param("locale", locale)
    })?;
    let etag = format!("\"{}\"", catalog.version);

    if headers
        .get(IF_NONE_MATCH)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| value.split(',').any(|value| value.trim() == etag))
    {
        return Ok(http::Response::builder()
            .status(StatusCode::NOT_MODIFIED)
            .header(ETAG, etag)
            .header(CACHE_CONTROL, "public, max-age=300")
            .header(CONTENT_LANGUAGE, catalog.locale)
            .body(axum::body::Body::empty())
            .expect("static catalog response must be valid"));
    }

    let locale = catalog.locale.clone();
    let mut response = Json(catalog).into_response();
    response.headers_mut().insert(
        ETAG,
        etag.parse().expect("catalog ETag must be a valid header"),
    );
    response.headers_mut().insert(
        CACHE_CONTROL,
        "public, max-age=300"
            .parse()
            .expect("cache policy must be a valid header"),
    );
    response.headers_mut().insert(
        CONTENT_LANGUAGE,
        locale
            .parse()
            .expect("normalized locale must be a valid header"),
    );
    Ok(response)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn catalogs_contain_every_published_code() {
        for messages in [&*ENGLISH_MESSAGES, &*ITALIAN_MESSAGES] {
            for code in ErrorCode::ALL {
                assert!(messages.errors.contains_key(code.as_str()), "{code}");
            }
            for code in MessageCode::ALL {
                assert!(messages.messages.contains_key(code.as_str()), "{code}");
            }
        }
    }

    #[test]
    fn translation_overrides_only_reference_published_codes() {
        let overrides: ApiMessages = serde_json::from_str(ITALIAN_OVERRIDES).unwrap();
        let error_codes = ErrorCode::ALL
            .iter()
            .map(|code| code.as_str())
            .collect::<HashSet<_>>();
        let message_codes = MessageCode::ALL
            .iter()
            .map(|code| code.as_str())
            .collect::<HashSet<_>>();

        assert!(
            overrides
                .errors
                .keys()
                .all(|code| error_codes.contains(code.as_str()))
        );
        assert!(
            overrides
                .messages
                .keys()
                .all(|code| message_codes.contains(code.as_str()))
        );
    }

    #[test]
    fn catalog_version_is_stable_for_the_same_content() {
        let first = catalog("it").unwrap();
        let second = catalog("it-IT").unwrap();

        assert_eq!(first.version, second.version);
        assert_eq!(first.locale, "it");
    }
}
