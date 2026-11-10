use serde::{Deserialize, Serialize};
use std::fmt;

/// Languages Stargate ships translations for. Keep this list aligned with the
/// email templates and the API message catalogs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Locale {
    En,
    It,
}

impl Locale {
    pub const ALL: &'static [Self] = &[Self::En, Self::It];
    pub const FALLBACK: Self = Self::En;

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::En => "en",
            Self::It => "it",
        }
    }

    /// Parse a language tag, reducing it to the primary language. Unknown
    /// languages return `None`; callers choose the installation fallback.
    pub fn parse(value: &str) -> Option<Self> {
        let primary = value
            .trim()
            .split(['-', '_'])
            .next()
            .unwrap_or_default()
            .to_ascii_lowercase();

        match primary.as_str() {
            "en" => Some(Self::En),
            "it" => Some(Self::It),
            _ => None,
        }
    }

    /// The first supported language from an `Accept-Language` header, honoring
    /// q-values and looking only at each tag's primary language. Wildcards and
    /// malformed q-values are ignored.
    pub fn from_accept_language(value: &str) -> Option<Self> {
        let mut candidates = Vec::new();

        for (index, part) in value.split(',').enumerate() {
            let mut segments = part.split(';');
            let Some(tag) = segments.next().map(str::trim).filter(|tag| !tag.is_empty()) else {
                continue;
            };

            let mut quality = 1.0_f32;
            let mut valid = true;
            for parameter in segments {
                let Some((name, value)) = parameter.trim().split_once('=') else {
                    valid = false;
                    break;
                };
                if !name.trim().eq_ignore_ascii_case("q") {
                    continue;
                }
                match value.trim().parse::<f32>() {
                    Ok(q) if (0.0..=1.0).contains(&q) => quality = q,
                    _ => {
                        valid = false;
                        break;
                    }
                }
            }

            if valid && quality > 0.0 {
                candidates.push((quality, index, tag));
            }
        }

        candidates.sort_by(|left, right| {
            right
                .0
                .partial_cmp(&left.0)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| left.1.cmp(&right.1))
        });

        candidates
            .into_iter()
            .find_map(|(_, _, tag)| Self::parse(tag))
    }
}

impl Default for Locale {
    fn default() -> Self {
        Self::FALLBACK
    }
}

impl fmt::Display for Locale {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::Locale;

    #[test]
    fn parses_primary_language_from_tags() {
        for (tag, expected) in [
            ("en", Some(Locale::En)),
            ("EN-us", Some(Locale::En)),
            ("it_IT", Some(Locale::It)),
            ("fr", None),
            ("", None),
        ] {
            assert_eq!(Locale::parse(tag), expected, "{tag}");
        }
    }

    #[test]
    fn accept_language_honors_quality_order() {
        assert_eq!(
            Locale::from_accept_language("it;q=0.8, en;q=0.9"),
            Some(Locale::En)
        );
        assert_eq!(
            Locale::from_accept_language("fr;q=1, it;q=0.5, en;q=0.6"),
            Some(Locale::En)
        );
        assert_eq!(
            Locale::from_accept_language("de-AT, it-CH;q=0.9"),
            Some(Locale::It)
        );
    }

    #[test]
    fn accept_language_ignores_zero_quality_wildcards_and_invalid_values() {
        assert_eq!(
            Locale::from_accept_language("it;q=0, en;q=0.5"),
            Some(Locale::En)
        );
        assert_eq!(Locale::from_accept_language("*;q=1"), None);
        assert_eq!(
            Locale::from_accept_language("it;q=bogus, en;q=0.2"),
            Some(Locale::En)
        );
        assert_eq!(Locale::from_accept_language("fr, de"), None);
        assert_eq!(Locale::from_accept_language(""), None);
    }

    #[test]
    fn explicit_request_wins_then_default_then_english() {
        let resolve = |requested: Option<Locale>, default: Locale| requested.unwrap_or(default);
        assert_eq!(resolve(Some(Locale::It), Locale::En), Locale::It);
        assert_eq!(resolve(None, Locale::It), Locale::It);
        assert_eq!(resolve(None, Locale::FALLBACK), Locale::En);
    }
}
