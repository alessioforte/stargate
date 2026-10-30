#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::gate) enum QueryMode {
    Tolerant,
    Strict,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::gate) struct QuerySanitizationError;

pub(in crate::gate) fn sanitize_auth_uri(
    uri: &http::Uri,
    mode: QueryMode,
) -> Result<http::Uri, QuerySanitizationError> {
    let Some(path_and_query) = uri.path_and_query() else {
        return Ok(uri.clone());
    };
    let Some(query) = path_and_query.query() else {
        return Ok(uri.clone());
    };
    let query = sanitize_auth_query(query, mode)?;
    let next = if query.is_empty() {
        path_and_query.path().to_owned()
    } else {
        format!("{}?{}", path_and_query.path(), query)
    };

    let mut parts = uri.clone().into_parts();
    parts.path_and_query = Some(
        next.parse::<http::uri::PathAndQuery>()
            .map_err(|_| QuerySanitizationError)?,
    );
    http::Uri::from_parts(parts).map_err(|_| QuerySanitizationError)
}

fn sanitize_auth_query(query: &str, mode: QueryMode) -> Result<String, QuerySanitizationError> {
    let mut retained = Vec::new();
    for pair in query.split('&') {
        let key = pair.split_once('=').map_or(pair, |(key, _)| key);
        match is_auth_query_key(key) {
            Ok(true) => {}
            Ok(false) => retained.push(pair),
            Err(_) if mode == QueryMode::Tolerant => retained.push(pair),
            Err(error) => return Err(error),
        }
    }
    Ok(retained.join("&"))
}

fn is_auth_query_key(key: &str) -> Result<bool, QuerySanitizationError> {
    let mut decoded = Vec::with_capacity(key.len());
    let bytes = key.as_bytes();
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] != b'%' {
            decoded.push(bytes[index]);
            index += 1;
            continue;
        }
        if index + 2 >= bytes.len() {
            return Err(QuerySanitizationError);
        }
        let high = hex_value(bytes[index + 1]).ok_or(QuerySanitizationError)?;
        let low = hex_value(bytes[index + 2]).ok_or(QuerySanitizationError)?;
        decoded.push((high << 4) | low);
        index += 3;
    }

    Ok(crate::web::request::TOKEN_QUERY_KEYS
        .iter()
        .any(|key| decoded == key.as_bytes()))
}

fn hex_value(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

#[cfg(test)]
mod tests;
