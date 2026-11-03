use ::db::service::Service;
use opentelemetry::KeyValue;

pub(super) fn register(service: &Service) {
    let meter = crate::obs::telemetry::meter();
    let pool = service.clone();
    meter
        .u64_observable_gauge("stargate.db.pool.connections")
        .with_description("SQL pool connections by sampled state")
        .with_callback(move |observer| {
            let stats = pool.pool_stats();
            observer.observe(
                u64::from(stats.idle),
                &[KeyValue::new("stargate.state", "idle")],
            );
            observer.observe(
                u64::from(stats.in_use),
                &[KeyValue::new("stargate.state", "in_use")],
            );
        })
        .build();
    let pool = service.clone();
    meter
        .u64_observable_gauge("stargate.db.pool.max")
        .with_description("Configured maximum SQL pool connections")
        .with_callback(move |observer| {
            observer.observe(u64::from(pool.pool_stats().max), &[]);
        })
        .build();
}
