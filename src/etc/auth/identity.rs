use super::subject::Subject;
use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuthKind {
    ApiKey,
    Jwt,
    OAuth,
    Anonymous,
}

#[derive(Clone)]
enum VerifiedAuthentication {
    ApiKey,
    Jwt {
        session_id: Box<str>,
        auth_time: Option<i64>,
    },
    OAuth {
        session_id: Box<str>,
        auth_time: i64,
        audience: Option<Box<str>>,
    },
    Anonymous,
}

/// Effective gateway identity produced only after credential and backing-state
/// verification. Subject attributes remain available to existing policies but
/// are deliberately not exposed as propagation facts.
#[derive(Clone)]
pub struct VerifiedIdentity {
    subject: Option<Subject>,
    authentication: VerifiedAuthentication,
}

impl VerifiedIdentity {
    pub fn anonymous() -> Self {
        Self {
            subject: None,
            authentication: VerifiedAuthentication::Anonymous,
        }
    }

    pub(super) fn api_key(subject: Subject) -> Option<Self> {
        (subject.sub_type == crate::etc::auth::subject::SubjectType::ApiKey).then_some(Self {
            subject: Some(subject),
            authentication: VerifiedAuthentication::ApiKey,
        })
    }

    pub(super) fn jwt(
        subject: Subject,
        session_id: String,
        auth_time: Option<usize>,
    ) -> Option<Self> {
        if subject.sub_type != crate::etc::auth::subject::SubjectType::User
            || session_id.trim().is_empty()
        {
            return None;
        }
        let auth_time = auth_time.and_then(|value| i64::try_from(value).ok());
        Some(Self {
            subject: Some(subject),
            authentication: VerifiedAuthentication::Jwt {
                session_id: session_id.into_boxed_str(),
                auth_time,
            },
        })
    }

    pub(super) fn oauth(
        subject: Subject,
        session_id: String,
        auth_time: usize,
        audience: Option<String>,
    ) -> Option<Self> {
        if subject.sub_type != crate::etc::auth::subject::SubjectType::User
            || session_id.trim().is_empty()
        {
            return None;
        }
        Some(Self {
            subject: Some(subject),
            authentication: VerifiedAuthentication::OAuth {
                session_id: session_id.into_boxed_str(),
                auth_time: i64::try_from(auth_time).ok()?,
                audience: audience.map(String::into_boxed_str),
            },
        })
    }

    pub fn auth_kind(&self) -> AuthKind {
        match self.authentication {
            VerifiedAuthentication::ApiKey => AuthKind::ApiKey,
            VerifiedAuthentication::Jwt { .. } => AuthKind::Jwt,
            VerifiedAuthentication::OAuth { .. } => AuthKind::OAuth,
            VerifiedAuthentication::Anonymous => AuthKind::Anonymous,
        }
    }

    pub fn subject(&self) -> Option<&Subject> {
        self.subject.as_ref()
    }

    pub fn session_id(&self) -> Option<&str> {
        match &self.authentication {
            VerifiedAuthentication::Jwt { session_id, .. }
            | VerifiedAuthentication::OAuth { session_id, .. } => Some(session_id),
            VerifiedAuthentication::ApiKey | VerifiedAuthentication::Anonymous => None,
        }
    }

    pub fn auth_time(&self) -> Option<i64> {
        match self.authentication {
            VerifiedAuthentication::Jwt { auth_time, .. } => auth_time,
            VerifiedAuthentication::OAuth { auth_time, .. } => Some(auth_time),
            VerifiedAuthentication::ApiKey | VerifiedAuthentication::Anonymous => None,
        }
    }

    pub fn oauth_audience(&self) -> Option<&str> {
        match &self.authentication {
            VerifiedAuthentication::OAuth { audience, .. } => audience.as_deref(),
            VerifiedAuthentication::ApiKey
            | VerifiedAuthentication::Jwt { .. }
            | VerifiedAuthentication::Anonymous => None,
        }
    }

    #[cfg(test)]
    pub(crate) fn test_jwt(subject: Subject, session_id: String, auth_time: Option<usize>) -> Self {
        Self::jwt(subject, session_id, auth_time).expect("valid test JWT identity")
    }

    #[cfg(test)]
    pub(crate) fn test_api_key(subject: Subject) -> Self {
        Self::api_key(subject).expect("valid test API-key identity")
    }

    #[cfg(test)]
    pub(crate) fn test_oauth(
        subject: Subject,
        session_id: String,
        auth_time: usize,
        audience: Option<String>,
    ) -> Self {
        Self::oauth(subject, session_id, auth_time, audience).expect("valid test OAuth identity")
    }
}

impl fmt::Debug for VerifiedIdentity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("VerifiedIdentity")
            .field("auth_kind", &self.auth_kind())
            .field("has_subject", &self.subject.is_some())
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests;
