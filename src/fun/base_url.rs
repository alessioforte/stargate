pub fn get_base_url() -> String {
    let protocol = if crate::etc::server::tls::enabled().unwrap_or(false) {
        "https"
    } else {
        "http"
    };
    let hostname = std::env::var("HOSTNAME").unwrap_or("localhost".to_string());
    let port = std::env::var("PORT").unwrap_or("5050".to_string());
    format!("{}://{}:{}", protocol, hostname, port)
}
