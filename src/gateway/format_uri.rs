use crate::etc::gate::config::Service;

pub fn format_uri(service: &Service, path: String) -> String {
    let mut protocol = service.protocol.clone();
    if protocol.is_empty() {
        protocol = "http".to_string();
    }
    let host = service.host.clone();
    let port = match service.port {
        Some(port) => format!(":{}", port),
        None => "".to_string(),
    };
    let path = path.replace(&service.path, "");

    format!("{}://{}{}{}", protocol, host, port, path)
}
