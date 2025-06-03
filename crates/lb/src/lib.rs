use std::net::IpAddr;

pub mod ip_hash;
pub mod random;
pub mod round_robin;

pub use ip_hash::IpHash;
pub use random::Random;
pub use round_robin::RoundRobin;

pub struct Upstream {
    pub base_url: String,
}

pub struct RequestContext {
    pub client_ip: Option<IpAddr>,
    pub path: String,
    pub method: String,
    // pub headers: Option<http::HeaderMap>,
    // pub key: Option<String>,
}

pub trait LoadBalancer {
    // fn select(&self) -> Option<&Upstream>;
    fn select(&self, context: &RequestContext) -> Option<&Upstream>;
    fn name(&self) -> &'static str;
}
