// use crate::etc::ext::RequestExt;
// use actix_web::{
//     body::{EitherBody, MessageBody},
//     dev::{ServiceRequest, ServiceResponse},
//     middleware::Next,
//     web::Data,
// };

// #[cfg(feature = "redis")]
// pub type RateLimiter = rl::RateLimiter<store::RedisStore>;

// #[cfg(feature = "memory")]
// pub type RateLimiter = rl::GcraRateLimiter<
//     store::MemoryStore,
//     rl::DefaultClock,
//     rl::middleware::NoOpMiddleware<rl::clock::MonotonicInstant>,
// >;

// pub fn init(
//     store: impl store::Store,
// ) -> Data<
//     rl::GcraRateLimiter<
//         impl store::Store,
//         rl::DefaultClock,
//         rl::middleware::NoOpMiddleware<rl::clock::MonotonicInstant>,
//     >,
// > {
//     let limiter = rl::GcraRateLimiterBuilder::new()
//         .store(store)
//         .jitter_strategy(rl::JitterStrategy::proportional(10))
//         .build()
//         .unwrap();
//     log::info!("Rate limiter initialized");

//     Data::new(limiter)
// }

// pub async fn rate_limit_middleware<B: MessageBody + 'static>(
//     req: ServiceRequest,
//     next: Next<B>,
// ) -> Result<ServiceResponse<EitherBody<B>>, actix_web::Error> {
//     let limiter = req.app_data::<Data<RateLimiter>>().unwrap();
//     let client_ip = req.request().get_client_ip();

//     let key = client_ip
//         .map(|ip| ip.to_string())
//         .unwrap_or_else(|| "unknown".to_string());

//     match limiter.check(&key).await {
//         Ok(_) => {
//             let res = next.call(req).await?;
//             Ok(res.map_into_left_body())
//         }
//         Err(e) => {
//             log::error!("Rate limiter error: {}", e);
//             return Err(actix_web::error::ErrorInternalServerError(
//                 "Rate limiter error",
//             ));
//         }
//     }
// }

// // use crate::etc::ext::RequestExt;
// // use actix_web::{
// //     body::{EitherBody, MessageBody},
// //     dev::{ServiceRequest, ServiceResponse},
// //     middleware::Next,
// //     web::Data,
// // };

// // #[cfg(feature = "redis")]
// // pub type RateLimiter = rl::RateLimiter<store::RedisStore>;

// // #[cfg(feature = "memory")]
// // pub type RateLimiter = rl::RateLimiter<store::MemoryStore>;

// // pub fn init(store: impl store::Store) -> Data<rl::RateLimiter<impl store::Store>> {
// //     let settings = rl::GlobalRateLimitSettings {
// //         enabled: true,
// //         default_config: rl::config::RateLimitConfig::default(),
// //         ..Default::default()
// //     };

// //     let limiter = rl::RateLimiter::new(store, settings);
// //     log::info!("Rate limiter initialized");

// //     Data::new(limiter)
// // }

// // pub async fn rate_limit_middleware<B: MessageBody + 'static>(
// //     req: ServiceRequest,
// //     next: Next<B>,
// // ) -> Result<ServiceResponse<EitherBody<B>>, actix_web::Error> {
// //     let limiter = req.app_data::<Data<RateLimiter>>().unwrap();
// //     let client_ip = req.request().get_client_ip();

// //     let key = client_ip
// //         .map(|ip| ip.to_string())
// //         .unwrap_or_else(|| "unknown".to_string());

// //     match limiter.check_rate_limit(&key, None).await {
// //         Ok(decision) => {
// //             if decision.allowed {
// //                 let res = next.call(req).await?;
// //                 Ok(res.map_into_left_body())
// //             } else {
// //                 let retry_after = decision.retry_after_ms.unwrap_or(60_000) / 1000; // in seconds
// //                 log::warn!(
// //                     "Rate limit exceeded for key {}. Retry after {} seconds.",
// //                     key,
// //                     retry_after
// //                 );
// //                 let body = format!(
// //                     "Too Many Requests. Please try again in {} seconds.",
// //                     retry_after
// //                 );
// //                 let resp = actix_web::HttpResponse::TooManyRequests()
// //                     .insert_header(("Retry-After", retry_after.to_string()))
// //                     .insert_header(("X-Ratelimit-After", retry_after.to_string()))
// //                     .body(body);
// //                 Ok(req.into_response(resp).map_into_right_body())
// //             }
// //         }
// //         Err(e) => {
// //             log::error!("Rate limiter error: {}", e);
// //             return Err(actix_web::error::ErrorInternalServerError(
// //                 "Rate limiter error",
// //             ));
// //         }
// //     }
// // }
