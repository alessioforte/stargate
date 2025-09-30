use crate::{
    err::{ErrorResponse, HttpError},
    etc::store::use_store,
};
use actix_web::{HttpResponse, delete, get, post, web};
use actix_web_grants::protect;
use db::Transaction;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Debug, utoipa::ToSchema, PartialEq, Eq, Hash, Clone)]
#[serde(rename_all = "lowercase")]
enum ResetPeriod {
    Daily,
    Weekly,
    Monthly,
    Yearly,
    Never, // For lifetime quotas
}

#[derive(Serialize, Deserialize, Debug, utoipa::ToSchema, Clone)]
#[serde(rename_all = "camelCase")]
struct Limits {
    pub requests_per_second: u32,
    pub burst_size: u32,
    pub window_size_seconds: u64,
    pub quotas: Option<std::collections::HashMap<ResetPeriod, u64>>,
}

#[derive(Serialize, Deserialize, Debug, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
struct ApiKey {
    owner: String,
    label: Option<String>,
    exp: Option<String>,
    attrs: Option<serde_json::Value>,
    limits: Option<Limits>,
}

#[utoipa::path(
    context_path = "/admin/apikeys",
    path = "/",
    responses(
        (status = 200, description = "OK")
    )
)]
#[get("")]
#[protect("SUPER_ADMIN")]
pub async fn get_api_keys() -> Result<HttpResponse, ErrorResponse> {
    // TODO: implement get api keys logic

    #[cfg(feature = "memory")]
    {
        let store = use_store();
        match store
            .export_to_simple_json("./.stargate/memory_store_backup.json")
            .await
        {
            Ok(_) => println!("Memory store backed up successfully."),
            Err(e) => eprintln!("Failed to back up memory store: {}", e),
        }
    }

    Ok(HttpResponse::Ok().json(web::Json("List of API keys")))
}

#[utoipa::path(
    context_path = "/admin/apikeys",
    path = "/",
    responses(
        (status = 200, description = "OK")
    )
)]
#[post("")]
#[protect("SUPER_ADMIN")]
pub async fn create_api_key(payload: web::Json<ApiKey>) -> Result<HttpResponse, ErrorResponse> {
    let exp = if let Some(ref exp) = payload.exp {
        match tools::parse_duration(exp) {
            Ok(dur) => Some((chrono::Utc::now() + dur).timestamp()),
            Err(e) => {
                return Err(ErrorResponse::from(HttpError::BadRequest(format!(
                    "Invalid exp format: {}",
                    e
                ))));
            }
        }
    } else {
        None
    };

    let service = crate::etc::db::service();
    let secret = apiks::generate_api_key();
    let key_hash = apiks::hash_api_key(&secret);

    let limits = match &payload.limits {
        Some(l) => Some(db::ent::Limits {
            requests_per_second: l.requests_per_second,
            burst_size: l.burst_size,
            window_size_seconds: l.window_size_seconds,
            quotas: l.quotas.as_ref().map(|q| {
                q.iter()
                    .map(|(k, v)| match k {
                        ResetPeriod::Daily => (db::ent::ResetPeriod::Daily, *v),
                        ResetPeriod::Weekly => (db::ent::ResetPeriod::Weekly, *v),
                        ResetPeriod::Monthly => (db::ent::ResetPeriod::Monthly, *v),
                        ResetPeriod::Yearly => (db::ent::ResetPeriod::Yearly, *v),
                        ResetPeriod::Never => (db::ent::ResetPeriod::Never, *v),
                    })
                    .collect()
            }),
        }),
        None => None,
    };

    // TODO: check if owner exists and is a user

    let api_key = match service
        .create_api_key(
            &payload.owner,
            db::ent::OwnerType::User,
            &key_hash,
            payload.label.clone(),
            payload.attrs.clone(),
            limits,
            exp,
        )
        .await
    {
        Ok(api_key) => api_key,
        Err(e) => {
            return Err(ErrorResponse::from(HttpError::InternalServerError(
                e.to_string(),
            )));
        }
    };

    let response = serde_json::json!({
        "id": api_key.id,
        "owner": api_key.owner,
        "owner_type": api_key.owner_type,
        "label": api_key.label,
        "exp": payload.exp,
        "attrs": payload.attrs,
        "api_key": secret, // Return the plain API key only once
    });

    Ok(HttpResponse::Ok().json(web::Json(response)))
}

#[utoipa::path(
    context_path = "/admin/apikeys",
    path = "/{id}",
    responses(
        (status = 200, description = "OK")
    )
)]
#[delete("/{id}")]
#[protect("SUPER_ADMIN")]
pub async fn delete_api_key(_params: web::Path<String>) -> Result<HttpResponse, ErrorResponse> {
    // TODO: implement delete api key logic
    Ok(HttpResponse::Ok().json(web::Json("API Key deleted successfully")))
}

pub fn routes() -> actix_web::Scope {
    web::scope("/apikeys")
        .service(create_api_key)
        .service(get_api_keys)
        .service(delete_api_key)
}
