//! Process-wide host selection for IAM routes.

use std::sync::OnceLock;
use tools::valid_hostname;

#[derive(Clone, Debug, Default)]
pub struct IamHosts {
    names: Vec<String>,
}

impl IamHosts {
    pub(crate) fn parse(value: Option<&str>, public_base_url: &str) -> anyhow::Result<Self> {
        let Some(value) = value.map(str::trim).filter(|value| !value.is_empty()) else {
            return Ok(Self::default());
        };

        let mut names = Vec::new();
        for name in value.split(',') {
            let name = name.trim().to_ascii_lowercase();
            anyhow::ensure!(
                valid_hostname(&name),
                "IAM_HOSTS entries must be hostnames without a scheme, port, path, or wildcard"
            );
            if !names.contains(&name) {
                names.push(name);
            }
        }

        let public_url = url::Url::parse(public_base_url)
            .map_err(|_| anyhow::anyhow!("IAM_HOSTS requires a valid public base URL"))?;
        let public_host = public_url
            .host_str()
            .ok_or_else(|| anyhow::anyhow!("IAM_HOSTS requires a public base URL with a host"))?;
        anyhow::ensure!(
            names.iter().any(|name| name == public_host),
            "IAM_HOSTS must include the public base URL host ({public_host})"
        );
        Ok(Self { names })
    }

    /// An empty list preserves IAM routing on every host, including hostless requests.
    pub(crate) fn allows(&self, host: Option<&str>) -> bool {
        self.names.is_empty() || host.is_some_and(|host| self.names.iter().any(|name| name == host))
    }
}

static IAM_HOSTS: OnceLock<IamHosts> = OnceLock::new();

/// Validate before opening the listener; requests use only the stored setting.
pub fn init() -> anyhow::Result<&'static IamHosts> {
    if let Some(hosts) = IAM_HOSTS.get() {
        return Ok(hosts);
    }
    let value = match std::env::var("IAM_HOSTS") {
        Ok(value) => Some(value),
        Err(std::env::VarError::NotPresent) => None,
        Err(error) => return Err(error.into()),
    };
    let hosts = IamHosts::parse(value.as_deref(), &super::public_base_url())?;
    Ok(IAM_HOSTS.get_or_init(|| hosts))
}

pub fn iam_hosts() -> &'static IamHosts {
    IAM_HOSTS.get().expect("IAM hosts are validated at startup")
}

#[cfg(test)]
mod tests {
    use super::IamHosts;

    const PUBLIC_URL: &str = "https://identity.example.com:8443/identity";

    #[test]
    fn absent_or_empty_hosts_allow_every_host() {
        for value in [None, Some(""), Some("  ")] {
            let hosts = IamHosts::parse(value, PUBLIC_URL).unwrap();
            for host in [None, Some("identity.example.com"), Some("api.example.com")] {
                assert!(hosts.allows(host));
            }
        }
    }

    #[test]
    fn hosts_are_trimmed_lowercased_and_deduplicated() {
        let hosts = IamHosts::parse(
            Some("Identity.Example.com, admin.example.com , identity.example.com"),
            PUBLIC_URL,
        )
        .unwrap();
        assert_eq!(hosts.names, ["identity.example.com", "admin.example.com"]);
        assert!(hosts.allows(Some("identity.example.com")));
        assert!(hosts.allows(Some("admin.example.com")));
        assert!(!hosts.allows(Some("api.example.com")));
        assert!(!hosts.allows(None));
    }

    #[test]
    fn invalid_host_entries_are_rejected() {
        for name in [
            "https://identity.example.com",
            "identity.example.com:443",
            "identity.example.com/identity",
            "*.example.com",
            "identity.example.com?query",
            "identity.example.com#fragment",
            "user@identity.example.com",
            "identity example.com",
            "identity..example.com",
            "-identity.example.com",
            "identity-.example.com",
            "identity_example.com",
            ",identity.example.com",
            "identity.example.com,",
            "identity.example.com,,admin.example.com",
        ] {
            assert!(IamHosts::parse(Some(name), PUBLIC_URL).is_err(), "{name}");
        }
        let long_label = format!("{}.example.com", "x".repeat(64));
        assert!(IamHosts::parse(Some(&long_label), PUBLIC_URL).is_err());
    }

    #[test]
    fn the_public_base_url_host_is_required() {
        let error = IamHosts::parse(Some("admin.example.com"), PUBLIC_URL).unwrap_err();
        assert!(error.to_string().contains("public base URL host"));
        for url in ["not a URL", "mailto:admin@example.com"] {
            assert!(IamHosts::parse(Some("identity.example.com"), url).is_err());
        }
        assert!(IamHosts::parse(Some("localhost"), "http://localhost:5050").is_ok());
    }
}
