use actix_web::web::Data;

#[cfg(feature = "redis")]
pub type RateLimiter = rl::RateLimiter<store::RedisStore>;

#[cfg(feature = "memory")]
pub type RateLimiter = rl::RateLimiter<store::MemoryStore>;

pub fn init(store: impl store::Store) -> Data<rl::RateLimiter<impl store::Store>> {
    let settings = rl::GlobalRateLimitSettings {
        enabled: true,
        default_config: rl::config::RateLimitConfig::new(1, 5, 60), // 100 req/sec, burst of 200
        ..Default::default()
    };

    println!("Rate limiter settings: {:?}", settings);

    let limiter = rl::RateLimiter::new(store, settings);
    log::info!("Rate limiter initialized");

    Data::new(limiter)
}
