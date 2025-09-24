use std::net::IpAddr;

/// Extract IP address from common header values or connection info
/// This is a helper function for web frameworks integration
pub fn extract_client_ip(
    x_forwarded_for: Option<&str>,
    x_real_ip: Option<&str>,
    remote_addr: Option<&str>,
) -> Option<IpAddr> {
    // Try X-Forwarded-For first (may contain multiple IPs, take the first)
    if let Some(xff) = x_forwarded_for {
        if let Some(first_ip) = xff.split(',').next() {
            if let Ok(ip) = first_ip.trim().parse::<IpAddr>() {
                return Some(ip);
            }
        }
    }

    // Try X-Real-IP
    if let Some(real_ip) = x_real_ip {
        if let Ok(ip) = real_ip.parse::<IpAddr>() {
            return Some(ip);
        }
    }

    // Fall back to remote address
    if let Some(remote) = remote_addr {
        // Handle socket address format (IP:port)
        let ip_str = if remote.contains(':') && !remote.starts_with('[') {
            // IPv4:port format
            remote.split(':').next().unwrap_or(remote)
        } else if remote.starts_with('[') {
            // [IPv6]:port format
            remote
                .trim_start_matches('[')
                .split(']')
                .next()
                .unwrap_or(remote)
        } else {
            remote
        };

        if let Ok(ip) = ip_str.parse::<IpAddr>() {
            return Some(ip);
        }
    }

    None
}

/// Check if an IP address is in a private/local range
/// Useful for applying different rate limits to internal vs external IPs
pub fn is_private_ip(ip: &IpAddr) -> bool {
    match ip {
        IpAddr::V4(ipv4) => ipv4.is_private() || ipv4.is_loopback() || ipv4.is_link_local(),
        IpAddr::V6(ipv6) => {
            ipv6.is_loopback() ||
            ipv6.segments()[0] & 0xfe00 == 0xfc00 || // Unique local addresses
            ipv6.segments()[0] & 0xffc0 == 0xfe80 // Link-local addresses
        }
    }
}

/// Format IP for rate limiting key (same as internal implementation)
pub fn format_ip_key(ip: &IpAddr) -> String {
    format!("ip:{}", ip)
}
