//! Serving planes. Every route Stargate serves belongs to exactly one plane,
//! and a plane owns both its routes and the HTTP layers applied to them.

use crate::api::middleware::{rate_limit::ingress_limit_middleware, telemetry::record_route};
use crate::srv::health::Lifecycle;
use axum::{
    Router,
    extract::Request,
    middleware::{Next, from_fn},
    response::Response,
};
use tower::service_fn;
use tower_http::compression::CompressionLayer;

/// The four planes Stargate serves.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Plane {
    /// `/livez` and `/readyz`; mounted outside host dispatch and the ingress limit.
    Management,
    /// OAuth/OIDC provider endpoints, accounts, sign-up, social sign-in, the
    /// hosted auth app, the OpenAPI documents, and the internal-context JWKS.
    Identity,
    /// `/admin/*` and the Console.
    Admin,
    /// Everything the gateway configuration matches; upstreams own the path,
    /// the preflights, and the content encoding.
    Gateway,
}

impl Plane {
    /// The plane's router with the layers the plane owns.
    pub fn router(self, lifecycle: Lifecycle) -> Router {
        match self {
            Self::Management => crate::api::health::router(lifecycle),
            Self::Identity => iam_layers(identity_routes()),
            Self::Admin => iam_layers(admin_routes()),
            Self::Gateway => Router::new().fallback_service(service_fn(|request| async {
                crate::gate::service(restore_client_uri(request)).await
            })),
        }
    }
}

/// Layers shared by the IAM planes: the per-IP ingress allowance, the matched
/// route for telemetry, and the CORS and compression the hosted API and apps
/// expect. The gateway plane deliberately has none of them.
fn iam_layers(router: Router) -> Router {
    router
        .layer(from_fn(ingress_limit_middleware))
        .layer(from_fn(record_route))
        .layer(crate::web::cors::hyper_configure())
        .layer(CompressionLayer::new())
}

/// Identity plane: the provider's public API and the hosted auth app.
pub(super) fn identity_routes() -> Router {
    use crate::api::{account, apps, docs, i18n, internal_context, oidc, signup, social};
    use axum::routing::get;

    Router::new()
        .route(
            "/.well-known/stargate-context-jwks.json",
            get(internal_context::get_internal_context_jwks),
        )
        .route("/docs", get(docs::get_api_doc))
        .route("/docs/errors", get(docs::get_error_catalog))
        .route("/i18n/{locale}", get(i18n::get_api_messages))
        .merge(signup::router())
        .merge(oidc::router())
        .merge(social::router())
        .merge(account::router())
        .merge(apps::auth::router())
}

/// Admin plane: the control plane API and the Console that drives it.
pub(super) fn admin_routes() -> Router {
    crate::api::admin::router().merge(crate::api::apps::console::router())
}

/// The URI the client sent, before the server's own routes trimmed a trailing
/// slash for matching.
#[derive(Clone)]
struct ClientUri(http::Uri);

/// Keeps the client's URI for the gateway plane. It runs before path
/// normalization, so the gateway always forwards the path the client sent even
/// when the request passed through the server's own routes first.
pub(super) async fn keep_client_uri(mut request: Request, next: Next) -> Response {
    let uri = request.uri().clone();
    request.extensions_mut().insert(ClientUri(uri));
    next.run(request).await
}

fn restore_client_uri(mut request: Request) -> Request {
    if let Some(ClientUri(uri)) = request.extensions_mut().remove::<ClientUri>() {
        *request.uri_mut() = uri;
    }
    request
}

#[cfg(all(test, feature = "memory"))]
mod tests;
