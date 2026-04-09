pub fn get_base_url() -> String {
    let protocol = if crate::etc::tls::enabled().unwrap_or(false) {
        "https"
    } else {
        "http"
    };
    let api_base_path = std::env::var("API_BASE_PATH").unwrap_or("stargate".to_string());
    let port = std::env::var("PORT").unwrap_or("5050".to_string());
    format!("{}://localhost:{}/{}", protocol, port, api_base_path)
}
