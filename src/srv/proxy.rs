//! Where a listener accepts forwarding headers from.
//!
//! A listener trusts an explicit list of sources and nothing by default: an
//! internal client that calls Stargate directly cannot choose its own address
//! in logs, limits, or audit. The list is modelled as sources so a proxy
//! certificate can become a second variant without changing listeners or the
//! ingress.

use std::net::IpAddr;

/// One source a forwarding header may come from.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ProxySource {
    /// A single address or a CIDR block (`10.42.0.0/16`).
    Network(Network),
}

impl ProxySource {
    fn contains(&self, ip: IpAddr) -> bool {
        match self {
            Self::Network(network) => network.contains(ip),
        }
    }
}

/// A single address (`10.0.0.1`, treated as a full-length prefix) or a CIDR
/// block.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct Network {
    address: IpAddr,
    prefix: u8,
}

impl Network {
    fn parse(entry: &str) -> Option<Self> {
        let (address, prefix) = match entry.split_once('/') {
            None => (entry.parse::<IpAddr>().ok()?, None),
            Some((address, prefix)) => (address.parse::<IpAddr>().ok()?, Some(prefix)),
        };
        let maximum = if address.is_ipv4() { 32 } else { 128 };
        let prefix = match prefix {
            None => maximum,
            Some(prefix) => prefix
                .parse::<u8>()
                .ok()
                .filter(|prefix| *prefix <= maximum)?,
        };
        Some(Self { address, prefix })
    }

    fn contains(&self, ip: IpAddr) -> bool {
        match (self.address, ip) {
            (IpAddr::V4(network), IpAddr::V4(candidate)) => {
                let mask = mask_v4(self.prefix);
                (u32::from(network) & mask) == (u32::from(candidate) & mask)
            }
            (IpAddr::V6(network), IpAddr::V6(candidate)) => {
                let mask = mask_v6(self.prefix);
                (u128::from(network) & mask) == (u128::from(candidate) & mask)
            }
            _ => false,
        }
    }
}

fn mask_v4(prefix: u8) -> u32 {
    if prefix == 0 {
        0
    } else {
        u32::MAX << (32 - u32::from(prefix))
    }
}

fn mask_v6(prefix: u8) -> u128 {
    if prefix == 0 {
        0
    } else {
        u128::MAX << (128 - u32::from(prefix))
    }
}

/// The forwarding-header sources of one listener. Empty means no forwarding
/// header is accepted from anyone.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ProxyTrust {
    sources: Vec<ProxySource>,
}

impl ProxyTrust {
    /// Parses a comma-separated list of addresses and CIDR blocks. An invalid
    /// entry is an error, never silently dropped: the process must not start
    /// with a trust boundary narrower or wider than the operator wrote.
    pub fn parse(raw: &str) -> Result<Self, String> {
        let mut sources = Vec::new();
        for entry in raw
            .split(',')
            .map(str::trim)
            .filter(|entry| !entry.is_empty())
        {
            let network = Network::parse(entry)
                .ok_or_else(|| format!("invalid address or CIDR block `{entry}`"))?;
            sources.push(ProxySource::Network(network));
        }
        Ok(Self { sources })
    }

    /// Whether `ip` may set forwarding headers on this listener.
    pub fn trusts(&self, ip: IpAddr) -> bool {
        self.sources.iter().any(|source| source.contains(ip))
    }

    /// Number of configured sources, for the startup log.
    pub fn source_count(&self) -> usize {
        self.sources.len()
    }
}

#[cfg(test)]
mod tests {
    use super::ProxyTrust;
    use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

    fn ip(value: &str) -> IpAddr {
        value.parse().unwrap()
    }

    #[test]
    fn an_empty_set_trusts_nobody() {
        for raw in ["", "  ", ", ,"] {
            let trust = ProxyTrust::parse(raw).unwrap();
            assert_eq!(trust.source_count(), 0);
            assert!(!trust.trusts(IpAddr::V4(Ipv4Addr::LOCALHOST)));
        }
    }

    #[test]
    fn addresses_and_networks_are_matched() {
        let trust = ProxyTrust::parse("127.0.0.1, 10.42.0.0/16, fd00::/8").unwrap();
        assert_eq!(trust.source_count(), 3);
        assert!(trust.trusts(ip("127.0.0.1")));
        assert!(!trust.trusts(ip("127.0.0.2")));
        assert!(trust.trusts(ip("10.42.9.8")));
        assert!(!trust.trusts(ip("10.43.0.1")));
        assert!(trust.trusts(ip("fd00::1")));
        assert!(!trust.trusts(ip("fe80::1")));
        // A network of one family never matches the other family.
        assert!(!trust.trusts(ip("::ffff:10.42.0.1")));
    }

    #[test]
    fn a_zero_prefix_covers_the_whole_family() {
        let trust = ProxyTrust::parse("0.0.0.0/0,::/0").unwrap();
        assert!(trust.trusts(IpAddr::V4(Ipv4Addr::new(203, 0, 113, 1))));
        assert!(trust.trusts(IpAddr::V6(Ipv6Addr::LOCALHOST)));
    }

    #[test]
    fn invalid_entries_stop_the_list_from_being_used() {
        for raw in [
            "not-an-ip",
            "10.0.0.0/33",
            "::/129",
            "10.0.0.0/",
            "10.0.0.0/-1",
            "127.0.0.1, broken",
            "http://10.0.0.1",
        ] {
            assert!(ProxyTrust::parse(raw).is_err(), "{raw}");
        }
    }
}
