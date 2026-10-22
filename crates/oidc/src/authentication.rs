//! How a login session authenticated its user: the OpenID Connect `amr`
//! (Authentication Methods References, RFC 8176) and `acr` (Authentication
//! Context Class Reference) claims.
//!
//! A session records its methods (`amr`); its assurance level, and so its
//! `acr`, follows from them: `mfa` means multi-factor, anything else single
//! factor. Relying parties ask for a level with `acr_values`.

use serde::{Deserialize, Serialize};

/// Password (RFC 8176 §2).
pub const AMR_PASSWORD: &str = "pwd";
/// One-time password, here an email code (RFC 8176 §2).
pub const AMR_OTP: &str = "otp";
/// More than one factor (RFC 8176 §2).
pub const AMR_MFA: &str = "mfa";
/// Signed in through an external identity provider (Google, GitHub). Not
/// registered by RFC 8176; the value other providers use for federation.
pub const AMR_FEDERATED: &str = "fed";

pub const ACR_SINGLE_FACTOR: &str = "urn:stargate:acr:single-factor";
pub const ACR_MULTI_FACTOR: &str = "urn:stargate:acr:multi-factor";

/// Assurance level of a session, lowest first.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum AssuranceLevel {
    SingleFactor,
    MultiFactor,
}

impl AssuranceLevel {
    pub fn of(amr: &[String]) -> Self {
        if amr.iter().any(|method| method == AMR_MFA) {
            AssuranceLevel::MultiFactor
        } else {
            AssuranceLevel::SingleFactor
        }
    }

    pub fn acr(self) -> &'static str {
        match self {
            AssuranceLevel::SingleFactor => ACR_SINGLE_FACTOR,
            AssuranceLevel::MultiFactor => ACR_MULTI_FACTOR,
        }
    }

    pub fn from_acr(value: &str) -> Option<Self> {
        match value {
            ACR_SINGLE_FACTOR => Some(AssuranceLevel::SingleFactor),
            ACR_MULTI_FACTOR => Some(AssuranceLevel::MultiFactor),
            _ => None,
        }
    }
}

/// `acr_values_supported` for discovery, lowest level first.
pub fn acr_values_supported() -> Vec<String> {
    vec![ACR_SINGLE_FACTOR.to_string(), ACR_MULTI_FACTOR.to_string()]
}

/// The authentication behind issued tokens, kept with authorization codes and
/// refresh-token families so every token of a grant reports the same.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Authentication {
    pub amr: Vec<String>,
    pub acr: String,
}

/// Outcome of comparing `acr_values` with a session's level.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AcrDecision {
    /// Issue tokens with this `acr`.
    Satisfied(&'static str),
    /// Every requested level is above the session's: the user must add a
    /// factor first (or the provider issues its actual level, since
    /// `acr_values` is voluntary).
    StepUp,
}

/// Match space-separated `acr_values` against a session at `level`. Unknown
/// values are ignored. The highest requested level the session reaches is
/// returned, so the `acr` is one the client asked for (OIDC Core 5.5.1.1).
/// Without known values the session's own level is returned.
pub fn decide_acr(acr_values: Option<&str>, level: AssuranceLevel) -> AcrDecision {
    let requested = acr_values
        .unwrap_or_default()
        .split_whitespace()
        .filter_map(AssuranceLevel::from_acr)
        .collect::<Vec<_>>();
    if requested.is_empty() {
        return AcrDecision::Satisfied(level.acr());
    }
    match requested.iter().filter(|wanted| **wanted <= level).max() {
        Some(satisfied) => AcrDecision::Satisfied(satisfied.acr()),
        None => AcrDecision::StepUp,
    }
}

/// Whether an email OTP adds a second factor to a session signed in with
/// `amr`: it does after a password or an external provider, not after an
/// email OTP alone.
pub fn otp_adds_factor(amr: &[String]) -> bool {
    amr.iter()
        .any(|method| method == AMR_PASSWORD || method == AMR_FEDERATED)
}

/// Methods after the user verified an email OTP in the session: `otp`, and
/// `mfa` when that made a second factor.
pub fn with_otp(amr: &[String]) -> Vec<String> {
    let mut methods = amr.to_vec();
    if !methods.iter().any(|method| method == AMR_OTP) {
        methods.push(AMR_OTP.to_string());
    }
    if otp_adds_factor(amr) && !methods.iter().any(|method| method == AMR_MFA) {
        methods.push(AMR_MFA.to_string());
    }
    methods
}

#[cfg(test)]
mod tests {
    use super::*;

    fn amr(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| value.to_string()).collect()
    }

    #[test]
    fn level_follows_mfa() {
        assert_eq!(
            AssuranceLevel::of(&amr(&["pwd"])),
            AssuranceLevel::SingleFactor
        );
        assert_eq!(
            AssuranceLevel::of(&amr(&["otp"])),
            AssuranceLevel::SingleFactor
        );
        assert_eq!(
            AssuranceLevel::of(&amr(&["pwd", "otp", "mfa"])),
            AssuranceLevel::MultiFactor
        );
    }

    #[test]
    fn acr_values_pick_the_highest_reached_level() {
        let single = AssuranceLevel::SingleFactor;
        let multi = AssuranceLevel::MultiFactor;
        let both = format!("{ACR_SINGLE_FACTOR} {ACR_MULTI_FACTOR}");

        assert_eq!(
            decide_acr(None, single),
            AcrDecision::Satisfied(ACR_SINGLE_FACTOR)
        );
        assert_eq!(
            decide_acr(None, multi),
            AcrDecision::Satisfied(ACR_MULTI_FACTOR)
        );
        assert_eq!(
            decide_acr(Some("1 2 urn:other"), multi),
            AcrDecision::Satisfied(ACR_MULTI_FACTOR)
        );
        assert_eq!(
            decide_acr(Some(&both), single),
            AcrDecision::Satisfied(ACR_SINGLE_FACTOR)
        );
        assert_eq!(
            decide_acr(Some(&both), multi),
            AcrDecision::Satisfied(ACR_MULTI_FACTOR)
        );
        assert_eq!(
            decide_acr(Some(ACR_SINGLE_FACTOR), multi),
            AcrDecision::Satisfied(ACR_SINGLE_FACTOR)
        );
        assert_eq!(
            decide_acr(Some(ACR_MULTI_FACTOR), single),
            AcrDecision::StepUp
        );
        assert_eq!(
            decide_acr(Some(ACR_MULTI_FACTOR), multi),
            AcrDecision::Satisfied(ACR_MULTI_FACTOR)
        );
    }

    #[test]
    fn an_email_code_is_a_second_factor_only_after_another_one() {
        assert_eq!(with_otp(&amr(&["pwd"])), amr(&["pwd", "otp", "mfa"]));
        assert_eq!(with_otp(&amr(&["fed"])), amr(&["fed", "otp", "mfa"]));
        assert_eq!(with_otp(&amr(&["otp"])), amr(&["otp"]));
        assert_eq!(
            with_otp(&amr(&["pwd", "otp", "mfa"])),
            amr(&["pwd", "otp", "mfa"])
        );
        assert!(!otp_adds_factor(&amr(&["otp"])));
    }
}
