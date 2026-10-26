use super::RequestContext;
use crate::etc::auth::access_control::Env;
use db::ent::{TrustedAuditContext, TrustedAuditRequest};
use gate::cfg::EnvProfile;
use ulid::Ulid;

impl RequestContext {
    pub fn audit_request(&self) -> TrustedAuditRequest {
        TrustedAuditRequest::from_http(
            self.request_id(),
            self.trace_id.as_deref().map(str::to_string),
            self.client_ip.map(|ip| ip.to_string()),
            self.user_agent.as_deref().map(str::to_string),
        )
    }

    pub fn env(&self, profile: EnvProfile) -> Env {
        match profile {
            EnvProfile::None => Env::default(),
            EnvProfile::Basic => self.basic_env(),
            EnvProfile::Geo => self.geo_env(),
        }
    }

    fn basic_env(&self) -> Env {
        Env {
            ip_address: self
                .client_ip
                .map(|ip| ip.to_string().into_boxed_str())
                .unwrap_or_else(|| "unknown".into()),
            user_agent: self.user_agent.clone().unwrap_or_else(|| "unknown".into()),
            country_code: Box::default(),
            country_name: Box::default(),
            city_name: Box::default(),
            date: self
                .started_at
                .format("%Y-%m-%d")
                .to_string()
                .into_boxed_str(),
            time: self.started_at.format("%H:%M").to_string().into_boxed_str(),
            day_of_week: self.started_at.format("%A").to_string().into_boxed_str(),
        }
    }

    fn geo_env(&self) -> Env {
        let mut env = self.basic_env();
        let geo = self.geo();
        env.country_code = geo.country_code.clone().unwrap_or_else(|| "unknown".into());
        env.country_name = geo.country_name.clone().unwrap_or_else(|| "unknown".into());
        env.city_name = geo.city_name.clone().unwrap_or_else(|| "unknown".into());
        env
    }
}

pub fn request_id_from(extensions: &http::Extensions) -> String {
    extensions
        .get::<RequestContext>()
        .map(|ctx| ctx.request_id().to_string())
        .unwrap_or_else(|| Ulid::generate().to_string())
}

pub fn audit_request_from(extensions: &http::Extensions) -> TrustedAuditRequest {
    extensions
        .get::<RequestContext>()
        .map(RequestContext::audit_request)
        .unwrap_or_else(|| {
            TrustedAuditRequest::from_http(request_id_from(extensions), None, None, None)
        })
}

/// Consume a context established by a trusted route or authentication
/// boundary. This never invents a scope.
pub fn take_trusted_audit_context_from(
    extensions: &mut http::Extensions,
) -> Option<TrustedAuditContext> {
    extensions.remove::<TrustedAuditContext>()
}

pub fn build_env_from(extensions: &mut http::Extensions, profile: EnvProfile) -> Env {
    if let Some(env) = extensions.remove::<Env>() {
        return env;
    }
    extensions
        .get::<RequestContext>()
        .map(|ctx| ctx.env(profile))
        .unwrap_or_default()
}
