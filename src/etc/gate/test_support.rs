use gate::cfg::{Config, MtlsConfig, SCHEMA};
use std::{fs, path::PathBuf};

pub(crate) const CA: &[u8] = include_bytes!("fixtures/ca.pem");
pub(crate) const CLIENT_CERT: &[u8] = include_bytes!("fixtures/client.pem");
pub(crate) const CLIENT_KEY: &[u8] = include_bytes!("fixtures/client-key.pem");
pub(crate) const SERVER_CERT: &[u8] = include_bytes!("fixtures/server.pem");
pub(crate) const SERVER_KEY: &[u8] = include_bytes!("fixtures/server-key.pem");

pub(crate) struct TlsFiles {
    pub(crate) directory: PathBuf,
}

impl TlsFiles {
    pub(crate) fn new() -> Self {
        let directory =
            std::env::temp_dir().join(format!("stargate-transport-{}", ulid::Ulid::generate()));
        fs::create_dir(&directory).unwrap();
        let files = Self { directory };
        files.write("ca.pem", CA);
        files.write("client.pem", CLIENT_CERT);
        files.write("client-key.pem", CLIENT_KEY);
        files
    }

    pub(crate) fn path(&self, name: &str) -> String {
        self.directory.join(name).to_str().unwrap().to_string()
    }

    pub(crate) fn write(&self, name: &str, data: &[u8]) -> String {
        let path = self.path(name);
        fs::write(&path, data).unwrap();
        path
    }

    pub(crate) fn mtls(&self) -> MtlsConfig {
        MtlsConfig {
            ca_cert_path: self.path("ca.pem"),
            client_cert_path: self.path("client.pem"),
            client_key_path: self.path("client-key.pem"),
        }
    }

    pub(crate) fn config(&self, url: &str) -> Config {
        serde_json::from_value(serde_json::json!({
            "schema": SCHEMA,
            "mtls": self.mtls(),
            "http": {
                "upstreams": { "secure": { "targets": [{ "url": url }] } },
                "services": { "secure": { "kind": "load_balancer", "upstream": "secure" } },
                "routers": { "secure": { "match": { "path": { "prefix": "/" } }, "service": "secure" } },
            },
        })).unwrap()
    }
}

impl Drop for TlsFiles {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.directory);
    }
}
