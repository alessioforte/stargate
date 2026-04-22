use crate::etc::{ac::Env, geoip};
use chrono::{DateTime, Utc};
use db::ent::AuditContext;
use gate::cfg::service::EnvProfile;
use std::net::IpAddr;
use std::sync::{Arc, OnceLock};
use ulid::Ulid;

#[derive(Clone)]
pub struct RequestContext {
    request_id: Box<str>,
    started_at: DateTime<Utc>,
    client_ip: Option<IpAddr>,
    user_agent: Option<Box<str>>,
    geo: OnceLock<Arc<geoip::GeoInfo>>,
}

impl RequestContext {
    pub fn new(
        request_id: String,
        started_at: DateTime<Utc>,
        client_ip: Option<IpAddr>,
        user_agent: Option<String>,
    ) -> Self {
        Self {
            request_id: request_id.into_boxed_str(),
            started_at,
            client_ip,
            user_agent: user_agent.map(String::into_boxed_str),
            geo: OnceLock::new(),
        }
    }

    pub fn request_id(&self) -> &str {
        &self.request_id
    }

    pub fn audit_context(&self) -> AuditContext {
        AuditContext::anonymous().with_request_id(self.request_id().to_string())
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

    fn geo(&self) -> &Arc<geoip::GeoInfo> {
        self.geo.get_or_init(|| {
            self.client_ip
                .and_then(geoip::lookup)
                .unwrap_or_else(geoip::GeoInfo::unknown)
        })
    }
}

pub fn request_id_from(extensions: &http::Extensions) -> String {
    extensions
        .get::<RequestContext>()
        .map(|ctx| ctx.request_id().to_string())
        .unwrap_or_else(|| Ulid::new().to_string())
}

pub fn take_audit_context_from(extensions: &mut http::Extensions) -> AuditContext {
    if let Some(ctx) = extensions.remove::<AuditContext>() {
        return ctx;
    }
    extensions
        .get::<RequestContext>()
        .map(RequestContext::audit_context)
        .unwrap_or_else(AuditContext::anonymous)
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
