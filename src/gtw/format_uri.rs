use crate::etc::gate::config::Service;

pub fn format_uri(service: &Service, path: String) -> String {
    let protocol = service
        .uri
        .protocol
        .clone()
        .unwrap_or_else(|| "http".to_string());
    let host = service.uri.host.clone();
    let port = match service.uri.port {
        Some(port) => format!(":{}", port),
        None => "".to_string(),
    };
    let uri_path = service.uri.path.clone().unwrap_or_else(|| "".to_string());
    let path = path.replace(&service.path, "");

    format!("{}://{}{}{}{}", protocol, host, port, uri_path, path)
}
