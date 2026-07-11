use rand::RngExt;

const UPPERCASE: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZ";
const LOWERCASE: &[u8] = b"abcdefghijklmnopqrstuvwxyz";
const DIGITS: &[u8] = b"0123456789";
const SPECIAL: &[u8] = b"!@#$%^&*()-_=+[]{}|;:,.<>?";

/// Random password/secret generator.
///
/// Enable character classes with the builder methods, then call
/// [`Generator::generate`]. When the requested length allows it, the output
/// is guaranteed to contain at least one character from every enabled class,
/// so generated passwords satisfy the composition rules they are built from.
/// # Example
/// ```
/// use pw::Generator;
///
/// let password = Generator::new(16)
///     .uppercase()
///     .lowercase()
///     .digits()
///     .special()
///     .generate();
/// assert_eq!(password.chars().count(), 16);
/// ```
pub struct Generator {
    length: usize,
    upper: bool,
    lower: bool,
    digits: bool,
    special: bool,
}

impl Generator {
    /// Create a generator for a value of `length` characters with no
    /// character classes enabled yet.
    pub fn new(length: usize) -> Self {
        Self {
            length,
            upper: false,
            lower: false,
            digits: false,
            special: false,
        }
    }

    pub fn uppercase(mut self) -> Self {
        self.upper = true;
        self
    }

    pub fn lowercase(mut self) -> Self {
        self.lower = true;
        self
    }

    pub fn digits(mut self) -> Self {
        self.digits = true;
        self
    }

    pub fn special(mut self) -> Self {
        self.special = true;
        self
    }

    /// Generate the random value.
    ///
    /// If `length` is at least the number of enabled classes, the output
    /// contains at least one character from each enabled class; positions
    /// are shuffled so the guaranteed characters are not predictable.
    ///
    /// # Panics
    /// Panics if no character class was enabled.
    pub fn generate(&self) -> String {
        let mut classes: Vec<&[u8]> = Vec::with_capacity(4);
        if self.upper {
            classes.push(UPPERCASE);
        }
        if self.lower {
            classes.push(LOWERCASE);
        }
        if self.digits {
            classes.push(DIGITS);
        }
        if self.special {
            classes.push(SPECIAL);
        }
        assert!(
            !classes.is_empty(),
            "Generator requires at least one enabled character class"
        );

        let charset: Vec<u8> = classes.concat();
        let mut rng = rand::rng();
        let mut out: Vec<u8> = Vec::with_capacity(self.length);

        // Guarantee one character per enabled class when the length allows.
        if self.length >= classes.len() {
            for class in &classes {
                out.push(class[rng.random_range(0..class.len())]);
            }
        }

        while out.len() < self.length {
            out.push(charset[rng.random_range(0..charset.len())]);
        }

        // Fisher-Yates shuffle so the per-class characters are not always at
        // the start of the value.
        for i in (1..out.len()).rev() {
            let j = rng.random_range(0..=i);
            out.swap(i, j);
        }

        String::from_utf8(out).expect("charsets are ASCII")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_generate_password() {
        let password = Generator::new(16)
            .uppercase()
            .lowercase()
            .digits()
            .special()
            .generate();
        assert_eq!(password.len(), 16);
    }

    #[test]
    fn test_generate_password_no_upper() {
        let password = Generator::new(16).lowercase().digits().special().generate();
        for c in password.chars() {
            assert!(!c.is_ascii_uppercase());
        }
    }

    #[test]
    fn every_enabled_class_is_present() {
        for _ in 0..100 {
            let password = Generator::new(8)
                .uppercase()
                .lowercase()
                .digits()
                .special()
                .generate();
            assert!(password.chars().any(|c| c.is_ascii_uppercase()));
            assert!(password.chars().any(|c| c.is_ascii_lowercase()));
            assert!(password.chars().any(|c| c.is_ascii_digit()));
            assert!(password.chars().any(|c| !c.is_ascii_alphanumeric()));
        }
    }

    #[test]
    fn length_shorter_than_class_count_is_respected() {
        let password = Generator::new(2)
            .uppercase()
            .lowercase()
            .digits()
            .special()
            .generate();
        assert_eq!(password.len(), 2);
    }

    #[test]
    #[should_panic(expected = "at least one enabled character class")]
    fn no_classes_panics() {
        Generator::new(8).generate();
    }
}
