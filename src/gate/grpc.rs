//! gRPC request recognition and gateway-owned error responses.

use crate::err::ErrorResponse;
use axum::{
    body::Body,
    response::{IntoResponse, Response},
};
use http::{HeaderMap, HeaderValue, StatusCode, header::CONTENT_TYPE};
use std::{
    fmt::Write,
    pin::Pin,
    task::{Context, Poll},
    time::Duration,
};

pub(super) fn is_grpc_request(headers: &HeaderMap) -> bool {
    headers
        .get(CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| {
            let value = value.to_ascii_lowercase();
            value.starts_with("application/grpc") && !value.starts_with("application/grpc-web")
        })
}

pub(super) fn timeout(headers: &HeaderMap) -> Result<Option<Duration>, ErrorResponse> {
    let mut values = headers.get_all("grpc-timeout").iter();
    let Some(value) = values.next() else {
        return Ok(None);
    };
    let invalid = || {
        ErrorResponse::new(crate::err::ErrorCode::RequestInvalid)
            .with_message("Invalid grpc-timeout")
    };
    if values.next().is_some() {
        return Err(invalid());
    }
    let value = value.to_str().map_err(|_| invalid())?;
    let (unit, digits) = value.as_bytes().split_last().ok_or_else(invalid)?;
    if digits.is_empty() || digits.len() > 8 || !digits.iter().all(|byte| byte.is_ascii_digit()) {
        return Err(invalid());
    }
    let amount: u64 = std::str::from_utf8(digits)
        .map_err(|_| invalid())?
        .parse()
        .map_err(|_| invalid())?;
    let duration = match unit {
        b'H' => Duration::from_secs(amount * 3600),
        b'M' => Duration::from_secs(amount * 60),
        b'S' => Duration::from_secs(amount),
        b'm' => Duration::from_millis(amount),
        b'u' => Duration::from_micros(amount),
        b'n' => Duration::from_nanos(amount),
        _ => return Err(invalid()),
    };
    Ok(Some(duration))
}

fn format_remaining_timeout(duration: Duration) -> String {
    let nanos = duration.as_nanos();
    for (unit, divisor) in [
        ("m", 1_000_000u128),
        ("S", 1_000_000_000),
        ("M", 60_000_000_000),
        ("H", 3_600_000_000_000),
    ] {
        let value = nanos.div_ceil(divisor);
        if value <= 99_999_999 {
            return format!("{value}{unit}");
        }
    }
    "99999999H".to_string()
}

pub(in crate::gate) fn forward_timeout(
    headers: &mut HeaderMap,
    execution: &super::lifecycle::Execution,
) -> Result<(), ErrorResponse> {
    execution.check()?;
    match execution.deadline {
        Some(deadline) => {
            let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
            if remaining.is_zero() {
                return Err(execution.timeout("total"));
            }
            let value = format_remaining_timeout(remaining);
            headers.insert(
                "grpc-timeout",
                HeaderValue::from_str(&value).expect("formatted gRPC timeout is a valid header"),
            );
        }
        None => {
            headers.remove("grpc-timeout");
        }
    }
    Ok(())
}

pub(super) fn status_for_http(status: StatusCode) -> u8 {
    match status.as_u16() {
        400 => 3,
        401 => 16,
        403 => 7,
        404 => 12,
        408 | 504 => 4,
        413 | 429 => 8,
        502 | 503 => 14,
        _ => 13,
    }
}

fn parsed_status(headers: &HeaderMap) -> Option<u8> {
    let status = headers
        .get("grpc-status")?
        .to_str()
        .ok()?
        .parse::<u8>()
        .ok()?;
    (status <= 16).then_some(status)
}

pub(super) fn observe_response(
    mut response: Response,
    router: String,
    service: String,
) -> Response {
    if let Some(status) = parsed_status(response.headers()) {
        crate::obs::telemetry::record_gateway_grpc_response(&router, &service, status);
    } else {
        let body = std::mem::replace(response.body_mut(), Body::empty());
        *response.body_mut() = Body::new(ObservedBody {
            body,
            router,
            service,
            recorded: false,
        });
    }
    response
}

struct ObservedBody {
    body: Body,
    router: String,
    service: String,
    recorded: bool,
}

impl hyper::body::Body for ObservedBody {
    type Data = hyper::body::Bytes;
    type Error = axum::Error;

