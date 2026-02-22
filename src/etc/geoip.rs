use maxminddb::geoip2;
use moka::sync::Cache;
use once_cell::sync::Lazy;
use std::net::IpAddr;
use std::sync::Arc;
use std::time::Duration;

type GeoDb = maxminddb::Reader<Vec<u8>>;
const GEOIP_CACHE_CAPACITY: u64 = 50_000;
const GEOIP_CACHE_TTL_SECS: u64 = 3600;

pub static GEO_DB: Lazy<Option<GeoDb>> = Lazy::new(|| {
    let path = std::env::var("GEOIP_DB_PATH")
        .unwrap_or_else(|_| ".stargate/geoip/GeoLite2-City.mmdb".to_string());
    match maxminddb::Reader::open_readfile(&path) {
        Ok(reader) => {
            tracing::info!("GeoIP database loaded from {}", path);
            Some(reader)
        }
        Err(e) => {
            tracing::warn!(
                "GeoIP database not available at {}: {}. GeoIP lookup disabled.",
                path,
                e
            );
            None
        }
    }
});

pub static GEO_CACHE: Lazy<Cache<IpAddr, Arc<GeoInfo>>> = Lazy::new(|| {
    Cache::builder()
        .max_capacity(GEOIP_CACHE_CAPACITY)
        .time_to_live(Duration::from_secs(GEOIP_CACHE_TTL_SECS))
        .build()
});

pub fn init() {
    Lazy::force(&GEO_DB);
    Lazy::force(&GEO_CACHE);
    tracing::info!("GeoIP module initialized");
}

#[derive(Debug, Clone)]
pub struct GeoInfo {
    pub country_code: Option<Box<str>>,
    pub country_name: Option<Box<str>>,
    pub city_name: Option<Box<str>>,
    pub latitude: Option<f64>,
    pub longitude: Option<f64>,
}

impl GeoInfo {
    pub fn unknown() -> Arc<Self> {
        Arc::new(Self {
            country_code: None,
            country_name: None,
            city_name: None,
            latitude: None,
            longitude: None,
        })
    }
}

pub fn lookup(ip_addr: IpAddr) -> Option<Arc<GeoInfo>> {
    let db = GEO_DB.as_ref()?;

    Some(GEO_CACHE.get_with_by_ref(&ip_addr, || {
        db.lookup(ip_addr)
            .ok()
            .and_then(|result| result.decode::<geoip2::City>().ok())
            .flatten()
            .map(|city| {
                Arc::new(GeoInfo {
                    country_code: city.country.iso_code.map(Box::from),
                    country_name: city.country.names.english.map(Box::from),
                    city_name: city.city.names.english.map(Box::from),
                    latitude: city.location.latitude,
                    longitude: city.location.longitude,
                })
            })
            .unwrap_or_else(GeoInfo::unknown)
    }))
}
