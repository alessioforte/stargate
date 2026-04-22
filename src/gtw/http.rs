use crate::err::{ErrorResponse, HttpError};

#[cfg(feature = "hyper-stack")]
use axum::{body::Body, response::Response};
#[cfg(feature = "hyper-stack")]
use http::header::{
    CONNECTION, HOST, PROXY_AUTHENTICATE, PROXY_AUTHORIZATION, TE, TRAILER, TRANSFER_ENCODING,
    UPGRADE,
};

#[cfg(feature = "hyper-stack")]
pub async fn handler(
    mut req: http::Request<Body>,
    headers: &http::HeaderMap,
    uri: &str,
    client: &crate::etc::gate::HyperClient,
) -> Result<Response, ErrorResponse> {
    let upstream_uri = uri.parse::<http::Uri>().map_err(|error| {
        tracing::error!(%uri, %error, "Invalid upstream URI");
        ErrorResponse::from(HttpError::BadGateway(
            "Failed to connect to backend service".to_string(),
        ))
    })?;

    *req.uri_mut() = upstream_uri;
    strip_hop_by_hop_headers(req.headers_mut());
    req.headers_mut().remove(HOST);

    let response = client.request(req).await.map_err(|error| {
        tracing::error!("Error forwarding request to backend: {}", error);
        ErrorResponse::from(HttpError::BadGateway(
            "Failed to connect to backend service".to_string(),
        ))
    })?;

    let (parts, body) = response.into_parts();
    let mut upstream_headers = parts.headers;
    strip_hop_by_hop_headers(&mut upstream_headers);

    let mut proxied = Response::new(Body::new(body));
    *proxied.status_mut() = parts.status;
    *proxied.version_mut() = parts.version;
    *proxied.extensions_mut() = parts.extensions;
    *proxied.headers_mut() = upstream_headers;

    for (name, value) in headers {
        proxied.headers_mut().insert(name, value.clone());
    }

    Ok(proxied)
}

#[cfg(feature = "hyper-stack")]
fn strip_hop_by_hop_headers(headers: &mut http::HeaderMap) {
    let mut connection_headers = Vec::new();
    for value in headers.get_all(CONNECTION) {
        if let Ok(value) = value.to_str() {
            for header in value.split(',') {
                let header = header.trim();
                if header.is_empty() {
                    continue;
                }
                if let Ok(name) = http::header::HeaderName::from_bytes(header.as_bytes()) {
                    connection_headers.push(name);
                }
            }
        }
    }

    for name in connection_headers {
        headers.remove(name);
    }

    headers.remove(CONNECTION);
    headers.remove("keep-alive");
    headers.remove(PROXY_AUTHENTICATE);
    headers.remove(PROXY_AUTHORIZATION);
    headers.remove(TE);
    headers.remove(TRAILER);
    headers.remove(TRANSFER_ENCODING);
    headers.remove(UPGRADE);
    headers.remove("proxy-connection");
    headers.remove("trailers");
}

#[cfg(all(test, feature = "hyper-stack"))]
mod tests {
    use super::strip_hop_by_hop_headers;
    use axum::body::Body;
    use http::header::{CONNECTION, TE};
    use http_body_util::{BodyExt, Full};
    use hyper::body::Bytes;

    #[tokio::test]
    async fn axum_body_wrapper_preserves_trailers() {
        let mut trailers = http::HeaderMap::new();
        trailers.insert("grpc-status", http::HeaderValue::from_static("0"));
        trailers.insert("x-upstream-trailer", http::HeaderValue::from_static("ok"));

        let body = Full::new(Bytes::from_static(b"pong"))
            .with_trailers(async move { Some(Ok::<_, std::convert::Infallible>(trailers)) });

        let collected = Body::new(body).collect().await.unwrap();
        let trailers = collected.trailers().cloned().expect("missing trailers");
        assert_eq!(collected.to_bytes(), Bytes::from_static(b"pong"));
        assert_eq!(trailers["grpc-status"], "0");
        assert_eq!(trailers["x-upstream-trailer"], "ok");
    }

    #[test]
    fn strip_hop_by_hop_headers_removes_standard_and_connection_listed_headers() {
        let mut headers = http::HeaderMap::new();
        headers.insert(
            CONNECTION,
            http::HeaderValue::from_static("keep-alive, x-remove"),
        );
        headers.insert("keep-alive", http::HeaderValue::from_static("timeout=5"));
        headers.insert("x-remove", http::HeaderValue::from_static("1"));
        headers.insert(TE, http::HeaderValue::from_static("trailers"));
        headers.insert("x-keep", http::HeaderValue::from_static("ok"));

        strip_hop_by_hop_headers(&mut headers);

        assert!(headers.get(CONNECTION).is_none());
        assert!(headers.get("keep-alive").is_none());
        assert!(headers.get("x-remove").is_none());
        assert!(headers.get(TE).is_none());
        assert_eq!(headers.get("x-keep").unwrap(), "ok");
    }
}
