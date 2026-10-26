mod material;
mod settings;
mod signing;

use ctx::{ContextSigner, IssueRequest, MAX_CLOCK_SKEW_SECS, MAX_KEY_ID_BYTES, SignerConfig};
#[cfg(test)]
use ctx::{IssueError, IssuedContext};
use gate::cfg::RuntimeConfig;
use material::{parse_private_key, validate_and_sanitize_jwks};
use rsa::traits::PublicKeyParts;
use serde_json::Value;
use settings::{ALGORITHM_ENV, CLOCK_SKEW_ENV, RuntimeSettings, load_from_env};
#[cfg(test)]
use settings::{DEFAULT_CACHE_MAX_AGE_SECS, DEFAULT_CLOCK_SKEW_SECS};
pub(crate) use signing::QueueError as SigningQueueError;
use std::{
    path::PathBuf,
    sync::{Arc, OnceLock},
};

static RUNTIME: OnceLock<Option<Arc<InternalContextRuntime>>> = OnceLock::new();

#[derive(Debug, thiserror::Error)]
pub enum InternalContextError {
    #[error("internal-context runtime has already been initialized")]
    AlreadyInitialized,
    #[error("{0} is required when internal context is configured")]
    MissingSetting(&'static str),
    #[error("{ALGORITHM_ENV} must be RS256 for version 1")]
    UnsupportedAlgorithm,
    #[error("{name} must be an unsigned integer")]
    InvalidInteger { name: &'static str },
    #[error("{CLOCK_SKEW_ENV} must not exceed {MAX_CLOCK_SKEW_SECS} seconds")]
    InvalidClockSkew,
    #[error("unable to read internal-context {kind} file '{path}': {source}")]
    ReadFile {
        kind: &'static str,
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("invalid internal-context signer configuration: {0}")]
    Signer(#[source] ctx::ConfigError),
    #[error("internal-context JWKS must be a JSON object containing a nonempty 'keys' array")]
    InvalidJwksShape,
    #[error("internal-context JWKS key {index} must be a public RS256 RSA JWK")]
    InvalidJwk { index: usize },
    #[error("internal-context JWKS key {index} contains private key material")]
    PrivateJwkMaterial { index: usize },
    #[error(
        "internal-context JWKS key ids must be nonblank, printable ASCII, and at most {MAX_KEY_ID_BYTES} bytes"
    )]
    InvalidKeyId,
    #[error("internal-context JWKS contains duplicate kid '{0}'")]
    DuplicateKeyId(String),
    #[error("internal-context JWKS does not contain the active kid '{0}'")]
    MissingActiveKey(String),
    #[error("the active internal-context public JWK does not match the private signing key")]
    KeyPairMismatch,
    #[error("internal context is not configured")]
    NotConfigured,
    #[error("{name} must be between 1 and {maximum}")]
    InvalidSigningBudget { name: &'static str, maximum: u64 },
}

#[derive(Debug)]
pub struct InternalContextRuntime {
    signer: Arc<ContextSigner>,
    signing_settings: signing::Settings,
    signing_workers: tokio::sync::OnceCell<signing::SigningWorkers>,
    public_jwks: Value,
    cache_max_age_secs: u64,
    _clock_skew_secs: u64,
}

impl InternalContextRuntime {
    #[cfg(test)]
    pub fn issue(&self, request: &IssueRequest) -> Result<IssuedContext, IssueError> {
        self.signer.issue(request)
    }

    pub(crate) async fn issue_async(
        &self,
        request: IssueRequest,
    ) -> Result<signing::Signed, SigningQueueError> {
        let workers = self
            .signing_workers
            .get_or_try_init(|| async {
                let signer = self.signer.clone();
                signing::SigningWorkers::start(self.signing_settings, move |request| {
                    signer.issue(request)
                })
                .map_err(|error| {
                    tracing::error!(%error, "Unable to start internal-context signing workers");
                    SigningQueueError::Unavailable
                })
            })
            .await?;
        workers.issue(request).await
    }

    pub fn public_jwks(&self) -> &Value {
        &self.public_jwks
    }

    pub const fn cache_max_age_secs(&self) -> u64 {
        self.cache_max_age_secs
    }

    fn from_material(
        settings: RuntimeSettings,
        private_key_pem: &[u8],
        jwks_json: &[u8],
    ) -> Result<Self, InternalContextError> {
        if settings.clock_skew_secs > MAX_CLOCK_SKEW_SECS {
            return Err(InternalContextError::InvalidClockSkew);
        }

        let signer_config =
            SignerConfig::new(settings.issuer, settings.key_id.clone(), settings.ttl_secs)
                .map_err(InternalContextError::Signer)?;
        let signer = ContextSigner::from_rsa_pem(signer_config, private_key_pem)
            .map_err(InternalContextError::Signer)?;
        let private_key = parse_private_key(private_key_pem)?;
        let public_jwks = validate_and_sanitize_jwks(
            jwks_json,
            &settings.key_id,
            private_key.n(),
            private_key.e(),
        )?;

        Ok(Self {
            signer: Arc::new(signer),
            signing_settings: settings.signing,
            signing_workers: tokio::sync::OnceCell::new(),
            public_jwks,
            cache_max_age_secs: settings.cache_max_age_secs,
            _clock_skew_secs: settings.clock_skew_secs,
        })
    }

    #[cfg(test)]
    pub(crate) fn from_signer_for_test(signer: ContextSigner) -> Arc<Self> {
        Arc::new(Self {
            signer: Arc::new(signer),
            signing_settings: signing::Settings::default(),
            signing_workers: tokio::sync::OnceCell::new(),
            public_jwks: serde_json::json!({ "keys": [] }),
            cache_max_age_secs: DEFAULT_CACHE_MAX_AGE_SECS,
            _clock_skew_secs: DEFAULT_CLOCK_SKEW_SECS,
        })
    }
}

pub fn init() -> Result<(), InternalContextError> {
    let runtime = load_from_env()?;
    RUNTIME
        .set(runtime)
        .map_err(|_| InternalContextError::AlreadyInitialized)
}

pub fn runtime() -> Option<&'static Arc<InternalContextRuntime>> {
    RUNTIME.get().and_then(Option::as_ref)
}

pub fn preflight_config(config: &RuntimeConfig) -> Result<(), InternalContextError> {
    preflight_config_with(config, load_from_env)
}

fn preflight_config_with(
    config: &RuntimeConfig,
    load: impl FnOnce() -> Result<Option<Arc<InternalContextRuntime>>, InternalContextError>,
) -> Result<(), InternalContextError> {
    let requires_internal_context = config
        .compiled()
        .http
        .upstreams
        .values()
        .any(|upstream| upstream.internal_context.is_some());
    if !requires_internal_context {
        return Ok(());
    }

    load()?.ok_or(InternalContextError::NotConfigured)?;
    Ok(())
}

#[cfg(test)]
pub(crate) mod test_support;
#[cfg(test)]
mod tests;
