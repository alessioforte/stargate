//! Unary gRPC health checks without a generated protobuf client.

use http_body_util::BodyExt;
use reqwest::Client;

const MAX_RESPONSE_BYTES: usize = 64 * 1024;

fn write_varint(mut value: u64, output: &mut Vec<u8>) {
    while value >= 0x80 {
        output.push((value as u8) | 0x80);
        value >>= 7;
    }
    output.push(value as u8);
}

fn request_body(service: &str) -> Option<Vec<u8>> {
    let mut message = Vec::new();
    if !service.is_empty() {
        message.push(0x0a);
        write_varint(u64::try_from(service.len()).ok()?, &mut message);
        message.extend_from_slice(service.as_bytes());
    }
    let length = u32::try_from(message.len()).ok()?;
    let mut body = Vec::with_capacity(message.len() + 5);
    body.push(0);
    body.extend_from_slice(&length.to_be_bytes());
    body.extend_from_slice(&message);
    Some(body)
}

fn read_varint(bytes: &[u8], offset: &mut usize) -> Option<u64> {
    let mut value = 0u64;
    for shift in (0..70).step_by(7) {
        let byte = *bytes.get(*offset)?;
        *offset += 1;
        if shift == 63 && byte > 1 {
            return None;
        }
        value |= u64::from(byte & 0x7f) << shift;
        if byte & 0x80 == 0 {
            return Some(value);
        }
    }
    None
}

fn serving(body: &[u8]) -> bool {
    if body.len() < 5 || body[0] != 0 {
        return false;
    }
    let length = u32::from_be_bytes([body[1], body[2], body[3], body[4]]) as usize;
    if body.len() != 5 + length {
        return false;
    }
    let message = &body[5..];
    let mut offset = 0;
    let mut serving = false;
    while offset < message.len() {
        let Some(key) = read_varint(message, &mut offset) else {
            return false;
        };
        if key >> 3 == 0 {
            return false;
        }
        match key & 7 {
            0 => {
                let Some(value) = read_varint(message, &mut offset) else {
                    return false;
                };
                if key >> 3 == 1 {
                    serving = value == 1;
                }
            }
            1 => {
                offset = match offset.checked_add(8) {
                    Some(value) => value,
                    None => return false,
                }
            }
            2 => {
                let Some(length) =
                    read_varint(message, &mut offset).and_then(|value| usize::try_from(value).ok())
                else {
                    return false;
                };
                offset = match offset.checked_add(length) {
                    Some(value) => value,
                    None => return false,
                };
            }
            5 => {
                offset = match offset.checked_add(4) {
                    Some(value) => value,
                    None => return false,
                }
            }
            _ => return false,
        }
        if offset > message.len() {
            return false;
        }
    }
    serving
}

pub(super) async fn check(client: &Client, base_url: &str, service: &str) -> bool {
    let Some(request) = request_body(service) else {
        return false;
    };
    let url = format!(
        "{}/grpc.health.v1.Health/Check",
        base_url.trim_end_matches('/')
    );
    let Ok(response) = client
        .post(url)
        .header("content-type", "application/grpc")
        .header("te", "trailers")
        .body(request)
        .send()
        .await
    else {
        return false;
    };
    if !response.status().is_success() || response.version() != http::Version::HTTP_2 {
        return false;
    }
    let response: http::Response<reqwest::Body> = response.into();
    let (parts, mut body) = response.into_parts();
    let mut status = parts
        .headers
        .get("grpc-status")
        .and_then(|value| value.to_str().ok())
        .map(str::to_owned);
    let mut bytes = Vec::new();
    while let Some(frame) = body.frame().await {
        let Ok(frame) = frame else {
            return false;
        };
        if let Some(data) = frame.data_ref() {
            if bytes.len().saturating_add(data.len()) > MAX_RESPONSE_BYTES {
                return false;
            }
            bytes.extend_from_slice(data);
        }
        if let Some(trailers) = frame.trailers_ref() {
            status = trailers
                .get("grpc-status")
                .and_then(|value| value.to_str().ok())
                .map(str::to_owned);
        }
    }
    status.as_deref() == Some("0") && serving(&bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encodes_requests_and_requires_a_serving_response() {
        assert_eq!(request_body("").unwrap(), [0, 0, 0, 0, 0]);
        assert_eq!(
            request_body("foo").unwrap(),
            [0, 0, 0, 0, 5, 10, 3, b'f', b'o', b'o']
        );
        assert!(serving(&[0, 0, 0, 0, 2, 8, 1]));
        assert!(!serving(&[0, 0, 0, 0, 2, 8, 2]));
        assert!(!serving(&[0, 0, 0, 0, 0]));
        assert!(!serving(&[1, 0, 0, 0, 2, 8, 1]));
        assert!(!serving(&[0, 0, 0, 0, 3, 8, 1]));
    }
}
