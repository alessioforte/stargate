use std::net::IpAddr;

/// Parsed once when compiling a source-IP matcher, rather than per router scan.
#[derive(Debug, Clone)]
pub struct IpNetwork {
    address: IpAddr,
    prefix: u8,
}

impl IpNetwork {
    pub fn parse(raw: &str) -> Option<Self> {
        let raw = raw.trim();
        let (address, prefix) = if let Some((address, prefix)) = raw.split_once('/') {
            (address.parse::<IpAddr>().ok()?, prefix.parse::<u8>().ok()?)
        } else {
            let address = raw.parse::<IpAddr>().ok()?;
            let prefix = if address.is_ipv4() { 32 } else { 128 };
            (address, prefix)
        };
        let maximum = if address.is_ipv4() { 32 } else { 128 };
        (prefix <= maximum).then_some(Self { address, prefix })
    }

    pub fn contains(&self, candidate: &IpAddr) -> bool {
        match (self.address, candidate) {
            (IpAddr::V4(address), IpAddr::V4(candidate)) => {
                let mask = u32::MAX
                    .checked_shl(u32::from(32 - self.prefix))
                    .unwrap_or(0);
                (u32::from(address) & mask) == (u32::from(*candidate) & mask)
            }
            (IpAddr::V6(address), IpAddr::V6(candidate)) => {
                let mask = u128::MAX
                    .checked_shl(u32::from(128 - self.prefix))
                    .unwrap_or(0);
                (u128::from(address) & mask) == (u128::from(*candidate) & mask)
            }
            _ => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn networks_preserve_family_boundaries_and_bare_address_matching() {
        for (network, included, excluded) in [
            (" 192.0.2.5/24 ", "192.0.2.255", "192.0.3.0"),
            ("0.0.0.0/0", "255.255.255.255", "::ffff:192.0.2.1"),
            ("2001:db8::5/64", "2001:db8::ffff", "2001:db8:0:1::"),
            ("::/0", "ffff::1", "127.0.0.1"),
            ("127.0.0.1", "127.0.0.1", "127.0.0.2"),
            ("::1/128", "::1", "::2"),
        ] {
            let network = IpNetwork::parse(network).unwrap();
            assert!(network.contains(&included.parse().unwrap()));
            assert!(!network.contains(&excluded.parse().unwrap()));
        }
        for invalid in [
            "",
            "not-an-ip",
            "127.0.0.1/33",
            "::1/129",
            "::1/-1",
            "::1/64/64",
            "127.0.0.1 /24",
        ] {
            assert!(IpNetwork::parse(invalid).is_none());
        }
    }
}
