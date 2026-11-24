//! Serving planes. Every route Stargate serves belongs to exactly one plane,
//! and a plane owns both its routes and the HTTP layers applied to them.

use crate::api::dispatch::HostDispatch;
use crate::api::middleware::{rate_limit::ingress_limit_middleware, telemetry::record_route};
use crate::conf::hosts::IamHosts;
use crate::srv::health::Lifecycle;
use axum::{
    Router,
    extract::Request,
    middleware::{Next, from_fn},
    response::Response,
};
use std::fmt;
use tower::service_fn;
use tower_http::{compression::CompressionLayer, normalize_path::NormalizePathLayer};

/// The four planes Stargate serves.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Plane {
    /// `/livez` and `/readyz`; mounted outside host dispatch and the ingress limit.
    Management,
    /// OAuth/OIDC provider endpoints, accounts, sign-up, social sign-in, the
    /// hosted auth app and the internal-context JWKS. Documentation is shared
    /// with the admin plane and describes the listener’s selected planes.
    Identity,
    /// `/admin/*` and the Console.
    Admin,
    /// Everything the gateway configuration matches; upstreams own the path,
    /// the preflights, and the content encoding.
    Gateway,
}

impl Plane {
    pub const ALL: [Self; 4] = [Self::Management, Self::Identity, Self::Admin, Self::Gateway];

    pub const fn name(self) -> &'static str {
        match self {
            Self::Management => "management",
            Self::Identity => "identity",
            Self::Admin => "admin",
            Self::Gateway => "gateway",
        }
    }

    pub fn from_name(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|plane| plane.name() == name)
    }

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

/// The planes a listener serves.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct PlaneSet(u8);

impl PlaneSet {
    pub const NONE: Self = Self(0);
    pub const ALL: Self = Self(0b1111);

    pub const fn of(plane: Plane) -> Self {
        Self(match plane {
            Plane::Management => 0b0001,
            Plane::Identity => 0b0010,
            Plane::Admin => 0b0100,
            Plane::Gateway => 0b1000,
        })
    }

    pub const fn contains(self, plane: Plane) -> bool {
        self.0 & Self::of(plane).0 != 0
    }

    /// True when the set is exactly `plane`: the management plane must stay
    /// alone so its listener can be plaintext while the others terminate TLS.
    pub const fn only(self, plane: Plane) -> bool {
        self.0 == Self::of(plane).0
    }

    pub fn names(self) -> impl Iterator<Item = &'static str> {
        Plane::ALL
            .into_iter()
            .filter(move |plane| self.contains(*plane))
            .map(Plane::name)
    }

    /// Parses a comma-separated plane list (`identity,gateway`).
    pub fn parse(value: &str) -> Result<Self, String> {
        let mut planes = Self::NONE;
        for name in value.split(',') {
            let name = name.trim().to_ascii_lowercase();
            let plane = Plane::from_name(&name).ok_or_else(|| {
                format!("unknown plane `{name}`; expected management, identity, admin, or gateway")
            })?;
            if planes.contains(plane) {
                return Err(format!("plane `{name}` is listed twice"));
            }
            planes.0 |= Self::of(plane).0;
        }
        if planes == Self::NONE {
            return Err("expected at least one plane".to_string());
        }
        Ok(planes)
    }
}

impl fmt::Display for PlaneSet {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut names = self.names().peekable();
        while let Some(name) = names.next() {
            f.write_str(name)?;
            if names.peek().is_some() {
                f.write_str(",")?;
            }
        }
        Ok(())
    }
}

/// The router a listener serves: the planes it declares, each with its own
/// layers. Requests on IAM hosts reach the gateway, if the listener serves it,
/// only when no IAM route matches, exactly as on the single implicit listener.
pub fn listener_router(planes: PlaneSet, lifecycle: Lifecycle, hosts: IamHosts) -> Router {
    let gateway = planes
        .contains(Plane::Gateway)
        .then(|| Plane::Gateway.router(lifecycle.clone()));
    let iam = iam_planes(planes, &lifecycle).map(|iam| match &gateway {
        Some(gateway) => kept_uri(normalized(iam.fallback_service(gateway.clone()))),
        None => normalized(iam),
    });
    let router = match (iam, gateway) {
        (Some(iam), Some(gateway)) => {
            Router::new().fallback_service(HostDispatch::new(hosts, iam, gateway))
        }
        (Some(iam), None) => {
            Router::new().fallback_service(HostDispatch::new(hosts, iam, Router::new()))
        }
        (None, Some(gateway)) => gateway,
        (None, None) => Router::new(),
    };
    if planes.contains(Plane::Management) {
        router.merge(Plane::Management.router(lifecycle))
    } else {
        router
    }
}

/// The IAM routes a listener serves, with the layers of each plane.
fn iam_planes(planes: PlaneSet, lifecycle: &Lifecycle) -> Option<Router> {
    let mut iam = Router::new();
    let mut serves_iam = false;
    for plane in [Plane::Identity, Plane::Admin] {
        if planes.contains(plane) {
            iam = iam.merge(plane.router(lifecycle.clone()));
            serves_iam = true;
        }
    }
    // Reserve control-plane paths on IAM hosts even on a gateway-only listener.
    // They must never reach an upstream through the gateway fallback.
    if !planes.contains(Plane::Admin) {
        iam = iam
            .nest_service("/admin", Router::new())
            .nest_service(&crate::conf::apps::console_app_base_path(), Router::new());
    }
    if serves_iam {
        iam = iam.merge(iam_layers(crate::api::docs::router(planes)));
    }
    (serves_iam || planes.contains(Plane::Gateway)).then_some(iam)
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
    use crate::api::{account, apps, i18n, internal_context, oidc, signup, social};
    use axum::routing::get;

    Router::new()
        .route(
            "/.well-known/stargate-context-jwks.json",
            get(internal_context::get_internal_context_jwks),
        )
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

/// Trims a trailing slash before the IAM routes match. The layer wraps the
/// router as a service: `Router::layer` would run it after matching.
fn normalized(router: Router) -> Router {
    Router::new().fallback_service(
        tower::ServiceBuilder::new()
            .layer(NormalizePathLayer::trim_trailing_slash())
            .service(router),
    )
}

/// The URI the client sent, before the IAM plane trimmed a trailing slash for
/// matching.
#[derive(Clone)]
struct ClientUri(http::Uri);

/// Keeps the client's URI for the gateway fallback, which matches the path the
/// client sent: normalization must not reach an upstream. Only requests on IAM
/// hosts pay for it.
fn kept_uri(router: Router) -> Router {
    Router::new().fallback_service(
        tower::ServiceBuilder::new()
            .layer(from_fn(keep_client_uri))
            .service(router),
    )
}

/// Keeps the client's URI for the gateway fallback.
async fn keep_client_uri(mut request: Request, next: Next) -> Response {
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
