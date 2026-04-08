use std::net::IpAddr;
use std::sync::OnceLock;

/// A parsed CIDR block (e.g. "10.0.0.0/8") or a single IP address.
#[derive(Debug, Clone)]
enum TrustedEntry {
    Single(IpAddr),
    CidrV4 { network: u32, prefix_len: u8 },
    CidrV6 { network: u128, prefix_len: u8 },
}

impl TrustedEntry {
    fn contains(&self, ip: &IpAddr) -> bool {
        match (self, ip) {
            (TrustedEntry::Single(trusted), candidate) => trusted == candidate,
            (
                TrustedEntry::CidrV4 {
                    network,
                    prefix_len,
                },
                IpAddr::V4(v4),
            ) => {
                let candidate = u32::from(*v4);
                let mask = if *prefix_len == 0 {
                    0
                } else {
                    !0u32 << (32 - prefix_len)
                };
                (candidate & mask) == (*network & mask)
            }
            (
                TrustedEntry::CidrV6 {
                    network,
                    prefix_len,
                },
                IpAddr::V6(v6),
            ) => {
                let candidate = u128::from(*v6);
                let mask = if *prefix_len == 0 {
                    0
                } else {
                    !0u128 << (128 - prefix_len)
                };
                (candidate & mask) == (*network & mask)
            }
            _ => false, // v4 entry vs v6 candidate or vice versa
        }
    }
}

fn parse_entry(s: &str) -> Option<TrustedEntry> {
    let s = s.trim();
    if s.is_empty() {
        return None;
    }
    if let Some((addr_str, prefix_str)) = s.split_once('/') {
        let prefix_len: u8 = prefix_str.parse().ok()?;
        let addr: IpAddr = addr_str.parse().ok()?;
        match addr {
            IpAddr::V4(v4) => {
                if prefix_len > 32 {
                    return None;
                }
                Some(TrustedEntry::CidrV4 {
                    network: u32::from(v4),
                    prefix_len,
                })
            }
            IpAddr::V6(v6) => {
                if prefix_len > 128 {
                    return None;
                }
                Some(TrustedEntry::CidrV6 {
                    network: u128::from(v6),
                    prefix_len,
                })
            }
        }
    } else {
        let addr: IpAddr = s.parse().ok()?;
        Some(TrustedEntry::Single(addr))
    }
}

static TRUSTED_PROXIES: OnceLock<Vec<TrustedEntry>> = OnceLock::new();

fn trusted_proxies() -> &'static Vec<TrustedEntry> {
    TRUSTED_PROXIES.get_or_init(|| {
        let raw = std::env::var("TRUSTED_PROXIES").unwrap_or_default();
        let entries: Vec<TrustedEntry> = raw.split(',').filter_map(parse_entry).collect();
        if !entries.is_empty() {
            tracing::info!(
                "Trusted proxies configured: {} entries from TRUSTED_PROXIES",
                entries.len()
            );
        }
        entries
    })
}

/// Returns `true` if the given IP belongs to a configured trusted proxy.
///
/// When `TRUSTED_PROXIES` is not set or empty, no IP is trusted —
/// forwarded headers will be ignored and the peer address is used directly.
pub fn is_trusted_proxy(ip: &IpAddr) -> bool {
    let proxies = trusted_proxies();
    proxies.iter().any(|entry| entry.contains(ip))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::{Ipv4Addr, Ipv6Addr};

    #[test]
    fn test_single_ip() {
        let entry = parse_entry("10.0.0.1").unwrap();
        assert!(entry.contains(&IpAddr::V4(Ipv4Addr::new(10, 0, 0, 1))));
        assert!(!entry.contains(&IpAddr::V4(Ipv4Addr::new(10, 0, 0, 2))));
    }

    #[test]
    fn test_cidr_v4() {
        let entry = parse_entry("10.0.0.0/8").unwrap();
        assert!(entry.contains(&IpAddr::V4(Ipv4Addr::new(10, 1, 2, 3))));
        assert!(!entry.contains(&IpAddr::V4(Ipv4Addr::new(11, 0, 0, 1))));
    }

    #[test]
    fn test_cidr_v6() {
        let entry = parse_entry("fd00::/8").unwrap();
        assert!(entry.contains(&IpAddr::V6(Ipv6Addr::new(0xfd00, 0, 0, 0, 0, 0, 0, 1))));
        assert!(!entry.contains(&IpAddr::V6(Ipv6Addr::new(0xfe00, 0, 0, 0, 0, 0, 0, 1))));
    }

    #[test]
    fn test_loopback() {
        let entry = parse_entry("127.0.0.1").unwrap();
        assert!(entry.contains(&IpAddr::V4(Ipv4Addr::LOCALHOST)));
    }
}