    fn poll_frame(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Option<Result<hyper::body::Frame<Self::Data>, Self::Error>>> {
        let result = Pin::new(&mut self.body).poll_frame(cx);
        if !self.recorded
            && let Poll::Ready(Some(Ok(frame))) = &result
            && let Some(status) = frame.trailers_ref().and_then(parsed_status)
        {
            crate::obs::telemetry::record_gateway_grpc_response(
                &self.router,
                &self.service,
                status,
            );
            self.recorded = true;
        }
        result
    }

    fn is_end_stream(&self) -> bool {
        self.body.is_end_stream()
    }
    fn size_hint(&self) -> hyper::body::SizeHint {
        self.body.size_hint()
    }
}

fn encode_message(message: &str) -> String {
    let mut encoded = String::new();
    for byte in message.bytes() {
        if (0x20..=0x7e).contains(&byte) && byte != b'%' {
            encoded.push(char::from(byte));
        } else {
            write!(encoded, "%{byte:02X}").expect("writing to a string cannot fail");
        }
    }
    encoded
}

pub(super) fn error_response(error: ErrorResponse) -> Response {
    let grpc_status = status_for_http(error.status);
    let message = encode_message(&error.message);
    let mut response = error.into_response();
    *response.status_mut() = StatusCode::OK;
    *response.body_mut() = Body::empty();
    let headers = response.headers_mut();
    headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/grpc"));
    headers.insert("grpc-status", HeaderValue::from(u16::from(grpc_status)));
    headers.insert(
        "grpc-message",
        HeaderValue::from_str(&message).expect("encoded gRPC message is a valid header"),
    );
    response
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::err::ErrorCode;

    #[test]
    fn recognizes_grpc_without_treating_grpc_web_as_grpc() {
        for (value, expected) in [
            ("application/grpc", true),
            ("application/grpc+proto", true),
            ("application/grpc-web", false),
            ("application/grpc-web+proto", false),
            ("application/json", false),
        ] {
            let mut headers = HeaderMap::new();
            headers.insert(CONTENT_TYPE, HeaderValue::from_str(value).unwrap());
            assert_eq!(is_grpc_request(&headers), expected, "{value}");
        }
    }

    #[test]
    fn maps_gateway_errors_and_percent_encodes_messages() {
        for (status, expected) in [
            (400, 3),
            (401, 16),
            (403, 7),
            (404, 12),
            (408, 4),
            (504, 4),
            (413, 8),
            (429, 8),
            (502, 14),
            (503, 14),
            (500, 13),
        ] {
            assert_eq!(
                status_for_http(StatusCode::from_u16(status).unwrap()),
                expected
            );
        }
        assert_eq!(
            encode_message("test 100% café\n"),
            "test 100%25 caf%C3%A9%0A"
        );
        let mut error = ErrorResponse::new(ErrorCode::GatewayIngressRateLimitExceeded);
        error
            .insert_header("retry-after", "10")
            .insert_header("x-ratelimit-scope", "ingress");
        let response = error_response(error);
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response.headers()["grpc-status"], "8");
        assert_eq!(response.headers()["retry-after"], "10");
        assert_eq!(response.headers()["x-ratelimit-scope"], "ingress");
    }

    #[test]
    fn parses_grpc_timeout_units_and_rejects_invalid_values() {
        for (value, expected) in [
            ("2H", Duration::from_secs(7200)),
            ("3M", Duration::from_secs(180)),
            ("5S", Duration::from_secs(5)),
            ("100m", Duration::from_millis(100)),
            ("12u", Duration::from_micros(12)),
            ("7n", Duration::from_nanos(7)),
            ("0n", Duration::ZERO),
        ] {
            let mut headers = HeaderMap::new();
            headers.insert("grpc-timeout", HeaderValue::from_str(value).unwrap());
            assert_eq!(timeout(&headers).unwrap(), Some(expected));
        }
        for value in ["", "1", "m", "123456789m", "1s", "1.5S", "-1S", " 1S"] {
            let mut headers = HeaderMap::new();
            headers.insert("grpc-timeout", HeaderValue::from_str(value).unwrap());
            assert!(timeout(&headers).is_err(), "{value}");
        }
    }

    #[test]
    fn formats_remaining_time_without_exceeding_eight_digits() {
        assert_eq!(format_remaining_timeout(Duration::from_nanos(1)), "1m");
        assert_eq!(format_remaining_timeout(Duration::from_millis(100)), "100m");
        assert_eq!(
            format_remaining_timeout(Duration::from_millis(100_000_001)),
            "100001S"
        );
        assert_eq!(
            format_remaining_timeout(Duration::from_secs(100_000_000)),
            "1666667M"
        );
        assert_eq!(
            format_remaining_timeout(Duration::from_secs(6_000_000_000)),
            "1666667H"
        );
    }

    #[test]
    fn removes_forwarded_timeout_when_there_is_no_deadline() {
        let mut execution = super::super::lifecycle::Execution::new(
            Duration::from_secs(1),
            tokio_util::sync::CancellationToken::new(),
        );
        execution.deadline = None;
        let mut headers = HeaderMap::new();
        headers.insert("grpc-timeout", HeaderValue::from_static("1S"));
        forward_timeout(&mut headers, &execution).unwrap();
        assert!(!headers.contains_key("grpc-timeout"));
    }

    #[test]
    fn observed_status_is_bounded_to_grpc_codes() {
        let mut headers = HeaderMap::new();
        for status in 0..=16 {
            headers.insert(
                "grpc-status",
                HeaderValue::from_str(&status.to_string()).unwrap(),
            );
            assert_eq!(parsed_status(&headers), Some(status));
        }
        for value in ["17", "-1", "bad"] {
            headers.insert("grpc-status", HeaderValue::from_str(value).unwrap());
            assert_eq!(parsed_status(&headers), None);
        }
    }
}
