pub mod cfg;
pub mod gate;
pub mod logo;
pub mod tls;

pub type Gate = actix_web::web::Data<gate::Gate>;
